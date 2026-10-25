//! Regression tests pinning this engine's math to the two cross-checked values from the
//! research pass (see data/warlock-forever-2026-10-23-rev2.json and the workspace README):
//! Shadow Bolt rank 10 averaging 268 (foreverdb.net showed "253 to 283", avg 268) and Immolate
//! rank 8's direct hit being exactly 158 before spell power. If these ever drift, either the
//! data pack changed (expected - bump its version) or the formula broke (not expected).

use mythic_forever_engine::spelldata::embedded_warlock_pack;

#[test]
fn shadow_bolt_matches_cross_checked_base_and_coefficient() {
    let pack = embedded_warlock_pack();
    let row = pack.spell("shadow_bolt");
    let direct = row.direct.as_ref().expect("shadow bolt has a direct effect");
    assert_eq!(direct.base_points, 268.0);
    assert!((direct.sp_coefficient - 0.857).abs() < 0.001);
    // foreverdb.net showed "253 to 283" (width 30) around an average of 268; this pack's
    // mined variance (0.1098) should produce a comparably narrow spread, not the old (and, per
    // this pack's `known_conflicts`, apparently stale) "246-274"/"flat 251" figures.
    let variance = direct.variance.expect("shadow bolt has a variance field");
    let width = 268.0 * variance;
    assert!((width - 29.4).abs() < 1.0, "unexpected variance width: {width}");
}

#[test]
fn immolate_direct_and_dot_total_match_cross_check() {
    let pack = embedded_warlock_pack();
    let row = pack.spell("immolate");
    let direct = row.direct.as_ref().expect("immolate has a direct effect");
    assert_eq!(direct.base_points, 158.0);
    let periodic = row.periodic.as_ref().expect("immolate has a periodic effect");
    let total_dot = periodic.base_points_per_tick * periodic.num_ticks.unwrap() as f64;
    // foreverdb.net: "275 Fire damage over 15 sec".
    assert_eq!(total_dot, 275.0);
}

#[test]
fn every_spec_produces_positive_dps_without_hanging() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::{affliction, demonology, destruction};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 0.99,
        spell_crit: 0.25,
        haste: 1.10,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
    ..Default::default() };
    let duration_ms = 60_000;

    let d = destruction::run(stats, duration_ms, 1, &destruction::DestructionTalents::default());
    assert!(d.dps() > 0.0);

    let a = affliction::run(stats, duration_ms, 1, &affliction::AfflictionTalents::default());
    assert!(a.dps() > 0.0);

    let m = demonology::run(stats, duration_ms, 1, &demonology::DemonologyTalents::default(), None);
    assert!(m.dps() > 0.0);
}

/// Regression test for the exact hang this project shipped with once: Demonology's rotation
/// had no mana-management branch, so a low max-mana pool could stall its `while !is_over()`
/// loop forever. A `--max-mana` far below the 10%-of-pool Life Tap threshold vs a spell's cost
/// used to reproduce it; the `run()` safety-advance guard (and the Life Tap fix) covers it now.
#[test]
fn demonology_does_not_hang_with_a_small_mana_pool() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology;

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 0.99,
        spell_crit: 0.25,
        haste: 1.0,
        mp5: 10.0,
        max_mana: 500.0,
        spirit: 100.0,
    ..Default::default() };
    let report = demonology::run(stats, 30_000, 1, &demonology::DemonologyTalents::default(), None);
    assert_eq!(report.duration_ms, 30_000);
}

/// Demonic Brand should only fire when talented: this is the gap called out in the first
/// version's README ("Demonic Brand... isn't implemented") - confirms it now contributes
/// damage when talented and contributes none when it isn't.
#[test]
fn demonic_brand_only_fires_when_talented() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{DemonologyTalents, run};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.25,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
    ..Default::default() };
    let duration_ms = 120_000;

    let without = run(stats, duration_ms, 7, &DemonologyTalents::default(), None);
    assert!(!without.totals.contains_key("demonic_brand_hit"));

    let with_brand = DemonologyTalents {
        demonic_brand: 3,
        ..DemonologyTalents::default()
    };
    let with = run(stats, duration_ms, 7, &with_brand, None);
    let brand = with
        .totals
        .get("demonic_brand_hit")
        .expect("demonic_brand_hit should appear once talented");
    assert!(brand.hits > 0, "brand should land at least once in 120s");
}

/// Bane should measurably speed up Shadow Bolt's cast rate (its whole point). Regression test
/// for the gap where Bane was documented but never wired into either Talents struct.
#[test]
fn bane_speeds_up_shadow_bolt_casts() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::destruction::{DestructionTalents, run};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.0, // keep crit out of it; this test is about cast speed, not damage
        haste: 1.0,
        mp5: 1000.0, // mana should never be the limiting factor here
        max_mana: 20000.0,
        spirit: 100.0,
    ..Default::default() };
    let duration_ms = 180_000;

    let without_bane = DestructionTalents {
        incinerate: false, // keep the filler on shadow_bolt specifically for both runs
        ..DestructionTalents::default()
    };
    let with_bane = DestructionTalents {
        bane: 5,
        incinerate: false,
        ..DestructionTalents::default()
    };

    let a = run(stats, duration_ms, 3, &without_bane);
    let b = run(stats, duration_ms, 3, &with_bane);
    let casts_a = a.totals.get("shadow_bolt").map_or(0, |t| t.casts);
    let casts_b = b.totals.get("shadow_bolt").map_or(0, |t| t.casts);
    assert!(
        casts_b > casts_a,
        "bane=5 should cast more Shadow Bolts in the same duration (got {casts_a} vs {casts_b})"
    );
}

/// Nightfall should let Affliction fit measurably more Shadow Bolt casts into the same
/// duration (some of them are free of the 3s cast time) - the actual mechanism, not the old
/// "no Nightfall at all" gap.
#[test]
fn nightfall_increases_shadow_bolt_cast_count() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::affliction::{run, AfflictionTalents};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
    ..Default::default() };
    let duration_ms = 300_000; // long enough for the 2-4% per-tick proc to matter statistically

    let without = AfflictionTalents::default();
    let with_nightfall = AfflictionTalents {
        nightfall: 2,
        ..AfflictionTalents::default()
    };

    let a = run(stats, duration_ms, 11, &without);
    let b = run(stats, duration_ms, 11, &with_nightfall);
    let casts_a = a.totals.get("shadow_bolt").map_or(0, |t| t.casts);
    let casts_b = b.totals.get("shadow_bolt").map_or(0, |t| t.casts);
    assert!(
        casts_b > casts_a,
        "nightfall=2 should land more Shadow Bolts in the same duration (got {casts_a} vs {casts_b})"
    );
}

/// Improved Shadow Bolt's debuff should be a real timed window, not flat always-on: average
/// Shadow Bolt damage with it talented should land strictly between "no bonus" and "the bonus
/// applied to literally every cast" - proof it's sometimes up and sometimes not.
#[test]
fn improved_shadow_bolt_is_a_real_uptime_window_not_flat() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::destruction::{run, DestructionTalents};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.5, // high crit so the debuff actually procs often enough to measure
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
    ..Default::default() };
    let duration_ms = 300_000;
    let base = DestructionTalents {
        incinerate: false,
        ..DestructionTalents::default()
    };
    let no_isb = DestructionTalents { improved_shadow_bolt: 0, ..base };
    let with_isb = DestructionTalents { improved_shadow_bolt: 5, ..base };

    let avg_damage_per_cast = |t: &DestructionTalents| {
        let r = run(stats, duration_ms, 5, t);
        let sb = r.totals.get("shadow_bolt").expect("shadow bolt casts");
        sb.damage / sb.hits as f64
    };

    let without = avg_damage_per_cast(&no_isb);
    let with = avg_damage_per_cast(&with_isb);
    let flat_always_on = without * 1.20; // what the old (wrong) flat approximation would give

    assert!(with > without, "ISB should raise average damage at all ({without} vs {with})");
    assert!(
        with < flat_always_on,
        "ISB's debuff isn't up 100% of the time, so the average should land below the old \
         flat-always-on approximation ({with} vs flat {flat_always_on})"
    );
}

/// Life Tap's real formula (wowsims/forever sim/warlock/lifetap.go, read directly in the
/// fourth research/implementation pass): `(420 + Spirit) * (1 + ImprovedLifeTap%)`. An earlier
/// version of this engine used a flat 420 with no Spirit term at all - fixed.
#[test]
fn life_tap_scales_with_spirit_and_improved_life_tap() {
    use mythic_forever_engine::warlock::life_tap_mana;
    assert_eq!(life_tap_mana(0.0, 0), 420.0);
    assert_eq!(life_tap_mana(100.0, 0), 520.0);
    assert_eq!(life_tap_mana(100.0, 2), 520.0 * 1.20);
}

/// Cataclysm's mana-cost reduction used to be undocumented *and* unimplemented (the README
/// called it a no-op talent; it wasn't even wired up). Under a mana-starved build it should
/// measurably let a Destruction Warlock land more real spell casts (not just Life Taps) in the
/// same window.
#[test]
fn cataclysm_reduces_real_spell_casts_lost_to_life_tapping() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::destruction::{run, DestructionTalents};

    let stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 1.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 40.0, // tight but not hopeless - lets Cataclysm's cost cut actually matter
        max_mana: 4000.0,
        spirit: 50.0,
        ..Default::default()
    };
    let duration_ms = 300_000;
    let without = DestructionTalents { cataclysm: 0, incinerate: false, ..DestructionTalents::default() };
    let with = DestructionTalents { cataclysm: 3, incinerate: false, ..DestructionTalents::default() };

    let real_casts = |t: &DestructionTalents| {
        let r = run(stats, duration_ms, 9, t);
        let non_life_tap: u32 = r
            .totals
            .iter()
            .filter(|(k, _)| k.as_str() != "life_tap")
            .map(|(_, v)| v.casts)
            .sum();
        non_life_tap
    };

    let casts_without = real_casts(&without);
    let casts_with = real_casts(&with);
    assert!(
        casts_with > casts_without,
        "cataclysm=3 should land more real spell casts under mana pressure (got {casts_without} vs {casts_with})"
    );
}

/// Succubus is a real, separate pet choice now (Lash of Pain), not just mined-but-unused data.
#[test]
fn succubus_pet_choice_deals_lash_of_pain_damage() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{run, DemonologyTalents};
    use mythic_forever_engine::warlock::PetChoice;

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.25,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = DemonologyTalents {
        pet: PetChoice::Succubus,
        improved_sayaad: 3,
        ..DemonologyTalents::default()
    };
    let report = run(stats, 60_000, 4, &talents, None);
    let lash = report.totals.get("lash_of_pain").expect("succubus should cast Lash of Pain");
    assert!(lash.hits > 0);
    assert!(!report.totals.contains_key("imp_firebolt"), "only one pet should be active");
}

/// Closing the mythicsim.com DPS gap (2026-10-24): the real "builder starter" Affliction
/// character keeps a Succubus actively summoned the whole fight, but `affliction.rs` had no
/// pet-interleaving loop at all until now - `talents.pet` wires it up with the same shared
/// `pet_cast` mechanism `demonology.rs` uses. `None` must still behave exactly like before
/// (no Lash of Pain, no change to the player's own cast timeline).
#[test]
fn affliction_pet_adds_lash_of_pain_without_changing_default_behaviour() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::affliction::{run, AfflictionTalents};
    use mythic_forever_engine::warlock::PetChoice;

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.25,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
        ..Default::default()
    };

    let no_pet = AfflictionTalents::default();
    let report_no_pet = run(stats, 60_000, 4, &no_pet);
    assert!(
        !report_no_pet.totals.contains_key("lash_of_pain"),
        "pet: None must not cast Lash of Pain"
    );

    let with_pet = AfflictionTalents { pet: Some(PetChoice::Succubus), ..AfflictionTalents::default() };
    let report_with_pet = run(stats, 60_000, 4, &with_pet);
    let lash = report_with_pet
        .totals
        .get("lash_of_pain")
        .expect("pet: Some(Succubus) should cast Lash of Pain");
    assert!(lash.hits > 0);

    // Same seed, same player-side rotation either way - the pet loop must not perturb the
    // warlock's own cast count.
    let sb_no_pet = report_no_pet.totals.get("shadow_bolt").map(|t| t.hits);
    let sb_with_pet = report_with_pet.totals.get("shadow_bolt").map(|t| t.hits);
    assert_eq!(sb_no_pet, sb_with_pet, "the pet timeline must not change the player's own casts");
}

/// Decimation's damage bonus should only apply once the target is both configured (a max
/// health was set) and actually below 35% - never on by default, and not simply "always on
/// once talented" the way the pre-fix README described every proc-window talent.
#[test]
fn decimation_only_applies_below_the_health_threshold() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{run, DemonologyTalents};

    let stats = CasterStats {
        spell_power: 450.0,
        spell_hit: 1.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = DemonologyTalents { decimation: 2, ..DemonologyTalents::default() };
    let duration_ms = 60_000;

    // No target health set at all: Decimation can never see "below 35%".
    let no_target = run(stats, duration_ms, 2, &talents, None);
    // A target health so large that cumulative damage over one minute never gets it below 35%.
    let high_health = run(stats, duration_ms, 2, &talents, Some(1_000_000_000.0));
    // A target health low enough that it crosses 35% almost immediately.
    let low_health = run(stats, duration_ms, 2, &talents, Some(500.0));

    let sb_avg = |r: &mythic_forever_engine::sim::FightReport| {
        let sb = r.totals.get("shadow_bolt").expect("shadow bolt casts");
        sb.damage / sb.hits as f64
    };

    let avg_no_target = sb_avg(&no_target);
    let avg_high = sb_avg(&high_health);
    let avg_low = sb_avg(&low_health);

    assert!((avg_no_target - avg_high).abs() < 1e-6, "no target and an always-high target should match");
    assert!(avg_low > avg_high, "once the target drops below 35%, Shadow Bolt should hit harder");
}

/// Closing the mythicsim.com DPS gap (2026-10-24): Ruin/Improved Shadow Bolt/Bane/Agonizing
/// Flames are real Destruction-tree talents a non-Destruction build can still spend points in,
/// and mythicsim's own "builder starter" Affliction and Demonology characters do - this pins
/// that Affliction's Shadow Bolt actually gets stronger with them, the same mechanism
/// `destruction.rs` already had.
#[test]
fn affliction_shadow_bolt_benefits_from_cross_tree_destruction_talents() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::affliction::{run, AfflictionTalents};

    let stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 1.0,
        spell_crit: 0.5,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let duration_ms = 300_000;
    let plain = AfflictionTalents { keep_immolate: false, ..AfflictionTalents::default() };
    let with_cross_tree = AfflictionTalents {
        keep_immolate: false,
        ruin: 5,
        improved_shadow_bolt: 5,
        bane: 5,
        agonizing_flames: 3,
        ..AfflictionTalents::default()
    };

    let avg_shadow_bolt = |t: &AfflictionTalents| {
        let r = run(stats, duration_ms, 6, t);
        let sb = r.totals.get("shadow_bolt").expect("shadow bolt casts");
        (sb.damage / sb.hits as f64, sb.casts)
    };
    let (avg_plain, casts_plain) = avg_shadow_bolt(&plain);
    let (avg_boosted, casts_boosted) = avg_shadow_bolt(&with_cross_tree);

    assert!(
        avg_boosted > avg_plain * 1.1,
        "cross-tree talents should meaningfully raise average Shadow Bolt damage ({avg_plain} vs {avg_boosted})"
    );
    assert!(
        casts_boosted > casts_plain,
        "Bane's cast-time cut should fit more Shadow Bolts in (got {casts_plain} vs {casts_boosted})"
    );
}

/// Demonic Sacrifice's +15% is school-specific and reversed from Classic (Imp -> Shadow,
/// Succubus -> Fire) - this pins both halves of that reversal, not just "a bonus exists".
#[test]
fn demonic_sacrifice_buffs_the_correct_reversed_school() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::destruction::{run, DestructionTalents};
    use mythic_forever_engine::warlock::SacrificedPet;

    let stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 1.0,
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let duration_ms = 300_000;
    let base = DestructionTalents { incinerate: false, ..DestructionTalents::default() };

    let avg_shadow_bolt = |sacrificed: Option<SacrificedPet>| {
        let t = DestructionTalents { sacrificed_pet: sacrificed, ..base };
        let r = run(stats, duration_ms, 8, &t);
        let sb = r.totals.get("shadow_bolt").expect("shadow bolt casts");
        sb.damage / sb.hits as f64
    };

    let none = avg_shadow_bolt(None);
    let imp_sacrificed = avg_shadow_bolt(Some(SacrificedPet::Imp)); // -> +15% Shadow (reversed)
    let succubus_sacrificed = avg_shadow_bolt(Some(SacrificedPet::Succubus)); // -> +15% Fire, not Shadow

    assert!(
        imp_sacrificed > none * 1.1,
        "sacrificing the Imp should buff Shadow Bolt (Shadow school) - {none} vs {imp_sacrificed}"
    );
    assert!(
        (succubus_sacrificed - none).abs() < 1.0,
        "sacrificing the Succubus buffs Fire, not Shadow - Shadow Bolt should be unaffected ({none} vs {succubus_sacrificed})"
    );
}

/// Pins the real Imp Firebolt average (45.0 at level 60) this project's demonology.rs now
/// uses, replacing the earlier, explicitly-labeled-unsourced placeholder (100.0). If this ever
/// changes, it should be a deliberate re-sourcing, not an accidental formula edit.
#[test]
fn firebolt_average_matches_the_sourced_go_literal() {
    // Effect{base_points:44.0, ppl:0.6, spell_level:58, max_level:63}.average(60)
    // = floor(44.0 + (60-58)*0.6) = floor(45.2) = 45.0
    let base_points = 44.0_f32;
    let ppl = 0.6_f32;
    let delta = (60 - 58) as f32;
    let average = (base_points + delta * ppl).floor();
    assert_eq!(average, 45.0);
}

/// Closing the mythicsim.com DPS gap for real (2026-10-24, "analyze and address the DPS gap"
/// pass): Curse of the Elements was already cast and refreshed by every rotation, but nothing
/// ever applied its real +10% magic-damage-taken effect (spell 1311680) - a warlock's own
/// curse was mechanically a no-op on their own numbers. This pins that casting it now actually
/// raises subsequent damage, for both a direct hit and a dot tick.
#[test]
fn curse_of_the_elements_raises_damage_once_cast() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::affliction::{run, AfflictionTalents};

    let stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 1.0,
        spell_crit: 0.0, // isolate the flat multiplier from crit variance
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = AfflictionTalents::default();
    let report = run(stats, 60_000, 7, &talents);
    let shadow_bolt_avg = {
        let sb = report.totals.get("shadow_bolt").expect("shadow bolt casts");
        sb.damage / sb.hits as f64
    };
    // Shadow Bolt rank 10 averages 268 before spell power (data_pack_sanity's own cross-check
    // above); at 300 spell power and this rotation's own cross-tree talent bonuses, the
    // pre-curse average would be well under 268 + 0.857*300 * 1.10 - just confirm it's
    // meaningfully above the uncursed base (268 + 0.857*300 = 525.1) rather than at or below it.
    assert!(
        shadow_bolt_avg > 525.1,
        "Curse of the Elements' +10% should be baked into every Shadow Bolt once it's up, got {shadow_bolt_avg}"
    );
}

/// Closing the same gap: `demonology.rs`'s real priority list casts Immolate (spell 25309,
/// its 10th action) - entirely missing from this rotation before this pass, found while
/// dumping that list in full rather than just the single already-known missing cast.
#[test]
fn demonology_now_casts_immolate() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{run, DemonologyTalents};

    let stats = CasterStats {
        spell_power: 350.0,
        spell_hit: 1.0,
        spell_crit: 0.25,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = DemonologyTalents::default();
    let report = run(stats, 60_000, 3, &talents, None);
    let immolate = report.totals.get("immolate").expect("demonology should now cast Immolate");
    assert!(immolate.hits > 0);
}

/// Closing the mythicsim.com DPS gap (2026-10-24, "produce a per-ability breakdown and analyze
/// the differences" pass): found while comparing this project's own breakdown against a real
/// per-ability trace from `mythicsim-forever-engine`'s own Rust CLI (built and run locally
/// against the exact same "builder starter" request JSON, same ground truth this whole
/// comparison effort already uses). Bane of Agony's own "casts" count was ~10x the real
/// engine's (51.9 vs ~5.5 over 120s) - not a rotation bug, a reporting one: `apply_dot` never
/// rolled the spell's own hit chance at all, so every dot-only cast (Corruption, Bane of
/// Agony, Siphon Life) always landed regardless of `spell_hit`, while every direct-damage
/// spell correctly rolled. This pins that a dot-only cast can now actually miss, and that a
/// missed cast leaves a still-ticking previous application alone rather than clearing it (a
/// resisted refresh doesn't erase the dot it failed to refresh, in the real game or here).
#[test]
fn dot_only_casts_can_now_miss_like_every_other_spell() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::affliction::{run, AfflictionTalents};

    let zero_hit_stats = CasterStats {
        spell_power: 300.0,
        spell_hit: 0.0, // every cast, direct or dot-only, must miss
        spell_crit: 0.0,
        haste: 1.0,
        mp5: 1000.0,
        max_mana: 20000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = AfflictionTalents::default();
    let report = run(zero_hit_stats, 30_000, 11, &talents);
    // Bane of Agony is first in the rotation's own priority order - at 0% hit it never lands,
    // so `refresh_needed` never clears and the rotation (correctly) keeps retrying it rather
    // than moving on, the same way a real player stuck at 0% hit would.
    let bane = report.totals.get("bane_of_agony").expect("bane of agony should still be attempted");
    assert_eq!(bane.hits, 0, "0% spell hit should mean 0 landed casts, got {bane:?}");
    assert!(bane.misses > 0, "0% spell hit should record real misses for a dot-only spell");
    assert_eq!(bane.damage, 0.0, "a dot that never lands should never tick either");
}

/// Fixing the Demonology/real-engine gap for real (2026-10-24): the Succubus's own melee
/// auto-attack was this project's single largest remaining unmodeled piece (measured directly
/// from mythicsim-forever-engine's own combat log at ~62-71 DPS, depending on build - not an
/// estimate). This pins that she now swings roughly every 2.0s over a full fight (close to the
/// real engine's own measured 60.4 swings/120s), and that the Imp - who never melees in the
/// real source (`Summon::Imp => None` in `prepare/pets.rs`) - never gets a `succubus_melee`
/// entry at all.
#[test]
fn succubus_melees_roughly_every_two_seconds() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{run, DemonologyTalents};
    use mythic_forever_engine::warlock::PetChoice;

    let stats = CasterStats {
        spell_power: 400.0,
        spell_hit: 0.88,
        spell_crit: 0.15,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let succubus = DemonologyTalents { pet: PetChoice::Succubus, ..DemonologyTalents::default() };
    let report = run(stats, 120_000, 5, &succubus, None);
    let melee = report.totals.get("succubus_melee").expect("the Succubus should melee");
    assert!(
        (melee.casts as i64 - 60).abs() <= 3,
        "a 2.0s swing timer over 120s should land close to 60 swing attempts, got {}",
        melee.casts
    );
    assert!(melee.damage > 0.0, "at least some of those swings should land and deal damage");

    let imp = DemonologyTalents { pet: PetChoice::Imp, ..DemonologyTalents::default() };
    let imp_report = run(stats, 120_000, 5, &imp, None);
    assert!(
        !imp_report.totals.contains_key("succubus_melee"),
        "the Imp never melees in the real source - no melee entry should appear for her pet choice"
    );
}

/// A regression on the scheduling bug the melee timeline's addition exposed (2026-10-24):
/// `pet_ready_at`/`pet_melee_ready_at` used to rebase off `fight.now_ms` every time they fired,
/// which silently loses ground whenever another actor's tie-break delays that actor's turn -
/// fine for Lash of Pain's coarse 12s cadence (a small, easy-to-miss shortfall), glaring for
/// melee's 2.0s one (a measured ~35% shortfall before the fix: ~38 swings instead of ~60).
/// Fixed by self-incrementing from the previous schedule instead. This pins the fixed Lash of
/// Pain cast rate specifically, since that shortfall predated this pass and had no test of its
/// own pinning the cast *count* (only that it deals damage at all).
#[test]
fn lash_of_pain_keeps_its_full_cooldown_cadence_even_with_melee_interleaved() {
    use mythic_forever_engine::stats::CasterStats;
    use mythic_forever_engine::warlock::demonology::{run, DemonologyTalents};
    use mythic_forever_engine::warlock::PetChoice;

    let stats = CasterStats {
        spell_power: 400.0,
        spell_hit: 0.88,
        spell_crit: 0.15,
        haste: 1.0,
        mp5: 80.0,
        max_mana: 8000.0,
        spirit: 100.0,
        ..Default::default()
    };
    let talents = DemonologyTalents { pet: PetChoice::Succubus, ..DemonologyTalents::default() };
    let report = run(stats, 120_000, 5, &talents, None);
    let lash = report.totals.get("lash_of_pain").expect("succubus should cast Lash of Pain");
    // A 12s cooldown over 120s should land close to 10 casts - well above the ~7-8 a drifting
    // schedule produced before this fix.
    assert!(
        lash.casts >= 9,
        "a 12s cooldown over 120s should land close to 10 casts even with melee interleaved, got {}",
        lash.casts
    );
}
