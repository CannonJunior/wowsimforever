//! Regression tests for the Priest data/gear packs and both rotations - the mirror of
//! `mage_sanity.rs` for this project's third class (2026-10-25).

use mythic_forever_engine::spelldata::embedded_priest_pack;

#[test]
fn mind_blast_matches_cross_checked_base_and_coefficient() {
    let pack = embedded_priest_pack();
    let row = pack.spell("mind_blast");
    let direct = row.direct.as_ref().expect("mind_blast has a direct effect");
    // data/spells.jsonl, spell id 10947 "Mind Blast Rank 9".
    assert_eq!(direct.base_points, 485.0);
    assert!((direct.sp_coefficient - 0.429).abs() < 1e-3);
    assert_eq!(row.cooldown_ms, Some(8000));
}

#[test]
fn shadow_word_pain_ticks_six_times_without_improved_shadow_word_pain() {
    let pack = embedded_priest_pack();
    let row = pack.spell("shadow_word_pain");
    let periodic = row.periodic.as_ref().expect("shadow_word_pain has a periodic effect");
    // DurationMs 18000 / PeriodMs 3000 = 6 ticks (spell id 10894) - Improved Shadow Word: Pain
    // (+1 tick/point) isn't modeled as a talent yet, so this pins the un-talented base count.
    assert_eq!(periodic.num_ticks, Some(6));
    assert_eq!(row.cast_time_ms, Some(0), "Shadow Word: Pain is instant - only the GCD applies");
}

#[test]
fn holy_fire_has_both_a_direct_hit_and_its_own_dot() {
    let pack = embedded_priest_pack();
    let row = pack.spell("holy_fire");
    assert!(row.direct.is_some(), "holy_fire should have a direct hit");
    let periodic = row.periodic.as_ref().expect("holy_fire has a periodic effect");
    assert_eq!(periodic.num_ticks, Some(5));
}

#[test]
fn shadow_priest_reference_build_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::gear::{embedded_priest_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs};
    use mythic_forever_engine::priest::shadow::{run, ShadowTalents};

    let pack = embedded_priest_gear_pack();
    let build = pack
        .example_builds
        .get("shadow_priest_starter_undead")
        .expect("the shadow priest starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::Priest).expect("valid gear");

    let talents = ShadowTalents {
        shadowform: true,
        shadow_weaving: 5,
        improved_mind_flay: 2,
        inner_focus: true,
        early_demise_crit_bonus: 0.30,
        twin_disciplines: 5,
    };
    let report = run(stats, 120_000, 42, &talents);
    assert!(report.dps() > 0.0, "shadow priest should deal positive DPS, got {}", report.dps());
    assert!(report.totals.contains_key("mind_flay"), "mind flay should be the filler");
    assert!(report.totals.contains_key("shadow_word_pain"), "shadow word pain should be kept up");
    assert!(report.totals.contains_key("mind_blast"), "mind blast should fire on cooldown");
}

#[test]
fn smite_priest_reference_build_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::gear::{embedded_priest_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs};
    use mythic_forever_engine::priest::smite::{run, SmiteTalents};

    let pack = embedded_priest_gear_pack();
    let build = pack
        .example_builds
        .get("smite_priest_starter_undead")
        .expect("the smite priest starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats = equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::Priest).expect("valid gear");

    let talents = SmiteTalents { power_in_light: true, inner_focus: true, twin_disciplines: 5, spiritual_guidance: 3 };
    let report = run(stats, 120_000, 42, &talents);
    assert!(report.dps() > 0.0, "smite priest should deal positive DPS, got {}", report.dps());
    assert!(report.totals.contains_key("smite"), "smite should be the main filler");
    assert!(report.totals.contains_key("holy_fire"), "holy fire should be kept up");
    assert!(report.totals.contains_key("penance"), "penance should fire on cooldown");
}

/// Shadowform's own one-time pre-pull mana cost (533.5, read directly from the real debug
/// log's own negative timestamp - see `priest/shadow.rs`'s module docs) should leave the
/// caster short of their own full mana pool at the very start of the simulated 120s fight.
#[test]
fn shadowform_spends_its_own_mana_before_the_fight_starts() {
    use mythic_forever_engine::gear::{embedded_priest_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs};
    use mythic_forever_engine::priest::shadow::ShadowTalents;
    use mythic_forever_engine::sim::Fight;
    use mythic_forever_engine::spelldata::embedded_priest_pack;

    let gear_pack = embedded_priest_gear_pack();
    let build = gear_pack
        .example_builds
        .get("shadow_priest_starter_undead")
        .expect("the shadow priest starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats = equipment_to_stats(&gear_pack, &build.race, &equipment, BuilderStarterBuffs::Priest).expect("valid gear");
    let max_mana = stats.max_mana;

    let mut fight = Fight::new(stats, embedded_priest_pack(), 120_000, 42);
    let talents = ShadowTalents { shadowform: true, ..Default::default() };
    mythic_forever_engine::priest::shadow::apply_talents(&mut fight, &talents);
    assert!(fight.mana < max_mana, "Shadowform should have spent mana before the fight starts");
    assert!((max_mana - fight.mana - 533.5).abs() < 1e-6, "got starting mana {}", fight.mana);
}
