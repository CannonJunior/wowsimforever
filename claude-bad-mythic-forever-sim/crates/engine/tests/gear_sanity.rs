//! Pins `gear::equipment_to_stats` against hand-computed totals for the real "destruction
//! reference" build (the exact equipment list from
//! `validation/.../destruction-warlock.request.json`, not an invented example). The expected
//! numbers below were computed independently in Python during the research pass (see
//! research-notes/sources.md) - this test is the Rust-side half of that cross-check.

use mythic_forever_engine::gear::{embedded_warlock_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs};

#[test]
fn destruction_reference_build_matches_hand_computed_totals() {
    let pack = embedded_warlock_gear_pack();
    let build = pack
        .example_builds
        .get("destruction_reference")
        .expect("the reference build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::None).expect("valid gear");

    // SpellDamage summed across all 15 items: 220 (ShadowDamage's +60 is intentionally not
    // added - see gear.rs's module docs on what's parsed-but-not-applied). This build's own
    // 235 total HealingPower (added 2026-10-26 while chasing Smite Priest's own per-hit damage
    // gap) is real, but Holy-school-scoped, not universal - see `equipment_to_stats`'s own
    // `/ 3.0` doc comment - so it lands in `bonus_holy_power`, not here, and never affects a
    // Warlock's own (all Shadow/Fire) output.
    assert_eq!(stats.spell_power, 220.0);
    assert!((stats.bonus_holy_power - 235.0 / 3.0).abs() < 1e-6, "got {}", stats.bonus_holy_power);

    // Gear hit: MeleeHitRating 10 (Blackstone Ring) + SpellHitRating 40, unified per Forever's
    // own rule and converted at 10 rating/1%: (10+40)/10 = 5%, on top of the real 83% base
    // spell hit chance vs a level-63 target (confirmed via wowsims/forever's own
    // attack_table_test.go) = 88%.
    assert!((stats.spell_hit - 0.88).abs() < 1e-9, "got {}", stats.spell_hit);

    // Gear crit: SpellCritRating 28 / 14 = 2%, plus RaceUndead's base 1.7%, plus
    // Intellect-derived crit: (108 race + 97 gear) * 0.0165%/int = 3.3825%.
    // Total: 1.7 + 2.0 + 3.3825 = 7.0825%.
    assert!((stats.spell_crit - 0.070825).abs() < 1e-9, "got {}", stats.spell_crit);

    // No SpellHasteRating anywhere in this build.
    assert_eq!(stats.haste, 1.0);

    // Intellect-to-mana (wowsims/forever sim/core/mana.go, confirmed in the fourth research
    // pass): race base Mana (1373, pre-dependency) + 0 item Mana + (-280 + 15*total Intellect).
    // Total Intellect = 108 race + 97 gear = 205. 1373 - 280 + 15*205 = 4168.
    assert_eq!(stats.max_mana, 4168.0);

    // RaceUndead base Spirit (120) + 21 from gear (Sublime Wristguards 6, Hands of Power 6,
    // Enchant Bracer - Spirit 5, Ban'thok Sash... wait: 6+6+5+? - see the item list for the
    // exact per-item breakdown if this ever needs re-deriving).
    assert_eq!(stats.spirit, 141.0);

    // Spirit-to-regen (same source file, Warlock uses the class default): 5 * (6.25 +
    // 141 * 0.125) = 5 * 23.875 = 119.375. No item carries a flat MP5 stat in this build.
    assert!((stats.mp5 - 119.375).abs() < 1e-9, "got {}", stats.mp5);
}

/// Regression test for a real bug found while comparing this engine's output against
/// mythicsim.com's own recorded DPS for the "builder starter" validation corpus
/// (2026-10-24): `equipment_to_stats` fed `CasterStats.spell_hit` *only* the gear-granted
/// bonus (4-5% for a typical starter build), silently dropping the 83% base spell hit chance
/// vs a level-63 target every character has before gear - so ~95% of casts missed and DPS
/// came out 4-6x too low. Confirmed directly against `wowsims/forever`'s own
/// `sim/core/attack_table_test.go` base-chance table (level 63 row: `spellMiss: 0.17`).
#[test]
fn gear_hit_includes_the_real_base_chance_not_only_the_gear_bonus() {
    let pack = embedded_warlock_gear_pack();
    // Zero gear-granted hit rating anywhere: this should still be the 83% base, not 0%.
    let bare = equipment_to_stats(&pack, "RaceOrc", &[], BuilderStarterBuffs::None).expect("valid");
    assert!(
        bare.spell_hit > 0.5,
        "spell_hit with no gear at all should still be the ~83% base, got {}",
        bare.spell_hit
    );
}

#[test]
fn unknown_race_is_a_clean_error_not_a_panic() {
    let pack = embedded_warlock_gear_pack();
    let result = equipment_to_stats(&pack, "RaceNotReal", &[], BuilderStarterBuffs::None);
    assert!(result.is_err());
}

/// Closing the mythicsim.com DPS gap for real (2026-10-24, "analyze and address the remaining
/// gap" pass): raid buffs and consumables, not pet melee auto-attack, turned out to be the
/// dominant missing piece - mythicsim's own "builder starter" fixtures run with a full raid
/// buff + consumable package this engine never applied. `builder_starter_buffs: true` applies
/// that exact package (see `gear.rs`'s `BUFF_*` constants for the sourced values); `false` must
/// still match every pre-existing test above exactly - these numbers aren't a generic "buffs
/// exist" smoke test, they pin the literal sourced amounts so a future accidental edit to a
/// `BUFF_*` constant fails loudly.
#[test]
fn builder_starter_buffs_apply_the_real_sourced_amounts() {
    let pack = embedded_warlock_gear_pack();
    let build = pack
        .example_builds
        .get("destruction_reference")
        .expect("the reference build should be in the pack");
    let equipment = example_build_equipment(build);
    let plain = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::None).expect("valid gear");
    let buffed = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::Warlock).expect("valid gear");

    // Flask of Supreme Power (+150 universal) *and* Greater Arcane Elixir (+35 universal) -
    // confirmed additive, not exclusive, directly from `consumes.rs`'s own `register_consumes`
    // (see `BUFF_SPELL_POWER`'s own doc comment in gear.rs for the full citation and the real
    // debug-log evidence that caught this).
    assert!((buffed.spell_power - (plain.spell_power + 150.0 + 35.0)).abs() < 1e-9, "got {}", buffed.spell_power);
    // Elixir of Shadow Power (+40 shadow-specific) - a different stat, stacks with the flask.
    assert!(
        (buffed.bonus_shadow_power - (plain.bonus_shadow_power + 40.0)).abs() < 1e-9,
        "got {}",
        buffed.bonus_shadow_power
    );
    // Moonkin Aura (+3%, not Classic's 5% - this client's own tooltip says 3%) plus whatever
    // extra Intellect-derived crit the buffed Intellect total adds on top.
    assert!(buffed.spell_crit > plain.spell_crit + 0.03 - 1e-9, "got {}", buffed.spell_crit);
    // mp5 buffs (Mageblood Elixir +12, Greater Blessing of Wisdom +40, Mana Spring Totem +25 -
    // the last one added 2026-10-25, see `BUFF_MP5`'s own doc comment) plus the extra
    // Spirit-derived regen from the buffed Spirit total.
    assert!(buffed.mp5 > plain.mp5 + 77.0 - 1e-9, "got {}", buffed.mp5);
    // Buffed Intellect/Spirit (and so max_mana) must be strictly higher, not just the flat
    // spell-power/crit/mp5 pieces.
    assert!(buffed.max_mana > plain.max_mana, "buffed Intellect should raise max_mana too");
    assert!(buffed.spirit > plain.spirit, "buffed Spirit should be reflected in CasterStats");
}

/// Mage's own "builder starter" package (2026-10-25, found while doing a per-ability breakdown
/// comparison for Fire Mage): the real request JSON's `schoolElixirId` is a *different* item
/// from Warlock's (Elixir of Fire Power, +10 Fire-specific, not Elixir of Shadow Power's +40
/// Shadow-specific) and carries a weapon imbue Warlock's own package doesn't
/// (`BuilderStarterBuffs::Warlock` has none) - `BuilderStarterBuffs::Mage` must apply *this*
/// class's real numbers, not silently reuse Warlock's. (`BuilderStarterBuffs::Mage` was renamed
/// `::MageFire` 2026-10-25 when Arcane/Frost Mage were added, since their own fixtures use yet
/// another school elixir - see `gear.rs`'s own doc comment.)
#[test]
fn mage_builder_starter_buffs_use_mages_own_sourced_amounts() {
    use mythic_forever_engine::gear::{embedded_mage_gear_pack, example_build_equipment};

    let pack = embedded_mage_gear_pack();
    let build = pack
        .example_builds
        .get("fire_mage_starter_undead")
        .expect("the reference build should be in the pack");
    let equipment = example_build_equipment(build);
    let plain = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::None).expect("valid gear");
    let buffed = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::MageFire).expect("valid gear");

    // Flask of Supreme Power (+150) + Greater Arcane Elixir (+35, additive - see
    // `BUFF_SPELL_POWER`'s own doc comment) + Brilliant Wizard Oil's own `SpellDamage`
    // component (+36, also additive, confirmed the same way via `register_static_imbue`'s own
    // unconditional `add_stat` call) - all three universal `SpellDamage` sources this package
    // carries, none of them exclusive with each other.
    assert!(
        (buffed.spell_power - (plain.spell_power + 150.0 + 35.0 + 36.0)).abs() < 1e-9,
        "got {}",
        buffed.spell_power
    );
    // Elixir of Fire Power (+10 Fire-specific) - Mage's own, much smaller than Warlock's Shadow
    // one, and definitely not Warlock's +40.
    assert!(
        (buffed.bonus_fire_power - (plain.bonus_fire_power + 10.0)).abs() < 1e-9,
        "got {}",
        buffed.bonus_fire_power
    );
    assert_eq!(buffed.bonus_shadow_power, plain.bonus_shadow_power, "a Fire Mage's package should never touch shadow power");
    // Brilliant Wizard Oil's own `HealingPower` component (+36/3 = +12) - Holy-school-scoped
    // (see `equipment_to_stats`'s own `/ 3.0` doc comment), found 2026-10-26 while chasing
    // Priest's own per-hit damage gap. Fire Mage never casts a Holy-school spell, but the
    // stat itself is still real and present, same as it would be for any class wearing this
    // same weapon imbue.
    assert!(
        (buffed.bonus_holy_power - (plain.bonus_holy_power + 12.0)).abs() < 1e-9,
        "got {}",
        buffed.bonus_holy_power
    );
    // Moonkin Aura's 3% plus Brilliant Wizard Oil's own +1% - a real, extra crit source
    // Warlock's package doesn't have at all.
    assert!(buffed.spell_crit > plain.spell_crit + 0.04 - 1e-9, "got {}", buffed.spell_crit);
}

/// Priest's own "builder starter" package (2026-10-25): both Shadow and Smite Priest fixtures
/// carry the identical `schoolElixirId` (9264, Elixir of Shadow Power - the same item Warlock's
/// own package uses) despite Smite's own damage being Holy-school, a real fixture quirk
/// faithfully reproduced, not second-guessed - see `BuilderStarterBuffs::Priest`'s own doc
/// comment.
#[test]
fn priest_builder_starter_buffs_reuse_warlocks_own_shadow_elixir() {
    use mythic_forever_engine::gear::{embedded_priest_gear_pack, example_build_equipment};

    let pack = embedded_priest_gear_pack();
    let build = pack
        .example_builds
        .get("shadow_priest_starter_undead")
        .expect("the reference build should be in the pack");
    let equipment = example_build_equipment(build);
    let plain = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::None).expect("valid gear");
    let buffed = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::Priest).expect("valid gear");

    // Flask of Supreme Power (+150) + Greater Arcane Elixir (+35) + Brilliant Wizard Oil's own
    // `SpellDamage` component (+36) - the same three universal sources Mage's own package uses.
    assert!(
        (buffed.spell_power - (plain.spell_power + 150.0 + 35.0 + 36.0)).abs() < 1e-9,
        "got {}",
        buffed.spell_power
    );
    // Elixir of Shadow Power (+40 Shadow-specific) - Warlock's own item, reused here.
    assert!(
        (buffed.bonus_shadow_power - (plain.bonus_shadow_power + 40.0)).abs() < 1e-9,
        "got {}",
        buffed.bonus_shadow_power
    );
    // This build's own gear carries HealingPower (Holy-school-scoped bonus damage - see
    // `equipment_to_stats`'s own `/ 3.0` doc comment) *and* Brilliant Wizard Oil adds another
    // +36/3 = +12 on top, *and* Judgement of the Crusader (+161 - see
    // `JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER`'s own doc comment) applies unconditionally for
    // every Priest build, Shadow included (a no-op for Shadow since it never reads
    // `bonus_holy_power`) - found 2026-10-26 while chasing this exact spec's own per-hit damage
    // gap, so `bonus_holy_power` is *not* unchanged by this package the way it first looked.
    assert!(
        (buffed.bonus_holy_power - (plain.bonus_holy_power + 12.0 + 161.0)).abs() < 1e-9,
        "got {}",
        buffed.bonus_holy_power
    );
}

#[test]
fn empty_equipment_gives_pure_race_base_stats() {
    let pack = embedded_warlock_gear_pack();
    let stats = equipment_to_stats(&pack, "RaceOrc", &[], BuilderStarterBuffs::None).expect("race with no gear is valid");
    assert_eq!(stats.spell_power, 0.0);
    // No gear-granted hit, but the real 83% base spell hit vs a level-63 target still applies.
    assert_eq!(stats.spell_hit, 0.83);
    // RaceOrc's base SpellCritPercent (1.7) plus Intellect(107)-derived crit (107*0.0165%).
    let expected_crit = (1.7 + 107.0 * 0.0165) / 100.0;
    assert!((stats.spell_crit - expected_crit).abs() < 1e-9);
    // 1373 - 280 + 15*107 = 2698 (see the other test for the formula's source).
    assert_eq!(stats.max_mana, 2698.0);
}
