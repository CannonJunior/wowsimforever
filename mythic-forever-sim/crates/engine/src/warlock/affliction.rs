//! Affliction single-target core rotation, in the order
//! `validation/2026-10-06-warlock-multi-target-census/bases/2-targets/affliction-warlock.request.json`
//! used (see research-notes/priority-lists.md), minus consumable/potion/trinket lines:
//!
//! 1. Life Tap below 10% mana. 2. Curse of the Elements if not up. 3. Amplify Curse whenever
//! off its 180s cooldown. 4-7. Keep Bane of Agony, Corruption, Siphon Life (if talented) and
//! Immolate from falling off. 8. Shadow Bolt filler (instant, consuming Nightfall's Shadow
//! Trance, if one is up).
//!
//! **Nightfall, tracked for real as of the 2026-10-23 follow-up pass**: used to not exist in
//! this module at all. It's a real proc now: each Corruption tick has a 2%/4% chance
//! (`spell-extras.json`'s curve for spell 18094: `[[2,4]]`) to grant "Shadow Trance" for 10s
//! (spell 17941's duration), which makes the next Shadow Bolt instant. The real trigger mask
//! (`masks::NIGHTFALL_SPELLS`, read from `src/classes/warlock/prepare/masks.rs`) also includes
//! Drain Life/Drain Soul/Wrack, none of which this engine's Affliction rotation casts, so
//! Corruption is the only source that matters here - not a narrowed approximation, just what
//! this rotation actually casts.
//!
//! **Cross-tree Destruction talents, added while closing the mythicsim.com DPS gap
//! (2026-10-24)**: mythicsim's own "builder starter" default Affliction character spends real
//! points in Ruin, Improved Shadow Bolt, Bane and Agonizing Flames - none of them
//! Affliction-tree talents, but all of them real and all of them buffing this rotation's own
//! Shadow Bolt filler (and, for Ruin/Agonizing Flames, Immolate when `keep_immolate` is set).
//! `AfflictionTalents` didn't have fields for any of these before; the mechanics themselves
//! (the proc-window debuff, the cast-time cut) are the exact same ones `destruction.rs` already
//! had working and tested, just applied to the spells this rotation actually casts.

use super::{
    cast_direct_maybe_instant, cast_direct_and_dot, cast_dot_only, cast_utility, life_tap_mana,
    pet_cast, pet_melee_swing, PetChoice, SacrificedPet, BASE_CRIT_MULTIPLIER, FIREBOLT_COEFFICIENT,
    LASH_OF_PAIN_COEFFICIENT, PET_SPELL_DAMAGE_FRACTION,
};
use crate::sim::{CastResult, Fight, FightReport};

#[derive(Clone, Copy, Debug, Default)]
pub struct AfflictionTalents {
    /// Tier 1, 0-5: Corruption casts -400ms/point faster (confirmed via `spell-extras.json`'s
    /// curve for spell 17810: `[[-400,...,-2000],[2,4,6,8,10]]`) and, in Forever, deals
    /// +2%/point more dot damage (the curve's second effect, also confirmed).
    pub improved_corruption: u8,
    /// Tier 1, 0-2: +10%/point Life Tap mana returned.
    pub improved_life_tap: u8,
    /// Tier 3, 0-2: +5%/point Bane of Agony dot damage.
    pub improved_bane_of_agony: u8,
    /// Tier 3, 0-3: Affliction dots crit for +33/67/100% more.
    pub pandemic: u8,
    /// Tier 4, 0-5: +1%/point shadow crit chance.
    pub malevolence: u8,
    /// Tier 6, 0-5: +2%/point flat damage to all shadow-school spells (this engine's note on
    /// the 16 Oct 2026 beta patch doubling it from 1% is already folded into this constant).
    pub shadow_mastery: u8,
    /// Tier 4, 0-2: a Corruption tick has a 2%/4% chance to make the next Shadow Bolt instant
    /// for 10s. See module docs.
    pub nightfall: u8,
    /// Tier 5: Siphon Life talented (adds it to the dot-maintenance rotation).
    pub siphon_life: bool,
    /// Whether this build also keeps Immolate up (several real Affliction builds do, for the
    /// extra free dot) - the fixture this rotation is based on did.
    pub keep_immolate: bool,
    /// Whether Amplify Curse is talented at all (it's a single-point talent).
    pub amplify_curse: bool,
    /// Destruction tree, 0-5: +20%/point crit damage multiplier, applied to the subset of
    /// Destruction-tagged spells this rotation casts (Shadow Bolt, and Immolate if
    /// `keep_immolate`).
    pub ruin: u8,
    /// Destruction tree, 0-5: a Shadow Bolt crit applies the same 12s "Shadow Vulnerability"
    /// debuff `destruction.rs` tracks, +4%/point to this warlock's own shadow damage - Shadow
    /// Bolt, Corruption, Bane of Agony, Siphon Life here.
    pub improved_shadow_bolt: u8,
    /// Destruction tree, 0-5: -100ms/point Shadow Bolt and Immolate cast time.
    pub bane: u8,
    /// Destruction tree, 0-3: +3/7/10% Shadow Bolt and Immolate (direct hit) damage; Immolate's
    /// dot gets the same percentage separately (see destruction.rs's own Agonizing Flames bug
    /// fix - this module picked up the corrected version from the start).
    pub agonizing_flames: u8,
    /// Which demon (if any) was permanently sacrificed for Demonic Sacrifice's school-wide
    /// +15% damage buff - see `super::SacrificedPet`'s docs.
    pub sacrificed_pet: Option<SacrificedPet>,
    /// Which demon (if any) is actively summoned and simulated alongside the warlock's own
    /// casting - added 2026-10-24 while closing the mythicsim.com DPS gap, see module docs.
    /// `None` keeps this spec's previous no-pet behaviour. The real "builder starter" reference
    /// build this module is checked against keeps a Succubus summoned (not sacrificed) the
    /// whole fight, so `Some(PetChoice::Succubus)` is the fixture-accurate setting - this
    /// struct doesn't gate it to that one choice since nothing about the mechanic requires it.
    pub pet: Option<PetChoice>,
    /// Tier 1, 0-5: +1%/point spell hit chance (spell 18174's own tooltip: "Improves your
    /// chance to hit by 1%" - not Classic's +2%/point). Found missing entirely while comparing
    /// a per-ability damage breakdown against mythicsim.com's own (2026-10-24); this comparison's
    /// own decoded build spends all 5 points here.
    pub suppression: u8,
}

const SHADOW_SPELLS: [&str; 4] = ["shadow_bolt", "corruption", "bane_of_agony", "siphon_life"];
const SHADOW_TRANCE: &str = "shadow_trance";
const SHADOW_TRANCE_DURATION_MS: i64 = 10_000;
const SHADOW_VULNERABILITY: &str = "shadow_vulnerability";
const SHADOW_VULNERABILITY_DURATION_MS: i64 = 12_000;

fn nightfall_chance(points: u8) -> f64 {
    match points {
        1 => 0.02,
        2 => 0.04,
        _ => 0.0,
    }
}

fn agonizing_flames_pct(points: u8) -> f64 {
    match points {
        1 => 0.03,
        2 => 0.07,
        3 => 0.10,
        _ => 0.0,
    }
}

pub fn apply_talents(fight: &mut Fight, talents: &AfflictionTalents) {
    if talents.suppression > 0 {
        fight.stats.spell_hit += 0.01 * talents.suppression as f64;
    }
    if talents.nightfall > 0 {
        fight.register_tick_proc(
            "corruption",
            nightfall_chance(talents.nightfall),
            SHADOW_TRANCE,
            SHADOW_TRANCE_DURATION_MS,
        );
    }
    let m = &mut fight.mods;
    if talents.improved_corruption > 0 {
        m.dot_pct
            .push(("corruption".to_string(), 0.02 * talents.improved_corruption as f64));
        m.cast_time_flat_ms
            .push(("corruption".to_string(), -400 * talents.improved_corruption as i64));
    }
    if talents.improved_bane_of_agony > 0 {
        m.dot_pct.push((
            "bane_of_agony".to_string(),
            0.05 * talents.improved_bane_of_agony as f64,
        ));
    }
    if talents.pandemic > 0 {
        let bonus = match talents.pandemic {
            1 => 0.33,
            2 => 0.67,
            3 => 1.00,
            _ => 0.0,
        };
        for key in ["corruption", "bane_of_agony", "siphon_life"] {
            m.crit_multiplier_bonus.push((key.to_string(), bonus));
        }
    }
    if talents.malevolence > 0 {
        let bonus = 0.01 * talents.malevolence as f64;
        for key in SHADOW_SPELLS {
            m.crit_chance_bonus.push((key.to_string(), bonus));
        }
    }
    if talents.shadow_mastery > 0 {
        let bonus = 0.02 * talents.shadow_mastery as f64;
        for key in SHADOW_SPELLS {
            m.direct_pct.push((key.to_string(), bonus));
            m.dot_pct.push((key.to_string(), bonus));
        }
    }
    if talents.ruin > 0 {
        let bonus = 0.20 * talents.ruin as f64;
        m.crit_multiplier_bonus.push(("shadow_bolt".to_string(), bonus));
        if talents.keep_immolate {
            m.crit_multiplier_bonus.push(("immolate".to_string(), bonus));
        }
    }
    if talents.bane > 0 {
        let flat = -100 * talents.bane as i64;
        m.cast_time_flat_ms.push(("shadow_bolt".to_string(), flat));
        if talents.keep_immolate {
            m.cast_time_flat_ms.push(("immolate".to_string(), flat));
        }
    }
    if talents.agonizing_flames > 0 {
        let pct = agonizing_flames_pct(talents.agonizing_flames);
        m.direct_pct.push(("shadow_bolt".to_string(), pct));
        if talents.keep_immolate {
            m.direct_pct.push(("immolate".to_string(), pct));
            m.dot_pct.push(("immolate".to_string(), pct));
        }
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

/// Amplify Curse's real effect: the next curse/bane cast while its buff is up deals 50% more
/// (mined BasePoints=50 on spell 18288, interpreted as a percent - not independently
/// cross-checked, see README "Known gaps").
const AMPLIFY_CURSE_BONUS: f64 = 0.50;

/// Improved Shadow Bolt's live debuff bonus, same mechanic as `destruction.rs::shadow_bonus`.
fn isb_bonus(fight: &Fight, talents: &AfflictionTalents) -> f64 {
    if talents.improved_shadow_bolt > 0 && fight.aura_active(SHADOW_VULNERABILITY) {
        0.04 * talents.improved_shadow_bolt as f64
    } else {
        0.0
    }
}

pub fn step(fight: &mut Fight, talents: &AfflictionTalents) {
    let crit = fight.stats.spell_crit;
    let mult = BASE_CRIT_MULTIPLIER;

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
    if talents.amplify_curse && !fight.is_on_cooldown("amplify_curse") {
        if cast_utility(fight, "amplify_curse") {
            fight.set_cooldown("amplify_curse", 180_000);
            fight.set_aura("amplify_curse", 30_000);
            return;
        }
    }
    if refresh_needed(fight, "bane_of_agony") {
        let consume_amplify = talents.amplify_curse && fight.aura_active("amplify_curse");
        let bonus = isb_bonus(fight, talents) + if consume_amplify { AMPLIFY_CURSE_BONUS } else { 0.0 };
        let cast = cast_dot_only(fight, "bane_of_agony", "bane_of_agony", crit, mult, bonus);
        if consume_amplify && cast {
            fight.clear_aura("amplify_curse");
        }
        if cast {
            return;
        }
    }
    if refresh_needed(fight, "corruption") {
        let bonus = isb_bonus(fight, talents);
        if cast_dot_only(fight, "corruption", "corruption", crit, mult, bonus) {
            return;
        }
    }
    if talents.siphon_life && refresh_needed(fight, "siphon_life") {
        let bonus = isb_bonus(fight, talents);
        if cast_dot_only(fight, "siphon_life", "siphon_life", crit, mult, bonus) {
            return;
        }
    }
    if talents.keep_immolate && refresh_needed(fight, "immolate") {
        if cast_direct_and_dot(fight, "immolate", "immolate", crit, mult, 0.0, 0.0) {
            return;
        }
    }
    let instant = talents.nightfall > 0 && fight.aura_active(SHADOW_TRANCE);
    let bonus = isb_bonus(fight, talents);
    let cast = cast_direct_maybe_instant(fight, "shadow_bolt", "shadow_bolt", crit, mult, bonus, instant);
    // Nightfall is spent by attempting the cast, hit or miss - the same as the real talent.
    if instant && cast.is_some() {
        fight.clear_aura(SHADOW_TRANCE);
    }
    if talents.improved_shadow_bolt > 0 {
        if let Some(CastResult::Landed { crit: true, .. }) = cast {
            fight.set_aura(SHADOW_VULNERABILITY, SHADOW_VULNERABILITY_DURATION_MS);
        }
    }
}

/// `talents.pet`, if set, runs that demon on the same shared fight timeline as the player (see
/// `demonology.rs::run`'s identical pattern - this spec's real build has no Demonology-tree
/// points, so unlike Demonology there's no pet-damage talent bonus to fold into the coefficient
/// here, just the base Firebolt/Lash of Pain formula).
pub fn run(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &AfflictionTalents,
) -> FightReport {
    let pack = crate::spelldata::embedded_warlock_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    apply_talents(&mut fight, talents);

    let Some(pet) = talents.pet else {
        while !fight.is_over() {
            let before = fight.now_ms;
            step(&mut fight, talents);
            if fight.now_ms == before {
                fight.advance(1000); // safety: see destruction.rs::run for why this guard exists
            }
        }
        return fight.report;
    };

    let pet_coefficient = match pet {
        PetChoice::Imp => FIREBOLT_COEFFICIENT * PET_SPELL_DAMAGE_FRACTION,
        PetChoice::Succubus => LASH_OF_PAIN_COEFFICIENT * PET_SPELL_DAMAGE_FRACTION,
    };
    let pet_cycle_ms = pet.cycle_ms();

    let mut player_ready_at = 0i64;
    let mut pet_ready_at = 0i64;
    // Third timeline, added while closing the mythicsim.com DPS gap's last real piece
    // (2026-10-24): the Succubus's own melee auto-attack, on its own 2.0s cadence - see
    // `demonology.rs::run`'s identical pattern for the full reasoning.
    let mut pet_melee_ready_at = if pet.melee_weapon_range().is_some() { 0i64 } else { i64::MAX };

    while !fight.is_over() {
        if player_ready_at <= pet_ready_at && player_ready_at <= pet_melee_ready_at {
            fight.advance(player_ready_at - fight.now_ms);
            let before = fight.now_ms;
            step(&mut fight, talents);
            if fight.now_ms == before {
                fight.advance(1000); // safety: see destruction.rs::run for why this guard exists
            }
            player_ready_at = fight.now_ms;
        } else if pet_ready_at <= pet_melee_ready_at {
            fight.advance(pet_ready_at - fight.now_ms);
            // Affliction has no Demonic Brand field - no pet build in this spec's real talent
            // tree ever takes it, so 0 charges (a no-op) is always correct here.
            pet_cast(&mut fight, pet, pet_coefficient, 0);
            // Self-incrementing, not `fight.now_ms + pet_cycle_ms` - see
            // `demonology.rs::run`'s identical fix for why (2026-10-24).
            pet_ready_at += pet_cycle_ms;
        } else {
            fight.advance(pet_melee_ready_at - fight.now_ms);
            pet_melee_swing(&mut fight, pet, 0);
            pet_melee_ready_at += super::DEMON_MELEE_SWING_MS;
        }
    }
    fight.report
}
