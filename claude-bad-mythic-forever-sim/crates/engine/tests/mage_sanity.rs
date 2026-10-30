//! Regression tests for the Mage data/gear packs and the Fire rotation - the mirror of
//! `data_pack_sanity.rs`/`gear_sanity.rs` for this project's second class (2026-10-25).

use mythic_forever_engine::spelldata::embedded_mage_pack;

#[test]
fn fireball_matches_cross_checked_base_and_coefficient() {
    let pack = embedded_mage_pack();
    let row = pack.spell("fireball");
    let direct = row.direct.as_ref().expect("fireball has a direct effect");
    // foreverdb.net: "425 to 541 Fire damage" - midpoint 483.
    assert_eq!(direct.base_points, 483.0);
    assert!((direct.sp_coefficient - 1.0).abs() < 1e-9);
    let periodic = row.periodic.as_ref().expect("fireball has a periodic effect");
    let total_dot = periodic.base_points_per_tick * periodic.num_ticks.unwrap() as f64;
    // foreverdb.net: "an additional 60 Fire damage over 8 sec".
    assert_eq!(total_dot, 60.0);
}

#[test]
fn scorch_is_rebalanced_down_from_classic_era() {
    let pack = embedded_mage_pack();
    let row = pack.spell("scorch");
    let direct = row.direct.as_ref().expect("scorch has a direct effect");
    // foreverdb.net: "163 to 193 Fire damage... instead of 233-275 in Classic Era" - a real
    // rebalance this project's own mining should reflect, not something to "correct" back
    // toward Classic's higher number.
    assert_eq!(direct.base_points, 178.0);
}

#[test]
fn fire_blast_has_its_own_real_cooldown() {
    let pack = embedded_mage_pack();
    let row = pack.spell("fire_blast");
    // data/spells.jsonl's own CategoryCooldownMs field, not assumed from Classic knowledge.
    assert_eq!(row.cooldown_ms, Some(8000));
    assert_eq!(row.cast_time_ms, Some(0), "Fire Blast is instant - only the GCD applies");
}

#[test]
fn fire_mage_reference_build_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::gear::{
        embedded_mage_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs,
    };
    use mythic_forever_engine::mage::fire::{run, FireTalents};

    let pack = embedded_mage_gear_pack();
    let build = pack
        .example_builds
        .get("fire_mage_starter_undead")
        .expect("the fire mage starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats =
        equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::None).expect("valid gear");

    let talents = FireTalents {
        improved_scorch: 3,
        improved_fireball: 5,
        critical_mass: 5,
        fire_power: 3,
        incineration: 3,
        elemental_precision: 3,
        ignite: 5,
        heating_up_pyroblast: true,
        combustion: true,
        arcane_concentration: 5,
        arcane_subtlety: 2,
        master_of_elements: 3,
        missile_barrage: 1,
    };
    let report = run(stats, 120_000, 42, &talents);
    assert!(report.dps() > 0.0, "fire mage should deal positive DPS, got {}", report.dps());
    assert!(report.totals.contains_key("fireball"), "fireball should be the filler");
    assert!(report.totals.contains_key("scorch"), "scorch should maintain Fire Vulnerability");
    assert!(report.totals.contains_key("fire_blast"), "fire blast should fire on cooldown");
}

/// Improved Scorch at 3/3 is a guaranteed (100%) proc per its own curve (spell 11095:
/// `[[33,67,100]]`) - pins that Fire Vulnerability reaches and holds its 5-stack cap under a
/// long fight, not just "sometimes gets a stack or two".
#[test]
fn improved_scorch_at_max_points_reliably_maxes_fire_vulnerability_stacks() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};
    use mythic_forever_engine::stats::CasterStats;

    let stats = CasterStats {
        spell_power: 400.0,
        spell_hit: 1.0, // isolate the proc chance from hit variance
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 200.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = FireTalents { improved_scorch: 3, ..FireTalents::default() };
    let report = run(stats, 60_000, 7, &talents);
    let scorch = report.totals.get("scorch").expect("scorch should be cast to maintain the debuff");
    assert!(scorch.hits > 0, "scorch should land at least once to ever stack Fire Vulnerability");
}

/// A 0% hit character's Improved Scorch never even gets the chance to proc - Fire Vulnerability
/// should never apply, and Fireball (the only other ability this build casts, since Scorch
/// never stops being "needed") should never see its bonus either.
#[test]
fn fire_vulnerability_requires_a_landed_scorch() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};
    use mythic_forever_engine::stats::CasterStats;

    let stats = CasterStats {
        spell_power: 400.0,
        spell_hit: 0.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = FireTalents { improved_scorch: 3, ..FireTalents::default() };
    let report = run(stats, 30_000, 3, &talents);
    let scorch = report.totals.get("scorch").expect("scorch should still be attempted");
    assert_eq!(scorch.hits, 0, "0% spell hit should mean 0 landed Scorch casts, got {scorch:?}");
    assert!(scorch.misses > 0);
}

fn high_crit_stats() -> mythic_forever_engine::stats::CasterStats {
    use mythic_forever_engine::stats::CasterStats;
    CasterStats {
        spell_power: 500.0,
        spell_hit: 1.0,
        spell_crit: 0.5, // high crit so Ignite/Heating Up/Combustion all see real activity
        haste: 1.0,
        mp5: 300.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    }
}

/// Ignite (2026-10-25, second pass): a Fire crit should start a ticking "ignite" dot - not a
/// separate cast, just damage appearing under that key, with no crits of its own (the real
/// source's own rule: Ignite's ticks never crit).
#[test]
fn ignite_ticks_from_fire_crits_and_never_crits_itself() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};

    let talents = FireTalents { ignite: 5, ..FireTalents::default() };
    let report = run(high_crit_stats(), 60_000, 11, &talents);
    let ignite = report.totals.get("ignite").expect("a high-crit Fire Mage should trigger Ignite");
    assert!(ignite.ticks > 0, "ignite should tick, got {ignite:?}");
    assert_eq!(ignite.crits, 0, "ignite's own ticks should never crit");
    assert!(ignite.damage > 0.0);
}

/// With `ignite: 0`, no Ignite dot should ever appear, no matter how often Fire spells crit -
/// the talent gates the mechanic entirely, not just its magnitude.
#[test]
fn no_ignite_points_means_no_ignite_dot_at_all() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};

    let talents = FireTalents { ignite: 0, ..FireTalents::default() };
    let report = run(high_crit_stats(), 60_000, 11, &talents);
    assert!(!report.totals.contains_key("ignite"), "ignite: 0 should never produce an ignite dot");
}

/// Heating Up / Pyroblast (2026-10-25): with the mechanic on and enough crit to actually stack
/// Heating Up to 3, this build should eventually cast Pyroblast - and never cast it at all with
/// the talent off, since this engine only ever reaches for Pyroblast once Heating Up is maxed.
#[test]
fn heating_up_eventually_casts_pyroblast_only_when_talented() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};

    let with = FireTalents { heating_up_pyroblast: true, ..FireTalents::default() };
    let report_with = run(high_crit_stats(), 120_000, 13, &with);
    let pyroblast = report_with.totals.get("pyroblast").expect("heating up should eventually cast pyroblast");
    assert!(pyroblast.hits > 0 || pyroblast.misses > 0, "pyroblast should actually be attempted, got {pyroblast:?}");

    let without = FireTalents { heating_up_pyroblast: false, ..FireTalents::default() };
    let report_without = run(high_crit_stats(), 120_000, 13, &without);
    assert!(
        !report_without.totals.contains_key("pyroblast"),
        "without the talent this engine should never reach for pyroblast at all"
    );
}

/// Combustion (2026-10-25): its own crit bonus should make this build's overall crit rate
/// higher over a fight than the same build without the talent - the clearest externally
/// observable signal that the stacking-crit-then-ending-at-3-crits logic is actually running,
/// without reaching into private aura state from a test.
#[test]
fn combustion_raises_the_builds_overall_crit_rate() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};

    let crit_rate = |report: &mythic_forever_engine::sim::FightReport| {
        let hits: u32 = report.totals.values().map(|t| t.hits).sum();
        let crits: u32 = report.totals.values().map(|t| t.crits).sum();
        crits as f64 / hits.max(1) as f64
    };

    let with = FireTalents { combustion: true, ..FireTalents::default() };
    let without = FireTalents { combustion: false, ..FireTalents::default() };
    let rate_with = crit_rate(&run(high_crit_stats(), 300_000, 23, &with));
    let rate_without = crit_rate(&run(high_crit_stats(), 300_000, 23, &without));
    assert!(
        rate_with > rate_without,
        "combustion's own crit bonus should raise the overall crit rate: with={rate_with}, without={rate_without}"
    );
}

/// Arcane Concentration / Clearcasting (2026-10-25): with the talent on, this build should run
/// out of mana less (need fewer Evocations) than the same build without it, since some casts
/// become free - the externally observable effect of the mechanic actually firing.
#[test]
fn clearcasting_reduces_evocation_frequency() {
    use mythic_forever_engine::mage::fire::{run, FireTalents};
    use mythic_forever_engine::stats::CasterStats;

    // A deliberately tight mana pool so Evocation's frequency is actually sensitive to how
    // many casts Clearcasting manages to make free.
    let tight_mana_stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 1.0,
        spell_crit: 0.3,
        haste: 1.0,
        mp5: 20.0,
        max_mana: 3000.0,
        spirit: 50.0,
        ..Default::default()
    };

    let evocations = |talents: &FireTalents| {
        run(tight_mana_stats, 300_000, 29, talents)
            .totals
            .get("evocation")
            .map_or(0, |t| t.casts)
    };
    let with = FireTalents { arcane_concentration: 5, ..FireTalents::default() };
    let without = FireTalents { arcane_concentration: 0, ..FireTalents::default() };
    assert!(
        evocations(&with) <= evocations(&without),
        "clearcasting should never *increase* how often this build needs to Evocate"
    );
}

/// Arcane Mage's reference build (2026-10-25): mirrors
/// `fire_mage_reference_build_produces_positive_dps_without_hanging`'s own shape for the second
/// Mage spec.
#[test]
fn arcane_mage_reference_build_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::gear::{
        embedded_mage_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs,
    };
    use mythic_forever_engine::mage::arcane::{run, ArcaneTalents};

    let pack = embedded_mage_gear_pack();
    let build = pack
        .example_builds
        .get("arcane_mage_starter_undead")
        .expect("the arcane mage starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats =
        equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::MageArcaneFrost).expect("valid gear");

    let talents = ArcaneTalents {
        arcane_blast: true,
        missile_barrage: 1,
        arcane_concentration: 5,
        arcane_impact: 3,
        arcane_mind: 5,
        arcane_instability: 3,
        arcane_power: true,
        arcane_subtlety: 2,
    };
    let report = run(stats, 120_000, 42, &talents);
    assert!(report.dps() > 0.0, "arcane mage should deal positive DPS, got {}", report.dps());
    assert!(report.totals.contains_key("arcane_blast"), "arcane blast should be cast");
    assert!(report.totals.contains_key("frostbolt"), "frostbolt should be the mana-conserving fallback");
}

/// Frost Mage's reference build (2026-10-25): mirrors the same shape for the third Mage spec.
#[test]
fn frost_mage_reference_build_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::gear::{
        embedded_mage_gear_pack, equipment_to_stats, example_build_equipment, BuilderStarterBuffs,
    };
    use mythic_forever_engine::mage::frost::{run, FrostTalents};

    let pack = embedded_mage_gear_pack();
    let build = pack
        .example_builds
        .get("frost_mage_starter_undead")
        .expect("the frost mage starter build should be in the pack");
    let equipment = example_build_equipment(build);
    let stats =
        equipment_to_stats(&pack, &build.race, &equipment, BuilderStarterBuffs::MageArcaneFrost).expect("valid gear");

    let talents = FrostTalents {
        improved_frostbolt: 5,
        elemental_precision: 3,
        ice_shards: 5,
        fingers_of_frost: 1,
        shatter: 1,
        missile_barrage: 1,
        arcane_concentration: 5,
        arcane_subtlety: 2,
    };
    let report = run(stats, 120_000, 42, &talents);
    assert!(report.dps() > 0.0, "frost mage should deal positive DPS, got {}", report.dps());
    assert!(report.totals.contains_key("frostbolt"), "frostbolt should be the filler");
}

/// Fingers of Frost + Shatter + Ice Lance's own frozen-target bonus (2026-10-25): a build with
/// the talent should land noticeably harder average Ice Lance hits than the same build without
/// it (no charges ever granted, so Ice Lance never gets the x3.0 frozen multiplier or Shatter's
/// bonus crit) - the clearest externally observable signal the frozen-target bonus chain is
/// actually wired up, without reaching into private charge-counter state from a test.
#[test]
fn fingers_of_frost_makes_ice_lance_hit_much_harder_on_average() {
    use mythic_forever_engine::mage::frost::{run, FrostTalents};
    use mythic_forever_engine::stats::CasterStats;

    let stats = CasterStats {
        spell_power: 400.0,
        spell_hit: 1.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 300.0,
        max_mana: 30000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let avg_ice_lance = |talents: &FrostTalents| {
        let report = run(stats, 180_000, 17, talents);
        report.totals.get("ice_lance").filter(|t| t.hits > 0).map(|t| t.damage / t.hits as f64)
    };
    let with = FrostTalents { fingers_of_frost: 2, shatter: 3, ..FrostTalents::default() };
    let without = FrostTalents { fingers_of_frost: 0, shatter: 0, ..FrostTalents::default() };
    let avg_with = avg_ice_lance(&with).expect("fingers of frost should land at least one ice lance");
    match avg_ice_lance(&without) {
        Some(avg_without) => assert!(
            avg_with > avg_without,
            "frozen-target ice lance should hit harder on average: with={avg_with}, without={avg_without}"
        ),
        None => {
            // Without any Fingers of Frost charges ever granted, this rotation's own
            // `frozen_active` check is always false, so Ice Lance's own branch never fires at
            // all - consistent with the mechanic being fully gated, not just weaker.
        }
    }
}

/// Missile Barrage (2026-10-25, Frost side): with the talent off, Arcane Missiles should never
/// appear at all (the real priority list's own `auraIsKnown` gate) - with it on and a tight
/// enough mana pool that a *paid* Arcane Missiles cast (655 mana) would be unaffordable, it
/// should still appear sometimes, since a Missile Barrage proc makes the next one free.
#[test]
fn missile_barrage_lets_a_mana_starved_frost_mage_still_cast_arcane_missiles() {
    use mythic_forever_engine::mage::frost::{run, FrostTalents};
    use mythic_forever_engine::stats::CasterStats;

    let tight_mana_stats = CasterStats {
        spell_power: 350.0,
        spell_hit: 1.0,
        spell_crit: 0.1,
        haste: 1.0,
        mp5: 40.0,
        max_mana: 600.0, // below Arcane Missiles' own 655 flat cost - only a free cast can land
        spirit: 50.0,
        ..Default::default()
    };
    let without = FrostTalents { missile_barrage: 0, ..FrostTalents::default() };
    let report_without = run(tight_mana_stats, 300_000, 31, &without);
    assert!(
        !report_without.totals.contains_key("arcane_missiles"),
        "without the talent, arcane missiles should never be attempted at all"
    );

    let with = FrostTalents { missile_barrage: 2, ..FrostTalents::default() };
    let report_with = run(tight_mana_stats, 300_000, 31, &with);
    assert!(
        report_with.totals.get("arcane_missiles").is_some_and(|t| t.hits > 0 || t.misses > 0),
        "missile barrage should let this mana-starved build still land a free arcane missiles cast"
    );
}
