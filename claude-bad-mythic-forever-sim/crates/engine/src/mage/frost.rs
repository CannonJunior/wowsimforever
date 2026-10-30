//! Frost Mage single-target core rotation, in the order
//! `validation/2026-10-04-builder-starters/frost-mage--undead.request.json` uses (decoded
//! 2026-10-25): 1. Evocation below 20% mana (with 25+ sec left in the fight). 2. Ice Lance
//! whenever Fingers of Frost is up. 3. Arcane Missiles whenever Missile Barrage is up (free,
//! double tick rate - same mechanism as Arcane Mage's, see `mage/arcane.rs`'s own module
//! docs). 4. Frostbolt otherwise (the default filler). A mana-potion item action at the top of
//! the real priority list is out of scope, same bucket as Fire Mage's own Mana Ruby cut.
//!
//! - **Fingers of Frost** (talent, 1/2 in the real decoded build): Frostbolt's own *launch*
//!   (not its landing) has a flat 15% chance (`talents/fingers_of_frost.rs`'s own
//!   `proc_chance` - not rank-scaled; the rank only changes how many charges, 1 vs 2, via
//!   spell 400647's own curve `[[1,2]]`) to grant that many charges, treating the mage's next
//!   consuming cast as hitting a "frozen" target. Ice Lance both consumes a charge *and* gets
//!   the frozen-target bonus; nothing else in this rotation does ("a consuming spell...spends
//!   its charge when it lands", same doc file - Ice Lance is this engine's only instant Frost
//!   direct-damage spell, so "lands" and "casts" are the same moment here).
//! - **Shatter** (talent, 1/3 in the real decoded build, folded into Fingers of Frost in the
//!   real source "the two only ever fire together" - same here): while frozen-charges are up,
//!   +17/33/50% bonus crit chance (spell 11170's curve `[[17,33,50]]`) on every spell, not
//!   just Ice Lance - matching the real source's own `class_mask: masks::ALL`.
//! - **Ice Lance's own 200%-vs-frozen bonus** is *not* talent-gated at all (the base spell's
//!   own tooltip: "Deals 200% increased damage to Frozen targets") - applied as a flat x3.0
//!   multiplier whenever Fingers of Frost's charges are up, independent of Shatter.
//! - **Ice Shards** (talent, 5/5): +20%/point crit-damage bonus on Frost-school spells (spell
//!   11207's curve `[[20,40,60,80,100]]`, `fraction_at` = value/100).
//! - **Improved Frostbolt** (talent, 5/5): -100ms/point Frostbolt cast time (spell 11070's
//!   curve `[[-100,-200,-300,-400,-500]]` - same shape as Fire Mage's own Improved Fireball).
//! - **Elemental Precision** (talent, 3/3): +2%/point spell hit, Fire *and* Frost schools (spell
//!   29438's curve `[[2,4,6]]` - the same talent Fire Mage's own module already models, now
//!   confirmed at its real 3-point max for both specs rather than Fire's own first-pass guess
//!   of 4).
//! - **Missile Barrage** (talent, 1/2 in the real decoded build), **Arcane Concentration /
//!   Clearcasting** (talent, 5/5) and **Arcane Subtlety** (talent, 2/2): three Arcane-tree
//!   talents a Frost build can still take. The first two are identical mechanism to
//!   `mage/arcane.rs`'s own, just this spec's own aura/ICD keys. Arcane Subtlety feeds
//!   `fight.stats.spell_piercing` directly - see `mage/arcane.rs`'s own module docs for the
//!   full real-engine-sourced formula this generic, cross-class mechanic uses
//!   (`Fight::spell_vulnerability_bonus`) and why a prior pass had wrongly assumed this talent
//!   only affected threat.
//!
//! Out of scope: Frost Channeling (mana cost/threat only, no damage effect - same "mp5-only
//! talent" bucket every other spec's own module already excludes), Winter's Chill (this
//! build's own 2/5 points are real, but its only damage effect buffs Cone of Cold, which this
//! rotation never casts - confirmed inert for this specific build, not unmodeled by mistake),
//! Ice Barrier (a defensive shield, no damage), the mana-potion item action, and the Undead
//! racial Touch of the Grave (still unmodeled project-wide).

use crate::casting::{cast_direct_with_cost_pct, cast_dot_only, cast_utility, BASE_CRIT_MULTIPLIER};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct FrostTalents {
    /// Tier 1, 0-5: -100ms/point Frostbolt cast time.
    pub improved_frostbolt: u8,
    /// Tier 1, 0-3: +2%/point spell hit.
    pub elemental_precision: u8,
    /// Tier 2, 0-5: +20%/point crit-damage bonus on Frost spells.
    pub ice_shards: u8,
    /// Tier 5, 0-2: Fingers of Frost's own charge count per proc (see module docs).
    pub fingers_of_frost: u8,
    /// Tier 4, 0-3: bonus crit chance while frozen-charges are up (see module docs).
    pub shatter: u8,
    /// Arcane tree, 0-2: Missile Barrage's own proc chances - same mechanism as
    /// `mage/arcane.rs`'s own talent of the same name.
    pub missile_barrage: u8,
    /// Arcane tree, 0-5: Clearcasting's own proc chance.
    pub arcane_concentration: u8,
    /// Arcane tree, 0-2: Spell Piercing (see `mage/arcane.rs`'s own module docs - this is the
    /// same talent, same mechanic, read by this build too since it's an Arcane-tree talent a
    /// Frost build can still take; this project had previously, wrongly, assumed it only
    /// affected threat).
    pub arcane_subtlety: u8,
}

const FINGERS_OF_FROST: &str = "fingers_of_frost";
/// `fingers_of_frost.rs`'s own flat `proc_chance` - not rank-scaled (the rank only changes
/// `max_stacks`, via spell 400647's curve `[[1,2]]`).
const FINGERS_OF_FROST_PROC_CHANCE: f64 = 0.15;
/// Spell 400669's own `DurationMs` wasn't independently found in this research pass - long
/// enough to never lapse before this rotation's own very next Ice Lance consumes it (checked
/// ahead of Frostbolt in the priority list), the same approximation `mage/arcane.rs` makes for
/// Missile Barrage's duration and for the same reason.
const FINGERS_OF_FROST_DURATION_MS: i64 = 20_000;
/// Ice Lance's own tooltip, not talent-gated: "Deals 200% increased damage to Frozen targets."
const ICE_LANCE_FROZEN_MULTIPLIER: f64 = 3.0;

const MISSILE_BARRAGE: &str = "frost_missile_barrage";
const MISSILE_BARRAGE_DURATION_MS: i64 = 20_000;

const CLEARCASTING: &str = "frost_clearcasting";
const CLEARCASTING_DURATION_MS: i64 = 15_000;
const CLEARCASTING_ICD_MS: i64 = 1_000;
const CLEARCASTING_ICD_KEY: &str = "frost_clearcasting_icd";

/// Same formula as `mage/arcane.rs::missile_barrage_chances` - see that function's own doc
/// comment for the source citation. Duplicated rather than shared (a few lines, not a real
/// mechanism - see this module's own doc comment on why).
fn missile_barrage_chances(rank: u8) -> (f64, f64, f64) {
    let arcane_blast_chance = 0.20 * rank as f64;
    (arcane_blast_chance, arcane_blast_chance / 4.0, arcane_blast_chance / 2.0)
}

fn arcane_concentration_chance(points: u8) -> f64 {
    0.02 * points as f64
}

fn shatter_crit_bonus(points: u8) -> f64 {
    match points {
        1 => 0.17,
        2 => 0.33,
        3 => 0.50,
        _ => 0.0,
    }
}

pub fn apply_talents(fight: &mut Fight, talents: &FrostTalents) {
    if talents.elemental_precision > 0 {
        fight.stats.spell_hit += 0.02 * talents.elemental_precision as f64;
    }
    if talents.improved_frostbolt > 0 {
        fight
            .mods
            .cast_time_flat_ms
            .push(("frostbolt".to_string(), -100 * talents.improved_frostbolt as i64));
    }
    if talents.ice_shards > 0 {
        let pct = 0.20 * talents.ice_shards as f64;
        for key in ["frostbolt", "ice_lance"] {
            fight.mods.crit_multiplier_bonus.push((key.to_string(), pct));
        }
    }
    if talents.arcane_subtlety > 0 {
        fight.stats.spell_piercing += match talents.arcane_subtlety {
            1 => 8.0,
            2 => 15.0,
            _ => 0.0,
        };
    }

    // Missile Barrage's own self-sustaining proc chain (each individual Arcane Missiles tick
    // independently rolls a chance to re-trigger the buff) - see `mage/arcane.rs`'s own
    // `apply_talents` for the full real-source citation; same mechanic, this spec's own key.
    if talents.missile_barrage > 0 {
        let (_, missile_chance, _) = missile_barrage_chances(talents.missile_barrage);
        if missile_chance > 0.0 {
            fight.register_tick_proc("arcane_missiles", missile_chance, MISSILE_BARRAGE, MISSILE_BARRAGE_DURATION_MS);
        }
    }
}

fn frozen_active(fight: &Fight) -> bool {
    fight.charges_remaining(FINGERS_OF_FROST) > 0
}

/// -1.0 (free) if Clearcasting is up, 0.0 otherwise.
fn clearcasting_cost_pct(fight: &Fight) -> f64 {
    if fight.aura_active(CLEARCASTING) {
        -1.0
    } else {
        0.0
    }
}

fn maybe_proc_missile_barrage(fight: &mut Fight, talents: &FrostTalents, ability_key: &str, landed: bool) {
    if talents.missile_barrage == 0 || !landed {
        return;
    }
    let (_arcane_blast_chance, _missile_chance, bolt_chance) = missile_barrage_chances(talents.missile_barrage);
    if ability_key == "frostbolt" && fight.roll_chance(bolt_chance) {
        fight.set_aura(MISSILE_BARRAGE, MISSILE_BARRAGE_DURATION_MS);
    }
}

fn maybe_proc_clearcasting(fight: &mut Fight, talents: &FrostTalents, landed: bool) {
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

/// Frostbolt's own launch-time Fingers of Frost roll (`fingers_of_frost.rs`'s own `launch`:
/// rolled on a *landed* Frostbolt, independent of Missile Barrage's own trigger on the same
/// cast - both can fire off one Frostbolt).
fn maybe_proc_fingers_of_frost(fight: &mut Fight, talents: &FrostTalents, landed: bool) {
    if talents.fingers_of_frost == 0 || !landed {
        return;
    }
    if fight.roll_chance(FINGERS_OF_FROST_PROC_CHANCE) {
        fight.set_charges(FINGERS_OF_FROST, talents.fingers_of_frost as i32, FINGERS_OF_FROST_DURATION_MS);
    }
}

pub fn step(fight: &mut Fight, talents: &FrostTalents) {
    let mult = BASE_CRIT_MULTIPLIER;

    // Real priority item #0, verbatim: `castSpell itemId 8008, condition currentManaPercent <=
    // 80%` - see `Fight::maybe_use_one_time_mana_item`'s own doc comment for the full citation
    // and the potion (`ItemID: 13444`) alongside it; both appear in this fixture's own real log
    // too.
    fight.maybe_use_one_time_mana_item("mana_item_8008", 0.80, 1100.0);
    fight.maybe_use_one_time_mana_item("mana_potion_13444", 1.0, 1800.0);

    if fight.mana_percent() < 0.20 && fight.end_ms - fight.now_ms > 25_000 {
        if cast_utility(fight, "evocation") {
            fight.add_mana(super::evocation_mana(fight.stats.mp5));
            return;
        }
    }

    if frozen_active(fight) {
        let crit = fight.stats.spell_crit + shatter_crit_bonus(talents.shatter);
        let cost_pct = clearcasting_cost_pct(fight);
        let extra_pct = ICE_LANCE_FROZEN_MULTIPLIER - 1.0;
        if let Some(result) = cast_direct_with_cost_pct(fight, "ice_lance", "ice_lance", crit, mult, extra_pct, cost_pct) {
            if matches!(result, CastResult::Landed { .. }) {
                fight.consume_charge(FINGERS_OF_FROST);
            }
            maybe_proc_clearcasting(fight, talents, matches!(result, CastResult::Landed { .. }));
            return;
        }
    }

    if fight.aura_active(MISSILE_BARRAGE) {
        let crit = fight.stats.spell_crit + if frozen_active(fight) { shatter_crit_bonus(talents.shatter) } else { 0.0 };
        // See `mage/arcane.rs`'s own identical block for why this is `cast_dot_only`, not a
        // direct-hit cast: Arcane Missiles has no direct component and doesn't trigger
        // Clearcasting at all (the real source's own mask excludes it).
        if cast_dot_only(fight, "arcane_missiles_barrage", "arcane_missiles", crit, mult, 0.0) {
            fight.clear_aura(MISSILE_BARRAGE);
            return;
        }
    }

    let crit = fight.stats.spell_crit + if frozen_active(fight) { shatter_crit_bonus(talents.shatter) } else { 0.0 };
    let cost_pct = clearcasting_cost_pct(fight);
    if let Some(result) = cast_direct_with_cost_pct(fight, "frostbolt", "frostbolt", crit, mult, 0.0, cost_pct) {
        let landed = matches!(result, CastResult::Landed { .. });
        maybe_proc_fingers_of_frost(fight, talents, landed);
        maybe_proc_missile_barrage(fight, talents, "frostbolt", landed);
        maybe_proc_clearcasting(fight, talents, landed);
    }
}

pub fn run(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &FrostTalents,
) -> crate::sim::FightReport {
    let pack = crate::spelldata::embedded_mage_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    super::apply_standing_debuffs(&mut fight);
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
