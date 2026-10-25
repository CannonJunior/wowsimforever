//! Destruction single-target core rotation, in the exact order
//! `validation/2026-10-06-warlock-multi-target-census/bases/2-targets/destruction-warlock.request.json`
//! used (see research-notes/priority-lists.md), minus consumable/potion/trinket lines this
//! engine doesn't model:
//!
//! 1. Life Tap below 10% mana. 2. Curse of the Elements if not up. 3-5. Keep Bane of Agony,
//! Corruption and Immolate from falling off (refresh when remaining duration <= that spell's
//! own cast time, the standard DoT-clip-avoidance rule the real fixture encodes as
//! `dotRemainingTime <= spellCastTime`). 6. Conflagrate, either to snipe Immolate just before
//! it expires or (with Shadow and Flame) to refresh the Fire buff. 7. Shadowburn whenever off
//! cooldown. 8. Incinerate if talented, else Shadow Bolt filler.
//!
//! **Proc-window talents, tracked for real as of the 2026-10-23 follow-up pass**: Improved
//! Shadow Bolt and Shadow and Flame used to be approximated as flat always-on bonuses once
//! talented. They're now real timed auras with sourced durations
//! (`src/classes/warlock/spells/*.rs`'s triggered-spell rows, read from `data/spells.jsonl`):
//! Shadow Vulnerability (17794) 12s, Shadow and Flame's Fire (426311) and Shadow (1293816)
//! buffs 20s each. Both only benefit "the warlock's own shadow/fire damage" per the source
//! engine's own comments, which this engine applies broadly to every shadow- or fire-school
//! ability it casts (not just the trigger spell itself) - the real scope, not a narrower
//! approximation of it.

use super::{
    cast_direct_and_dot, cast_direct_with_cost_pct, cast_dot_only, cast_utility, life_tap_mana,
    BASE_CRIT_MULTIPLIER,
};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct DestructionTalents {
    /// Tier 3, 0-5: +20%/point crit damage multiplier on Destruction spells.
    pub ruin: u8,
    /// Tier 1, 0-5: a Shadow Bolt crit applies "Shadow Vulnerability" on the target for 12s:
    /// +4%/point to the warlock's own shadow damage against it (confirmed duration from
    /// spell 17794; this engine applies the bonus to every shadow-school spell it casts -
    /// shadow_bolt, corruption, bane_of_agony - while the debuff is up, not just Shadow Bolt).
    pub improved_shadow_bolt: u8,
    /// Tier 6, 0-5: a landed Conflagrate or Shadowburn grants both the Fire (20s) and Shadow
    /// (20s) buffs together - +2%/point to the warlock's own fire or shadow damage
    /// respectively (confirmed durations from spells 426311/1293816).
    pub shadow_and_flame: u8,
    /// Tier 4, 0-3: +3/7/10% Destruction direct damage (and the same to Searing Pain crit).
    pub agonizing_flames: u8,
    /// Tier 5, 0-3: +8/17/25% Conflagrate crit chance.
    pub fire_and_brimstone: u8,
    /// Tier 2, 0-5: +10%/point Immolate's direct impact damage.
    pub aftermath: u8,
    /// Tier 1, 0-5: -100ms/point cast time on Shadow Bolt/Immolate/Incinerate, -400ms/point on
    /// Soul Fire. Confirmed via `spell-extras.json`'s curve for spell 17788:
    /// `[[-100,...,-500],[-400,...,-2000]]`.
    pub bane: u8,
    /// Tier 7: Incinerate unlocked. When true, Incinerate replaces Shadow Bolt as the filler
    /// (and only while Immolate is up, per its own +25%-on-Immolate-target design).
    pub incinerate: bool,
    /// Affliction tree tier 1, 0-2: +10%/point Life Tap mana returned. A tier-1 talent is
    /// legal to pick from any build (no prerequisite points needed) even outside its own tree,
    /// so this field exists on all three Talents structs, not just Affliction's.
    pub improved_life_tap: u8,
    /// Tier 2, 0-3: -3/6/10% mana cost on Destruction spells (Shadow Bolt, Incinerate,
    /// Conflagrate, Shadowburn, Immolate). Not a flat 3%/point - confirmed via
    /// `spell-extras.json`'s curve for spell 17778: `[[-3,-6,-10]]`. Previously undocumented
    /// as "has no effect in a sim that never runs out of mana" - true only when mana isn't
    /// constrained; under a low-mp5 gear-driven build (e.g. `--gear` without `--mp5`) this
    /// now measurably matters, which is why it's implemented rather than left as a no-op.
    pub cataclysm: u8,
    /// Demonology tree, 1pt: which demon was sacrificed for its permanent damage buff (not
    /// which pet is actively summoned - `sacrifice_pet` and this engine's own `pet`-choice-less
    /// Destruction rotation are independent, matching how the real mechanic lets a sacrifice
    /// from before the pull outlive the demon itself). Forever reverses Classic's pairing:
    /// sacrificing the Imp buffs Shadow damage (+15%, spell 18789 "Burning Shadow"),
    /// sacrificing the Succubus buffs Fire (+15%, spell 18791 "Touch of Fire") - confirmed via
    /// both spells' own BasePoints and the source engine's comment calling out the reversal.
    pub sacrificed_pet: Option<super::SacrificedPet>,
    /// Affliction tree tier 1, 0-5: +1%/point spell hit chance (spell 18174's own tooltip:
    /// "Improves your chance to hit by 1%" - not Classic's +2%/point). Found missing entirely
    /// while comparing a per-ability damage breakdown against mythicsim.com's own (2026-10-24):
    /// every one of this project's three "builder starter" comparison builds spends real
    /// points here (2-5), on all three Talents structs for the same reason `improved_life_tap`
    /// is.
    pub suppression: u8,
    /// Affliction tree tier 1, 0-5: +2%/point Corruption dot damage, -400ms/point cast time
    /// (same formula as `affliction.rs`'s field of the same name). This specific comparison's
    /// own decoded Destruction build spends 2 points here - also found missing in the same
    /// pass as `suppression` above.
    pub improved_corruption: u8,
}

const SHADOW_VULNERABILITY: &str = "shadow_vulnerability";
const SHADOW_VULNERABILITY_DURATION_MS: i64 = 12_000;
const SHADOW_AND_FLAME_FIRE: &str = "shadow_and_flame_fire";
const SHADOW_AND_FLAME_SHADOW: &str = "shadow_and_flame_shadow";
const SHADOW_AND_FLAME_DURATION_MS: i64 = 20_000;

// Which school each call site below passes `shadow_bonus()` vs `fire_bonus()` for is the real
// scoping mechanism (shadow_bolt/corruption/bane_of_agony/shadowburn -> shadow school;
// immolate/incinerate -> fire school; Shadowburn is Shadow school despite the name, confirmed
// by the client's own school field, same value as Shadow Bolt).

fn agonizing_flames_pct(points: u8) -> f64 {
    match points {
        1 => 0.03,
        2 => 0.07,
        3 => 0.10,
        _ => 0.0,
    }
}

fn fire_and_brimstone_crit(points: u8) -> f64 {
    match points {
        1 => 0.08,
        2 => 0.17,
        3 => 0.25,
        _ => 0.0,
    }
}

fn cataclysm_cost_pct(points: u8) -> f64 {
    match points {
        1 => -0.03,
        2 => -0.06,
        3 => -0.10,
        _ => 0.0,
    }
}

/// Applies this build's static (non-proc-window) talent modifiers once, before the fight
/// starts. Improved Shadow Bolt and Shadow and Flame are handled live in `step` instead - see
/// module docs.
pub fn apply_talents(fight: &mut Fight, talents: &DestructionTalents) {
    if talents.suppression > 0 {
        fight.stats.spell_hit += 0.01 * talents.suppression as f64;
    }
    let m = &mut fight.mods;
    let destruction_spells = ["shadow_bolt", "incinerate", "conflagrate", "shadowburn", "immolate"];

    if talents.ruin > 0 {
        let bonus = 0.20 * talents.ruin as f64;
        for key in destruction_spells {
            m.crit_multiplier_bonus.push((key.to_string(), bonus));
        }
    }
    if talents.agonizing_flames > 0 {
        // Real mask is `DESTRUCTION_SPELLS & !(IMMOLATE_DOT | HELLFIRE)` for the direct half -
        // Immolate's own *direct* hit is not excluded, only its dot (which gets its own
        // DotDamageDonePct line below). An earlier version of this function left Immolate's
        // direct hit out entirely - found and fixed while closing the mythicsim.com DPS gap.
        let pct = agonizing_flames_pct(talents.agonizing_flames);
        for key in ["shadow_bolt", "incinerate", "conflagrate", "shadowburn", "immolate"] {
            m.direct_pct.push((key.to_string(), pct));
        }
        m.dot_pct.push(("immolate".to_string(), pct));
        m.crit_chance_bonus
            .push(("searing_pain".to_string(), pct));
    }
    if talents.improved_corruption > 0 {
        m.dot_pct
            .push(("corruption".to_string(), 0.02 * talents.improved_corruption as f64));
        m.cast_time_flat_ms
            .push(("corruption".to_string(), -400 * talents.improved_corruption as i64));
    }
    if talents.fire_and_brimstone > 0 {
        m.crit_chance_bonus.push((
            "conflagrate".to_string(),
            fire_and_brimstone_crit(talents.fire_and_brimstone),
        ));
    }
    if talents.aftermath > 0 {
        m.direct_pct
            .push(("immolate".to_string(), 0.10 * talents.aftermath as f64));
    }
    if talents.bane > 0 {
        let points = talents.bane as i64;
        for key in ["shadow_bolt", "immolate", "incinerate"] {
            m.cast_time_flat_ms.push((key.to_string(), -100 * points));
        }
        m.cast_time_flat_ms.push(("soul_fire".to_string(), -400 * points));
    }
    super::apply_demonic_sacrifice(fight, talents.sacrificed_pet);
}

fn refresh_needed(fight: &Fight, spell_key: &str) -> bool {
    let cast_ms = fight.modified_cast_ms(spell_key, fight.pack.spell(spell_key).cast_time_ms.unwrap_or(0));
    match fight.dot_remaining_ms(spell_key) {
        None => true,
        Some(remaining) => remaining <= cast_ms,
    }
}

/// The live (proc-window) bonus for a shadow-school ability: Improved Shadow Bolt's debuff
/// and/or Shadow and Flame's Shadow buff, whichever are currently up.
fn shadow_bonus(fight: &Fight, talents: &DestructionTalents) -> f64 {
    let mut bonus = 0.0;
    if talents.improved_shadow_bolt > 0 && fight.aura_active(SHADOW_VULNERABILITY) {
        bonus += 0.04 * talents.improved_shadow_bolt as f64;
    }
    if talents.shadow_and_flame > 0 && fight.aura_active(SHADOW_AND_FLAME_SHADOW) {
        bonus += 0.02 * talents.shadow_and_flame as f64;
    }
    bonus
}

fn fire_bonus(fight: &Fight, talents: &DestructionTalents) -> f64 {
    if talents.shadow_and_flame > 0 && fight.aura_active(SHADOW_AND_FLAME_FIRE) {
        0.02 * talents.shadow_and_flame as f64
    } else {
        0.0
    }
}

/// Reacts to a resolved cast: Shadow Bolt crit -> Shadow Vulnerability; Conflagrate/Shadowburn
/// landing -> both Shadow and Flame buffs.
fn on_cast_resolved(fight: &mut Fight, talents: &DestructionTalents, ability_key: &str, result: &CastResult) {
    if talents.improved_shadow_bolt > 0 && ability_key == "shadow_bolt" {
        if let CastResult::Landed { crit: true, .. } = result {
            fight.set_aura(SHADOW_VULNERABILITY, SHADOW_VULNERABILITY_DURATION_MS);
        }
    }
    if talents.shadow_and_flame > 0 && (ability_key == "conflagrate" || ability_key == "shadowburn") {
        if matches!(result, CastResult::Landed { .. }) {
            fight.set_aura(SHADOW_AND_FLAME_FIRE, SHADOW_AND_FLAME_DURATION_MS);
            fight.set_aura(SHADOW_AND_FLAME_SHADOW, SHADOW_AND_FLAME_DURATION_MS);
        }
    }
}

/// Performs exactly one action (one cast, or one Life Tap/curse application) and advances the
/// fight clock accordingly. Call in a loop until `fight.is_over()`.
pub fn step(fight: &mut Fight, talents: &DestructionTalents) {
    let crit = fight.stats.spell_crit;
    let mult = BASE_CRIT_MULTIPLIER;
    let cost_pct = cataclysm_cost_pct(talents.cataclysm);

    if fight.mana_percent() < 0.10 {
        if cast_utility(fight, "life_tap") {
            fight.add_mana(life_tap_mana(fight.stats.spirit, talents.improved_life_tap));
            return;
        }
    }
    if !fight.aura_active("curse_of_the_elements") {
        if cast_utility(fight, "curse_of_the_elements") {
            fight.set_aura("curse_of_the_elements", 300_000);
            return;
        }
    }
    if refresh_needed(fight, "bane_of_agony") {
        let bonus = shadow_bonus(fight, talents);
        if cast_dot_only(fight, "bane_of_agony", "bane_of_agony", crit, mult, bonus) {
            return;
        }
    }
    if refresh_needed(fight, "corruption") {
        let bonus = shadow_bonus(fight, talents);
        if cast_dot_only(fight, "corruption", "corruption", crit, mult, bonus) {
            return;
        }
    }
    if refresh_needed(fight, "immolate") {
        let bonus = fire_bonus(fight, talents);
        if cast_direct_and_dot(fight, "immolate", "immolate", crit, mult, bonus, cost_pct) {
            return;
        }
    }
    let immolate_up = fight.dot_remaining_ms("immolate").is_some();
    if immolate_up && !fight.is_on_cooldown("conflagrate") {
        let falling_off = fight.dot_remaining_ms("immolate").unwrap() < 4000;
        let wants_fire_buff_refresh =
            talents.shadow_and_flame > 0 && !fight.aura_active(SHADOW_AND_FLAME_FIRE);
        if falling_off || wants_fire_buff_refresh {
            let bonus = fire_bonus(fight, talents);
            if let Some(result) =
                cast_direct_with_cost_pct(fight, "conflagrate", "conflagrate", crit, mult, bonus, cost_pct)
            {
                on_cast_resolved(fight, talents, "conflagrate", &result);
                return;
            }
        }
    }
    {
        let bonus = shadow_bonus(fight, talents);
        if let Some(result) =
            cast_direct_with_cost_pct(fight, "shadowburn", "shadowburn", crit, mult, bonus, cost_pct)
        {
            on_cast_resolved(fight, talents, "shadowburn", &result);
            return;
        }
    }
    if talents.incinerate && immolate_up {
        let bonus = fire_bonus(fight, talents);
        if cast_direct_with_cost_pct(fight, "incinerate", "incinerate", crit, mult, bonus, cost_pct).is_some() {
            return;
        }
    }
    if talents.incinerate {
        let bonus = fire_bonus(fight, talents);
        cast_direct_with_cost_pct(fight, "incinerate", "incinerate", crit, mult, bonus, cost_pct);
    } else {
        let bonus = shadow_bonus(fight, talents);
        if let Some(result) =
            cast_direct_with_cost_pct(fight, "shadow_bolt", "shadow_bolt", crit, mult, bonus, cost_pct)
        {
            on_cast_resolved(fight, talents, "shadow_bolt", &result);
        }
    }
}

pub fn run(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &DestructionTalents,
) -> crate::sim::FightReport {
    let pack = crate::spelldata::embedded_warlock_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    apply_talents(&mut fight, talents);
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents);
        if fight.now_ms == before {
            // Nothing could be cast (e.g. an unusually small mana pool starving every option
            // between the Life Tap threshold and the cheapest spell's cost) - force progress
            // rather than loop forever. See demonology.rs's module history for why this guard
            // exists: an earlier build of that spec's rotation really did hang this way.
            fight.advance(1000);
        }
    }
    fight.report
}
