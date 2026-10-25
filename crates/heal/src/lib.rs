//! Healer-throughput model (Plan Section 8: "a healing-throughput model
//! (healer output vs. a simulated incoming raid-damage profile)").
//!
//! This now has both halves, but they are not the same *kind* of claim and
//! must not be read as equally grounded:
//!
//! - Healer **output** (`simulate_hps`): a real, tested HPS simulation for
//!   a rotation of healing spells, with real sourced spell data
//!   (`crates/ingest` against thewowdb.com, same discipline as every
//!   damage spell in this project).
//! - **Incoming raid damage** (`evaluate_healing_sufficiency`,
//!   `SyntheticIncomingDamageProfile`): there is no real encounter/boss
//!   damage data for WoW Forever, because the game hasn't launched and no
//!   combat logs exist to source from. Rather than leave "does the healer
//!   keep the raid alive" permanently unanswerable, this models incoming
//!   damage as an explicit, caller-supplied *assumption* -- a number the
//!   caller states and owns, analogous to how `--overrides` requires an
//!   explicit flag for placeholder spell data. `SyntheticIncomingDamageProfile`
//!   is never treated as sourced, never defaults to anything, and its
//!   confidence is not a `Confidence` tag at all (that taxonomy is for game
//!   facts; this isn't one) -- it is a modeling input the caller must
//!   justify.
//!
//! Why this isn't a new simulation engine: healing math in this data model
//! is *literally identical* to damage math -- `base + coefficient *
//! spell_power`, the same crit multiplier, the same GCD/cast-time
//! scheduling, the same DoT-shaped periodic-tick structure (a HoT like
//! Renew uses `DotDef` exactly as Corruption does; the game's own client
//! data doesn't distinguish them structurally either -- both are "Apply
//! Aura, periodic" effects, see `crates/ingest`'s Renew-vs-Corruption
//! regex reuse). Reimplementing that scheduling/crit logic here would be
//! pure duplication with no new engineering value, so this crate is a
//! thin, honestly-labeled wrapper around `wowforever_engine::simulate`,
//! not a parallel engine. The Warlock-specific fields on `BuildConfig`
//! (pet, Demonic Sacrifice, the dispute mechanism) are set to inert
//! no-op defaults for a healer build, not omitted or faked.

use serde::{Deserialize, Serialize};
use wowforever_data::DataPack;
use wowforever_engine::{simulate, simulate_samples, BuildConfig, PetType};
use wowforever_stats::{stats_from, DpsStats};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealBuildConfig {
    pub spell_power: f64,
    pub crit_chance: f64,
    pub fight_duration_ms: u32,
    /// Ordered spell-id priority list, same simplified-APL model as the DPS
    /// engine (Plan Section 2.2/3) -- not real HoT-uptime-maintenance logic
    /// beyond what `BuildConfig`'s rotation already does for DoTs.
    pub rotation: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HealOutput {
    pub hps: DpsStats,
    pub data_pack_version: String,
}

/// Public so other crates (e.g. `wowforever-wasm`, for a detailed
/// per-event breakdown of a heal build) can reuse the exact same
/// conversion rather than risk a second, possibly-drifting copy.
pub fn to_engine_build(build: &HealBuildConfig) -> BuildConfig {
    BuildConfig {
        spell_power: build.spell_power,
        crit_chance: build.crit_chance,
        fight_duration_ms: build.fight_duration_ms,
        rotation: build.rotation.clone(),
        // Inert no-op defaults: a healer build has no pet, no Demonic
        // Sacrifice, and never exercises the dispute mechanism -- these
        // fields are Warlock-specific and meaningless here, set to values
        // that guarantee they have zero effect rather than omitted.
        pet_type: PetType::Imp,
        demonic_sacrifice_active: false,
        has_demonic_pact: false,
        resummon_event_ms: None,
        resummon_is_same_pet: false,
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

pub fn simulate_hps(data: &DataPack, build: &HealBuildConfig, iterations: u32, seed: u64) -> HealOutput {
    let engine_build = to_engine_build(build);
    let result = simulate(data, &engine_build, iterations, seed);
    HealOutput { hps: result.dps, data_pack_version: result.data_pack_version }
}

/// A caller-supplied assumption about incoming raid damage -- **not**
/// sourced from any real encounter (see module docs for why no such data
/// can exist yet). `name` exists so the caller is forced to write down
/// what this assumption represents (e.g. "illustrative: one boss hit every
/// 2s for 1200, split across 5 raid members") rather than passing a bare
/// number with no record of what it was meant to approximate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticIncomingDamageProfile {
    pub name: String,
    /// Total raid-wide incoming damage per second this one healer is
    /// assumed to be responsible for covering (e.g. the raid's total
    /// incoming DPS divided by however many healers are assigned to it --
    /// that division is the caller's judgment call, not this crate's).
    pub raid_damage_per_second: f64,
}

#[derive(Debug, Clone)]
pub struct HealingSufficiencyOutput {
    pub healer_hps: DpsStats,
    pub incoming_profile_name: String,
    pub raid_damage_per_second: f64,
    /// `raid_damage_per_second - healer_hps.mean`: positive means this
    /// profile's assumed damage outpaces this healer's average output
    /// (the raid loses net health over time under this assumption).
    pub mean_deficit_per_second: f64,
    /// Fraction of Monte Carlo iterations (0-100) in which this healer's
    /// *that-iteration* HPS met or exceeded the assumed incoming damage --
    /// a distributional answer to "how often is this enough," not just a
    /// single mean-vs-mean comparison.
    pub pct_iterations_sufficient: f64,
}

/// Compares a real, simulated healer-output distribution against an
/// explicitly-labeled synthetic incoming-damage assumption. See
/// `SyntheticIncomingDamageProfile`'s doc comment and this module's docs
/// for why the second half of that comparison is not sourced data and must
/// never be presented as though it were.
pub fn evaluate_healing_sufficiency(
    data: &DataPack,
    build: &HealBuildConfig,
    profile: &SyntheticIncomingDamageProfile,
    iterations: u32,
    seed: u64,
) -> HealingSufficiencyOutput {
    let engine_build = to_engine_build(build);
    let mut samples = simulate_samples(data, &engine_build, iterations, seed);
    let sufficient = samples.iter().filter(|&&s| s >= profile.raid_damage_per_second).count();
    let pct_iterations_sufficient = sufficient as f64 / samples.len() as f64 * 100.0;
    let healer_hps = stats_from(&mut samples);

    HealingSufficiencyOutput {
        mean_deficit_per_second: profile.raid_damage_per_second - healer_hps.mean,
        healer_hps,
        incoming_profile_name: profile.name.clone(),
        raid_damage_per_second: profile.raid_damage_per_second,
        pct_iterations_sufficient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowforever_data::{Confidence, DotDef, School, Source, SpellDef};

    fn demo_data_pack() -> DataPack {
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![
                SpellDef {
                    id: "renew".into(),
                    name: "Renew".into(),
                    school: School::Holy,
                    cast_time_ms: 0,
                    cooldown_ms: 0,
                    resource_cost: 410.0,
                    base_min: 0.0,
                    base_max: 0.0,
                    coefficient: 0.0,
                    ap_coefficient: 0.0,
                    resource_type: wowforever_data::ResourceType::Mana,
                    uses_weapon_damage: false,
                    mana_coefficient: 0.0,
                    stacking_buff: None,
                    timed_buff: None,
                    dot: Some(DotDef { tick_interval_ms: 3000, num_ticks: 5, tick_base: 166.0, tick_coefficient: 0.2, can_crit: false }),
                    confidence: Confidence::BetaDatamine,
                    source: Source { name: "test".into(), url: None },
                },
                SpellDef {
                    id: "flash_heal".into(),
                    name: "Flash Heal".into(),
                    school: School::Holy,
                    cast_time_ms: 1500,
                    cooldown_ms: 0,
                    resource_cost: 380.0,
                    base_min: 825.0,
                    base_max: 825.0,
                    coefficient: 0.429,
                    ap_coefficient: 0.0,
                    resource_type: wowforever_data::ResourceType::Mana,
                    uses_weapon_damage: false,
                    mana_coefficient: 0.0,
                    stacking_buff: None,
                    timed_buff: None,
                    dot: None,
                    confidence: Confidence::BetaDatamine,
                    source: Source { name: "test".into(), url: None },
                },
            ],
            disputed_mechanics: vec![],
            dispute_ledger: vec![],
            demonic_sacrifice_effects: vec![],
            raid_debuffs: vec![],
        }
    }

    fn base_build() -> HealBuildConfig {
        HealBuildConfig { spell_power: 350.0, crit_chance: 0.0, fight_duration_ms: 30_000, rotation: vec!["flash_heal".into()] }
    }

    /// Hand-computed: with crit disabled and a 30s fight filled entirely by
    /// repeated 1.5s-cast Flash Heals, total healing should equal exactly
    /// 20 casts * (825 + 0.429*350).
    #[test]
    fn hps_matches_hand_computed_value_for_a_pure_direct_heal_rotation() {
        let data = demo_data_pack();
        let build = base_build();
        let result = simulate_hps(&data, &build, 1000, 1);

        let per_cast = 825.0 + 0.429 * 350.0;
        let casts = 30_000 / 1500; // 20
        let expected_hps = (per_cast * casts as f64) / 30.0;
        assert!((result.hps.mean - expected_hps).abs() < 1e-6, "expected {expected_hps}, got {}", result.hps.mean);
    }

    /// A HoT-only rotation with no filler legitimately has nothing to cast
    /// once Renew is still ticking -- the same "rotation exhausted" panic
    /// Corruption/Immolate-only rotations would hit in the DPS engine. Real
    /// healer play (like real Warlock/Mage play) needs a filler spell;
    /// Renew + Flash Heal mirrors the maintenance-DoT-plus-filler pattern
    /// the Warlock demo build already uses.
    #[test]
    fn hot_plus_filler_rotation_produces_real_positive_hps() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.rotation = vec!["renew".into(), "flash_heal".into()];
        let result = simulate_hps(&data, &build, 1000, 1);
        assert!(result.hps.mean > 0.0);
    }

    #[test]
    fn data_pack_version_is_passed_through() {
        let data = demo_data_pack();
        let result = simulate_hps(&data, &base_build(), 100, 1);
        assert_eq!(result.data_pack_version, "test-fixture");
    }

    /// Hand-computed, crit disabled: per-cast healing is exactly
    /// 825 + 0.429*350 = 975.15, HPS = 20 casts/30s = 650.1. A profile
    /// demanding less than that must report 100% of iterations sufficient
    /// and a negative (surplus) deficit.
    #[test]
    fn healing_sufficiency_reports_full_coverage_when_profile_demands_less_than_output() {
        let data = demo_data_pack();
        let build = base_build(); // crit_chance 0.0 -> deterministic HPS every iteration
        let profile = SyntheticIncomingDamageProfile {
            name: "test: well below this healer's deterministic output".into(),
            raid_damage_per_second: 500.0,
        };
        let result = evaluate_healing_sufficiency(&data, &build, &profile, 200, 1);
        assert_eq!(result.pct_iterations_sufficient, 100.0);
        assert!(result.mean_deficit_per_second < 0.0, "a surplus should report as a negative deficit, got {}", result.mean_deficit_per_second);
    }

    /// Symmetric case: a profile demanding more than this (deterministic,
    /// crit-disabled) healer can ever produce must report 0% sufficient
    /// and a positive deficit.
    #[test]
    fn healing_sufficiency_reports_no_coverage_when_profile_demands_more_than_output() {
        let data = demo_data_pack();
        let build = base_build();
        let profile = SyntheticIncomingDamageProfile {
            name: "test: well above this healer's deterministic output".into(),
            raid_damage_per_second: 10_000.0,
        };
        let result = evaluate_healing_sufficiency(&data, &build, &profile, 200, 1);
        assert_eq!(result.pct_iterations_sufficient, 0.0);
        assert!(result.mean_deficit_per_second > 0.0);
    }

    /// With real crit variance, a profile set between the no-crit and
    /// max-crit HPS should be sufficient on *some* iterations and not
    /// others -- confirms this is a genuine per-iteration Monte Carlo
    /// comparison, not just a mean-vs-mean check dressed up as one.
    #[test]
    fn healing_sufficiency_is_a_real_per_iteration_distributional_comparison() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.crit_chance = 0.5; // real variance across iterations
        let no_crit_hps = 975.15_f64 * 20.0 / 30.0; // 650.1
        let profile = SyntheticIncomingDamageProfile {
            name: "test: between no-crit and max-crit output".into(),
            raid_damage_per_second: no_crit_hps * 1.1, // above the no-crit floor, below the crit ceiling
        };
        let result = evaluate_healing_sufficiency(&data, &build, &profile, 2000, 1);
        assert!(result.pct_iterations_sufficient > 0.0 && result.pct_iterations_sufficient < 100.0, "expected a mixed outcome, got {}%", result.pct_iterations_sufficient);
    }
}
