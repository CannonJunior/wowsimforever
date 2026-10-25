//! Golden-file regression test (Plan Section 0.3 / 4.5): pins today's
//! shipped data pack plus a fixed Warlock build to a recorded expected
//! output. If a future data-pack edit (a patch update, a curation fix)
//! silently changes the simulated DPS or the dispute delta, this test
//! fails and forces a conscious decision -- update the golden file with a
//! reviewed rationale, or treat the change as a bug.
//!
//! This intentionally tests against the real shipped data pack, not a
//! synthetic fixture, so it also catches JSON-schema drift between the
//! `wowforever-data` types and `data-packs/forever-beta-2026-10-16.json`.

use std::fs;
use std::path::PathBuf;
use wowforever_data::DataPack;
use wowforever_engine::{simulate, simulate_dispute_delta, BuildConfig, PetType};

fn load_shipped_data_pack() -> DataPack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data-packs/forever-beta-2026-10-17-rev9.json");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("golden test could not read {}: {e}", path.display()));
    DataPack::from_json_str(&raw).unwrap_or_else(|e| panic!("golden test could not parse {}: {e}", path.display()))
}

fn golden_build() -> BuildConfig {
    BuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["corruption".into(), "immolate".into(), "shadow_bolt".into()],
        pet_type: PetType::Imp,
        demonic_sacrifice_active: true,
        has_demonic_pact: true,
        resummon_event_ms: Some(30_000),
        resummon_is_same_pet: true,
        dispute_rule_override: None,
        starting_mana: None,
        mana_regen_per_second: 0.0,
        attack_power: 0.0,
        weapon_min: 0.0,
        weapon_max: 0.0,
        weapon_speed_ms: 0,
        rage_enabled: false,
        is_two_handed: false,
        heroic_strike_spell_id: None,
        max_mana: 0.0,
    }
}

const GOLDEN_SEED: u64 = 1;
const GOLDEN_ITERATIONS: u32 = 10_000;

// Recorded against data pack version "forever-beta-2026-10-17-rev3" and
// unchanged under "-rev4" (which only appends Mage spells -- the Warlock
// demo build below doesn't reference any of them, so its DPS is identical;
// verified, not assumed, when the path was bumped to -rev4) and under
// "-rev6" (which adds a `raid_debuffs` entry that plain `simulate` never
// reads at all -- raid debuffs only affect `wowforever-raid`'s
// `simulate_raid_with_debuffs`, so this golden build's DPS is untouched by
// construction, not just by observation) and under "-rev7" (which only
// appends Druid/Shaman/Paladin spells -- the Warlock golden build below
// doesn't reference any of them).
// (crates/cli's demo build, 10_000 iterations, seed 1). Reviewed reason:
// REV2's WebSearch-synthesized damage ranges for Shadow Bolt/Shadowburn
// (and Immolate's single-source base values) were themselves replaced by
// values read directly from thewowdb.com's structured, explicitly
// client-data-cited spell pages -- a materially more reliable source (see
// the data pack's game_build_reference and each spell's source note).
// Notably, this also revealed none of these spells have a real min/max
// range in WoW Forever at all (flat base values), so REV2's ranges are
// gone entirely, not just re-measured.
const GOLDEN_MEAN_DPS: f64 = 251.52545986250942;
const GOLDEN_DISPUTE_DELTA_DPS: f64 = 13.819086506250756;

#[test]
fn shipped_data_pack_mean_dps_matches_golden_value() {
    let data = load_shipped_data_pack();
    let build = golden_build();
    let result = simulate(&data, &build, GOLDEN_ITERATIONS, GOLDEN_SEED);

    assert!(
        (result.dps.mean - GOLDEN_MEAN_DPS).abs() < 1e-6,
        "mean DPS drifted from the recorded golden value: got {}, expected {}. \
         If this is an intentional data-pack or engine change, update GOLDEN_MEAN_DPS \
         with a reviewed reason, per Plan Section 0.3.",
        result.dps.mean,
        GOLDEN_MEAN_DPS
    );
}

#[test]
fn shipped_data_pack_dispute_delta_matches_golden_value() {
    let data = load_shipped_data_pack();
    let build = golden_build();
    let delta = simulate_dispute_delta(&data, &build, GOLDEN_ITERATIONS, GOLDEN_SEED)
        .expect("golden build is constructed to exercise the Demonic Sacrifice dispute");

    assert!(
        (delta.mean_dps_delta - GOLDEN_DISPUTE_DELTA_DPS).abs() < 1e-6,
        "dispute DPS delta drifted from the recorded golden value: got {}, expected {}.",
        delta.mean_dps_delta,
        GOLDEN_DISPUTE_DELTA_DPS
    );
}
