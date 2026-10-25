//! Historical results storage (Plan Section 4.3): "Every simulation run
//! (user-initiated or ingested) is persisted with its full input
//! configuration and the data-pack version it ran under... supports trend
//! views... regression detection... and the comparison feature in 4.2."
//!
//! Phase 1 scope: a local, append-only JSON-Lines file is the storage
//! engine, not a hosted database -- this is CLI-stage bring-up, and Section
//! 5's eventual "Results store" (a queryable service behind a web app) is
//! explicitly later-phase work this does not attempt to preempt. What *is*
//! implemented for real: every run is persisted with its full build config
//! and the data-pack version it ran under, and a durable regression check
//! runs on every append (see `RunStore::append`'s return value).
//!
//! Known limitation, stated rather than hidden: this store assumes a single
//! writer. Concurrent appends from multiple processes can interleave lines;
//! that's acceptable for Phase 1 CLI bring-up and must be revisited before
//! any multi-user web backend reuses this format.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use wowforever_engine::{BuildConfig, DisputeDeltaOutput, DpsStats};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeDeltaSummary {
    pub mechanic_id: String,
    pub rule_a_label: String,
    pub rule_a_mean_dps: f64,
    pub rule_b_label: String,
    pub rule_b_mean_dps: f64,
    pub mean_dps_delta: f64,
    pub pct_swing: f64,
}

impl From<&DisputeDeltaOutput> for DisputeDeltaSummary {
    fn from(d: &DisputeDeltaOutput) -> Self {
        DisputeDeltaSummary {
            mechanic_id: d.mechanic_id.clone(),
            rule_a_label: d.rule_a_label.clone(),
            rule_a_mean_dps: d.rule_a_dps.mean,
            rule_b_label: d.rule_b_label.clone(),
            rule_b_mean_dps: d.rule_b_dps.mean,
            mean_dps_delta: d.mean_dps_delta,
            pct_swing: d.pct_swing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalRun {
    pub run_id: String,
    pub recorded_at_unix: u64,
    pub data_pack_version: String,
    /// Identifies "the same build" across data-pack versions (Plan 4.3's
    /// regression-detection use case) without needing semantic build
    /// equality -- two runs share a fingerprint iff their full `build`
    /// configs serialize identically.
    pub build_fingerprint: String,
    pub build: BuildConfig,
    pub dps: DpsStats,
    pub touches_disputed_mechanic: bool,
    pub dispute_delta: Option<DisputeDeltaSummary>,
}

pub fn build_fingerprint(build: &BuildConfig) -> String {
    let json = serde_json::to_string(build).expect("BuildConfig must serialize");
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub struct RunStore {
    path: PathBuf,
}

impl RunStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        RunStore { path: path.into() }
    }

    /// Reads every historical run from the store, oldest first. A missing
    /// file is treated as an empty store (the natural state before the
    /// first run is ever recorded), not an error.
    pub fn load_all(&self) -> io::Result<Vec<HistoricalRun>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let reader = BufReader::new(file);
        let mut runs = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let run: HistoricalRun = serde_json::from_str(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("corrupt history line: {e}")))?;
            runs.push(run);
        }
        Ok(runs)
    }

    pub fn runs_for_build_fingerprint(&self, fingerprint: &str) -> io::Result<Vec<HistoricalRun>> {
        Ok(self.load_all()?.into_iter().filter(|r| r.build_fingerprint == fingerprint).collect())
    }

    /// Appends a new run, tagging it with the given data-pack version.
    /// Returns the most recent prior run that shares this build's exact
    /// fingerprint but was computed under a *different* data-pack version,
    /// if one exists -- this is the regression-detection signal Plan 4.3
    /// calls for ("did a hotfix quietly nerf a build"), computed eagerly so
    /// callers don't have to re-scan history themselves.
    pub fn append(
        &self,
        data_pack_version: &str,
        build: &BuildConfig,
        dps: &DpsStats,
        touches_disputed_mechanic: bool,
        dispute_delta: Option<&DisputeDeltaOutput>,
    ) -> io::Result<Option<HistoricalRun>> {
        let fingerprint = build_fingerprint(build);
        let prior_comparable = self
            .runs_for_build_fingerprint(&fingerprint)?
            .into_iter()
            .filter(|r| r.data_pack_version != data_pack_version)
            .max_by_key(|r| r.recorded_at_unix);

        let recorded_at_unix = now_unix();
        let run = HistoricalRun {
            run_id: format!("{recorded_at_unix}-{}", &fingerprint[..8]),
            recorded_at_unix,
            data_pack_version: data_pack_version.to_string(),
            build_fingerprint: fingerprint,
            build: build.clone(),
            dps: dps.clone(),
            touches_disputed_mechanic,
            dispute_delta: dispute_delta.map(DisputeDeltaSummary::from),
        };

        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let line = serde_json::to_string(&run).expect("HistoricalRun must serialize");
        writeln!(file, "{line}")?;

        Ok(prior_comparable)
    }
}

/// One externally-sourced result (Plan Section 4.2): "A result-ingestion
/// layer that imports external results as attributed records... for
/// side-by-side comparison against this app's own simulation of the same
/// build." Per Plan Section 0.2's legal review, ingestion is **user-pasted
/// only** -- there is no scraper here, and this type has no fetch method.
/// A human reads a number off wowforeversim.com or mythicsim.com themselves
/// and enters it; `build_fingerprint` is supplied by that same human (e.g.
/// read from `--show-history`) to say which of *our* builds it corresponds
/// to, since an external site's own build description can't be parsed into
/// our `BuildConfig` automatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalResult {
    pub entry_id: String,
    pub entered_at_unix: u64,
    pub build_fingerprint: String,
    pub source_name: String,
    /// Whatever version/build identifier the source itself reports, if any
    /// (Plan Principle 10: "source, source-confidence, and talent-data
    /// version side by side").
    pub source_talent_version: Option<String>,
    pub reported_dps_mean: f64,
    pub notes: String,
}

pub struct ExternalResultStore {
    path: PathBuf,
}

impl ExternalResultStore {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        ExternalResultStore { path: path.into() }
    }

    pub fn load_all(&self) -> io::Result<Vec<ExternalResult>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let reader = BufReader::new(file);
        let mut results = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let result: ExternalResult = serde_json::from_str(&line)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("corrupt external-result line: {e}")))?;
            results.push(result);
        }
        Ok(results)
    }

    /// Appends one user-entered external result. Never merges it into
    /// `RunStore` -- external results are comparison-only and must never be
    /// mistaken for this app's own simulation output (Plan Principle 10).
    pub fn append(&self, mut result: ExternalResult) -> io::Result<()> {
        if result.entered_at_unix == 0 {
            result.entered_at_unix = now_unix();
        }
        if result.entry_id.is_empty() {
            result.entry_id = format!("{}-{}", result.entered_at_unix, &result.build_fingerprint[..8.min(result.build_fingerprint.len())]);
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let line = serde_json::to_string(&result).expect("ExternalResult must serialize");
        writeln!(file, "{line}")
    }
}

#[derive(Debug, Clone)]
pub struct Comparison {
    pub build_fingerprint: String,
    /// Our own most recent run of this build, if we have one.
    pub our_latest: Option<HistoricalRun>,
    pub external: Vec<ExternalResult>,
}

/// Builds one `Comparison` per build fingerprint that has at least one
/// external result, joining it with our own most recent run of that same
/// build if one exists. Never resolves disagreements -- per Plan Principle
/// 10, both sides are surfaced, not reconciled into one number.
pub fn build_comparisons(run_store: &RunStore, external_store: &ExternalResultStore) -> io::Result<Vec<Comparison>> {
    let our_runs = run_store.load_all()?;
    let external = external_store.load_all()?;

    let mut by_fingerprint: std::collections::BTreeMap<String, Vec<ExternalResult>> = std::collections::BTreeMap::new();
    for ext in external {
        by_fingerprint.entry(ext.build_fingerprint.clone()).or_default().push(ext);
    }

    let comparisons = by_fingerprint
        .into_iter()
        .map(|(fingerprint, external)| {
            let our_latest = our_runs.iter().filter(|r| r.build_fingerprint == fingerprint).max_by_key(|r| r.recorded_at_unix).cloned();
            Comparison { build_fingerprint: fingerprint, our_latest, external }
        })
        .collect();

    Ok(comparisons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowforever_engine::PetType;

    fn sample_build() -> BuildConfig {
        BuildConfig {
            spell_power: 350.0,
            crit_chance: 0.20,
            fight_duration_ms: 60_000,
            rotation: vec!["corruption".into(), "shadow_bolt".into()],
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

    fn sample_dps(mean: f64) -> DpsStats {
        DpsStats { mean, stdev: 1.0, p5: mean - 2.0, p50: mean, p95: mean + 2.0 }
    }

    #[test]
    fn identical_build_configs_produce_identical_fingerprints() {
        assert_eq!(build_fingerprint(&sample_build()), build_fingerprint(&sample_build()));
    }

    #[test]
    fn different_build_configs_produce_different_fingerprints() {
        let mut other = sample_build();
        other.spell_power = 400.0;
        assert_ne!(build_fingerprint(&sample_build()), build_fingerprint(&other));
    }

    #[test]
    fn empty_store_has_no_history_and_append_finds_no_prior_run() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-{}", now_unix()));
        let store = RunStore::open(dir.join("runs.jsonl"));

        assert!(store.load_all().unwrap().is_empty());

        let prior = store
            .append("forever-beta-2026-10-17", &sample_build(), &sample_dps(200.0), true, None)
            .unwrap();
        assert!(prior.is_none(), "first-ever run for this build must have no prior comparable run");

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].data_pack_version, "forever-beta-2026-10-17");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The regression-detection property this whole module exists for: a
    /// second run of the *same* build under a *different* data-pack version
    /// must surface the earlier run so the caller can compute "did this
    /// patch change this build's DPS."
    #[test]
    fn appending_same_build_under_new_data_pack_version_surfaces_prior_run() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-{}", now_unix() + 1));
        let store = RunStore::open(dir.join("runs.jsonl"));

        store.append("forever-beta-2026-10-16", &sample_build(), &sample_dps(200.0), true, None).unwrap();
        let prior = store
            .append("forever-beta-2026-10-17", &sample_build(), &sample_dps(223.3), true, None)
            .unwrap()
            .expect("same build under a new data-pack version must find the prior run");

        assert_eq!(prior.data_pack_version, "forever-beta-2026-10-16");
        assert_eq!(prior.dps.mean, 200.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A different build (different fingerprint) must never be mistaken for
    /// a prior run of "the same build" -- regression detection must not
    /// cross builds.
    #[test]
    fn appending_different_build_does_not_surface_unrelated_runs() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-{}", now_unix() + 2));
        let store = RunStore::open(dir.join("runs.jsonl"));

        store.append("forever-beta-2026-10-16", &sample_build(), &sample_dps(200.0), true, None).unwrap();

        let mut different_build = sample_build();
        different_build.spell_power = 500.0;
        let prior = store
            .append("forever-beta-2026-10-17", &different_build, &sample_dps(260.0), true, None)
            .unwrap();

        assert!(prior.is_none(), "a different build must not match the unrelated prior run");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sample_external(fingerprint: &str, source: &str, dps: f64) -> ExternalResult {
        ExternalResult {
            entry_id: String::new(),
            entered_at_unix: 0,
            build_fingerprint: fingerprint.to_string(),
            source_name: source.to_string(),
            source_talent_version: Some("test-talent-version".to_string()),
            reported_dps_mean: dps,
            notes: "synthetic test fixture, not a real site submission".to_string(),
        }
    }

    #[test]
    fn external_result_store_round_trips_and_autofills_id_and_timestamp() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-ext-{}", now_unix()));
        let store = ExternalResultStore::open(dir.join("external.jsonl"));

        assert!(store.load_all().unwrap().is_empty());
        store.append(sample_external("abcdef1234567890", "wowforeversim.com", 240.0)).unwrap();

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].source_name, "wowforeversim.com");
        assert_eq!(all[0].reported_dps_mean, 240.0);
        assert!(!all[0].entry_id.is_empty(), "entry_id must be auto-filled when left empty");
        assert_ne!(all[0].entered_at_unix, 0, "entered_at_unix must be auto-filled when left 0");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// External results are comparison-only -- appending one must never
    /// touch the separate RunStore (Plan Principle 10).
    #[test]
    fn external_results_never_leak_into_run_store() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-ext2-{}", now_unix()));
        let run_store = RunStore::open(dir.join("runs.jsonl"));
        let ext_store = ExternalResultStore::open(dir.join("external.jsonl"));

        run_store.append("pack-v1", &sample_build(), &sample_dps(200.0), false, None).unwrap();
        let fp = build_fingerprint(&sample_build());
        ext_store.append(sample_external(&fp, "mythicsim.com", 215.0)).unwrap();

        assert_eq!(run_store.load_all().unwrap().len(), 1, "external append must not add to the run store");
        assert_eq!(ext_store.load_all().unwrap().len(), 1, "run append must not add to the external store");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_comparisons_joins_our_latest_run_with_all_external_results_for_the_same_build() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-ext3-{}", now_unix()));
        let run_store = RunStore::open(dir.join("runs.jsonl"));
        let ext_store = ExternalResultStore::open(dir.join("external.jsonl"));

        let fp = build_fingerprint(&sample_build());
        run_store.append("pack-v1", &sample_build(), &sample_dps(200.0), false, None).unwrap();
        run_store.append("pack-v2", &sample_build(), &sample_dps(210.0), false, None).unwrap(); // latest
        ext_store.append(sample_external(&fp, "wowforeversim.com", 195.0)).unwrap();
        ext_store.append(sample_external(&fp, "mythicsim.com", 205.0)).unwrap();

        let comparisons = build_comparisons(&run_store, &ext_store).unwrap();
        assert_eq!(comparisons.len(), 1);
        let cmp = &comparisons[0];
        assert_eq!(cmp.build_fingerprint, fp);
        assert_eq!(cmp.our_latest.as_ref().unwrap().dps.mean, 210.0, "must use our most recent run (pack-v2), not the older one");
        assert_eq!(cmp.external.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A build with external results but no run of our own yet must still
    /// surface the external data (so comparison doesn't silently drop
    /// results just because we haven't simulated that build), with
    /// `our_latest` honestly `None` rather than a fabricated zero.
    #[test]
    fn build_comparisons_surfaces_external_results_even_with_no_matching_run_of_our_own() {
        let dir = std::env::temp_dir().join(format!("wowforever-storage-test-ext4-{}", now_unix()));
        let run_store = RunStore::open(dir.join("runs.jsonl"));
        let ext_store = ExternalResultStore::open(dir.join("external.jsonl"));

        ext_store.append(sample_external("unmatched-fingerprint", "mythicsim.com", 300.0)).unwrap();

        let comparisons = build_comparisons(&run_store, &ext_store).unwrap();
        assert_eq!(comparisons.len(), 1);
        assert!(comparisons[0].our_latest.is_none());
        assert_eq!(comparisons[0].external.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
