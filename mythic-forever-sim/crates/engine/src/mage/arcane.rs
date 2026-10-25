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
//!     arcane_blast.rs`). The 15%-Intellect bonus is a *flat* spell-power addition, not a
//!     percentage, so it's applied via `cast_direct_with_bonus_power` rather than the
//!     `extra_pct` parameter every other proc-window bonus in this project uses.
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
//!
//! Out of scope, matching Fire Mage's own precedent for the same category of cut: Presence of
//! Mind and Arcane Power (both `autocastOtherCooldowns`-driven in the real priority list, the
//! same "trinkets and off-GCD cooldown buttons" bucket Fire Mage's own module docs already
//! exclude) and the Undead racial Touch of the Grave (still unmodeled project-wide - see
//! `research-notes/mage-fire-comparison.md`).

use crate::casting::{
    cast_direct_with_bonus_power, cast_direct_with_cost_pct, cast_dot_only, cast_utility, BASE_CRIT_MULTIPLIER,
};
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
}

const ARCANE_SPELLS: [&str; 2] = ["arcane_blast", "arcane_missiles"];

const ARCANE_BLAST_BUFF: &str = "arcane_blast_buff";
const ARCANE_BLAST_MAX_STACKS: u8 = 4;
/// Spell 400573's own `DurationMs`.
const ARCANE_BLAST_DURATION_MS: i64 = 15_000;
/// `arcane_blast.rs`'s own `intellect_per_stack`/`cost_per_stack` (both plain-text-confirmed on
/// foreverdb.net too - see the data pack's `arcane_blast` entry).
const ARCANE_BLAST_INTELLECT_PER_STACK: f64 = 0.15;
const ARCANE_BLAST_COST_PCT_PER_STACK: f64 = 1.75;

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
        fight.stats.intellect *= 1.0 + 0.02 * rank;
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

    if let Some(row) = fight.pack.spells.get_mut("arcane_blast") {
        row.mana_cost = Some(0.15 * fight.stats.max_mana);
    }
}

fn arcane_blast_bonus_power(fight: &Fight) -> f64 {
    let stacks = fight.aura_stacks(ARCANE_BLAST_BUFF) as f64;
    stacks * ARCANE_BLAST_INTELLECT_PER_STACK * fight.stats.intellect
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

pub fn step(fight: &mut Fight, talents: &ArcaneTalents) {
    let mult = BASE_CRIT_MULTIPLIER;
    let crit = fight.stats.spell_crit;

    if fight.mana_percent() < 0.15 && fight.end_ms - fight.now_ms > 20_000 {
        if cast_utility(fight, "evocation") {
            fight.add_mana(super::evocation_mana(fight.stats.mp5));
            return;
        }
    }

    if fight.aura_active(MISSILE_BARRAGE) {
        // Arcane Missiles has no direct-hit component (periodic ticks only) and - per the real
        // source's own mask (`ALL_DAMAGING & !ARCANE_MISSILES_TICK`) - doesn't trigger
        // Clearcasting at all, so this just pays/advances and applies the dot, no landed-status
        // bookkeeping needed. Missile Barrage's own aura is consumed by *casting* regardless of
        // outcome (`missile_barrage.rs`'s own `on_cast_complete`, unconditional on landing).
        if cast_dot_only(fight, "arcane_missiles_barrage", "arcane_missiles", crit, mult, 0.0) {
            fight.clear_aura(MISSILE_BARRAGE);
            return;
        }
    }

    let arcane_blast_stacks = fight.aura_stacks(ARCANE_BLAST_BUFF);
    if arcane_blast_stacks >= 3 || fight.mana_percent() < 0.40 {
        let cost_pct = clearcasting_cost_pct(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "frostbolt", "frostbolt", crit, mult, 0.0, cost_pct) {
            let landed = matches!(result, CastResult::Landed { .. });
            maybe_proc_missile_barrage(fight, talents, "frostbolt", landed);
            maybe_proc_clearcasting(fight, talents, landed);
            return;
        }
    }

    if talents.arcane_blast {
        let bonus_power = arcane_blast_bonus_power(fight);
        let cost_pct = arcane_blast_cost_pct(fight) + clearcasting_cost_pct(fight);
        if let Some(result) =
            cast_direct_with_bonus_power(fight, "arcane_blast", "arcane_blast", crit, mult, 0.0, cost_pct, bonus_power)
        {
            let landed = matches!(result, CastResult::Landed { .. });
            if landed {
                fight.add_aura_stack_no_refresh(ARCANE_BLAST_BUFF, ARCANE_BLAST_MAX_STACKS, ARCANE_BLAST_DURATION_MS);
            }
            maybe_proc_missile_barrage(fight, talents, "arcane_blast", landed);
            maybe_proc_clearcasting(fight, talents, landed);
        }
    } else {
        let cost_pct = clearcasting_cost_pct(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "frostbolt", "frostbolt", crit, mult, 0.0, cost_pct) {
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
    let pack = crate::spelldata::embedded_mage_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    apply_talents(&mut fight, talents);
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents);
        if fight.now_ms == before {
            fight.advance(1000);
        }
    }
    fight.report
}
