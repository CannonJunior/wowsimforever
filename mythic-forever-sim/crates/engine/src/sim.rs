//! A minimal discrete-event single-target DPS simulation. Deliberately not a generic
//! "sim everything" engine: see README for exactly what this models and what it skips
//! (multi-target, gear items, travel time, the 5-second mana rule, partial resists).

use crate::spelldata::{DataPack, SpellRow};
use crate::stats::CasterStats;
use rand::Rng;
use rand_pcg::Pcg32;
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct AbilityTotals {
    /// Real cast attempts only (hit or miss) - *not* dot ticks. Found conflated while producing
    /// a per-ability breakdown to compare against mythicsim.com's own (2026-10-24): this field
    /// used to be incremented by `process_due_dot_ticks` too, so e.g. Bane of Agony's 12 ticks
    /// per 24s application inflated its own "casts" column roughly 10x over its real cast
    /// count - a reporting/display bug, not a damage-calculation one (every tick's damage was
    /// always summed into `damage`/`total_damage` correctly either way).
    pub casts: u32,
    /// Dot tick events, counted separately from `casts` for the reason above.
    pub ticks: u32,
    pub hits: u32,
    pub crits: u32,
    pub misses: u32,
    pub damage: f64,
}

#[derive(Debug, Default, Clone)]
pub struct FightReport {
    pub totals: BTreeMap<String, AbilityTotals>,
    pub total_damage: f64,
    pub duration_ms: i64,
}

impl FightReport {
    pub fn dps(&self) -> f64 {
        if self.duration_ms <= 0 {
            return 0.0;
        }
        self.total_damage / (self.duration_ms as f64 / 1000.0)
    }

    fn record(&mut self, ability: &str, landed: bool, crit: bool, amount: f64) {
        let e = self.totals.entry(ability.to_string()).or_default();
        e.casts += 1;
        if landed {
            e.hits += 1;
            if crit {
                e.crits += 1;
            }
            e.damage += amount;
            self.total_damage += amount;
        } else {
            e.misses += 1;
        }
    }

    /// Same bookkeeping as `record`, for a dot tick rather than a cast - a tick never misses
    /// and never counts as a `casts` increment (see `AbilityTotals::casts`'s doc comment).
    fn record_tick(&mut self, ability: &str, crit: bool, amount: f64) {
        let e = self.totals.entry(ability.to_string()).or_default();
        e.ticks += 1;
        e.hits += 1;
        if crit {
            e.crits += 1;
        }
        e.damage += amount;
        self.total_damage += amount;
    }
}

struct ActiveDot {
    key: String,
    next_tick_ms: i64,
    period_ms: i64,
    ticks_remaining: i64,
    /// Snapshotted at application time, per this engine's Classic-style dot model.
    tick_amount: f64,
    crit_chance: f64,
    crit_multiplier: f64,
}

/// A caster-wide damage modifier a talent applies to a named spell group. Kept as a simple
/// flat additive percentage list rather than a general effect system - sufficient for the
/// talents this engine's three Warlock specs actually use (see warlock/*.rs).
#[derive(Default, Clone)]
pub struct DamageModifiers {
    /// (ability_key, additive_percent) e.g. ("shadow_bolt", 0.08) for +8% Shadow Bolt damage.
    pub direct_pct: Vec<(String, f64)>,
    pub dot_pct: Vec<(String, f64)>,
    /// Flat additive crit multiplier bonus applied to a set of abilities, e.g. Ruin's +0.20/pt.
    pub crit_multiplier_bonus: Vec<(String, f64)>,
    /// Flat additive crit chance bonus (0.0-1.0) applied to a set of abilities.
    pub crit_chance_bonus: Vec<(String, f64)>,
    /// Extra GCD-free cast time multiplier per ability, e.g. Bane's flat cast time reduction
    /// is applied by the caller before calling into the fight; this exists for pct-based ones.
    pub cast_time_pct: Vec<(String, f64)>,
    /// Additive percent applied to every direct and dot hit regardless of ability, e.g. Curse
    /// of the Elements' "+10% all spell damage taken" debuff on the target, or Shadow Mastery's
    /// flat school-wide bonus. Simpler than filtering by school since this engine only models
    /// one caster's own abilities.
    pub global_pct: f64,
    /// Flat milliseconds subtracted from an ability's cast time, e.g. Bane's -100ms/point on
    /// Shadow Bolt/Immolate/Incinerate. Applied before haste scaling, same as the real game
    /// (SpellMod::CastTimeFlat in the source engine's own terms).
    pub cast_time_flat_ms: Vec<(String, i64)>,
}

impl DamageModifiers {
    fn pct(list: &[(String, f64)], key: &str) -> f64 {
        list.iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v)
            .sum()
    }

    fn flat_ms(list: &[(String, i64)], key: &str) -> i64 {
        list.iter().filter(|(k, _)| k == key).map(|(_, v)| v).sum()
    }
}

pub struct Fight {
    pub now_ms: i64,
    pub end_ms: i64,
    pub stats: CasterStats,
    pub mana: f64,
    pub pack: DataPack,
    pub mods: DamageModifiers,
    pub report: FightReport,
    rng: Pcg32,
    gcd_free_at: i64,
    cooldowns: BTreeMap<String, i64>,
    dots: Vec<ActiveDot>,
    /// Generic named timed buffs/auras (expires_at_ms), for things like Curse of the Elements
    /// uptime tracking and Amplify Curse's "used" flag.
    auras: BTreeMap<String, i64>,
    /// Stack counts for auras that build up rather than just being on/off (Fire Vulnerability:
    /// up to 5 stacks of +3% fire damage taken) - a separate map from `auras` above rather than
    /// changing that one's value type, since most auras this engine tracks are plain on/off and
    /// never need a count at all. Added 2026-10-25, beginning Mage support.
    aura_stacks: BTreeMap<String, u8>,
    /// Generic named timed *stacking* buffs (expires_at_ms, remaining count), for Demonic
    /// Brand's charges - a plain aura (`auras` above) is enough for on/off buffs, but Brand is
    /// consumed one stack at a time by the pet's hits.
    charges: BTreeMap<String, (i64, i32)>,
    /// Registered via `register_tick_proc` - checked each time a matching dot ticks.
    tick_procs: Vec<TickProc>,
    /// Set via `set_target_max_health` - enables `target_health_percent` for execute-phase
    /// talents (Decimation). `None` (the default) means no such talent can ever activate,
    /// rather than silently assuming a health pool this engine doesn't otherwise track.
    target_max_health: Option<f64>,
    last_mana_tick_ms: i64,
}

/// A chance-on-tick proc (Nightfall: a chance per Corruption/Drain Life/Drain Soul/Wrack tick
/// to grant a buff). Kept generic rather than Nightfall-specific since the shape (roll on a
/// named dot's tick, grant a named timed buff) is common to several WoW proc talents.
struct TickProc {
    source_key: String,
    chance: f64,
    buff_name: String,
    buff_duration_ms: i64,
}

#[derive(Clone, Copy, Debug)]
pub enum CastResult {
    /// Not enough mana, or on cooldown, or GCD not free - caller should try something else.
    CouldNotCast,
    Landed { crit: bool, amount: f64 },
    Missed,
}

impl Fight {
    pub fn new(stats: CasterStats, pack: DataPack, duration_ms: i64, seed: u64) -> Self {
        Fight {
            now_ms: 0,
            end_ms: duration_ms,
            mana: stats.max_mana,
            stats,
            pack,
            mods: DamageModifiers::default(),
            report: FightReport {
                duration_ms,
                ..FightReport::default()
            },
            rng: Pcg32::new(seed, 0xa02bdbf7bb3c0a7),
            gcd_free_at: 0,
            cooldowns: BTreeMap::new(),
            dots: Vec::new(),
            auras: BTreeMap::new(),
            aura_stacks: BTreeMap::new(),
            charges: BTreeMap::new(),
            tick_procs: Vec::new(),
            target_max_health: None,
            last_mana_tick_ms: 0,
        }
    }

    pub fn is_over(&self) -> bool {
        self.now_ms >= self.end_ms
    }

    pub fn aura_active(&self, name: &str) -> bool {
        self.auras.get(name).map_or(false, |&exp| exp > self.now_ms)
    }

    /// Curse of the Elements (spell 1311680, confirmed via `foreverdb.net`, 2026-10-24): "+10%
    /// Magic damage taken" on the target - a real damage multiplier, not just a utility debuff
    /// to track for refresh timing. Found missing entirely while chasing the mythicsim.com DPS
    /// gap: all three rotations cast and refresh this (`aura_active("curse_of_the_elements")`
    /// already existed for that), but nothing ever multiplied damage by it - a warlock's own
    /// curse was doing literally nothing to their own numbers. Every school this engine deals
    /// damage in (Shadow, Fire) is "Magic" for this effect's purposes (it excludes only Physical
    /// damage, which this engine doesn't model for the warlock's own casts anyway).
    const CURSE_OF_THE_ELEMENTS_MAGIC_DAMAGE_TAKEN_PCT: f64 = 0.10;

    fn curse_of_the_elements_bonus(&self) -> f64 {
        if self.aura_active("curse_of_the_elements") {
            Self::CURSE_OF_THE_ELEMENTS_MAGIC_DAMAGE_TAKEN_PCT
        } else {
            0.0
        }
    }

    pub fn set_aura(&mut self, name: &str, duration_ms: i64) {
        self.auras.insert(name.to_string(), self.now_ms + duration_ms);
    }

    /// Ends a named aura immediately (e.g. consuming Nightfall's Shadow Trance on cast).
    pub fn clear_aura(&mut self, name: &str) {
        self.auras.remove(name);
    }

    /// The current stack count of a stacking aura (Fire Vulnerability) - 0 if it's not up at
    /// all, including if it was up but has since expired (no separate bookkeeping needed:
    /// `aura_active` already means "not expired").
    pub fn aura_stacks(&self, name: &str) -> u8 {
        if self.aura_active(name) {
            self.aura_stacks.get(name).copied().unwrap_or(0)
        } else {
            0
        }
    }

    /// Adds one stack (capped at `max_stacks`) and refreshes the aura's duration - a landed
    /// Scorch extending Fire Vulnerability. Starts fresh at 1 stack if the aura had expired
    /// (or never existed) rather than requiring a separate "apply" call first.
    pub fn add_aura_stack(&mut self, name: &str, max_stacks: u8, duration_ms: i64) {
        let current = self.aura_stacks(name);
        self.aura_stacks.insert(name.to_string(), (current + 1).min(max_stacks));
        self.set_aura(name, duration_ms);
    }

    /// Same as `add_aura_stack`, but only starts the duration timer on the *first* stack -
    /// later stacks add to the count without refreshing it. Arcane Blast's own buff works this
    /// way (`src/classes/mage/spells/arcane_blast.rs`'s own module doc: "a stack does not
    /// refresh it and nothing spends it" - it runs out 15 sec after the first cast regardless
    /// of how many more stacks landed in between), unlike every other stacking aura this
    /// project had modeled before it (Fire Vulnerability, Heating Up, Combustion all refresh
    /// on every new stack via `add_aura_stack` above).
    pub fn add_aura_stack_no_refresh(&mut self, name: &str, max_stacks: u8, duration_ms: i64) {
        let already_active = self.aura_active(name);
        let current = self.aura_stacks(name);
        self.aura_stacks.insert(name.to_string(), (current + 1).min(max_stacks));
        if !already_active {
            self.set_aura(name, duration_ms);
        }
    }

    /// Milliseconds left on a named aura, `None` if it's not active at all - for a "refresh
    /// before it drops" rotation check (Fire Vulnerability's own real condition: refresh once
    /// stacks aren't maxed *or* under 5s remain, mirroring `dot_remaining_ms`'s shape).
    pub fn aura_remaining_ms(&self, name: &str) -> Option<i64> {
        self.auras.get(name).and_then(|&exp| {
            let remaining = exp - self.now_ms;
            (remaining > 0).then_some(remaining)
        })
    }

    /// Registers a chance-per-tick proc on a dot tracked under `source_key` (Nightfall: a
    /// Corruption tick has a 2%/4% chance to grant 10s of "Shadow Trance"). Checked inside
    /// `process_due_dot_ticks` for every tick of a matching dot, for as long as that dot is up
    /// - there's no need to re-register when the dot refreshes.
    pub fn register_tick_proc(&mut self, source_key: &str, chance: f64, buff_name: &str, buff_duration_ms: i64) {
        self.tick_procs.push(TickProc {
            source_key: source_key.to_string(),
            chance,
            buff_name: buff_name.to_string(),
            buff_duration_ms,
        });
    }

    /// (Re)sets a named stacking charge counter (e.g. Demonic Brand), replacing whatever
    /// charges/expiry it had before - matching the real game's "a fresh application resets the
    /// stack count" behavior rather than adding on top of a partial stack.
    pub fn set_charges(&mut self, name: &str, count: i32, duration_ms: i64) {
        self.charges.insert(name.to_string(), (self.now_ms + duration_ms, count));
    }

    pub fn charges_remaining(&self, name: &str) -> i32 {
        match self.charges.get(name) {
            Some((expires_at, count)) if *expires_at > self.now_ms => *count,
            _ => 0,
        }
    }

    /// Spends one charge if any remain (and the buff hasn't expired); returns whether one was
    /// actually consumed.
    pub fn consume_charge(&mut self, name: &str) -> bool {
        if let Some((expires_at, count)) = self.charges.get_mut(name) {
            if *expires_at > self.now_ms && *count > 0 {
                *count -= 1;
                return true;
            }
        }
        false
    }

    /// Opts into execute-phase tracking (Decimation) by giving the target a total health
    /// pool. Without this, `target_health_percent` always returns `None` and any
    /// health-gated bonus simply never activates - an explicit opt-in rather than an assumed
    /// default health pool this engine has no other reason to know.
    pub fn set_target_max_health(&mut self, max_health: f64) {
        self.target_max_health = Some(max_health);
    }

    /// The target's remaining health as a fraction of `set_target_max_health`'s value, based
    /// on cumulative damage dealt so far (`report.total_damage`) - `None` if no max health was
    /// ever set. This engine does not simulate incoming healing on the target (there's only
    /// one, and nothing heals it), so this is a simple "damage taken so far" subtraction, not
    /// a real HP-over-time model.
    pub fn target_health_percent(&self) -> Option<f64> {
        self.target_max_health
            .map(|max| ((max - self.report.total_damage) / max).max(0.0))
    }

    pub fn charges_remaining_ms(&self, name: &str) -> Option<i64> {
        self.charges.get(name).and_then(|(expires_at, count)| {
            if *count > 0 {
                Some(expires_at - self.now_ms)
            } else {
                None
            }
        })
    }

    pub fn dot_remaining_ms(&self, key: &str) -> Option<i64> {
        self.dots
            .iter()
            .find(|d| d.key == key)
            .map(|d| d.next_tick_ms + (d.ticks_remaining.saturating_sub(1)) * d.period_ms - self.now_ms)
    }

    pub fn is_on_cooldown(&self, key: &str) -> bool {
        self.cooldowns.get(key).map_or(false, |&t| t > self.now_ms)
    }

    /// A generic proc-chance roll (Improved Scorch's own chance to apply a Fire Vulnerability
    /// stack, at talent points below the maximum where it isn't simply guaranteed) - exposed
    /// publicly since `register_tick_proc`'s shape doesn't fit a "roll once, right after a
    /// landed hit, for an effect the caller applies itself" proc, only a per-tick one.
    pub fn roll_chance(&mut self, chance: f64) -> bool {
        self.rng.gen::<f64>() < chance
    }

    pub fn mana_percent(&self) -> f64 {
        if self.stats.max_mana <= 0.0 {
            0.0
        } else {
            self.mana / self.stats.max_mana
        }
    }

    pub fn haste_scale(&self, base_ms: i64) -> i64 {
        ((base_ms as f64) / self.stats.haste.max(0.01)).round() as i64
    }

    /// `base_cast_ms` with any flat talent reduction (e.g. Bane) applied first, then haste -
    /// matching the real game's SpellMod order, and floored at 0 (haste_scale/advance already
    /// treat 0 as "instant", with the GCD remaining the real floor on cast frequency).
    pub fn modified_cast_ms(&self, spell_key: &str, base_cast_ms: i64) -> i64 {
        let flat = DamageModifiers::flat_ms(&self.mods.cast_time_flat_ms, spell_key);
        // `cast_time_pct` existed as a field but nothing ever read it until Heating Up needed
        // a genuinely *dynamic* per-cast percentage (stack count changes every cast, unlike a
        // talent's flat, one-time modifier) - found while beginning to model it, 2026-10-25.
        let pct = DamageModifiers::pct(&self.mods.cast_time_pct, spell_key);
        let after_flat = (base_cast_ms + flat).max(0);
        self.haste_scale(((after_flat as f64) * (1.0 + pct)).max(0.0) as i64)
    }

    /// Advances time to `now_ms + delta_ms`, ticking dots and regenerating mana as it goes.
    /// Callers should always route time forward through this (never mutate `now_ms` directly)
    /// so dots and mana regen stay correct.
    pub fn advance(&mut self, delta_ms: i64) {
        if delta_ms <= 0 {
            return;
        }
        let target = self.now_ms + delta_ms;
        loop {
            let next_tick = self.dots.iter().map(|d| d.next_tick_ms).min();
            let stop_at = next_tick.map_or(target, |t| t.min(target));
            self.advance_mana_to(stop_at);
            self.now_ms = stop_at;
            self.process_due_dot_ticks();
            if self.now_ms >= target {
                break;
            }
        }
    }

    fn advance_mana_to(&mut self, t: i64) {
        if t <= self.last_mana_tick_ms {
            return;
        }
        let dt_ms = t - self.last_mana_tick_ms;
        self.mana = (self.mana + self.stats.mp5 * (dt_ms as f64 / 5000.0)).min(self.stats.max_mana);
        self.last_mana_tick_ms = t;
    }

    fn process_due_dot_ticks(&mut self) {
        loop {
            let Some(idx) = self
                .dots
                .iter()
                .position(|d| d.next_tick_ms <= self.now_ms)
            else {
                break;
            };
            let crit = self.rng.gen::<f64>() < self.dots[idx].crit_chance;
            let mult = if crit { self.dots[idx].crit_multiplier } else { 1.0 };
            let amount = self.dots[idx].tick_amount * mult;
            let key = self.dots[idx].key.clone();
            self.report.record_tick(&key, crit, amount);

            for proc in &self.tick_procs {
                if proc.source_key == key && self.rng.gen::<f64>() < proc.chance {
                    self.auras
                        .insert(proc.buff_name.clone(), self.now_ms + proc.buff_duration_ms);
                }
            }

            self.dots[idx].ticks_remaining -= 1;
            if self.dots[idx].ticks_remaining <= 0 {
                self.dots.remove(idx);
            } else {
                self.dots[idx].next_tick_ms += self.dots[idx].period_ms;
            }
        }
    }

    fn roll_variance(&mut self, base: f64, variance: Option<f64>) -> f64 {
        match variance {
            Some(v) if v > 0.0 => {
                let low = base * (1.0 - v / 2.0);
                let high = base * (1.0 + v / 2.0);
                low + (high - low) * self.rng.gen::<f64>()
            }
            _ => base,
        }
    }

    /// Universal `spell_power` plus whichever school-specific bonus (`bonus_shadow_power` or
    /// `bonus_fire_power`) matches `row`'s school - see `stats.rs`'s doc comments and
    /// `gear.rs`'s module docs for where these two come from.
    fn effective_spell_power(&self, row: &SpellRow) -> f64 {
        self.stats.spell_power
            + match row.school() {
                crate::stats::School::Shadow => self.stats.bonus_shadow_power,
                crate::stats::School::Fire => self.stats.bonus_fire_power,
                crate::stats::School::Arcane => self.stats.bonus_arcane_power,
                crate::stats::School::Frost => self.stats.bonus_frost_power,
            }
    }

    fn average_direct(&self, row: &SpellRow) -> f64 {
        let d = row
            .direct
            .as_ref()
            .expect("average_direct called on a spell with no direct effect");
        d.base_points + self.effective_spell_power(row) * d.sp_coefficient
    }

    /// Casts a direct-damage spell by data-pack key. `ability_key` is the report/modifier
    /// attribution key (usually the same as `spell_key`, but callers can pass a different one
    /// e.g. to attribute Incinerate's bonus separately).
    ///
    /// GCD and cast time are consumed by the caller via `advance` *before* calling this for
    /// instant-resolution spells, or the caller should call `advance(cast_time)` then this -
    /// see `warlock/mod.rs::cast_and_resolve` which does this consistently so spec code never
    /// has to.
    /// `extra_pct` is an additive bonus the caller computes per cast from live state this
    /// engine doesn't track as a static modifier - a proc-window buff's current uptime (e.g.
    /// Shadow Vulnerability, Shadow and Flame), rather than a permanent talent percentage.
    /// `DamageModifiers` on `self.mods` still applies on top for the static ones.
    pub fn resolve_direct(
        &mut self,
        spell_key: &str,
        ability_key: &str,
        crit_chance: f64,
        crit_multiplier: f64,
        extra_pct: f64,
    ) -> CastResult {
        let row = self.pack.spell(spell_key).clone();
        if self.rng.gen::<f64>() >= self.stats.spell_hit {
            self.report.record(ability_key, false, false, 0.0);
            return CastResult::Missed;
        }
        let base = self.average_direct(&row);
        let rolled = self.roll_variance(base, row.direct.as_ref().unwrap().variance);
        let pct_bonus = DamageModifiers::pct(&self.mods.direct_pct, ability_key)
            + self.mods.global_pct
            + extra_pct;
        let crit_chance_total =
            crit_chance + DamageModifiers::pct(&self.mods.crit_chance_bonus, ability_key);
        let crit_mult_total =
            crit_multiplier + DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key);
        let crit = self.rng.gen::<f64>() < crit_chance_total;
        let mult = if crit { crit_mult_total } else { 1.0 };
        let amount = rolled * (1.0 + pct_bonus) * (1.0 + self.curse_of_the_elements_bonus()) * mult;
        self.report.record(ability_key, true, crit, amount);
        CastResult::Landed { crit, amount }
    }

    /// Same as `resolve_direct`, but with an extra flat spell-power bonus added on top of
    /// `effective_spell_power` before the `sp_coefficient` multiply - for a buff that adds raw
    /// spell power dynamically (e.g. Arcane Blast's own stacking buff, 15% of Intellect per
    /// stack as spell damage - see `mage/arcane.rs`), which isn't a static `CasterStats` field
    /// since its value depends on the caster's *current* stack count, not gear alone.
    pub fn resolve_direct_with_bonus_power(
        &mut self,
        spell_key: &str,
        ability_key: &str,
        crit_chance: f64,
        crit_multiplier: f64,
        extra_pct: f64,
        bonus_power: f64,
    ) -> CastResult {
        let row = self.pack.spell(spell_key).clone();
        if self.rng.gen::<f64>() >= self.stats.spell_hit {
            self.report.record(ability_key, false, false, 0.0);
            return CastResult::Missed;
        }
        let d = row
            .direct
            .as_ref()
            .expect("resolve_direct_with_bonus_power called on a spell with no direct effect");
        let base = d.base_points + (self.effective_spell_power(&row) + bonus_power) * d.sp_coefficient;
        let rolled = self.roll_variance(base, d.variance);
        let pct_bonus = DamageModifiers::pct(&self.mods.direct_pct, ability_key)
            + self.mods.global_pct
            + extra_pct;
        let crit_chance_total =
            crit_chance + DamageModifiers::pct(&self.mods.crit_chance_bonus, ability_key);
        let crit_mult_total =
            crit_multiplier + DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key);
        let crit = self.rng.gen::<f64>() < crit_chance_total;
        let mult = if crit { crit_mult_total } else { 1.0 };
        let amount = rolled * (1.0 + pct_bonus) * (1.0 + self.curse_of_the_elements_bonus()) * mult;
        self.report.record(ability_key, true, crit, amount);
        CastResult::Landed { crit, amount }
    }

    /// Same as `resolve_direct`, but for a damage source with no data-pack row (Demonic
    /// Brand's pet hit: its min/max/coefficient are a Go-literal formula, not a client spell
    /// row - see `warlock/demonology.rs`). `min`/`max` bound a uniform roll (the source engine
    /// calls this `go_roll`, a flat min-max range rather than this engine's base*variance
    /// shape elsewhere - kept faithful to that since it's how the real formula is shaped here).
    /// `bonus_power` is the caller-computed school-specific bonus (`bonus_shadow_power` or
    /// `bonus_fire_power`) to add on top of `spell_power` - there's no data-pack row here to
    /// read a school off of (see this function's own doc comment above), so the caller picks.
    pub fn resolve_direct_raw(
        &mut self,
        ability_key: &str,
        min: f64,
        max: f64,
        sp_coefficient: f64,
        crit_chance: f64,
        crit_multiplier: f64,
        can_miss: bool,
        bonus_power: f64,
    ) -> CastResult {
        if can_miss && self.rng.gen::<f64>() >= self.stats.spell_hit {
            self.report.record(ability_key, false, false, 0.0);
            return CastResult::Missed;
        }
        let roll = min + (max - min) * self.rng.gen::<f64>();
        let amount_base = sp_coefficient.mul_add(self.stats.spell_power + bonus_power, roll);
        let pct_bonus =
            DamageModifiers::pct(&self.mods.direct_pct, ability_key) + self.mods.global_pct;
        let crit_chance_total =
            crit_chance + DamageModifiers::pct(&self.mods.crit_chance_bonus, ability_key);
        let crit_mult_total =
            crit_multiplier + DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key);
        let crit = self.rng.gen::<f64>() < crit_chance_total;
        let mult = if crit { crit_mult_total } else { 1.0 };
        let amount = amount_base * (1.0 + pct_bonus) * (1.0 + self.curse_of_the_elements_bonus()) * mult;
        self.report.record(ability_key, true, crit, amount);
        CastResult::Landed { crit, amount }
    }

    /// A demon pet's melee swing - the real attack-table order (miss, dodge, glance, crit,
    /// hit) on one shared roll, the same structure `warlock/mod.rs`'s `pet_melee_swing`'s own
    /// doc comment cites from the real engine's source. No Curse of the Elements bonus here
    /// (that debuff is "Magic damage taken" only - a melee swing is Physical) and no data-pack
    /// row (weapon damage, not a spell), which is why this is its own method rather than a
    /// variant of `resolve_direct_raw`.
    pub fn resolve_melee_swing(
        &mut self,
        ability_key: &str,
        min: f64,
        max: f64,
        ap_bonus: f64,
        miss_chance: f64,
        dodge_chance: f64,
        glance_chance: f64,
        glance_multiplier: f64,
        glance_spread: f64,
        crit_chance: f64,
        crit_multiplier: f64,
    ) -> CastResult {
        let roll = self.rng.gen::<f64>();
        let mut cumulative = miss_chance.max(0.0);
        if roll < cumulative {
            self.report.record(ability_key, false, false, 0.0);
            return CastResult::Missed;
        }
        cumulative += dodge_chance.max(0.0);
        if roll < cumulative {
            // A dodge, not a miss, but this engine's `CastResult` has no separate outcome for
            // it and nothing downstream distinguishes them - same zero-damage effect either way.
            self.report.record(ability_key, false, false, 0.0);
            return CastResult::Missed;
        }
        let pct_bonus =
            DamageModifiers::pct(&self.mods.direct_pct, ability_key) + self.mods.global_pct;
        let base = (min + (max - min) * self.rng.gen::<f64>() + ap_bonus) * (1.0 + pct_bonus);
        cumulative += glance_chance.max(0.0);
        if roll < cumulative {
            let spread = 2.0 * self.rng.gen::<f64>() - 1.0;
            let amount = base * glance_spread.mul_add(spread, glance_multiplier);
            self.report.record(ability_key, true, false, amount);
            return CastResult::Landed { crit: false, amount };
        }
        cumulative += crit_chance.max(0.0);
        if roll < cumulative {
            let amount = base * crit_multiplier;
            self.report.record(ability_key, true, true, amount);
            return CastResult::Landed { crit: true, amount };
        }
        self.report.record(ability_key, true, false, base);
        CastResult::Landed { crit: false, amount: base }
    }

    /// A dot-only cast (Corruption, Bane of Agony, Siphon Life): rolls the spell's own hit
    /// chance - a real roll this engine didn't make until found missing while comparing a
    /// per-ability damage breakdown against mythicsim.com's own (2026-10-24): `cast_dot_only`
    /// called straight into what's now `apply_dot_unconditional` with no hit check at all, so
    /// every pure-dot spell silently landed 100% of the time regardless of `spell_hit` (~87-88%
    /// for the builds this project's own comparison uses) while every direct-damage spell
    /// correctly rolled. Records the cast itself (0 damage - a non-damage event, same as
    /// `resolve_direct`'s own miss case) separately from each tick `process_due_dot_ticks`
    /// records once the dot is actually ticking.
    pub fn apply_dot(
        &mut self,
        spell_key: &str,
        ability_key: &str,
        crit_chance: f64,
        crit_multiplier: f64,
        extra_pct: f64,
    ) {
        if self.rng.gen::<f64>() >= self.stats.spell_hit {
            self.report.record(ability_key, false, false, 0.0);
            return;
        }
        self.report.record(ability_key, true, false, 0.0);
        self.apply_dot_unconditional(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct);
    }

    /// The actual dot-application logic, with no hit roll of its own - for a direct+dot spell
    /// (Immolate) whose caller already rolled hit once for the whole cast (`resolve_direct`)
    /// and should apply the dot only when that one roll landed, not roll a second, independent
    /// time (a real cast either lands as a whole or misses as a whole - two independent rolls
    /// would be wrong either way, over- or under-applying the dot relative to the real one-roll
    /// behavior). `apply_dot` above is the hit-rolling version for dot-only spells.
    pub fn apply_dot_unconditional(
        &mut self,
        spell_key: &str,
        ability_key: &str,
        crit_chance: f64,
        crit_multiplier: f64,
        extra_pct: f64,
    ) {
        let row = self.pack.spell(spell_key).clone();
        let p = row
            .periodic
            .as_ref()
            .expect("apply_dot called on a spell with no periodic effect");
        let base = p.base_points_per_tick + self.effective_spell_power(&row) * p.sp_coefficient;
        let pct_bonus = DamageModifiers::pct(&self.mods.dot_pct, ability_key)
            + self.mods.global_pct
            + extra_pct;
        let tick_amount = base * (1.0 + pct_bonus) * (1.0 + self.curse_of_the_elements_bonus());
        let num_ticks = p.num_ticks.unwrap_or(1);
        self.dots.retain(|d| d.key != ability_key);
        self.dots.push(ActiveDot {
            key: ability_key.to_string(),
            next_tick_ms: self.now_ms + p.period_ms,
            period_ms: p.period_ms,
            ticks_remaining: num_ticks,
            tick_amount,
            crit_chance: crit_chance + DamageModifiers::pct(&self.mods.crit_chance_bonus, ability_key),
            crit_multiplier: crit_multiplier
                + DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key),
        });
    }

    /// Total damage a still-active dot has left to deal (`tick_amount * ticks_remaining`), 0.0
    /// if it's not active. For Ignite's own "roll what's still owed into the new application"
    /// merge (`mage/fire.rs`'s `apply_ignite`) - there's no other reason an *amount*, not just
    /// a duration, would matter for an existing dot elsewhere in this engine.
    pub fn dot_owed_damage(&self, key: &str) -> f64 {
        self.dots
            .iter()
            .find(|d| d.key == key)
            .map(|d| d.tick_amount * d.ticks_remaining as f64)
            .unwrap_or(0.0)
    }

    /// A dot whose tick amount is already fully computed by the caller (Ignite: a share of the
    /// triggering crit's own damage) - bypasses the data-pack-driven base/coefficient
    /// calculation `apply_dot_unconditional` does, since there's no spell row this amount
    /// comes from. Never crits, never rerolls Curse-of-the-Elements-style bonuses (the real
    /// source's own module docs: "the ticks skip them, never roll a resist and never crit" -
    /// the amount is already final when it's set).
    pub fn apply_fixed_dot(&mut self, ability_key: &str, tick_amount: f64, period_ms: i64, num_ticks: i64) {
        self.dots.retain(|d| d.key != ability_key);
        self.dots.push(ActiveDot {
            key: ability_key.to_string(),
            next_tick_ms: self.now_ms + period_ms,
            period_ms,
            ticks_remaining: num_ticks,
            tick_amount,
            crit_chance: 0.0,
            crit_multiplier: 1.0,
        });
    }

    pub fn spend_mana(&mut self, amount: f64) -> bool {
        if self.mana + 1e-6 < amount {
            return false;
        }
        self.mana -= amount;
        true
    }

    pub fn add_mana(&mut self, amount: f64) {
        self.mana = (self.mana + amount).min(self.stats.max_mana);
    }

    pub fn set_cooldown(&mut self, key: &str, duration_ms: i64) {
        self.cooldowns.insert(key.to_string(), self.now_ms + duration_ms);
    }

    pub fn set_gcd(&mut self, duration_ms: i64) {
        self.gcd_free_at = self.now_ms + duration_ms;
    }

    pub fn gcd_remaining(&self) -> i64 {
        (self.gcd_free_at - self.now_ms).max(0)
    }
}
