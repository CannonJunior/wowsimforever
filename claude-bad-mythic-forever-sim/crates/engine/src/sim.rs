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
    /// Registered via `register_dynamic_spell_power_aura` - `(aura_name, intellect_multiplier_
    /// per_stack)` pairs, read live by `effective_spell_power` every cast rather than mutated
    /// imperatively, since a stacking aura can expire silently (no method call fires at expiry)
    /// and `aura_stacks` already handles that lazily. Added 2026-10-25 for Arcane Blast's own
    /// buff (400573): a real debug log showed Frostbolt's own `SP:` field alternate between
    /// 480.0 and 665.4 over the course of the fight with no gear/talent change - exactly
    /// matching `480 + 3*0.15*Intellect(412) = 665.4` - and the real log's own `Dynamic dep
    /// updated (SpellDamage += Intellect * 0.15)` lines, fired on every Arcane Blast stack
    /// gain/loss, confirm this is a *standing stat increase* applied to the caster's SpellDamage
    /// (affecting every spell cast while any stack is up, Frostbolt included), not a bonus
    /// local to Arcane Blast's own cast the way this project had modeled it before (`mage/
    /// arcane.rs`'s old `arcane_blast_bonus_power`/`resolve_direct_with_bonus_power` path,
    /// which only ever touched Arcane Blast's own damage). Generalized here (not Arcane-
    /// specific) since the real mechanic - "N stacks of aura X each contribute a flat Intellect
    /// fraction to SpellDamage, live" - could recur for any class/aura.
    dynamic_spell_power_auras: Vec<(String, f64)>,
    /// Set via `set_target_max_health` - enables `target_health_percent` for execute-phase
    /// talents (Decimation). `None` (the default) means no such talent can ever activate,
    /// rather than silently assuming a health pool this engine doesn't otherwise track.
    target_max_health: Option<f64>,
    last_mana_tick_ms: i64,
    /// `Some` once `enable_debug_log` has been called - a human-readable, timestamped line per
    /// rotation decision/event, mirroring the real engine's own per-cast debug log shape
    /// closely enough to diff the two side by side. `None` by default (every existing call
    /// site keeps working unchanged, and a 3000-iteration run doesn't pay for building a log
    /// nobody reads). Added 2026-10-25 specifically to find the real cause of a Fire Mage
    /// cast-count skew (Fire Blast under-cast, Scorch over-cast) that two independently
    /// source-verified condition fixes hadn't closed - this project's own aggregate-only
    /// `FightReport` had no way to show *when* each cast happened or *why* a given decision
    /// point chose what it chose, the same blind spot every real-engine-sourced fix in this
    /// project up to this point had only ever needed the *real* engine's own log to see past.
    pub debug_log: Option<Vec<String>>,
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
            dynamic_spell_power_auras: Vec::new(),
            target_max_health: None,
            last_mana_tick_ms: 0,
            debug_log: None,
        }
    }

    /// Opts into `debug_log` (see that field's own doc comment) - call once, right after
    /// `Fight::new`, before running a single-iteration fight meant for human inspection, not a
    /// multi-thousand-iteration DPS average (this engine doesn't optimize the log away, so
    /// leaving it on for a real run would just waste memory on a log nobody reads).
    pub fn enable_debug_log(&mut self) {
        self.debug_log = Some(Vec::new());
    }

    /// Appends one line to `debug_log` if enabled, timestamped the same way the real engine's
    /// own log is (`[seconds.ms]`) so the two can be read side by side. A no-op otherwise, so
    /// every call site can log unconditionally without checking `is_some()` itself.
    pub fn log_event(&mut self, msg: &str) {
        if let Some(log) = &mut self.debug_log {
            log.push(format!("[{:.2}] {msg}", self.now_ms as f64 / 1000.0));
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

    /// This engine's own validation targets are always a level-63 raid boss with no resistance
    /// stat set at all (confirmed directly in every "builder starter" fixture's own `encounter.
    /// targets[0].stats` - every resistance index is 0) - the same "not level-aware, always
    /// assumes level 63" simplification `gear.rs`'s own `BASE_SPELL_HIT_VS_LEVEL_63_PERCENT`
    /// already makes for spell hit, applied here too rather than threading a real target level
    /// through every caller.
    const TARGET_LEVEL: f64 = 63.0;

    /// Go `spellVulnerabilityMultiplier` (confirmed directly against the real engine's own
    /// `src/core/fight/damage.rs`): a caster's Spell Piercing past the target's own resistance
    /// (floored at 0, so a target's own innate resistance never stacks as *extra* vulnerability
    /// through this path) adds `1 / (5 * target_level)` of bonus damage per point, on every
    /// magic school with a resistance type (Arcane/Fire/Frost/Nature/Shadow - every school this
    /// engine or any future class resolves damage in). Found 2026-10-25 during a second,
    /// rigorous root-cause pass after being told the comparisons were still "woefully
    /// inaccurate": a real debug log showed Frostbolt landing a *consistent* ~4.76% bonus this
    /// project had never modeled at all, on a build whose target has zero resistance and whose
    /// gear/talents this project had already fully accounted for - the one remaining unexplained
    /// multiplier in the real engine's own four-stage damage pipeline
    /// (BaseDamage→AfterAttackerMods→AfterResistances→AfterTargetMods→AfterOutcome). Traced to
    /// Arcane Subtlety (2/2 in both the Arcane and Frost "builder starter" builds - a talent
    /// this project had previously, wrongly, assumed only affected threat): its own second
    /// effect is `-15` target resistance (spell 11210's own curve, rank 2), which `arcane.rs`'s
    /// own `register_arcane_subtlety` stores as `+15 SpellPiercing` directly - `15 / (5*63) =
    /// 0.04762`, matching the observed ~4.76% exactly. This is a *generic* caster mechanic
    /// (any class's gear/talents/racials could grant Spell Piercing against any magic school),
    /// not a Mage-specific one - modeled here in the shared damage-resolution path, not in
    /// `mage/*.rs`, for exactly that reason.
    fn spell_vulnerability_bonus(&self) -> f64 {
        self.stats.spell_piercing.max(0.0) / (5.0 * Self::TARGET_LEVEL)
    }

    /// Judgement of Wisdom's own real values, confirmed directly against the real engine's
    /// own `JUDGEMENT_PROC_CHANCE` constant and a real debug log's own consistent `Gained
    /// 59.000 mana from {SpellID: 20355}` lines - see `CasterStats::standing_judgement_of_
    /// wisdom`'s own doc comment for the full citation.
    const JUDGEMENT_OF_WISDOM_PROC_CHANCE: f64 = 0.5;
    const JUDGEMENT_OF_WISDOM_MANA: f64 = 59.0;

    /// Rolls Judgement of Wisdom's own proc on a landed *direct* hit (never a dot tick - the
    /// real source's own `ProcMask::DIRECT`) - a no-op unless `standing_judgement_of_wisdom`
    /// is set. Called from every direct-hit resolution path, not `apply_dot_unconditional`.
    fn maybe_proc_judgement_of_wisdom(&mut self) {
        if self.stats.standing_judgement_of_wisdom
            && self.rng.gen::<f64>() < Self::JUDGEMENT_OF_WISDOM_PROC_CHANCE
        {
            self.add_mana(Self::JUDGEMENT_OF_WISDOM_MANA);
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

    /// Registers a stacking aura whose current stack count contributes
    /// `stacks * intellect_multiplier_per_stack * self.stats.intellect` to every subsequent
    /// cast's `effective_spell_power` for as long as any stack is up - see
    /// `dynamic_spell_power_auras`'s own doc comment for the real-engine citation (Arcane
    /// Blast's own buff, the mechanic this was built for). Call once during talent setup, not
    /// per cast - `effective_spell_power` re-reads the live stack count every time.
    pub fn register_dynamic_spell_power_aura(&mut self, aura_name: &str, intellect_multiplier_per_stack: f64) {
        self.dynamic_spell_power_auras
            .push((aura_name.to_string(), intellect_multiplier_per_stack));
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

    /// Combines a base crit multiplier (1.5 for every spell this engine resolves - it only
    /// models casters) with a talent's own "+X% crit damage bonus" (`mods.crit_multiplier_bonus`,
    /// e.g. Incineration, Arcane Mind, Ice Shards) the *correct* way: the bonus percentage
    /// scales only the crit's *extra* damage above a normal hit, not the whole multiplier.
    /// Found and fixed 2026-10-25 during a rigorous per-ability gap investigation across all
    /// three Mage specs: this project's own formula used to be a plain
    /// `base_multiplier + bonus_pct` (e.g. 1.5 + 1.00 = 2.5 for a +100% talent), which a real
    /// engine debug log proved wrong for *two independent* builds/talents (Arcane Mage's Arcane
    /// Mind 5/5 and Frost Mage's Ice Shards 5/5, both a +100% bonus) - both showed a crit
    /// landing at exactly 2.0x base damage, not 2.5x. The real engine's own source
    /// (`sage3648/mythicsim-forever-engine`'s `src/mechanics/damage.rs::crit_damage_multiplier`)
    /// confirms the correct formula directly, with its own doc comment: "multiplicative
    /// modifiers scale the base, additive ones scale only the bonus above 100%" - i.e.
    /// `1.0 + (base_multiplier - 1.0) * (1.0 + bonus_pct)`, which gives exactly 2.0 for
    /// base=1.5, bonus=1.00: `1.0 + 0.5 * 2.0 = 2.0`. This is a single shared function (not
    /// duplicated per call site) specifically so the fix applies uniformly everywhere this
    /// engine computes a crit multiplier, for every class - a formula bug, not a
    /// Mage-specific or Warlock-specific one.
    fn combined_crit_multiplier(base_multiplier: f64, bonus_pct: f64) -> f64 {
        1.0 + (base_multiplier - 1.0) * (1.0 + bonus_pct)
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
        let dynamic_bonus: f64 = self
            .dynamic_spell_power_auras
            .iter()
            .map(|(name, mult)| self.aura_stacks(name) as f64 * mult * self.stats.intellect)
            .sum();
        self.stats.spell_power
            + dynamic_bonus
            + match row.school() {
                crate::stats::School::Shadow => self.stats.bonus_shadow_power,
                crate::stats::School::Fire => self.stats.bonus_fire_power,
                crate::stats::School::Arcane => self.stats.bonus_arcane_power,
                crate::stats::School::Frost => self.stats.bonus_frost_power,
                crate::stats::School::Holy => self.stats.bonus_holy_power,
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
        let crit_mult_total = Self::combined_crit_multiplier(
            crit_multiplier,
            DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key),
        );
        let crit = self.rng.gen::<f64>() < crit_chance_total;
        let mult = if crit { crit_mult_total } else { 1.0 };
        let amount = rolled * (1.0 + pct_bonus) * (1.0 + self.spell_vulnerability_bonus()) * (1.0 + self.curse_of_the_elements_bonus()) * mult;
        self.report.record(ability_key, true, crit, amount);
        self.maybe_proc_judgement_of_wisdom();
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
        let crit_mult_total = Self::combined_crit_multiplier(
            crit_multiplier,
            DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key),
        );
        let crit = self.rng.gen::<f64>() < crit_chance_total;
        let mult = if crit { crit_mult_total } else { 1.0 };
        let amount = amount_base * (1.0 + pct_bonus) * (1.0 + self.spell_vulnerability_bonus()) * (1.0 + self.curse_of_the_elements_bonus()) * mult;
        self.report.record(ability_key, true, crit, amount);
        // Deliberately no `maybe_proc_judgement_of_wisdom` here: every caller of this function
        // is a *pet's* own hit (Firebolt, Lash of Pain, Demonic Brand), and Judgement of
        // Wisdom returns mana to whoever's hit actually landed - this engine doesn't model a
        // pet's own mana pool at all, so crediting the *player* for the pet's landed hit would
        // be a real attribution error, not a faithful read of the mechanic.
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
    /// Returns whether the cast landed - added 2026-10-25 for Shadow Priest's own Shadow
    /// Weaving (a proc gated on a *landed* hit, same as every other direct-hit proc in this
    /// project), which `cast_dot_only`'s own plain `bool` (whether the cast was even attempted,
    /// not whether it landed) can't answer. `apply_dot` is only ever called from `cast_dot_only`
    /// below, so widening this return type doesn't touch any other caller.
    pub fn apply_dot(
        &mut self,
        spell_key: &str,
        ability_key: &str,
        crit_chance: f64,
        crit_multiplier: f64,
        extra_pct: f64,
    ) -> bool {
        if self.rng.gen::<f64>() >= self.stats.spell_hit {
            self.report.record(ability_key, false, false, 0.0);
            return false;
        }
        self.report.record(ability_key, true, false, 0.0);
        self.apply_dot_unconditional(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct);
        true
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
        let tick_amount = base * (1.0 + pct_bonus) * (1.0 + self.spell_vulnerability_bonus()) * (1.0 + self.curse_of_the_elements_bonus());
        let num_ticks = p.num_ticks.unwrap_or(1);
        self.dots.retain(|d| d.key != ability_key);
        self.dots.push(ActiveDot {
            key: ability_key.to_string(),
            next_tick_ms: self.now_ms + p.period_ms,
            period_ms: p.period_ms,
            ticks_remaining: num_ticks,
            tick_amount,
            crit_chance: crit_chance + DamageModifiers::pct(&self.mods.crit_chance_bonus, ability_key),
            crit_multiplier: Self::combined_crit_multiplier(
                crit_multiplier,
                DamageModifiers::pct(&self.mods.crit_multiplier_bonus, ability_key),
            ),
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

    /// Multiplies this caster's own total Intellect by `multiplier` (e.g. 1.10 for Arcane
    /// Mind's own +10%) and raises `max_mana`/current `mana` by the matching delta - since
    /// Intellect feeds max mana at a flat `gear::INTELLECT_TO_MANA_PER_POINT` (+15) per point,
    /// a talent that boosts Intellect *after* `Fight::new` already totaled gear/buffs into
    /// `max_mana` (every Mage talent does, since `apply_talents` always runs after
    /// `Fight::new`) would otherwise leave the mana pool silently 15-per-point stale. Found
    /// 2026-10-25 during a second, deeper root-cause pass: a real debug log showed Arcane
    /// Mage's own `max_mana` at 7113, 553 higher than this engine's own 6560 for the identical
    /// build - exactly `15 * (post-Arcane-Mind Intellect - pre-Arcane-Mind Intellect)`. Generic
    /// across every class: any future Intellect-boosting talent (any class could have one, not
    /// just Mage) should call this instead of mutating `stats.intellect` directly, so the same
    /// bug can't recur for a different spec.
    pub fn multiply_intellect(&mut self, multiplier: f64) {
        let old = self.stats.intellect;
        let new = old * multiplier;
        self.stats.intellect = new;
        let mana_delta = crate::gear::INTELLECT_TO_MANA_PER_POINT * (new - old);
        self.stats.max_mana += mana_delta;
        self.mana += mana_delta;
    }

    pub fn set_cooldown(&mut self, key: &str, duration_ms: i64) {
        self.cooldowns.insert(key.to_string(), self.now_ms + duration_ms);
    }

    /// A one-time item/potion use that restores a flat amount of mana, gated on a mana-percent
    /// ceiling (`1.0` = always available) and usable once per fight (modeled as an effectively
    /// infinite cooldown after use, not a timed one - real potions/trinkets can't be reused
    /// within a single fight either way, so "once" is the correct behavior without needing a
    /// separate "already used" flag). `key` must be unique per item.
    ///
    /// Found 2026-10-25 while chasing Fire Mage's own Fire-Blast-under-cast skew back to its
    /// real cause: the real engine's own debug log shows three "Major cooldown used" one-time
    /// mana items in *every* "builder starter" Mage fixture checked so far (Fire and Arcane
    /// both carry `ItemID: 8008`/`+1100` and `ItemID: 13444`/`+1800`, a mana potion) totaling
    /// more than half this build's own max mana pool - previously left entirely unmodeled under
    /// this project's own "no mid-fight consumable-use modeling" scope note, which was correct
    /// for *random, player-discretion* item use but not for these two: `ItemID: 8008`'s own
    /// real priority-list entry (Fire's fixture) gives an explicit, sourced trigger condition
    /// (`currentManaPercent <= 80%`), and the potion's own amount is exact and identical across
    /// both fixtures checked, even though its own real trigger timing isn't explicitly
    /// specified in either request JSON (modeled here as "available from the start", the same
    /// unconditional semantics the real engine's own `autocastOtherCooldowns{}` action uses
    /// when it carries no explicit condition at all - Arcane's own fixture's position 0).
    /// A third one-time item seen in the same logs (`ItemID: 14152`, a worn trinket) was *not*
    /// added - its observed amount differs between the two fixtures (593.8 vs 547.2) in a way
    /// that doesn't resolve to a clean flat or percentage value from one sample each, suggesting
    /// a randomized "restore X to Y mana" range this project doesn't have enough samples to
    /// source confidently - left open rather than guessed at.
    pub fn maybe_use_one_time_mana_item(&mut self, key: &str, mana_percent_ceiling: f64, amount: f64) -> bool {
        if self.is_on_cooldown(key) || self.mana_percent() > mana_percent_ceiling {
            return false;
        }
        self.add_mana(amount);
        self.set_cooldown(key, i64::MAX / 2);
        true
    }

    /// A one-time, once-per-fight mana cooldown gated on *headroom* (only usable once the full
    /// amount would actually fit under `max_mana`) rather than a mana-percent ceiling - the
    /// real engine's own activation rule for a periodic self-mana-regen effect (its own
    /// `ShouldActivate: max_mana - mana >= amount`, checked once at the moment of casting, not
    /// re-checked per tick), modeled here as one flat grant at that same decision point rather
    /// than genuinely spreading it over its own real 5 ticks/15s - defensible because the real
    /// engine's own gate already commits to "is there room for the *whole* output" up front, so
    /// collapsing the delivery to one instant doesn't change which fights can use it, only
    /// exactly when the mana lands within one. Found 2026-10-26 while addressing the mana
    /// economy behind Priest's own cast-frequency gap: **Dark Sacrifice**, the Undead racial
    /// (`src/classes/priest/spells/dark_sacrifice.rs`, registered "for Undead only" per
    /// `prepare/export.rs`'s own comment - not Priest-specific, any Undead character of any
    /// class gets it) grants `320 + Spirit/5` mana per tick, 5 ticks, 15s duration, a 10-minute
    /// (`CategoryCooldownMs: 600000`) cooldown far exceeding any modeled fight - confirmed
    /// exactly from two independent real traces: total gained (`1600 + Spirit`) matched to the
    /// tenth of a point on both the Shadow (`1839.0` at `Spirit: 239`) and Smite (`1837.0` at
    /// `Spirit: 237`) Priest builds. At ~1600-1850 mana on pools of 6600-6900, this is the
    /// single largest undiscovered mana source found in this project's own "diff every real
    /// `Gained X mana` line" pass across every class so far.
    pub fn maybe_use_headroom_gated_mana_cooldown(&mut self, key: &str, amount: f64) -> bool {
        if self.is_on_cooldown(key) || self.stats.max_mana - self.mana < amount {
            return false;
        }
        self.add_mana(amount);
        self.set_cooldown(key, i64::MAX / 2);
        true
    }

    /// Same as `set_cooldown`, but measured from an explicit starting timestamp rather than
    /// `self.now_ms` - for a cooldown that must start at the moment a spell was *cast*, not
    /// after its own cast time/GCD has *also* elapsed (every real WoW ability's own cooldown
    /// works this way: using a spell starts its cooldown immediately, the GCD that follows is
    /// a separate, shorter lock on taking any *other* action). Added 2026-10-25 after a
    /// per-cast debug log found Fire Blast's own real cast cadence running 1+ GCD slower than
    /// its 8-second cooldown should allow - `casting.rs::cast_direct_full` was calling
    /// `pay_and_advance` (which advances `now_ms` by the cast's own GCD) *before*
    /// `set_cooldown`, silently adding one full GCD to every cooldown-bearing spell project-
    /// wide, not just Fire Blast's own. Fixed by having that caller snapshot `now_ms` before
    /// paying/advancing and passing it here instead.
    pub fn set_cooldown_from(&mut self, key: &str, from_ms: i64, duration_ms: i64) {
        self.cooldowns.insert(key.to_string(), from_ms + duration_ms);
    }

    pub fn set_gcd(&mut self, duration_ms: i64) {
        self.gcd_free_at = self.now_ms + duration_ms;
    }

    pub fn gcd_remaining(&self) -> i64 {
        (self.gcd_free_at - self.now_ms).max(0)
    }
}
