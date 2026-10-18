//! Client-side simulation compute (Plan Section 5: "the core simulation
//! engine compiles to run client-side (e.g. WASM, mirroring
//! `wowsims/classic`'s own approach) so that user-initiated simulation
//! runs cost this app's server budget nothing marginal").
//!
//! Design: every real decision lives in a plain, natively-testable Rust
//! function (`simulate_dps_core`, `simulate_burst_core`) that takes and
//! returns JSON strings -- no network, no filesystem, no OS clock, nothing
//! that `wasm32-unknown-unknown` can't do. The `#[wasm_bindgen]`-annotated
//! wrappers are thin pass-throughs with no logic of their own, so `cargo
//! test` on a native target exercises the exact same code the browser
//! runs, without needing a wasm test runner. See `tests/` for the native
//! tests, and `web/` (the frontend) plus this crate's README for how the
//! actual `wasm32-unknown-unknown` build is produced and smoke-tested with
//! Node before being trusted in a browser.

use serde::Serialize;
use std::panic;
use wasm_bindgen::prelude::*;
use wowforever_burst::{evaluate_sequence, find_best_burst_sequence, BurstBuildConfig};
use wowforever_data::DataPack;
use wowforever_engine::{simulate, simulate_dispute_delta, BuildConfig};
use wowforever_heal::{simulate_hps, HealBuildConfig};

/// Runs `f`, converting a Rust panic into a readable error string on
/// every *other* target. **Does not work on `wasm32-unknown-unknown`**:
/// that target has no stack-unwinding support in stable Rust, so a panic
/// there is always an abort/trap regardless of `catch_unwind` -- verified
/// directly against the real compiled `.wasm` in a browser, not assumed
/// (calling `simulate_dps` with an unknown rotation spell id still threw
/// a bare `unreachable` even with this wrapper in place). Kept anyway as
/// a real safety net for native testing and any future non-wasm embedding
/// of this crate, but **the actual browser-facing fix is
/// `validate_spell_ids` below, called *before* any panic-prone engine
/// code runs**, not this function. See that function's doc comment for
/// the concrete bug this closes: a build JSON that's syntactically valid
/// but references a spell id missing from whichever data pack revision
/// got loaded (the frontend's hardcoded data-pack path going stale after
/// a new revision ships -- this has now happened twice).
fn catch_sim_panic<T>(f: impl FnOnce() -> T + panic::UnwindSafe) -> Result<T, String> {
    panic::catch_unwind(f).map_err(|payload| {
        if let Some(s) = payload.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "simulation panicked with a non-string payload".to_string()
        }
    })
}

/// Checks every spell id in `ids` against `data`, before any panic-prone
/// engine code (`BuildConfig.rotation`, `BurstBuildConfig
/// .available_spell_ids`) runs -- this is the actual fix for the
/// `wasm32-unknown-unknown` "an unknown spell id traps the whole wasm
/// instance with no error message" problem `catch_sim_panic` cannot solve
/// on this target (see its doc comment). Returns the first missing id
/// found, named explicitly, so the resulting error message says exactly
/// which id was the problem rather than a generic "simulation failed."
fn validate_spell_ids<'a>(data: &DataPack, ids: impl IntoIterator<Item = &'a String>) -> Result<(), String> {
    for id in ids {
        if data.spell(id).is_none() {
            return Err(format!(
                "build references spell id `{id}`, which is not in the loaded data pack (version `{}`) -- \
                 check that the frontend's data-pack path matches the version this build was written against",
                data.version
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct WasmDispute {
    mechanic_id: String,
    rule_a_label: String,
    rule_a_mean_dps: f64,
    rule_b_label: String,
    rule_b_mean_dps: f64,
    mean_dps_delta: f64,
    pct_swing: f64,
}

#[derive(Debug, Serialize)]
struct WasmDpsResult {
    ok: bool,
    error: Option<String>,
    data_pack_version: Option<String>,
    mean: Option<f64>,
    stdev: Option<f64>,
    p5: Option<f64>,
    p50: Option<f64>,
    p95: Option<f64>,
    touches_disputed_mechanic: Option<bool>,
    dispute: Option<WasmDispute>,
}

impl WasmDpsResult {
    fn err(message: impl Into<String>) -> Self {
        WasmDpsResult {
            ok: false,
            error: Some(message.into()),
            data_pack_version: None,
            mean: None,
            stdev: None,
            p5: None,
            p50: None,
            p95: None,
            touches_disputed_mechanic: None,
            dispute: None,
        }
    }
}

/// The real logic, independent of wasm-bindgen -- takes/returns plain
/// strings so it's testable with a normal `#[test]` on any target.
pub fn simulate_dps_core(data_pack_json: &str, build_json: &str, iterations: u32, seed: u64) -> String {
    let data = match DataPack::from_json_str(data_pack_json) {
        Ok(d) => d,
        Err(e) => return serde_json::to_string(&WasmDpsResult::err(format!("invalid data pack JSON: {e}"))).unwrap(),
    };
    let build: BuildConfig = match serde_json::from_str(build_json) {
        Ok(b) => b,
        Err(e) => return serde_json::to_string(&WasmDpsResult::err(format!("invalid build JSON: {e}"))).unwrap(),
    };
    if let Err(msg) = validate_spell_ids(&data, &build.rotation) {
        return serde_json::to_string(&WasmDpsResult::err(msg)).unwrap();
    }

    let result = match catch_sim_panic(|| simulate(&data, &build, iterations, seed)) {
        Ok(r) => r,
        Err(msg) => return serde_json::to_string(&WasmDpsResult::err(msg)).unwrap(),
    };
    let dispute = catch_sim_panic(|| simulate_dispute_delta(&data, &build, iterations, seed))
        .unwrap_or(None)
        .map(|d| WasmDispute {
            mechanic_id: d.mechanic_id,
            rule_a_label: d.rule_a_label,
            rule_a_mean_dps: d.rule_a_dps.mean,
            rule_b_label: d.rule_b_label,
            rule_b_mean_dps: d.rule_b_dps.mean,
            mean_dps_delta: d.mean_dps_delta,
            pct_swing: d.pct_swing,
        });

    let out = WasmDpsResult {
        ok: true,
        error: None,
        data_pack_version: Some(result.data_pack_version),
        mean: Some(result.dps.mean),
        stdev: Some(result.dps.stdev),
        p5: Some(result.dps.p5),
        p50: Some(result.dps.p50),
        p95: Some(result.dps.p95),
        touches_disputed_mechanic: Some(result.touches_disputed_mechanic),
        dispute,
    };
    serde_json::to_string(&out).expect("WasmDpsResult must serialize")
}

#[derive(Debug, Serialize)]
struct WasmDetailResult {
    ok: bool,
    error: Option<String>,
    fight_duration_ms: Option<u32>,
    events: Option<Vec<wowforever_engine::DetailEvent>>,
    abilities: Option<Vec<wowforever_engine::AbilityBreakdown>>,
    buffs: Option<Vec<wowforever_engine::BuffWindow>>,
}

impl WasmDetailResult {
    fn err(message: impl Into<String>) -> Self {
        WasmDetailResult { ok: false, error: Some(message.into()), fight_duration_ms: None, events: None, abilities: None, buffs: None }
    }
}

/// One representative run's full detail (ability breakdown, cast
/// timeline, buff windows) -- see `wowforever_engine::simulate_detail`'s
/// doc comment for why this is a *separate* kind of output from
/// `simulate_dps_core`'s Monte Carlo summary, not a replacement for it.
pub fn simulate_dps_detail_core(data_pack_json: &str, build_json: &str, seed: u64) -> String {
    let data = match DataPack::from_json_str(data_pack_json) {
        Ok(d) => d,
        Err(e) => return serde_json::to_string(&WasmDetailResult::err(format!("invalid data pack JSON: {e}"))).unwrap(),
    };
    let build: BuildConfig = match serde_json::from_str(build_json) {
        Ok(b) => b,
        Err(e) => return serde_json::to_string(&WasmDetailResult::err(format!("invalid build JSON: {e}"))).unwrap(),
    };
    if let Err(msg) = validate_spell_ids(&data, &build.rotation) {
        return serde_json::to_string(&WasmDetailResult::err(msg)).unwrap();
    }

    let detail = match catch_sim_panic(|| wowforever_engine::simulate_detail(&data, &build, seed)) {
        Ok(d) => d,
        Err(msg) => return serde_json::to_string(&WasmDetailResult::err(msg)).unwrap(),
    };

    let out = WasmDetailResult {
        ok: true,
        error: None,
        fight_duration_ms: Some(detail.fight_duration_ms),
        events: Some(detail.events),
        abilities: Some(detail.abilities),
        buffs: Some(detail.buffs),
    };
    serde_json::to_string(&out).expect("WasmDetailResult must serialize")
}

#[wasm_bindgen]
pub fn simulate_dps_detail(data_pack_json: &str, build_json: &str, seed: u32) -> String {
    simulate_dps_detail_core(data_pack_json, build_json, seed as u64)
}

#[derive(Debug, Serialize)]
struct WasmBurstResult {
    ok: bool,
    error: Option<String>,
    expected_damage: Option<f64>,
    guaranteed_floor: Option<f64>,
    mc_mean: Option<f64>,
    mc_p5: Option<f64>,
    mc_p50: Option<f64>,
    mc_p95: Option<f64>,
    provably_optimal: Option<bool>,
    sequence: Option<Vec<(u32, String)>>,
}

pub fn simulate_burst_core(data_pack_json: &str, build_json: &str, mc_iterations: u32, seed: u64) -> String {
    let data = match DataPack::from_json_str(data_pack_json) {
        Ok(d) => d,
        Err(e) => return serde_json::to_string(&err_burst(format!("invalid data pack JSON: {e}"))).unwrap(),
    };
    let build: BurstBuildConfig = match serde_json::from_str(build_json) {
        Ok(b) => b,
        Err(e) => return serde_json::to_string(&err_burst(format!("invalid build JSON: {e}"))).unwrap(),
    };
    if let Err(msg) = validate_spell_ids(&data, &build.available_spell_ids) {
        return serde_json::to_string(&err_burst(msg)).unwrap();
    }

    let search = match catch_sim_panic(|| find_best_burst_sequence(&data, &build)) {
        Ok(s) => s,
        Err(msg) => return serde_json::to_string(&err_burst(msg)).unwrap(),
    };
    let eval = match catch_sim_panic(|| evaluate_sequence(&data, &build, &search.sequence, mc_iterations, seed)) {
        Ok(e) => e,
        Err(msg) => return serde_json::to_string(&err_burst(msg)).unwrap(),
    };

    let out = WasmBurstResult {
        ok: true,
        error: None,
        expected_damage: Some(search.expected_damage),
        guaranteed_floor: Some(eval.guaranteed_floor),
        mc_mean: Some(eval.monte_carlo.mean),
        mc_p5: Some(eval.monte_carlo.p5),
        mc_p50: Some(eval.monte_carlo.p50),
        mc_p95: Some(eval.monte_carlo.p95),
        provably_optimal: Some(search.provably_optimal),
        sequence: Some(search.sequence),
    };
    serde_json::to_string(&out).expect("WasmBurstResult must serialize")
}

fn err_burst(message: impl Into<String>) -> WasmBurstResult {
    WasmBurstResult {
        ok: false,
        error: Some(message.into()),
        expected_damage: None,
        guaranteed_floor: None,
        mc_mean: None,
        mc_p5: None,
        mc_p50: None,
        mc_p95: None,
        provably_optimal: None,
        sequence: None,
    }
}

#[wasm_bindgen]
pub fn simulate_dps(data_pack_json: &str, build_json: &str, iterations: u32, seed: u32) -> String {
    simulate_dps_core(data_pack_json, build_json, iterations, seed as u64)
}

#[wasm_bindgen]
pub fn simulate_burst(data_pack_json: &str, build_json: &str, mc_iterations: u32, seed: u32) -> String {
    simulate_burst_core(data_pack_json, build_json, mc_iterations, seed as u64)
}

#[derive(Debug, Serialize)]
struct WasmHealResult {
    ok: bool,
    error: Option<String>,
    data_pack_version: Option<String>,
    mean: Option<f64>,
    stdev: Option<f64>,
    p5: Option<f64>,
    p50: Option<f64>,
    p95: Option<f64>,
}

pub fn simulate_hps_core(data_pack_json: &str, build_json: &str, iterations: u32, seed: u64) -> String {
    let data = match DataPack::from_json_str(data_pack_json) {
        Ok(d) => d,
        Err(e) => {
            return serde_json::to_string(&WasmHealResult {
                ok: false,
                error: Some(format!("invalid data pack JSON: {e}")),
                data_pack_version: None,
                mean: None,
                stdev: None,
                p5: None,
                p50: None,
                p95: None,
            })
            .unwrap()
        }
    };
    let build: HealBuildConfig = match serde_json::from_str(build_json) {
        Ok(b) => b,
        Err(e) => {
            return serde_json::to_string(&WasmHealResult {
                ok: false,
                error: Some(format!("invalid build JSON: {e}")),
                data_pack_version: None,
                mean: None,
                stdev: None,
                p5: None,
                p50: None,
                p95: None,
            })
            .unwrap()
        }
    };
    if let Err(msg) = validate_spell_ids(&data, &build.rotation) {
        return serde_json::to_string(&WasmHealResult {
            ok: false,
            error: Some(msg),
            data_pack_version: None,
            mean: None,
            stdev: None,
            p5: None,
            p50: None,
            p95: None,
        })
        .unwrap();
    }

    let result = match catch_sim_panic(|| simulate_hps(&data, &build, iterations, seed)) {
        Ok(r) => r,
        Err(msg) => {
            return serde_json::to_string(&WasmHealResult {
                ok: false,
                error: Some(msg),
                data_pack_version: None,
                mean: None,
                stdev: None,
                p5: None,
                p50: None,
                p95: None,
            })
            .unwrap()
        }
    };
    serde_json::to_string(&WasmHealResult {
        ok: true,
        error: None,
        data_pack_version: Some(result.data_pack_version),
        mean: Some(result.hps.mean),
        stdev: Some(result.hps.stdev),
        p5: Some(result.hps.p5),
        p50: Some(result.hps.p50),
        p95: Some(result.hps.p95),
    })
    .expect("WasmHealResult must serialize")
}

#[wasm_bindgen]
pub fn simulate_heal(data_pack_json: &str, build_json: &str, iterations: u32, seed: u32) -> String {
    simulate_hps_core(data_pack_json, build_json, iterations, seed as u64)
}

/// Same shape as `simulate_dps_detail_core`, for a `HealBuildConfig`
/// instead -- reuses `wowforever_heal::to_engine_build` (the same
/// conversion `simulate_hps` itself uses) so this can't drift from how
/// `--mode heal` actually builds its engine call.
pub fn simulate_hps_detail_core(data_pack_json: &str, build_json: &str, seed: u64) -> String {
    let data = match DataPack::from_json_str(data_pack_json) {
        Ok(d) => d,
        Err(e) => return serde_json::to_string(&WasmDetailResult::err(format!("invalid data pack JSON: {e}"))).unwrap(),
    };
    let build: HealBuildConfig = match serde_json::from_str(build_json) {
        Ok(b) => b,
        Err(e) => return serde_json::to_string(&WasmDetailResult::err(format!("invalid build JSON: {e}"))).unwrap(),
    };
    let engine_build = wowforever_heal::to_engine_build(&build);
    if let Err(msg) = validate_spell_ids(&data, &engine_build.rotation) {
        return serde_json::to_string(&WasmDetailResult::err(msg)).unwrap();
    }

    let detail = match catch_sim_panic(|| wowforever_engine::simulate_detail(&data, &engine_build, seed)) {
        Ok(d) => d,
        Err(msg) => return serde_json::to_string(&WasmDetailResult::err(msg)).unwrap(),
    };

    let out = WasmDetailResult {
        ok: true,
        error: None,
        fight_duration_ms: Some(detail.fight_duration_ms),
        events: Some(detail.events),
        abilities: Some(detail.abilities),
        buffs: Some(detail.buffs),
    };
    serde_json::to_string(&out).expect("WasmDetailResult must serialize")
}

#[wasm_bindgen]
pub fn simulate_hps_detail(data_pack_json: &str, build_json: &str, seed: u32) -> String {
    simulate_hps_detail_core(data_pack_json, build_json, seed as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATA_PACK_JSON: &str = r#"{
        "version": "wasm-test-fixture",
        "game_build_reference": "test",
        "spells": [
            {
                "id": "shadow_bolt", "name": "Shadow Bolt", "school": "shadow",
                "cast_time_ms": 3000, "cooldown_ms": 0, "resource_cost": 370.0,
                "base_min": 251.0, "base_max": 251.0, "coefficient": 0.857,
                "dot": null, "confidence": "beta-datamine",
                "source": { "name": "test", "url": null }
            }
        ],
        "disputed_mechanics": [], "dispute_ledger": [], "demonic_sacrifice_effects": []
    }"#;

    const BUILD_JSON: &str = r#"{
        "spell_power": 350.0, "crit_chance": 0.2, "fight_duration_ms": 60000,
        "rotation": ["shadow_bolt"], "pet_type": "imp",
        "demonic_sacrifice_active": false, "has_demonic_pact": false,
        "resummon_event_ms": null, "resummon_is_same_pet": false,
        "dispute_rule_override": null
    }"#;

    #[test]
    fn simulate_dps_core_returns_valid_json_with_sane_dps() {
        let result_json = simulate_dps_core(DATA_PACK_JSON, BUILD_JSON, 2000, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("must be valid JSON");

        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["data_pack_version"], "wasm-test-fixture");
        let mean = parsed["mean"].as_f64().expect("mean must be a number");
        assert!(mean > 100.0 && mean < 300.0, "mean DPS {mean} should be in a sane range for this fixture");
    }

    #[test]
    fn simulate_dps_core_reports_invalid_data_pack_json_as_an_error_not_a_panic() {
        let result_json = simulate_dps_core("not json", BUILD_JSON, 100, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("error path must still be valid JSON");
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("invalid data pack JSON"));
    }

    #[test]
    fn simulate_dps_core_reports_invalid_build_json_as_an_error_not_a_panic() {
        let result_json = simulate_dps_core(DATA_PACK_JSON, "not json", 100, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("error path must still be valid JSON");
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("invalid build JSON"));
    }

    /// Regression test for a real, recurring class of bug in this project:
    /// the frontend's hardcoded data-pack path going stale after a new
    /// revision ships, so a build references a spell id the *loaded* data
    /// pack doesn't have. That used to panic deep inside
    /// `wowforever_engine::build_schedule` with no JSON-parse-error check
    /// catching it, surfacing in a real browser as a bare "unreachable"
    /// wasm trap with no indication of what went wrong. Must now come
    /// back as a normal `{ok: false, error: "..."}` response.
    #[test]
    fn simulate_dps_core_reports_an_unknown_rotation_spell_id_as_an_error_not_a_panic() {
        let build_with_unknown_spell = r#"{
            "spell_power": 350.0, "crit_chance": 0.2, "fight_duration_ms": 60000,
            "rotation": ["this_spell_does_not_exist"], "pet_type": "imp",
            "demonic_sacrifice_active": false, "has_demonic_pact": false,
            "resummon_event_ms": null, "resummon_is_same_pet": false,
            "dispute_rule_override": null
        }"#;
        let result_json = simulate_dps_core(DATA_PACK_JSON, build_with_unknown_spell, 100, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("must still be valid JSON, not a crashed process");
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("this_spell_does_not_exist"));
    }

    #[test]
    fn simulate_dps_core_is_deterministic_for_the_same_seed() {
        let a = simulate_dps_core(DATA_PACK_JSON, BUILD_JSON, 500, 42);
        let b = simulate_dps_core(DATA_PACK_JSON, BUILD_JSON, 500, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn simulate_dps_detail_core_returns_a_real_breakdown_and_timeline() {
        let result_json = simulate_dps_detail_core(DATA_PACK_JSON, BUILD_JSON, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("must be valid JSON");

        assert_eq!(parsed["ok"], true);
        let abilities = parsed["abilities"].as_array().expect("abilities must be an array");
        assert_eq!(abilities.len(), 1);
        assert_eq!(abilities[0]["source_id"], "shadow_bolt");
        assert_eq!(abilities[0]["kind"], "direct");
        assert!(abilities[0]["casts"].as_u64().unwrap() > 0);

        let events = parsed["events"].as_array().expect("events must be an array");
        assert!(!events.is_empty());
        assert_eq!(events[0]["source_id"], "shadow_bolt");

        // No Demonic Sacrifice in this fixture build -- an honest empty list.
        assert!(parsed["buffs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn simulate_dps_detail_core_reports_an_unknown_rotation_spell_id_as_an_error_not_a_panic() {
        let build_with_unknown_spell = r#"{
            "spell_power": 350.0, "crit_chance": 0.2, "fight_duration_ms": 60000,
            "rotation": ["this_spell_does_not_exist"], "pet_type": "imp",
            "demonic_sacrifice_active": false, "has_demonic_pact": false,
            "resummon_event_ms": null, "resummon_is_same_pet": false,
            "dispute_rule_override": null
        }"#;
        let result_json = simulate_dps_detail_core(DATA_PACK_JSON, build_with_unknown_spell, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("must still be valid JSON");
        assert_eq!(parsed["ok"], false);
        assert!(parsed["error"].as_str().unwrap().contains("this_spell_does_not_exist"));
    }

    const BURST_BUILD_JSON: &str = r#"{
        "spell_power": 350.0, "crit_chance": 0.2, "window_ms": 15000,
        "available_spell_ids": ["shadow_bolt"], "pet_type": "imp",
        "demonic_sacrifice_active": false,
        "opponent": { "name": "test", "shadow_resist_pct": 0.0, "fire_resist_pct": 0.0, "physical_resist_pct": 0.0, "frost_resist_pct": 0.0, "arcane_resist_pct": 0.0, "holy_resist_pct": 0.0 },
        "opener": { "starting_mana": 5000.0, "opened_from_stealth_or_cc": false },
        "search_beam_width": 200
    }"#;

    #[test]
    fn simulate_burst_core_returns_valid_json_with_a_real_sequence() {
        let result_json = simulate_burst_core(DATA_PACK_JSON, BURST_BUILD_JSON, 1000, 1);
        let parsed: serde_json::Value = serde_json::from_str(&result_json).expect("must be valid JSON");

        assert_eq!(parsed["ok"], true);
        assert!(parsed["expected_damage"].as_f64().unwrap() > 0.0);
        assert!(!parsed["sequence"].as_array().unwrap().is_empty());
    }
}
