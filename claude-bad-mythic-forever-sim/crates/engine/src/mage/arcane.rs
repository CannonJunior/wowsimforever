//! Arcane Mage single-target core rotation, in the order
//! `validation/2026-10-04-builder-starters/arcane-mage--undead.request.json` uses (decoded
//! 2026-10-25): 1. Evocation below 15% mana. 2. Arcane Missiles whenever Missile Barrage is up
//! (free, double tick rate). 3. Frostbolt once the Arcane Blast buff has 3+ stacks or mana
//! drops below 40% (a cheaper filler while waiting on more mana/for the buff to fall off).
//! 4. Arcane Blast otherwise (the default, mana-hungry nuke that builds its own buff).
//!
//! Every mechanic below is read directly from the Rust port's own source, not guessed at from a
//! tooltip summary:
//!
//! - **Arcane Blast** (talent, 1/1): landing adds a stack (cap 4, 15 sec from the *first* stack
//!   - later stacks don't refresh that timer, `Fight::add_aura_stack_no_refresh`'s own doc
//!     comment) of a buff giving 15% of the caster's Intellect as spell damage per stack and
//!     raising Arcane Blast's own mana cost by 175% per stack (`src/classes/mage/spells/
//!     arcane_blast.rs`). **Found to be a standing stat increase, not a self-only bonus
//!     (2026-10-25, during a per-ability damage investigation)**: a real debug log's own
//!     `Dynamic dep updated (SpellDamage += Intellect * 0.15)` lines, firing on every Arcane
//!     Blast stack gain/loss, and Frostbolt's own `SP:` field alternating between 480.0 (no
//!     stacks) and 665.4 (3 stacks: `480 + 3*0.15*412 = 665.4`, exact) over the same fight,
//!     prove this buff raises the caster's *general* SpellDamage stat for as long as any
//!     stack is up - every spell cast during that window benefits (Frostbolt included), not
//!     just Arcane Blast's own next cast. Modeled via the generic `Fight::
//!     register_dynamic_spell_power_aura` (not Arcane-specific - any class/aura with this
//!     "N stacks of X each add a flat Intellect fraction to SpellDamage" shape could reuse it),
//!     replacing the earlier, narrower `cast_direct_with_bonus_power` path that only touched
//!     Arcane Blast's own damage.
//! - **Missile Barrage** (talent, 1/2 in the real decoded build): a landed Arcane Blast has a
//!   20%/40%-at-1/2-points chance (Frostbolt/Fireball/Frost Blast/Arcane Wrath: half that;
//!   Arcane Missiles' own tick: a quarter) to grant a buff making the next Arcane Missiles free
//!   and tick twice as fast (`talents/missile_barrage.rs`) - modeled as a second data-pack
//!   entry (`arcane_missiles_barrage`) rather than a live field override, see that entry's own
//!   `source` note in the data pack.
//! - **Arcane Concentration / Clearcasting** (talent, 5/5): identical mechanism to Fire Mage's
//!   own (`mage/fire.rs`) - reused here rather than duplicated logic, just a separate aura key
//!   so the two specs' own independent `Fight` runs never collide (they never share one, but
//!   keeping the keys spec-specific avoids relying on that).
//! - **Arcane Impact** (talent, 3/3): +2%/point spell crit chance on Arcane-school spells only
//!   (spell 11242's curve `[[2,4,6]]`) - applied as a static crit bonus restricted to this
//!   spec's own Arcane-school abilities via `crit_chance_bonus`'s per-ability keying.
//! - **Arcane Mind** (talent, 5/5): +2%/point total Intellect (spell 11232's first curve
//!   `[[2,4,6,8,10]]`, folded directly into `fight.stats.intellect` once at setup - this is
//!   *why* Intellect needed its own `CasterStats` field, see that field's own doc comment) and
//!   +20%/point crit-damage bonus on Arcane spells (same spell's second curve
//!   `[[20,40,60,80,100]]`, `fraction_at` convention = value/100, applied as
//!   `crit_multiplier_bonus`).
//! - **Arcane Instability** (talent, 3/3): +1%/point damage *and* +1%/point crit chance, both
//!   on every spell (spell 15058's three curves, all `[[1,2,3]]` - cumulative value *at* each
//!   rank, not a per-point delta, confirmed by the raw numbers matching the rank index exactly).
//! - **Arcane Subtlety** (talent, 2/2): +8/15 Spell Piercing at 1/2 points (spell 11210's own
//!   curve `[[-8,-15]]` for its target-resistance effect, stored as the negation -
//!   `register_arcane_subtlety`'s own `sim.add_stat(unit, Stat::SpellPiercing, -value)`).
//!   **Found during a second, deeper root-cause pass (2026-10-25) after being told this
//!   project's comparisons were still "woefully inaccurate" overall**: a prior pass had
//!   assumed this talent only affected threat (its *other* effect, a flat threat% reduction,
//!   genuinely does nothing for a DPS-only engine) and skipped it entirely - missing that its
//!   second effect feeds a real, generic, cross-class damage formula
//!   (`Fight::spell_vulnerability_bonus`, Go `spellVulnerabilityMultiplier`): a caster's own
//!   Spell Piercing past the target's resistance (0 in every validation fixture) adds
//!   `1/(5*target_level)` bonus damage per point - `15/(5*63) = 4.76%`, confirmed by a real
//!   debug log showing *exactly* that consistent bonus on every single cast, unexplained until
//!   this talent's second effect was found.
//! - **Arcane Power** (talent, 1/1): a major cooldown - +30% damage *and* +30% mana cost on
//!   every spell, 15 sec, 180 sec cooldown (spell 12042's own `DurationMs`/`CooldownMs`,
//!   `BasePoints: 30` on both its damage and cost effects - `src/classes/mage/spells/
//!   arcane_power.rs`). **Found during the per-ability comparison below, not assumed from the
//!   talent's name**: this project's first pass had bucketed it with Presence of Mind as an
//!   `autocastOtherCooldowns`-driven cooldown out of scope, the same category Fire Mage's own
//!   module docs use for trinkets - but a real engine debug log showed a clean, exactly-15s
//!   window at the start of the fight where every ability's own `AfterAttackerMods/BaseDamage`
//!   ratio read 1.33 instead of the steady-state 1.03 (Arcane Instability's own +3%) for the
//!   rest of the fight - a flat +30% that only Arcane Power's own numbers explain, confirmed
//!   against its exact `DurationMs`/`BasePoints`. Modeled as an off-GCD, zero-cast-time
//!   self-buff (consistent with `autocastOtherCooldowns` never consuming a normal action slot)
//!   using the same cooldown/timed-aura primitives Fire Mage's own Combustion already uses, not
//!   a new mechanism.
//!
//! Out of scope: Presence of Mind (also `autocastOtherCooldowns`-driven, but only changes cast
//! *time*, not damage - lower-priority than Arcane Power's own clear, measured effect) and the
//! Undead racial Touch of the Grave (still unmodeled project-wide - see
//! `research-notes/mage-fire-comparison.md`).

use crate::casting::{cast_direct_with_cost_pct, cast_dot_only, cast_utility, BASE_CRIT_MULTIPLIER};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct ArcaneTalents {
    /// Tier 3, 1pt: unlocks the Arcane Blast stacking buff (see module docs).
    pub arcane_blast: bool,
    /// Tier 4, 0-2: Missile Barrage's own proc chances (see module docs).
    pub missile_barrage: u8,
    /// Tier 2, 0-5: Clearcasting's own proc chance - same mechanism as Fire Mage's.
    pub arcane_concentration: u8,
    /// Tier 3, 0-3: +2%/point spell crit chance on Arcane spells.
    pub arcane_impact: u8,
    /// Tier 5, 0-5: +2%/point total Intellect, +20%/point Arcane crit-damage bonus.
    pub arcane_mind: u8,
    /// Tier 6, 0-3: +1%/point damage and crit chance, every spell.
    pub arcane_instability: u8,
    /// Tier 7, 1pt: the Arcane Power major cooldown (see module docs).
    pub arcane_power: bool,
    /// Tier 2, 0-2: +8/15 Spell Piercing at 1/2 points (see module docs - this project had
    /// previously, wrongly, assumed this talent only affected threat).
    pub arcane_subtlety: u8,
}

const ARCANE_SPELLS: [&str; 2] = ["arcane_blast", "arcane_missiles"];

const ARCANE_POWER: &str = "arcane_power";
/// Spell 12042's own `DurationMs`.
const ARCANE_POWER_DURATION_MS: i64 = 15_000;
/// Spell 12042's own `CooldownMs`.
const ARCANE_POWER_COOLDOWN_MS: i64 = 180_000;
/// Spell 12042's own `BasePoints: 30` (both its damage and cost effects).
const ARCANE_POWER_DAMAGE_PCT: f64 = 0.30;
const ARCANE_POWER_COST_PCT: f64 = 0.30;

const ARCANE_BLAST_BUFF: &str = "arcane_blast_buff";
const ARCANE_BLAST_MAX_STACKS: u8 = 4;
/// Spell 400573's own `DurationMs`.
const ARCANE_BLAST_DURATION_MS: i64 = 15_000;
/// `arcane_blast.rs`'s own `intellect_per_stack`/`cost_per_stack` (both plain-text-confirmed on
/// foreverdb.net too - see the data pack's `arcane_blast` entry).
const ARCANE_BLAST_INTELLECT_PER_STACK: f64 = 0.15;
const ARCANE_BLAST_COST_PCT_PER_STACK: f64 = 1.75;

/// Arcane Blast's own flat base mana cost - **not** 15% of the caster's own (buffed, gear-
/// dependent) `max_mana`, which a prior pass had wrongly assumed from the client row's literal
/// `CostPct: 15` field and which caused a severe mana-pacing stall (this engine idling ~43% of
/// the fight while the real one never idles at all - confirmed by extracting every cast
/// timestamp from a real debug trace and finding no gap larger than Frostbolt's own 3s cast
/// time anywhere in a 125s fight). Re-derived 2026-10-25 by reading the real engine's own
/// per-cast `Spent X mana from {SpellID: 1239700}` log lines directly: six consecutive samples
/// (three with Arcane Power's own +30% cost active, three without) all solve exactly for one
/// flat base of 181.0, with `cost = base * (1 + 1.75*stacks + 0.30*arcane_power_active)` - the
/// *same* additive-percentage cost-stacking shape this project's own `cost_pct` mechanism
/// already uses elsewhere, just with the wrong base value feeding it. `CostPct`-type costs are
/// a real WoW mechanic (a percentage of the *class/level* base mana table, not the player's
/// own current pool) - 181.0 is this one build's own empirically-confirmed value for it, not
/// independently re-derived from the base-mana-at-level-60 table (close to but not exactly 15%
/// of this pack's own `race_base_stats` `Mana: 1213`, which is identical across every race and
/// so is itself class/level-only - the ~0.5% residual wasn't chased further). Any *future*
/// class/spell with this same cost type should use the identical technique (a real debug trace,
/// not the player's own max_mana) rather than repeating this exact mistake.
const ARCANE_BLAST_BASE_MANA_COST: f64 = 181.0;

const MISSILE_BARRAGE: &str = "missile_barrage";
/// Not a real-sourced number (the real buff's own duration wasn't found in `spells.jsonl` under
/// id 44404 - it may be synthesized at prepare time rather than a direct client row) - long
/// enough to never lapse before this rotation's own very next action consumes it (Arcane
/// Missiles is checked immediately after Evocation, ahead of everything else), which is all
/// that actually matters for this rotation's output. Flagged as an approximation, not a mined
/// value, unlike every duration elsewhere in this module.
const MISSILE_BARRAGE_DURATION_MS: i64 = 20_000;

const CLEARCASTING: &str = "arcane_clearcasting";
/// Spell 12536's own `DurationMs` (same buff Fire Mage's own Clearcasting uses).
const CLEARCASTING_DURATION_MS: i64 = 15_000;
/// Spell 11213's own `ICDMs`.
const CLEARCASTING_ICD_MS: i64 = 1_000;
const CLEARCASTING_ICD_KEY: &str = "arcane_clearcasting_icd";

/// (arcane_blast_chance, missile_chance, bolt_chance) - `missile_barrage.rs`'s own module doc:
/// "Arcane Blast landing (20/40% at 1/2 points)...an Arcane Missiles missile hitting (a quarter
/// of it), or Arcane Wrath, Fireball, Frostbolt or Frostfire Bolt (half of it)".
fn missile_barrage_chances(rank: u8) -> (f64, f64, f64) {
    let arcane_blast_chance = 0.20 * rank as f64;
    (arcane_blast_chance, arcane_blast_chance / 4.0, arcane_blast_chance / 2.0)
}

fn arcane_concentration_chance(points: u8) -> f64 {
    0.02 * points as f64
}

fn arcane_impact_crit_bonus(points: u8) -> f64 {
    0.02 * points as f64
}

/// Applies this build's static talent modifiers once, before the fight starts - mirrors
/// `mage/fire.rs::apply_talents`'s own shape. Arcane Blast's own mana-cost field is
/// deliberately rewritten here too (see the data pack's own `arcane_blast` entry's `source`
/// note on why it starts `null`): 15% of this caster's own max mana, computed once since this
/// engine's `max_mana` doesn't change mid-fight.
pub fn apply_talents(fight: &mut Fight, talents: &ArcaneTalents) {
    if talents.arcane_impact > 0 {
        let pct = arcane_impact_crit_bonus(talents.arcane_impact);
        for key in ARCANE_SPELLS {
            fight.mods.crit_chance_bonus.push((key.to_string(), pct));
        }
    }
    if talents.arcane_mind > 0 {
        let rank = talents.arcane_mind as f64;
        fight.multiply_intellect(1.0 + 0.02 * rank);
        let crit_dmg_bonus = 0.20 * rank;
        for key in ARCANE_SPELLS {
            fight.mods.crit_multiplier_bonus.push((key.to_string(), crit_dmg_bonus));
        }
    }
    if talents.arcane_instability > 0 {
        let rank = talents.arcane_instability as f64;
        let pct = 0.01 * rank;
        fight.stats.spell_crit += pct;
        let m = &mut fight.mods;
        for key in ["arcane_blast", "arcane_missiles", "arcane_missiles_barrage", "frostbolt"] {
            m.direct_pct.push((key.to_string(), pct));
            m.dot_pct.push((key.to_string(), pct));
        }
    }

    if talents.arcane_subtlety > 0 {
        fight.stats.spell_piercing += arcane_subtlety_spell_piercing(talents.arcane_subtlety);
    }

    // Missile Barrage's own self-sustaining proc chain: each individual Arcane Missiles *tick*
    // (not just the cast) independently rolls `missile_chance` to re-trigger the buff -
    // `talents/missile_barrage.rs`'s own `Some("arcane_missiles_tick") if self.missile_chance >
    // 0.0 => self.missile_chance` match arm, confirmed via the real source directly. Found
    // 2026-10-25 while diffing this engine's own mana trajectory against a real one (fewer
    // free Arcane Missiles casts than real meant more cost-bearing Frostbolt/Arcane Blast casts
    // filling the gap, a real contributor to the mana-pacing gap beyond the base-cost/mp5 fixes
    // already landed) - this engine's own generic `register_tick_proc` (already used by
    // Warlock's Nightfall) is the exact right primitive, not a new mechanism.
    if talents.missile_barrage > 0 {
        let (_, missile_chance, _) = missile_barrage_chances(talents.missile_barrage);
        if missile_chance > 0.0 {
            fight.register_tick_proc("arcane_missiles", missile_chance, MISSILE_BARRAGE, MISSILE_BARRAGE_DURATION_MS);
        }
    }

    if let Some(row) = fight.pack.spells.get_mut("arcane_blast") {
        row.mana_cost = Some(ARCANE_BLAST_BASE_MANA_COST);
    }

    // Arcane Blast's own buff raises the caster's general SpellDamage while any stack is up
    // (not just a bonus to Arcane Blast's own next cast) - see the module doc's own citation.
    if talents.arcane_blast {
        fight.register_dynamic_spell_power_aura(ARCANE_BLAST_BUFF, ARCANE_BLAST_INTELLECT_PER_STACK);
    }
}

/// Spell 11210's own curve for its target-resistance effect (`[[-8,-15]]` for ranks 1-2) -
/// stored as `+SpellPiercing` directly (`register_arcane_subtlety`'s own
/// `sim.add_stat(unit, Stat::SpellPiercing, -data...value_at(rank))`).
fn arcane_subtlety_spell_piercing(rank: u8) -> f64 {
    match rank {
        1 => 8.0,
        2 => 15.0,
        _ => 0.0,
    }
}

fn arcane_blast_cost_pct(fight: &Fight) -> f64 {
    let stacks = fight.aura_stacks(ARCANE_BLAST_BUFF) as f64;
    stacks * ARCANE_BLAST_COST_PCT_PER_STACK
}

/// -1.0 (free) if Clearcasting is up, 0.0 otherwise.
fn clearcasting_cost_pct(fight: &Fight) -> f64 {
    if fight.aura_active(CLEARCASTING) {
        -1.0
    } else {
        0.0
    }
}

/// Missile Barrage's own proc roll, shared by every landed trigger spell. `class_spell` is the
/// *ability key* the trigger cast resolved under (`"arcane_blast"`, `"arcane_missiles_tick"` -
/// synthetic, this engine doesn't tick Arcane Missiles separately from the cast, so this key is
/// never actually reached here - `"frostbolt"`).
fn maybe_proc_missile_barrage(fight: &mut Fight, talents: &ArcaneTalents, ability_key: &str, landed: bool) {
    if talents.missile_barrage == 0 || !landed {
        return;
    }
    let (arcane_blast_chance, _missile_chance, bolt_chance) = missile_barrage_chances(talents.missile_barrage);
    let chance = match ability_key {
        "arcane_blast" => arcane_blast_chance,
        "frostbolt" => bolt_chance,
        _ => return,
    };
    if fight.roll_chance(chance) {
        fight.set_aura(MISSILE_BARRAGE, MISSILE_BARRAGE_DURATION_MS);
    }
}

/// Clearcasting's own proc roll and consumption - identical mechanism to Fire Mage's own
/// (`mage/fire.rs::on_fire_cast_resolved`), just this spec's own aura/ICD keys.
fn maybe_proc_clearcasting(fight: &mut Fight, talents: &ArcaneTalents, landed: bool) {
    if landed && fight.aura_active(CLEARCASTING) {
        fight.clear_aura(CLEARCASTING);
    }
    if landed && talents.arcane_concentration > 0 && !fight.is_on_cooldown(CLEARCASTING_ICD_KEY) {
        fight.set_cooldown(CLEARCASTING_ICD_KEY, CLEARCASTING_ICD_MS);
        if fight.roll_chance(arcane_concentration_chance(talents.arcane_concentration)) {
            fight.set_aura(CLEARCASTING, CLEARCASTING_DURATION_MS);
        }
    }
}

/// Arcane Power is off-GCD and consumes no cast time at all (consistent with the real
/// priority list's own `autocastOtherCooldowns` - never a `castSpell` action with its own
/// mana/time cost) - just a direct aura+cooldown activation whenever it's up, checked every
/// `step()` so it fires again the instant its 180s cooldown clears.
fn maybe_activate_arcane_power(fight: &mut Fight, talents: &ArcaneTalents) {
    if !talents.arcane_power || fight.aura_active(ARCANE_POWER) || fight.is_on_cooldown(ARCANE_POWER) {
        return;
    }
    fight.set_aura(ARCANE_POWER, ARCANE_POWER_DURATION_MS);
    fight.set_cooldown(ARCANE_POWER, ARCANE_POWER_COOLDOWN_MS);
}

fn arcane_power_damage_pct(fight: &Fight) -> f64 {
    if fight.aura_active(ARCANE_POWER) {
        ARCANE_POWER_DAMAGE_PCT
    } else {
        0.0
    }
}

fn arcane_power_cost_pct(fight: &Fight) -> f64 {
    if fight.aura_active(ARCANE_POWER) {
        ARCANE_POWER_COST_PCT
    } else {
        0.0
    }
}

/// Formats a `CastResult` the way the real engine's own log does - same shape as
/// `mage/fire.rs::log_result`, duplicated rather than shared since it's a few lines and each
/// spec's own module is meant to stand alone (see this project's "duplicate small things,
/// share real mechanisms" style, already applied to `missile_barrage_chances`).
fn log_result(fight: &mut Fight, ability: &str, result: CastResult) {
    let line = match result {
        CastResult::Landed { crit: true, amount } => format!("{ability} Crit for {amount:.1}"),
        CastResult::Landed { crit: false, amount } => format!("{ability} Hit for {amount:.1}"),
        CastResult::Missed => format!("{ability} Miss"),
        CastResult::CouldNotCast => format!("{ability} CouldNotCast"),
    };
    fight.log_event(&line);
}

pub fn step(fight: &mut Fight, talents: &ArcaneTalents) {
    let mult = BASE_CRIT_MULTIPLIER;
    let crit = fight.stats.spell_crit;

    // Same two real, one-time mana items Fire Mage's own fixture carries explicitly - see
    // `Fight::maybe_use_one_time_mana_item`'s own doc comment for the full citation. Not in
    // this spec's own priority list text (position 0 here is an unconditional
    // `autocastOtherCooldowns{}`), but the real debug log shows the identical `ItemID: 8008`/
    // `ItemID: 13444` "Major cooldown used" lines for this build too - reusing Fire's own
    // sourced mana<=80% condition for 8008 rather than leaving it ungated here.
    fight.maybe_use_one_time_mana_item("mana_item_8008", 0.80, 1100.0);
    fight.maybe_use_one_time_mana_item("mana_potion_13444", 1.0, 1800.0);

    maybe_activate_arcane_power(fight, talents);
    let extra_pct = arcane_power_damage_pct(fight);

    fight.log_event(&format!(
        "decision: mana%={:.1} mana={:.0}/{:.0} mp5={:.1} arcane_blast_stacks={} missile_barrage_active={} arcane_power_active={}",
        fight.mana_percent() * 100.0,
        fight.mana,
        fight.stats.max_mana,
        fight.stats.mp5,
        fight.aura_stacks(ARCANE_BLAST_BUFF),
        fight.aura_active(MISSILE_BARRAGE),
        fight.aura_active(ARCANE_POWER),
    ));

    if fight.mana_percent() < 0.15 && fight.end_ms - fight.now_ms > 20_000 {
        fight.log_event(&format!("evocation attempt at mana%={:.1}", fight.mana_percent() * 100.0));
        if cast_utility(fight, "evocation") {
            fight.add_mana(super::evocation_mana(fight.stats.mp5));
            fight.log_event(&format!("evocation Cast -> mana%={:.1}", fight.mana_percent() * 100.0));
            return;
        }
    }

    if fight.aura_active(MISSILE_BARRAGE) {
        // Arcane Missiles has no direct-hit component (periodic ticks only) and - per the real
        // source's own mask (`ALL_DAMAGING & !ARCANE_MISSILES_TICK`) - doesn't trigger
        // Clearcasting at all, so this just pays/advances and applies the dot, no landed-status
        // bookkeeping needed. Missile Barrage's own aura is consumed by *casting* regardless of
        // outcome (`missile_barrage.rs`'s own `on_cast_complete`, unconditional on landing).
        if cast_dot_only(fight, "arcane_missiles_barrage", "arcane_missiles", crit, mult, extra_pct) {
            fight.log_event("arcane_missiles_barrage Cast (channel, per-tick damage logged separately)");
            fight.clear_aura(MISSILE_BARRAGE);
            return;
        }
    }

    let arcane_blast_stacks = fight.aura_stacks(ARCANE_BLAST_BUFF);
    if arcane_blast_stacks >= 3 || fight.mana_percent() < 0.40 {
        let cost_pct = clearcasting_cost_pct(fight) + arcane_power_cost_pct(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "frostbolt", "frostbolt", crit, mult, extra_pct, cost_pct) {
            log_result(fight, "frostbolt", result);
            let landed = matches!(result, CastResult::Landed { .. });
            maybe_proc_missile_barrage(fight, talents, "frostbolt", landed);
            maybe_proc_clearcasting(fight, talents, landed);
            return;
        }
    }

    if talents.arcane_blast {
        let cost_pct = arcane_blast_cost_pct(fight) + clearcasting_cost_pct(fight) + arcane_power_cost_pct(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "arcane_blast", "arcane_blast", crit, mult, extra_pct, cost_pct) {
            log_result(fight, "arcane_blast", result);
            let landed = matches!(result, CastResult::Landed { .. });
            if landed {
                fight.add_aura_stack_no_refresh(ARCANE_BLAST_BUFF, ARCANE_BLAST_MAX_STACKS, ARCANE_BLAST_DURATION_MS);
            }
            maybe_proc_missile_barrage(fight, talents, "arcane_blast", landed);
            maybe_proc_clearcasting(fight, talents, landed);
        }
    } else {
        let cost_pct = clearcasting_cost_pct(fight) + arcane_power_cost_pct(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "frostbolt", "frostbolt", crit, mult, extra_pct, cost_pct) {
            log_result(fight, "frostbolt", result);
            let landed = matches!(result, CastResult::Landed { .. });
            maybe_proc_missile_barrage(fight, talents, "frostbolt", landed);
            maybe_proc_clearcasting(fight, talents, landed);
        }
    }
}

pub fn run(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &ArcaneTalents,
) -> crate::sim::FightReport {
    run_inner(stats, duration_ms, seed, talents, false).0
}

/// Same as `run`, but also returns a human-readable, timestamped per-decision log - see
/// `mage/fire.rs::run_with_debug_log`'s own doc comment for the full citation (same mechanism,
/// this spec's own entry point).
pub fn run_with_debug_log(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &ArcaneTalents,
) -> (crate::sim::FightReport, Vec<String>) {
    run_inner(stats, duration_ms, seed, talents, true)
}

fn run_inner(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &ArcaneTalents,
    debug_log: bool,
) -> (crate::sim::FightReport, Vec<String>) {
    let pack = crate::spelldata::embedded_mage_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    if debug_log {
        fight.enable_debug_log();
    }
    super::apply_standing_debuffs(&mut fight);
    apply_talents(&mut fight, talents);
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents);
        if fight.now_ms == before {
            fight.advance(1000);
        }
    }
    let log = fight.debug_log.take().unwrap_or_default();
    (fight.report, log)
}
