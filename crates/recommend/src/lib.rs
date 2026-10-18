//! Change-Driven Suggestion/Recommendation Engine (Plan Section 4.4): "When
//! a new talent/spell data version is published, the system diffs it
//! against the prior version to identify which specs/builds are
//! mechanically affected, and surfaces a 'builds worth re-testing' list."
//!
//! Scope for this slice: diffing two `DataPack`s field-by-field, and
//! cross-referencing the result against `wowforever-storage`'s historical
//! run log to find which previously-run builds reference a changed spell.
//! Re-simulating those builds under the new pack (closing the loop via
//! `RunStore::append`'s existing regression-detection machinery) is the
//! caller's job, not this crate's -- this crate only identifies what's
//! worth re-testing, deliberately kept cheap (no simulation, just data
//! comparison) per the plan's cost-controls note (Section 4.4): screening
//! should be cheap, full re-simulation is a separate, costlier step.
//!
//! Popularity-weighted prioritization (also named in 4.4) is not
//! implemented here -- it needs real usage telemetry this project doesn't
//! collect yet (Phase 2's accounts/identity layer, still unbuilt). What's
//! here is the mechanically-affected half of the feature, which doesn't
//! need that telemetry to be useful on its own.

use std::collections::BTreeSet;
use wowforever_data::{DataPack, DisputeStatus};
use wowforever_storage::HistoricalRun;

#[derive(Debug, Clone)]
pub struct SpellChange {
    pub spell_id: String,
    /// Human-readable description of what changed, e.g. "coefficient: 0.2 -> 0.13".
    pub changes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MechanicStatusChange {
    pub mechanic_id: String,
    pub old_status: DisputeStatus,
    pub new_status: DisputeStatus,
}

#[derive(Debug, Clone, Default)]
pub struct DataPackDiff {
    pub added_spell_ids: Vec<String>,
    pub removed_spell_ids: Vec<String>,
    pub changed_spells: Vec<SpellChange>,
    pub mechanic_status_changes: Vec<MechanicStatusChange>,
}

impl DataPackDiff {
    /// Every spell id touched by this diff in any way -- the set to check
    /// historical builds against.
    pub fn touched_spell_ids(&self) -> BTreeSet<String> {
        let mut ids: BTreeSet<String> = self.added_spell_ids.iter().cloned().collect();
        ids.extend(self.removed_spell_ids.iter().cloned());
        ids.extend(self.changed_spells.iter().map(|c| c.spell_id.clone()));
        ids
    }

    pub fn is_empty(&self) -> bool {
        self.added_spell_ids.is_empty() && self.removed_spell_ids.is_empty() && self.changed_spells.is_empty() && self.mechanic_status_changes.is_empty()
    }
}

fn describe_spell_changes(old: &wowforever_data::SpellDef, new: &wowforever_data::SpellDef) -> Vec<String> {
    let mut changes = Vec::new();
    if old.base_min != new.base_min || old.base_max != new.base_max {
        changes.push(format!("damage {}-{} -> {}-{}", old.base_min, old.base_max, new.base_min, new.base_max));
    }
    if old.coefficient != new.coefficient {
        changes.push(format!("coefficient {} -> {}", old.coefficient, new.coefficient));
    }
    if old.cast_time_ms != new.cast_time_ms {
        changes.push(format!("cast_time_ms {} -> {}", old.cast_time_ms, new.cast_time_ms));
    }
    if old.cooldown_ms != new.cooldown_ms {
        changes.push(format!("cooldown_ms {} -> {}", old.cooldown_ms, new.cooldown_ms));
    }
    if old.resource_cost != new.resource_cost {
        changes.push(format!("resource_cost {} -> {}", old.resource_cost, new.resource_cost));
    }
    match (&old.dot, &new.dot) {
        (None, None) => {}
        (Some(_), None) => changes.push("dot component removed".to_string()),
        (None, Some(_)) => changes.push("dot component added".to_string()),
        (Some(o), Some(n)) => {
            if o.tick_base != n.tick_base || o.tick_coefficient != n.tick_coefficient || o.num_ticks != n.num_ticks || o.tick_interval_ms != n.tick_interval_ms {
                changes.push(format!(
                    "dot {}x{}base/{}coeff every {}ms -> {}x{}base/{}coeff every {}ms",
                    o.num_ticks, o.tick_base, o.tick_coefficient, o.tick_interval_ms, n.num_ticks, n.tick_base, n.tick_coefficient, n.tick_interval_ms
                ));
            }
        }
    }
    changes
}

/// Diffs two data packs. Deliberately cheap: pure data comparison, no
/// simulation. Order-independent in spell listing (compares by id).
pub fn diff_data_packs(old: &DataPack, new: &DataPack) -> DataPackDiff {
    let mut diff = DataPackDiff::default();

    for new_spell in &new.spells {
        match old.spell(&new_spell.id) {
            None => diff.added_spell_ids.push(new_spell.id.clone()),
            Some(old_spell) => {
                let changes = describe_spell_changes(old_spell, new_spell);
                if !changes.is_empty() {
                    diff.changed_spells.push(SpellChange { spell_id: new_spell.id.clone(), changes });
                }
            }
        }
    }
    for old_spell in &old.spells {
        if new.spell(&old_spell.id).is_none() {
            diff.removed_spell_ids.push(old_spell.id.clone());
        }
    }

    for new_mechanic in &new.disputed_mechanics {
        if let (Some(old_status), Some(new_status)) =
            (old.dispute_status(&new_mechanic.id), new.dispute_status(&new_mechanic.id))
        {
            if old_status != new_status {
                diff.mechanic_status_changes.push(MechanicStatusChange { mechanic_id: new_mechanic.id.clone(), old_status, new_status });
            }
        }
    }

    diff
}

#[derive(Debug, Clone)]
pub struct AffectedBuild {
    pub build_fingerprint: String,
    pub last_known_data_pack_version: String,
    pub last_known_dps_mean: f64,
    /// Which of the diff's touched spell ids this build's rotation actually references.
    pub matching_spell_ids: Vec<String>,
}

/// Finds which historical builds (one entry per distinct build fingerprint,
/// using its most recent run) reference any spell touched by `diff`. This
/// is the "builds worth re-testing" list from Plan 4.4 -- identification
/// only, deliberately not re-simulating anything here (see module docs).
pub fn find_affected_builds(history: &[HistoricalRun], diff: &DataPackDiff) -> Vec<AffectedBuild> {
    let touched = diff.touched_spell_ids();
    if touched.is_empty() {
        return Vec::new();
    }

    let mut most_recent_by_fingerprint: std::collections::HashMap<&str, &HistoricalRun> = std::collections::HashMap::new();
    for run in history {
        most_recent_by_fingerprint
            .entry(&run.build_fingerprint)
            .and_modify(|existing| {
                if run.recorded_at_unix > existing.recorded_at_unix {
                    *existing = run;
                }
            })
            .or_insert(run);
    }

    let mut affected = Vec::new();
    for run in most_recent_by_fingerprint.values() {
        let matching: Vec<String> = run.build.rotation.iter().filter(|id| touched.contains(id.as_str())).cloned().collect();
        if !matching.is_empty() {
            affected.push(AffectedBuild {
                build_fingerprint: run.build_fingerprint.clone(),
                last_known_data_pack_version: run.data_pack_version.clone(),
                last_known_dps_mean: run.dps.mean,
                matching_spell_ids: matching,
            });
        }
    }
    affected.sort_by(|a, b| a.build_fingerprint.cmp(&b.build_fingerprint));
    affected
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load_pack(filename: &str) -> DataPack {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data-packs").join(filename);
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
        DataPack::from_json_str(&raw).unwrap_or_else(|e| panic!("could not parse {}: {e}", path.display()))
    }

    /// Real diff, not synthetic: this project's own REV2 -> REV3 data-pack
    /// revisions, which genuinely changed every one of the four Warlock
    /// spells (see docs/PLAN.md's data-sourcing investigation).
    #[test]
    fn diffing_rev2_to_rev3_finds_all_four_real_spell_changes() {
        let rev2 = load_pack("forever-beta-2026-10-17-rev2.json");
        let rev3 = load_pack("forever-beta-2026-10-17-rev3.json");

        let diff = diff_data_packs(&rev2, &rev3);

        let changed_ids: BTreeSet<String> = diff.changed_spells.iter().map(|c| c.spell_id.clone()).collect();
        let expected: BTreeSet<String> = ["corruption", "immolate", "shadow_bolt", "shadowburn"].iter().map(|s| s.to_string()).collect();
        assert_eq!(changed_ids, expected, "expected all four Warlock spells to show as changed between REV2 and REV3");
        assert!(diff.added_spell_ids.is_empty());
        assert!(diff.removed_spell_ids.is_empty());

        let shadow_bolt = diff.changed_spells.iter().find(|c| c.spell_id == "shadow_bolt").unwrap();
        assert!(
            shadow_bolt.changes.iter().any(|c| c.contains("damage")),
            "Shadow Bolt's damage range (246-274 -> 251-251) should be reported, got {:?}",
            shadow_bolt.changes
        );
    }

    #[test]
    fn diffing_a_pack_against_itself_finds_nothing() {
        let rev3 = load_pack("forever-beta-2026-10-17-rev3.json");
        let diff = diff_data_packs(&rev3, &rev3);
        assert!(diff.is_empty(), "diffing identical packs must find no changes, got {:?}", diff);
    }

    fn sample_build(rotation: Vec<&str>) -> wowforever_engine::BuildConfig {
        wowforever_engine::BuildConfig {
            spell_power: 350.0,
            crit_chance: 0.2,
            fight_duration_ms: 60_000,
            rotation: rotation.into_iter().map(String::from).collect(),
            pet_type: wowforever_engine::PetType::Imp,
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
        }
    }

    fn sample_run(fingerprint: &str, rotation: Vec<&str>, recorded_at: u64) -> HistoricalRun {
        HistoricalRun {
            run_id: format!("test-{fingerprint}-{recorded_at}"),
            recorded_at_unix: recorded_at,
            data_pack_version: "test".into(),
            build_fingerprint: fingerprint.into(),
            build: sample_build(rotation),
            dps: wowforever_stats::DpsStats { mean: 200.0, stdev: 5.0, p5: 190.0, p50: 200.0, p95: 210.0 },
            touches_disputed_mechanic: false,
            dispute_delta: None,
        }
    }

    #[test]
    fn find_affected_builds_matches_only_builds_referencing_touched_spells() {
        let diff = DataPackDiff { changed_spells: vec![SpellChange { spell_id: "shadow_bolt".into(), changes: vec!["coefficient changed".into()] }], ..Default::default() };

        let history = vec![
            sample_run("build-a", vec!["corruption", "shadow_bolt"], 100),
            sample_run("build-b", vec!["corruption", "immolate"], 100),
        ];

        let affected = find_affected_builds(&history, &diff);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].build_fingerprint, "build-a");
        assert_eq!(affected[0].matching_spell_ids, vec!["shadow_bolt".to_string()]);
    }

    #[test]
    fn find_affected_builds_uses_only_the_most_recent_run_per_fingerprint() {
        let diff = DataPackDiff { changed_spells: vec![SpellChange { spell_id: "shadow_bolt".into(), changes: vec!["x".into()] }], ..Default::default() };

        // Same fingerprint recorded twice; the later one should win.
        let mut old_run = sample_run("build-a", vec!["shadow_bolt"], 100);
        old_run.dps.mean = 999.0; // if the wrong (older) run were picked, this would show up
        let new_run = sample_run("build-a", vec!["shadow_bolt"], 200);

        let affected = find_affected_builds(&[old_run, new_run], &diff);
        assert_eq!(affected.len(), 1);
        assert_eq!(affected[0].last_known_dps_mean, 200.0, "must use the most recent run's DPS, not the older one's 999.0");
    }

    #[test]
    fn empty_diff_finds_no_affected_builds() {
        let diff = DataPackDiff::default();
        let history = vec![sample_run("build-a", vec!["shadow_bolt"], 100)];
        assert!(find_affected_builds(&history, &diff).is_empty());
    }
}
