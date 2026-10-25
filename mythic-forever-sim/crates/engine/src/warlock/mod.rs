//! Warlock spec rotations. Each spec module (`destruction`, `affliction`, `demonology`)
//! implements the *exact* single-target priority order mined from mythicsim-forever-engine's
//! own validation fixtures (see research-notes/ for the extracted priority lists), translated
//! from its declarative condition tree into plain Rust `if`/`else`. Utility lines that fixture
//! had (potion use, "autocastOtherCooldowns", a non-Warlock-core trinket proc) are left out -
//! this engine's scope is Warlock ability damage, not consumables or gear procs.

pub mod affliction;
pub mod demonology;
pub mod destruction;

use crate::sim::{CastResult, Fight};

/// Generic cast-resolution helpers (mana/cast-time/cooldown handling) moved to
/// `crate::casting` once Mage needed the exact same mechanisms (2026-10-25) - re-exported here
/// so every existing Warlock spec file's `use super::{...}` keeps working unchanged.
pub use crate::casting::{
    cast_direct, cast_direct_and_dot, cast_direct_maybe_instant, cast_direct_with_cost_pct,
    cast_dot_only, cast_utility, BASE_CRIT_MULTIPLIER,
};

/// Life Tap's base amount at this rank (spell id 11689, BasePoints=420), from the mined
/// client row - but the real formula (`sim/warlock/lifetap.go` in wowsims/forever, the real
/// upstream of the engine everything else was mined from, read directly in the fourth
/// research/implementation pass) is `(420 + Spirit) * (1 + ImprovedLifeTap%)`, converting that
/// much *health* to mana 1:1 before the Improved Life Tap multiplier. An earlier version of
/// this engine used a flat 420 with no Spirit term - fixed; see `life_tap_mana`.
pub const LIFE_TAP_BASE: f64 = 420.0;

/// Improved Life Tap: +10%/point (2 points), confirmed via `spell-extras.json`'s curve for
/// spell 18182: `[[10,20]]`. `spirit` is the caster's current Spirit stat - see `LIFE_TAP_BASE`'s
/// docs for the real formula this implements.
pub fn life_tap_mana(spirit: f64, improved_life_tap_points: u8) -> f64 {
    (LIFE_TAP_BASE + spirit) * (1.0 + 0.10 * improved_life_tap_points as f64)
}

/// Which demon was permanently sacrificed for Demonic Sacrifice's damage buff - independent of
/// which pet (if any) is actively simulated, matching the real mechanic (a sacrifice from
/// before the pull outlives the demon; Demonic Pact even lets it persist under a *different*
/// active pet). Forever reverses Classic's school pairing - see `DEMONIC_SACRIFICE_PCT`'s docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SacrificedPet {
    Imp,
    Succubus,
}

impl SacrificedPet {
    pub fn school(self) -> crate::stats::School {
        match self {
            SacrificedPet::Imp => crate::stats::School::Shadow,
            SacrificedPet::Succubus => crate::stats::School::Fire,
        }
    }
}

/// +15%, confirmed from the client rows directly: spell 18789 "Burning Shadow" (Imp's
/// sacrifice, despite the Fire-sounding demon) and 18791 "Touch of Fire" (Succubus's), both
/// `BasePoints: 15`. The source engine's own comment calls out that "Forever's pairing is the
/// reverse of Classic's" - not a mistake in this reading, a real rebalance.
pub const DEMONIC_SACRIFICE_PCT: f64 = 0.15;

/// Every Shadow-school spell any of this project's three rotations cast, for applying a
/// school-wide bonus (Demonic Sacrifice) without each spec needing its own copy of the list -
/// an entry for an ability a given spec never casts is simply inert (the mods map just never
/// matches that key).
pub const SHADOW_SCHOOL_SPELLS: [&str; 5] =
    ["shadow_bolt", "corruption", "bane_of_agony", "siphon_life", "shadowburn"];
/// Same, for Fire.
pub const FIRE_SCHOOL_SPELLS: [&str; 4] = ["immolate", "incinerate", "conflagrate", "searing_pain"];

/// Pushes Demonic Sacrifice's +15% onto every spell of the sacrificed pet's school, both
/// direct and dot (so Immolate's tick and its direct hit both get it, same as a real
/// `DamageDoneFlat`-style modifier would apply to each independently).
pub fn apply_demonic_sacrifice(fight: &mut Fight, sacrificed: Option<SacrificedPet>) {
    let Some(pet) = sacrificed else { return };
    let spells: &[&str] = match pet.school() {
        crate::stats::School::Shadow => &SHADOW_SCHOOL_SPELLS,
        crate::stats::School::Fire => &FIRE_SCHOOL_SPELLS,
        crate::stats::School::Arcane | crate::stats::School::Frost => {
            unreachable!("a sacrificed Warlock pet is always Fire or Shadow school")
        }
    };
    for key in spells {
        fight.mods.direct_pct.push((key.to_string(), DEMONIC_SACRIFICE_PCT));
        fight.mods.dot_pct.push((key.to_string(), DEMONIC_SACRIFICE_PCT));
    }
}

/// Which demon (if any) is actively simulated as a second damage source alongside the
/// warlock's own casting. Shared between `demonology.rs` (which always has one) and
/// `affliction.rs` (which, added while closing the mythicsim.com DPS gap, now can too - the
/// real "builder starter" Affliction reference build keeps a Succubus out the whole fight,
/// and until this addition this engine simulated zero damage from her).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PetChoice {
    #[default]
    Imp,
    Succubus,
}

impl PetChoice {
    /// The school of that pet's one damage ability (Firebolt is Fire, Lash of Pain is Shadow)
    /// - also the school Demonic Brand's hit takes when that pet lands it (1293698 Fire for
    /// the Imp, 1293697 Shadow for "the other demons", per `talents_demonology.go`/export.rs).
    pub fn school(self) -> crate::stats::School {
        match self {
            PetChoice::Imp => crate::stats::School::Fire,
            PetChoice::Succubus => crate::stats::School::Shadow,
        }
    }

    pub fn ability_key(self) -> &'static str {
        match self {
            PetChoice::Imp => "imp_firebolt",
            PetChoice::Succubus => "lash_of_pain",
        }
    }

    /// Time between casts: the Imp's ~2.4s Firebolt cadence (2000ms cast + 400ms pause,
    /// `prepare/pets.rs`), or the Succubus's 12s Lash of Pain cooldown.
    pub fn cycle_ms(self) -> i64 {
        match self {
            PetChoice::Imp => FIREBOLT_CAST_MS + FIREBOLT_PAUSE_MS,
            PetChoice::Succubus => LASH_OF_PAIN_COOLDOWN_MS,
        }
    }

    /// The demon's own melee weapon damage range - `petAutoAttacks` in
    /// `src/classes/warlock/prepare/pets.rs`, read directly from a local clone of
    /// `sage3648/mythicsim-forever-engine` (2026-10-24, closing the mythicsim.com DPS gap's
    /// last real piece): the Imp genuinely has none (`Summon::Imp => None` in that source, she
    /// never melees - only casts Firebolt), the Succubus's is 95.0-131.0.
    pub fn melee_weapon_range(self) -> Option<(f64, f64)> {
        match self {
            PetChoice::Imp => None,
            PetChoice::Succubus => Some((95.0, 131.0)),
        }
    }

    /// Unreachable for the Imp (`melee_weapon_range` is `None`, so `pet_melee_swing` returns
    /// before ever using this) - exists only so the match above doesn't need a wildcard arm.
    fn melee_ability_key(self) -> &'static str {
        match self {
            PetChoice::Imp => "imp_melee_unused",
            PetChoice::Succubus => "succubus_melee",
        }
    }
}

/// Demon melee swing speed - confirmed empirically against this engine's own combat log for
/// the mythicsim.com comparison fixture (exactly 60.4 swings over a 120s fight, both builds
/// checked) and consistent with every classic-style one-handed demon weapon.
pub const DEMON_MELEE_SWING_MS: i64 = 2000;

/// `wowsims/forever`'s own `sim/core/character_constants.go`/this project's Rust-port clone's
/// `src/prepare/character.rs`: 14 attack power per 1 DPS, the standard conversion
/// `Weapon::CalculateWeaponDamage` (`src/core/fight/melee.rs`) uses - confirmed directly from
/// source, not the Classic-wide "14" figure assumed without checking.
const ATTACK_POWER_PER_DPS: f64 = 14.0;

/// `PET_ATTACK_POWER_FRACTION` in `prepare/pets.rs`: 17% of the warlock's own spell power
/// becomes the demon's inherited attack power - already named in this project's docs before
/// now, but never actually defined since nothing used it until this melee implementation.
const PET_ATTACK_POWER_FRACTION: f64 = 0.17;

/// The Succubus's own innate attack power from her fixed Strength/Agility (129/85,
/// `prepare/pets.rs`'s `base_stats`), independent of her owner. The exact Strength/Agility-to-
/// AP conversion formula wasn't found in the time available to find it; this is instead read
/// directly off this engine's own debug combat log for the mythicsim.com comparison fixture
/// ("Pet stats" AttackPower 334.16 minus "Pet inherited stats" AttackPower 76.16 = 258.0, the
/// remainder being hers alone) - same confidence tier as this project's other pet constants
/// already flagged as "thinner sourcing than the main pack" (a real number read from the real
/// engine's own output, not independently re-derived from a general formula).
const SUCCUBUS_BASE_ATTACK_POWER: f64 = 258.0;

/// Glancing blow mechanics for a pet (level 60) swinging at a level-63 target: a defense-skill
/// deficit of 15 (5 skill/level x 3 levels), landing on the highest band of
/// `src/prepare/attack.rs`'s own level-indexed table (`unit_level_f64(level, 0.0, 0.10, 0.20,
/// 0.30, 0.40)` for the chance, `(..., 0.95, 0.95, 0.95, 0.85, 0.65)` for the multiplier,
/// `(..., 0.04, 0.04, 0.04, 0.05, 0.10)` for its spread) - confirmed directly from that source,
/// then cross-checked twice against this engine's own combat log (two different builds):
/// measured glance rate 40.1-40.2%, measured average glance damage 65.0% of a normal hit's
/// average, both exact matches.
const GLANCE_CHANCE: f64 = 0.40;
const GLANCE_MULTIPLIER: f64 = 0.65;
const GLANCE_SPREAD: f64 = 0.10;

/// Melee crit deals double damage - confirmed via this engine's own combat log (measured crit
/// average / measured normal-hit average = 2.001, both builds checked).
const MELEE_CRIT_MULTIPLIER: f64 = 2.0;

/// A level-63 target's own dodge chance against a level-60 attacker - measured directly from
/// this engine's own combat log for the comparison fixture (~6.5%, both builds checked); this
/// project didn't trace it back to the target config's own raw dodge stat in the time
/// available, same confidence tier as `SUCCUBUS_BASE_ATTACK_POWER` above.
const TARGET_DODGE_CHANCE: f64 = 0.065;

/// Base melee miss chance vs a level-63 target before any melee hit bonus - the +3-level row of
/// `src/prepare/attack.rs`'s own table (`unit_level_f64(level, 0.04, 0.05, 0.055, 0.06, 0.08)`).
const BASE_MELEE_MISS_CHANCE: f64 = 0.06;

/// The real base chance every character has before gear (`gear.rs`'s own constant of the same
/// value) - duplicated here rather than exported across modules, matching this project's usual
/// style for small cross-referenced constants. Needed below to recover just the gear/talent
/// *bonus* portion of `spell_hit`, which is what the real melee-hit conversion actually wants.
const BASE_SPELL_HIT_VS_LEVEL_63_PERCENT: f64 = 0.83;

/// Forever's own melee-hit conversion for a demon (`DEMON_MELEE_HIT_PER_SPELL_HIT` in
/// `src/classes/warlock/prepare/pets.rs`, confirmed directly): 8/17ths of the owner's own
/// *gear/talent* spell hit bonus becomes melee hit - not the full `spell_hit` stat, which
/// already has the base chance every character has before gear baked in (see
/// `BASE_SPELL_HIT_VS_LEVEL_63_PERCENT` above); that base was never meant to feed this
/// mechanic, so it's subtracted back out first.
const DEMON_MELEE_HIT_PER_SPELL_HIT: f64 = 8.0 / 17.0;

/// Demon pets' own base physical crit, additive on top of the owner's inherited spell crit
/// (`PhysicalCritPercent = owner's SpellCritPercent` in the real source, confirmed directly) -
/// measured as the residual from this engine's own combat log (18.312% total physical crit -
/// 14.062% inherited from the owner = 4.25%), not independently re-derived from an
/// Agility/rating formula in the time available - same confidence tier as
/// `SUCCUBUS_BASE_ATTACK_POWER` above.
const SUCCUBUS_BASE_PHYSICAL_CRIT: f64 = 0.0425;

/// One melee swing from `pet`, if it has one (the Imp never melees - see
/// `PetChoice::melee_weapon_range`). Runs on its own 2.0s timeline, separate from `pet_cast`'s
/// spell-cast cadence - see `demonology.rs`/`affliction.rs`'s `run()` for the three-way
/// interleave (player GCDs, pet spell casts, pet melee swings). A landed swing can also spend
/// a Demonic Brand charge, the same as a landed spell hit already can - this engine's combat
/// log showed melee swings are a real, large source of brand triggers (181267 swings vs. 31435
/// Lash of Pain casts for the same pet in the comparison fixture), not just Lash of Pain.
pub fn pet_melee_swing(fight: &mut Fight, pet: PetChoice, brand_points: u8) {
    let Some((min, max)) = pet.melee_weapon_range() else {
        return;
    };
    let attack_power = SUCCUBUS_BASE_ATTACK_POWER + fight.stats.spell_power * PET_ATTACK_POWER_FRACTION;
    let ap_bonus = (DEMON_MELEE_SWING_MS as f64 / 1000.0) * attack_power / ATTACK_POWER_PER_DPS;
    let hit_bonus = (fight.stats.spell_hit - BASE_SPELL_HIT_VS_LEVEL_63_PERCENT).max(0.0)
        * DEMON_MELEE_HIT_PER_SPELL_HIT;
    let miss_chance = (BASE_MELEE_MISS_CHANCE - hit_bonus).max(0.0);
    let crit_chance = fight.stats.spell_crit + SUCCUBUS_BASE_PHYSICAL_CRIT;
    let result = fight.resolve_melee_swing(
        pet.melee_ability_key(),
        min,
        max,
        ap_bonus,
        miss_chance,
        TARGET_DODGE_CHANCE,
        GLANCE_CHANCE,
        GLANCE_MULTIPLIER,
        GLANCE_SPREAD,
        crit_chance,
        MELEE_CRIT_MULTIPLIER,
    );
    if brand_points > 0 && matches!(result, CastResult::Landed { .. }) && fight.consume_charge("demonic_brand") {
        let bonus_power = match pet.school() {
            crate::stats::School::Fire => fight.stats.bonus_fire_power,
            crate::stats::School::Shadow => fight.stats.bonus_shadow_power,
            crate::stats::School::Arcane | crate::stats::School::Frost => {
                unreachable!("a Warlock pet is always Fire or Shadow school")
            }
        };
        fight.resolve_direct_raw(
            "demonic_brand_hit", BRAND_MIN_DAMAGE, BRAND_MAX_DAMAGE, BRAND_COEFFICIENT,
            fight.stats.spell_crit, BASE_CRIT_MULTIPLIER, false, bonus_power,
        );
    }
}

/// `impFireboltEffect` (export.rs), "client 1.60.1.70094 11763": `Effect{base_points:44.0,
/// ppl:0.6, variance:0.11363636702, spell_level:58, max_level:63}`. `Effect::average(60)` -
/// `floor(44.0 + (60-58)*0.6)` - is 45.0; this engine bakes that result in rather than
/// reimplementing the generic level-scaling formula for one spell.
const FIREBOLT_AVERAGE: f64 = 45.0;
const FIREBOLT_VARIANCE: f64 = 0.11363636702;
/// prepare/pets.rs, independently read.
pub const FIREBOLT_COEFFICIENT: f64 = 0.571;
const FIREBOLT_CAST_MS: i64 = 2000;
const FIREBOLT_PAUSE_MS: i64 = 400;

/// export.rs: "pets.go registerLashOfPainSpell: a Go literal base" = 50.0, flat (no variance
/// shown in source). Coefficient 0.429 and cooldown 12s from `prepare/pets.rs`.
const LASH_OF_PAIN_BASE: f64 = 50.0;
/// prepare/pets.rs, independently read.
pub const LASH_OF_PAIN_COEFFICIENT: f64 = 0.429;
const LASH_OF_PAIN_COOLDOWN_MS: i64 = 12_000;

/// export.rs applyDemonicBrand: `level_bonus = (CHARACTER_LEVEL - 26) * 1.5` = 51.0 at level
/// 60, so min/max = 65.0/68.0. Coefficient 0.078. "The hit's roll and spell power share are Go
/// literals" per that file's own comment.
const BRAND_MIN_DAMAGE: f64 = 65.0;
const BRAND_MAX_DAMAGE: f64 = 68.0;
const BRAND_COEFFICIENT: f64 = 0.078;
pub const BRAND_DURATION_MS: i64 = 10_000;

/// The pet's 10% share of the warlock's own spell power - `prepare/pets.rs`'s
/// `PET_SPELL_DAMAGE_FRACTION`, real value.
pub const PET_SPELL_DAMAGE_FRACTION: f64 = 0.10;

/// One cast from whichever pet is active (Firebolt for the Imp, Lash of Pain for the
/// Succubus), plus a Demonic Brand hit if `brand_points > 0` and a charge is available.
/// `coefficient` should already be pre-scaled by `PET_SPELL_DAMAGE_FRACTION` (see
/// `demonology.rs`/`affliction.rs`'s callers for why: it keeps the 10% share out of
/// `fight.stats.spell_power` itself, which the warlock's own spells also read from the same
/// `Fight`). Shared between `demonology.rs` and `affliction.rs` so this mechanic - including
/// Demonic Brand working with either pet - only has one implementation to keep correct.
pub fn pet_cast(fight: &mut Fight, pet: PetChoice, coefficient: f64, brand_points: u8) {
    let crit_chance = fight.stats.spell_crit; // the pet inherits the owner's crit% directly
    let result = match pet {
        PetChoice::Imp => {
            let min = FIREBOLT_AVERAGE * (1.0 - FIREBOLT_VARIANCE / 2.0);
            let max = FIREBOLT_AVERAGE * (1.0 + FIREBOLT_VARIANCE / 2.0);
            fight.resolve_direct_raw(
                pet.ability_key(), min, max, coefficient, crit_chance, BASE_CRIT_MULTIPLIER,
                true, fight.stats.bonus_fire_power,
            )
        }
        PetChoice::Succubus => {
            // No variance in the source (see `LASH_OF_PAIN_BASE`'s docs) - min==max.
            fight.resolve_direct_raw(
                pet.ability_key(), LASH_OF_PAIN_BASE, LASH_OF_PAIN_BASE, coefficient, crit_chance,
                BASE_CRIT_MULTIPLIER, true, fight.stats.bonus_shadow_power,
            )
        }
    };
    if brand_points > 0 && matches!(result, CastResult::Landed { .. }) && fight.consume_charge("demonic_brand") {
        let bonus_power = match pet.school() {
            crate::stats::School::Fire => fight.stats.bonus_fire_power,
            crate::stats::School::Shadow => fight.stats.bonus_shadow_power,
            crate::stats::School::Arcane | crate::stats::School::Frost => {
                unreachable!("a Warlock pet is always Fire or Shadow school")
            }
        };
        fight.resolve_direct_raw(
            "demonic_brand_hit", BRAND_MIN_DAMAGE, BRAND_MAX_DAMAGE, BRAND_COEFFICIENT,
            fight.stats.spell_crit, BASE_CRIT_MULTIPLIER, false, bonus_power,
        );
    }
}

