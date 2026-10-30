//! Demonology single-target core rotation. Most of a Demonology Warlock's damage is its pet's
//! (Imp Firebolt, or Succubus Lash of Pain), so the pet runs on the *same* fight timeline as
//! the player (see `run`'s interleaving loop) rather than a separate detached simulation -
//! Demonic Brand needs that: the warlock's own Searing Pain cast has to land before the pet's
//! next hits can spend its charges, which only works if both actors share one clock. The pet
//! mechanics themselves (`PetChoice`, the Firebolt/Lash of Pain formulas, Demonic Brand) live
//! in `warlock/mod.rs` now, shared with `affliction.rs` since both specs can run a pet.
//!
//! **Sourcing, read before trusting the numbers**: the Imp's Firebolt (11763), Succubus's Lash
//! of Pain (11780) and the Demonic Brand hit are all *pet-only* mechanics with no row in the
//! auto-generated client-data table the rest of this pack was mined from (that generator
//! "makes no table for pet spells" - mythicsim-forever-engine's own words). All three numbers
//! (in `mod.rs`) instead come from `src/classes/warlock/prepare/export.rs`, which carries them
//! as literal Go-ported constants with their own provenance comments - genuinely sourced, but
//! from a narrower, less-independently-verified trail than the rest of this pack: none of the
//! three were confirmed against a second site (foreverdb.net 404s for both pet spell IDs -
//! checked 2026-10-23). See `data/warlock-forever-2026-10-23-rev2.json`'s
//! `firebolt`/`lash_of_pain`/`pet_and_talent_mechanics.demonic_brand_hit` entries for the full
//! citations.
//!
//! **Succubus**: `PetChoice::Succubus` simulates Lash of Pain the same way the Imp's Firebolt
//! was already modeled. Her melee auto-attack (95-131 weapon damage, 2.0s swing, scaling with
//! 17% of the warlock's spell power as attack power per `prepare/pets.rs`'s
//! `PET_ATTACK_POWER_FRACTION`) is *not* modeled - that needs a melee hit/crit/weapon-damage
//! system this engine doesn't have for anything else either, so it's a deliberate scope cut,
//! not an oversight. A Succubus-build's real DPS is higher than this engine will show.
//!
//! **Decimation**: Soul Fire's -20%/-40% cast time and -45%/-90% cooldown (talent spell
//! 440870, confirmed via `spell-extras.json`'s curve) are real but have no observable effect
//! here since this rotation doesn't cast Soul Fire (it wasn't in the real priority-list
//! fixture this rotation is translated from - see research-notes/priority-lists.md, and this
//! module stays faithful to that rather than inventing a Soul Fire line). What *is* observable:
//! Shadow Bolt and Searing Pain hit +3%/+6% harder while the target is below 35% health -
//! gated on `Fight::target_health_percent`, which is `None` (never triggers) unless the caller
//! opts in via `Fight::set_target_max_health` (the CLI's `--target-health`).
//!
//! **Cross-tree Destruction talents and pet-brand/Master-Demonologist fixes, added while
//! closing the mythicsim.com DPS gap (2026-10-24)**: mythicsim's own "builder starter" default
//! Demonology character spends real points in Ruin, Improved Shadow Bolt, Bane and Agonizing
//! Flames (none of which are Demonology-tree talents) that buff Shadow Bolt regardless of
//! spec - this module now carries and applies them the same way `destruction.rs` already did.
//! That same reference build also runs a Succubus with Demonic Brand and Master Demonologist
//! both talented; both were previously gated to `PetChoice::Imp` even though the real mechanic
//! supports any pet (the brand hit's school and the Master Demonologist bonus both just follow
//! whichever demon is actually out) - fixed. Also added in that pass: Unholy Power and Soul
//! Link, found while re-checking this build's full talent list for anything else missing.

use crate::sim::{CastResult, Fight, FightReport};
use crate::stats::CasterStats;

use super::{
    cast_direct, cast_direct_and_dot, cast_dot_only, cast_utility, life_tap_mana, pet_cast,
    pet_melee_swing, PetChoice, SacrificedPet, BASE_CRIT_MULTIPLIER, FIREBOLT_COEFFICIENT,
    LASH_OF_PAIN_COEFFICIENT, PET_SPELL_DAMAGE_FRACTION,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct DemonologyTalents {
    pub pet: PetChoice,
    /// Tier 1, 0-3: +10/20/30% Imp Firebolt damage. Only applies when `pet` is `Imp`.
    pub improved_imp: u8,
    /// Tier 3, 0-3: +10/20/30% Succubus Lash of Pain damage. Only applies when `pet` is
    /// `Succubus`. Confirmed via `spell-extras.json`'s curve for spell 18754: `[[10,20,30],[10,20,30]]`.
    pub improved_sayaad: u8,
    /// Tier 6, 0-5: +2%/point to the active pet's own damage - Fire for an Imp, Shadow for a
    /// Succubus (both demons' matching-school halves are modeled; earlier this was gated to
    /// the Imp only, as if the Shadow half for a Succubus didn't exist).
    pub master_demonologist: u8,
    /// Tier 5, 0-3: +33/67/100% of the warlock's level (60) as spell power, for warlock and
    /// demon - i.e. +19.8/+40.2/+60 spell power at this engine's assumed level.
    pub demonic_knowledge: u8,
    /// Tier 4, 0-3: a landed Searing Pain brands the target with 2/4/6 charges for 10s; each
    /// landed pet hit while a charge remains spends one and adds a separate brand hit, Fire or
    /// Shadow depending on which pet is out (previously gated to the Imp only).
    pub demonic_brand: u8,
    /// Tier 4, 0-2: Soul Fire cast/cooldown halves have no effect here (see module docs);
    /// +3%/6% Shadow Bolt and Searing Pain damage while the target is below 35% health does.
    pub decimation: u8,
    /// Affliction tree tier 1, 0-2: +10%/point Life Tap mana returned (see destruction.rs's
    /// field of the same name for why this is on all three Talents structs).
    pub improved_life_tap: u8,
    /// Destruction tree, 0-5: +20%/point crit damage multiplier (see destruction.rs's field of
    /// the same name - this is the same cross-tree talent, applied to the subset of
    /// Destruction-tagged spells this rotation actually casts: Shadow Bolt).
    pub ruin: u8,
    /// Destruction tree, 0-5: a Shadow Bolt crit applies the same 12s "Shadow Vulnerability"
    /// debuff `destruction.rs` tracks, +4%/point to this warlock's own shadow damage -
    /// Shadow Bolt, Corruption, Bane of Agony here.
    pub improved_shadow_bolt: u8,
    /// Destruction tree, 0-5: -100ms/point Shadow Bolt cast time (the only one of Bane's
    /// targets this rotation casts).
    pub bane: u8,
    /// Destruction tree, 0-3: +3/7/10% Shadow Bolt damage (this rotation doesn't cast
    /// Incinerate/Conflagrate/Shadowburn, so only Shadow Bolt sees the direct-damage half;
    /// Searing Pain's crit-chance half still applies when Demonic Brand is talented).
    pub agonizing_flames: u8,
    /// Which demon (if any) was permanently sacrificed for Demonic Sacrifice's school-wide
    /// +15% damage buff - independent of `pet` (see `super::SacrificedPet`'s docs; this is
    /// what lets a Succubus-build Demonology character still carry an Imp's sacrifice bonus,
    /// matching the real "builder starter" reference build this module was checked against).
    pub sacrificed_pet: Option<SacrificedPet>,
    /// Tier 1, 0-5: +2%/point to the active pet's own damage (any demon, not Imp-specific -
    /// confirmed via `apply_unholy_power`'s `for pet in &self.pets.base` in the source).
    pub unholy_power: u8,
    /// Tier 5, 1pt: +3% damage dealt for both the warlock and the active demon (spell 25228,
    /// `BasePoints: 3` on its damage-dealt effect, confirmed directly from the client row).
    pub soul_link: bool,
    /// Affliction tree tier 1, 0-5: +1%/point spell hit chance (spell 18174's own tooltip:
    /// "Improves your chance to hit by 1%" - not Classic's +2%/point). Found missing entirely
    /// while comparing a per-ability damage breakdown against mythicsim.com's own
    /// (2026-10-24); this comparison's own decoded build spends 2 points here.
    pub suppression: u8,
}

/// Decimation's health threshold and bonus (spell 440870) - confirmed via
/// `spell-extras.json`'s curve `[[-20,-40],[-45,-90],[35,35],[3,6]]`: the third effect (35,35,
/// unchanged across both points) is the health-percent threshold, the fourth is the damage
/// bonus.
const DECIMATION_HEALTH_THRESHOLD: f64 = 0.35;

/// src/prepare/character.rs: `pub const CHARACTER_LEVEL: i32 = 60;` - confirmed directly, not
/// inferred (an earlier version of this module guessed 65 from an unrelated field and was
/// wrong; fixed after finding the real constant).
const CHARACTER_LEVEL: f64 = 60.0;

const SHADOW_VULNERABILITY: &str = "shadow_vulnerability";
const SHADOW_VULNERABILITY_DURATION_MS: i64 = 12_000;

fn improved_imp_pct(points: u8) -> f64 {
    0.10 * points as f64
}

fn improved_sayaad_pct(points: u8) -> f64 {
    0.10 * points as f64
}

fn decimation_damage_pct(points: u8) -> f64 {
    match points {
        1 => 0.03,
        2 => 0.06,
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

fn demonic_knowledge_bonus(points: u8) -> f64 {
    let fraction = match points {
        1 => 0.33,
        2 => 0.67,
        3 => 1.00,
        _ => 0.0,
    };
    fraction * CHARACTER_LEVEL
}

fn brand_charges(points: u8) -> i32 {
    match points {
        1 => 2,
        2 => 4,
        3 => 6,
        _ => 0,
    }
}

/// Applies this build's static (non-proc-window) talent modifiers once, before the fight
/// starts. Improved Shadow Bolt is handled live in `step` instead, same as `destruction.rs`.
///
/// Ruin/Bane/Agonizing Flames apply to Immolate too, same as `affliction.rs`'s own
/// `keep_immolate` case - this spec's real priority list casts Immolate unconditionally (see
/// the missing-Immolate fix in `step`), so unlike Affliction there's no flag gating it; these
/// three cross-tree Destruction talents' real class-masks reach it regardless of which spec is
/// casting it.
fn apply_talents(fight: &mut Fight, talents: &DemonologyTalents) {
    if talents.suppression > 0 {
        fight.stats.spell_hit += 0.01 * talents.suppression as f64;
    }
    let m = &mut fight.mods;
    if talents.ruin > 0 {
        let bonus = 0.20 * talents.ruin as f64;
        m.crit_multiplier_bonus.push(("shadow_bolt".to_string(), bonus));
        m.crit_multiplier_bonus.push(("immolate".to_string(), bonus));
    }
    if talents.bane > 0 {
        let flat = -100 * talents.bane as i64;
        m.cast_time_flat_ms.push(("shadow_bolt".to_string(), flat));
        m.cast_time_flat_ms.push(("immolate".to_string(), flat));
    }
    if talents.agonizing_flames > 0 {
        let pct = agonizing_flames_pct(talents.agonizing_flames);
        m.direct_pct.push(("shadow_bolt".to_string(), pct));
        m.direct_pct.push(("immolate".to_string(), pct));
        m.dot_pct.push(("immolate".to_string(), pct));
        m.crit_chance_bonus.push(("searing_pain".to_string(), pct));
    }
    super::apply_demonic_sacrifice(fight, talents.sacrificed_pet);
}

fn refresh_needed(fight: &Fight, spell_key: &str) -> bool {
    let cast_ms =
        fight.modified_cast_ms(spell_key, fight.pack.spell(spell_key).cast_time_ms.unwrap_or(0));
    match fight.dot_remaining_ms(spell_key) {
        None => true,
        Some(remaining) => remaining <= cast_ms,
    }
}

/// Decimation's live damage bonus for Shadow Bolt/Searing Pain: 0 unless a target max health
/// was set (`Fight::set_target_max_health`) and the target is currently below 35%.
fn decimation_bonus(fight: &Fight, talents: &DemonologyTalents) -> f64 {
    if talents.decimation == 0 {
        return 0.0;
    }
    match fight.target_health_percent() {
        Some(pct) if pct < DECIMATION_HEALTH_THRESHOLD => decimation_damage_pct(talents.decimation),
        _ => 0.0,
    }
}

/// Improved Shadow Bolt's live debuff bonus, same mechanic as `destruction.rs::shadow_bonus`.
fn improved_shadow_bolt_bonus(fight: &Fight, talents: &DemonologyTalents) -> f64 {
    if talents.improved_shadow_bolt > 0 && fight.aura_active(SHADOW_VULNERABILITY) {
        0.04 * talents.improved_shadow_bolt as f64
    } else {
        0.0
    }
}

/// The warlock's own casting: Curse of the Elements, dot maintenance (Bane of Agony,
/// Corruption, Immolate), a Searing Pain cast to keep Demonic Brand's charges up when talented
/// (the real priority list's line 11 - see research-notes/priority-lists.md), then Shadow Bolt
/// filler.
pub fn step(fight: &mut Fight, talents: &DemonologyTalents) {
    let crit = fight.stats.spell_crit;
    let mult = BASE_CRIT_MULTIPLIER;
    if fight.mana_percent() < 0.10 {
        if cast_utility(fight, "life_tap") {
            fight.add_mana(life_tap_mana(fight.stats.spirit, talents.improved_life_tap));
            return;
        }
    }
    // Spell 1311680 in the real priority list this rotation is translated from (confirmed
    // directly in the "builder starter" validation fixture's own request JSON, 7th action) -
    // missing here until the mythicsim.com DPS gap investigation found it. Same cast/refresh
    // pattern `affliction.rs`/`destruction.rs` already use.
    if !fight.aura_active("curse_of_the_elements") {
        if cast_utility(fight, "curse_of_the_elements") {
            fight.set_aura("curse_of_the_elements", 300_000);
            return;
        }
    }
    if refresh_needed(fight, "bane_of_agony") {
        if cast_dot_only(fight, "bane_of_agony", "bane_of_agony", crit, mult, 0.0) {
            return;
        }
    }
    if refresh_needed(fight, "corruption") {
        if cast_dot_only(fight, "corruption", "corruption", crit, mult, 0.0) {
            return;
        }
    }
    // Spell 25309 ("Immolate Rank 8") in the real priority list's 9th action - also missing
    // here until the same gap investigation that found the missing Curse of the Elements cast
    // above. Confirmed via the request JSON directly, not assumed from the other two specs.
    if refresh_needed(fight, "immolate") {
        if cast_direct_and_dot(fight, "immolate", "immolate", crit, mult, 0.0, 0.0) {
            return;
        }
    }
    if talents.demonic_brand > 0 {
        let needs_refresh = fight
            .charges_remaining_ms("demonic_brand")
            .map_or(true, |remaining| remaining <= 3500);
        if needs_refresh {
            let bonus = decimation_bonus(fight, talents); // Searing Pain benefits from Decimation too
            if cast_direct(fight, "searing_pain", "searing_pain", crit, mult, bonus).is_some() {
                fight.set_charges(
                    "demonic_brand",
                    brand_charges(talents.demonic_brand),
                    super::BRAND_DURATION_MS,
                );
                return;
            }
        }
    }
    let bonus = decimation_bonus(fight, talents) + improved_shadow_bolt_bonus(fight, talents);
    if let Some(result) = cast_direct(fight, "shadow_bolt", "shadow_bolt", crit, mult, bonus) {
        if talents.improved_shadow_bolt > 0 {
            if let CastResult::Landed { crit: true, .. } = result {
                fight.set_aura(SHADOW_VULNERABILITY, SHADOW_VULNERABILITY_DURATION_MS);
            }
        }
    }
}

/// `target_max_health`, if set, enables Decimation's health-gated bonus (see module docs) via
/// `Fight::set_target_max_health`. `None` leaves it off.
pub fn run(
    stats: CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &DemonologyTalents,
    target_max_health: Option<f64>,
) -> FightReport {
    let pack = crate::spelldata::embedded_warlock_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    if let Some(max_health) = target_max_health {
        fight.set_target_max_health(max_health);
    }
    apply_talents(&mut fight, talents);

    let mut pet_sp_bonus_pct = 0.0;
    if talents.pet == PetChoice::Imp && talents.improved_imp > 0 {
        pet_sp_bonus_pct += improved_imp_pct(talents.improved_imp);
    }
    if talents.pet == PetChoice::Succubus && talents.improved_sayaad > 0 {
        pet_sp_bonus_pct += improved_sayaad_pct(talents.improved_sayaad);
    }
    if talents.master_demonologist > 0 {
        // Both demons' matching-school halves are the same +2%/point - see the talent's own
        // doc comment for why this is no longer gated to the Imp.
        pet_sp_bonus_pct += 0.02 * talents.master_demonologist as f64;
    }
    if talents.unholy_power > 0 {
        // `for pet in &self.pets.base` in the source - every demon, not Imp-specific.
        pet_sp_bonus_pct += 0.02 * talents.unholy_power as f64;
    }
    let pet_ability_key = talents.pet.ability_key();
    // Master Demonologist/Unholy Power are "the active pet's own damage" per their own doc
    // comments above - not spell-specific, so this also covers the Succubus's melee swing
    // (added while closing the mythicsim.com DPS gap's last real piece, 2026-10-24) - a no-op
    // push for the Imp, who has no melee entries to ever match this key.
    fight.mods.direct_pct.push((pet_ability_key.to_string(), pet_sp_bonus_pct));
    fight.mods.direct_pct.push(("succubus_melee".to_string(), pet_sp_bonus_pct));
    if talents.soul_link {
        // +3% damage dealt for "the warlock and the demon" - player's own spells plus the pet,
        // melee included (same reasoning as above).
        for key in ["shadow_bolt", "corruption", "bane_of_agony", "searing_pain", pet_ability_key, "succubus_melee"] {
            fight.mods.direct_pct.push((key.to_string(), 0.03));
            fight.mods.dot_pct.push((key.to_string(), 0.03));
        }
    }
    // Demonic Knowledge raises the warlock's own spell power too (it buffs "the warlock and
    // the demon while it is out"), which also inflates the player's own Shadow Bolt etc. - a
    // real, intended interaction, not a leak between pet and player math.
    if talents.demonic_knowledge > 0 {
        fight.stats.spell_power += demonic_knowledge_bonus(talents.demonic_knowledge);
    }
    // The pet's own share of spell power is computed fresh each hit from `fight.stats`
    // (already includes Demonic Knowledge above) via `resolve_direct_raw`'s sp_coefficient -
    // but that function multiplies the *player's* spell_power, not 10% of it. Scale the
    // coefficient instead of the stat so the player's own casts aren't affected.
    let pet_coefficient = match talents.pet {
        PetChoice::Imp => FIREBOLT_COEFFICIENT * PET_SPELL_DAMAGE_FRACTION,
        PetChoice::Succubus => LASH_OF_PAIN_COEFFICIENT * PET_SPELL_DAMAGE_FRACTION,
    };
    let pet_cycle_ms = talents.pet.cycle_ms();

    let mut player_ready_at = 0i64;
    let mut pet_ready_at = 0i64;
    // Third timeline, added while closing the mythicsim.com DPS gap's last real piece
    // (2026-10-24): the Succubus's own melee auto-attack, on its own 2.0s cadence, independent
    // of her spell-cast cycle above. `i64::MAX` for the Imp, who never melees, so this branch
    // is simply never the earliest and never fires - see `PetChoice::melee_weapon_range`.
    let mut pet_melee_ready_at = if talents.pet.melee_weapon_range().is_some() { 0i64 } else { i64::MAX };

    while !fight.is_over() {
        if player_ready_at <= pet_ready_at && player_ready_at <= pet_melee_ready_at {
            fight.advance(player_ready_at - fight.now_ms);
            let before = fight.now_ms;
            step(&mut fight, talents);
            if fight.now_ms == before {
                // Safety: see destruction.rs::run - nothing could be cast, force progress.
                fight.advance(1000);
            }
            player_ready_at = fight.now_ms;
        } else if pet_ready_at <= pet_melee_ready_at {
            fight.advance(pet_ready_at - fight.now_ms);
            pet_cast(&mut fight, talents.pet, pet_coefficient, talents.demonic_brand);
            // Self-incrementing from the *previous* schedule, not `fight.now_ms + pet_cycle_ms`
            // - found while the melee timeline below drifted badly on the same mistake (fixed
            // here too, 2026-10-24): when this branch's turn is delayed by the player/melee
            // tie-break above, `advance` no-ops (already past due) and the cast fires at
            // whatever `now_ms` currently is - rebasing off `now_ms` would silently lose
            // whatever time it was "behind" by, every single cycle, compounding into a real
            // cast-rate shortfall. A free-running, no-resource-cost timer like this should
            // never lose ground to how often something else preempts it.
            pet_ready_at += pet_cycle_ms;
        } else {
            fight.advance(pet_melee_ready_at - fight.now_ms);
            pet_melee_swing(&mut fight, talents.pet, talents.demonic_brand);
            pet_melee_ready_at += super::DEMON_MELEE_SWING_MS;
        }
    }
    fight.report
}
