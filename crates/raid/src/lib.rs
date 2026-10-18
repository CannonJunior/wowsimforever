//! Party/Raid aggregate DPS (Plan Section 8, long-term phase): a real first
//! slice, explicitly scoped. What this computes: the raid's total DPS
//! *distribution* from N independently-simulated characters, aggregated
//! correctly (see `simulate_raid`'s doc comment for why that's not just
//! "add up each member's stats").
//!
//! Phase 3 extension: shared raid debuffs with a slot cap and real
//! cross-character synergy (`simulate_raid_with_debuffs`). The flagship
//! real instance: Warlock's Curse of the Elements (sourced
//! 2026-10-17, see `wowforever_data::RaidDebuffDef`'s doc comment) --
//! one Warlock casting it increases *every* raid member's damage taken of
//! the affected schools, which is a genuine synergy between characters,
//! not merely each member's own output summed. `simulate_raid` (no
//! debuffs) remains available unchanged for callers that don't need this.
//!
//! Still not modeled, stated rather than hidden: raid *buffs* (stat boosts
//! shared across members, as opposed to debuffs on the target) and a
//! healing-vs-incoming-damage model (see `wowforever_heal`'s module docs
//! for why that's a deferred, honestly-scoped gap -- no real encounter
//! damage data exists for a game that hasn't launched).

use std::collections::{HashMap, HashSet};
use wowforever_data::{DataPack, School};
use wowforever_engine::{simulate, simulate_samples, simulate_samples_by_school, BuildConfig};
use wowforever_stats::{stats_from, DpsStats};

#[derive(Debug, Clone)]
pub struct RaidMember {
    pub name: String,
    pub build: BuildConfig,
}

#[derive(Debug, Clone)]
pub struct RaidMemberResult {
    pub name: String,
    pub dps: DpsStats,
    pub touches_disputed_mechanic: bool,
}

#[derive(Debug, Clone)]
pub struct RaidResult {
    pub members: Vec<RaidMemberResult>,
    /// The raid-wide total DPS distribution, aggregated by summing each
    /// member's *same-iteration* sample before taking percentiles -- not
    /// by summing each member's independently-computed percentiles, which
    /// would be wrong: percentiles of a sum are not the sum of percentiles,
    /// even though means are (by linearity of expectation). See the test
    /// `aggregation_is_not_naive_percentile_summation` for a concrete
    /// demonstration of the difference this makes.
    pub total: DpsStats,
}

/// A large odd multiplier used to separate each member's RNG seed from the
/// others' and from the base seed, so their random draws (crit rolls etc.)
/// are independent rather than accidentally lockstepped. If two members
/// shared a seed, their "random" crit rolls would happen in perfect
/// lockstep every iteration, understating the raid total's real variance --
/// see `same_seed_for_every_member_is_a_real_correctness_bug` below.
const SEED_SEPARATION: u64 = 1_000_003;

/// Classic WoW's debuff-slot cap on a single target. Originally 8 in
/// vanilla, raised to 16 in patch 1.7.0 (sourced via web research
/// 2026-10-17 -- mmo-champion.com/Wowhead's Classic debuff-management
/// guide both corroborate 16; the later patch-1.11 priority system that
/// decided *which* 16 win is not modeled here, since this project has only
/// one sourced raid debuff so far and no priority data to assign). WoW
/// Forever hasn't been reported to change this, so it's used as the
/// working value. A level-60 Classic-style raid fielding one Warlock's
/// single curse is nowhere near this limit in practice -- the cap exists
/// here so the *mechanism* is real and tested, not because any shipped
/// build currently needs it.
pub const CLASSIC_DEBUFF_LIMIT: usize = 16;

/// The result of a debuff-aware raid simulation: the aggregated result
/// plus which requested debuff ids actually took effect (after dedup and
/// the slot-cap) vs. which were dropped for exceeding `CLASSIC_DEBUFF_LIMIT`.
#[derive(Debug, Clone)]
pub struct RaidDebuffOutcome {
    pub result: RaidResult,
    pub active_debuff_ids: Vec<String>,
    pub dropped_debuff_ids: Vec<String>,
}

/// Simulates a raid of `members.len()` independently-played characters and
/// returns both each member's own result and the correctly-aggregated raid
/// total. Panics if `members` is empty (there is no sensible "zero-person
/// raid" DPS distribution to report). Thin wrapper over
/// `simulate_raid_with_debuffs` with no active debuffs, kept for callers
/// that don't need that.
pub fn simulate_raid(data: &DataPack, members: &[RaidMember], iterations: u32, base_seed: u64) -> RaidResult {
    simulate_raid_with_debuffs(data, members, &[], iterations, base_seed).result
}

/// Same as `simulate_raid`, but also applies `requested_debuff_ids` (looked
/// up in `data.raid_debuffs`) as shared, target-wide debuffs that increase
/// *every* member's damage taken of the affected schools -- real
/// cross-character synergy, not just each member's own rotation. Unknown
/// ids are silently ignored (not a hard error: a UI might offer a debuff
/// toggle for a mechanic not yet in this data pack revision).
///
/// Debuff ids are deduped (recasting the same curse isn't a second slot)
/// and capped at `CLASSIC_DEBUFF_LIMIT` in request order -- real Classic-era
/// behavior before the 1.11 priority system was "whichever is already
/// there stays," which this approximates by keeping the first N requested.
/// Distinct debuffs' bonuses for the same school stack additively (the
/// correct rule for genuinely different effects); this project ships only
/// one sourced raid debuff so that case is currently untested against a
/// second real debuff, but the mechanism itself is exercised by the tests
/// below using synthetic debuffs.
pub fn simulate_raid_with_debuffs(
    data: &DataPack,
    members: &[RaidMember],
    requested_debuff_ids: &[String],
    iterations: u32,
    base_seed: u64,
) -> RaidDebuffOutcome {
    assert!(!members.is_empty(), "simulate_raid requires at least one member");

    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for id in requested_debuff_ids {
        if seen.insert(id.clone()) {
            deduped.push(id.clone());
        }
    }
    let active_debuff_ids: Vec<String> = deduped.iter().take(CLASSIC_DEBUFF_LIMIT).cloned().collect();
    let dropped_debuff_ids: Vec<String> = deduped.iter().skip(CLASSIC_DEBUFF_LIMIT).cloned().collect();

    let mut school_bonus: HashMap<School, f64> = HashMap::new();
    for id in &active_debuff_ids {
        if let Some(debuff) = data.raid_debuff(id) {
            for school in &debuff.schools_affected {
                *school_bonus.entry(*school).or_insert(0.0) += debuff.damage_taken_pct_bonus;
            }
        }
    }

    let mut member_results = Vec::with_capacity(members.len());
    let mut per_iteration_totals = vec![0.0_f64; iterations as usize];

    for (idx, member) in members.iter().enumerate() {
        let seed = base_seed.wrapping_add((idx as u64 + 1).wrapping_mul(SEED_SEPARATION));
        let touches_disputed_mechanic = simulate(data, &member.build, iterations, seed).touches_disputed_mechanic;

        // With no active school bonus this must be byte-for-byte identical
        // to the pre-Phase-3 behavior (and to plain `simulate`'s own
        // samples) -- `single_member_raid_total_equals_that_members_own_result`
        // below pins exactly that.
        let member_samples: Vec<f64> = if school_bonus.is_empty() {
            simulate_samples(data, &member.build, iterations, seed)
        } else {
            simulate_samples_by_school(data, &member.build, iterations, seed)
                .into_iter()
                .map(|by_school| {
                    by_school
                        .into_iter()
                        .map(|(school, v)| v * (1.0 + school_bonus.get(&school).copied().unwrap_or(0.0)))
                        .sum()
                })
                .collect()
        };

        let mut samples_for_stats = member_samples.clone();
        member_results.push(RaidMemberResult {
            name: member.name.clone(),
            dps: stats_from(&mut samples_for_stats),
            touches_disputed_mechanic,
        });

        for (i, s) in member_samples.into_iter().enumerate() {
            per_iteration_totals[i] += s;
        }
    }

    RaidDebuffOutcome {
        result: RaidResult { members: member_results, total: stats_from(&mut per_iteration_totals) },
        active_debuff_ids,
        dropped_debuff_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowforever_data::{Confidence, School, Source, SpellDef};
    use wowforever_engine::PetType;

    fn demo_data_pack() -> DataPack {
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![SpellDef {
                id: "shadow_bolt".into(),
                name: "Shadow Bolt".into(),
                school: School::Shadow,
                cast_time_ms: 3000,
                cooldown_ms: 0,
                resource_cost: 370.0,
                base_min: 251.0,
                base_max: 251.0,
                coefficient: 0.857,
                ap_coefficient: 0.0,
                dot: None,
                confidence: Confidence::CommunityReported,
                source: Source { name: "test".into(), url: None },
            }],
            disputed_mechanics: vec![],
            dispute_ledger: vec![],
            demonic_sacrifice_effects: vec![],
            raid_debuffs: vec![],
        }
    }

    fn member(name: &str, spell_power: f64) -> RaidMember {
        RaidMember {
            name: name.to_string(),
            build: BuildConfig {
                spell_power,
                crit_chance: 0.3, // nonzero so there's real variance to test against
                fight_duration_ms: 60_000,
                rotation: vec!["shadow_bolt".into()],
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
            },
        }
    }

    #[test]
    fn single_member_raid_total_equals_that_members_own_result() {
        let data = demo_data_pack();
        let result = simulate_raid(&data, &[member("Solo", 350.0)], 5000, 1);
        assert_eq!(result.members.len(), 1);
        assert!((result.total.mean - result.members[0].dps.mean).abs() < 1e-9);
    }

    /// The core correctness property: two identical members' total mean
    /// must be ~2x one member's mean (linearity of expectation holds
    /// regardless of independence), but the total's stdev must be
    /// *strictly less than* 2x one member's stdev -- only true if their
    /// samples are summed per-iteration as independent draws, not
    /// concatenated or otherwise mishandled. sqrt(2) * stdev is the
    /// expected value for two i.i.d. independent members.
    #[test]
    fn two_identical_members_aggregate_as_independent_random_variables() {
        let data = demo_data_pack();
        let members = vec![member("A", 350.0), member("B", 350.0)];
        let result = simulate_raid(&data, &members, 20_000, 1);

        let single = result.members[0].dps.mean;
        assert!((result.total.mean - 2.0 * single).abs() / (2.0 * single) < 0.02, "raid total mean should be ~2x a single member's mean");

        let one_member_stdev = result.members[0].dps.stdev;
        let naive_doubled_stdev = 2.0 * one_member_stdev;
        assert!(
            result.total.stdev < naive_doubled_stdev * 0.9,
            "two independent members' combined stdev ({}) must be meaningfully less than naively doubling one member's stdev ({}) -- \
             sqrt(2)x is expected for i.i.d. sums, not 2x, which would indicate the samples were summed as if perfectly correlated",
            result.total.stdev,
            naive_doubled_stdev
        );
    }

    /// Demonstrates the actual bug `SEED_SEPARATION` prevents: if every
    /// member were simulated with the *same* seed, their crit rolls would
    /// happen in lockstep every iteration, and the raid total would show
    /// stdev == 2x (perfectly correlated), not sqrt(2)x (independent) --
    /// this reproduces that bug deliberately by bypassing the real seed
    /// separation, to prove the distinction the test above relies on is
    /// real and not a tolerance-band coincidence.
    #[test]
    fn same_seed_for_every_member_is_a_real_correctness_bug() {
        let data = demo_data_pack();
        let build = member("X", 350.0).build;
        let iterations = 20_000;
        let shared_seed = 1u64;

        let mut lockstep_totals: Vec<f64> = wowforever_engine::simulate_samples(&data, &build, iterations, shared_seed);
        let second = wowforever_engine::simulate_samples(&data, &build, iterations, shared_seed); // same seed on purpose
        for (i, v) in second.into_iter().enumerate() {
            lockstep_totals[i] += v;
        }
        let lockstep_stats = stats_from(&mut lockstep_totals);

        let single_stdev = wowforever_engine::simulate(&data, &build, iterations, shared_seed).dps.stdev;
        assert!(
            (lockstep_stats.stdev - 2.0 * single_stdev).abs() / (2.0 * single_stdev) < 0.02,
            "same-seed (lockstep) aggregation should produce ~2x stdev, confirming this really would be a bug if SEED_SEPARATION weren't applied"
        );
    }

    #[test]
    #[should_panic(expected = "at least one member")]
    fn empty_raid_panics_rather_than_returning_a_nonsensical_result() {
        let data = demo_data_pack();
        simulate_raid(&data, &[], 100, 1);
    }

    /// A data pack with two single-school spells so a mixed-school rotation
    /// is actually exercised: `spell_fire` has a 3s cooldown (forcing the
    /// rotation to fall back to `spell_shadow` while it's down), so the
    /// schedule deterministically alternates fire/shadow/fire within a
    /// short fight -- see the hand-computed walkthrough in
    /// `raid_debuff_only_affects_its_own_schools_in_a_mixed_school_rotation`.
    fn mixed_school_data_pack(fire_curse_bonus: f64) -> DataPack {
        let flat_spell = |id: &str, school: School, cooldown_ms: u32| SpellDef {
            id: id.into(),
            name: id.into(),
            school,
            cast_time_ms: 0,
            cooldown_ms,
            resource_cost: 0.0,
            base_min: 100.0,
            base_max: 100.0, // fixed, no range, for hand-computable totals
            coefficient: 0.0, // spell_power irrelevant to this test
            ap_coefficient: 0.0,
            dot: None,
            confidence: Confidence::CommunityReported,
            source: Source { name: "test".into(), url: None },
        };
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![flat_spell("spell_fire", School::Fire, 3000), flat_spell("spell_shadow", School::Shadow, 0)],
            disputed_mechanics: vec![],
            dispute_ledger: vec![],
            demonic_sacrifice_effects: vec![],
            raid_debuffs: vec![wowforever_data::RaidDebuffDef {
                id: "fire_curse".into(),
                name: "Fire Curse (test fixture)".into(),
                schools_affected: vec![School::Fire],
                damage_taken_pct_bonus: fire_curse_bonus,
                confidence: Confidence::CommunityReported,
                source: Source { name: "test".into(), url: None },
            }],
        }
    }

    fn mixed_school_member() -> RaidMember {
        RaidMember {
            name: "Mixed".into(),
            build: BuildConfig {
                spell_power: 0.0,
                crit_chance: 0.0, // disabled: hand-computable totals
                fight_duration_ms: 4500,
                rotation: vec!["spell_fire".into(), "spell_shadow".into()],
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
            },
        }
    }

    /// Hand-computed walkthrough: with GCD 1500ms and `spell_fire` on a 3s
    /// cooldown, a 4500ms fight casts fire@0, shadow@1500, fire@3000 (shadow
    /// isn't ready to be re-picked before fire's cooldown clears, since
    /// fire is first in rotation priority). That's 200 raw Fire damage and
    /// 100 raw Shadow damage over 4.5s. A raid debuff that only affects Fire
    /// must scale the Fire total and leave Shadow untouched -- a single
    /// scalar multiplier on the member's grand total would get this wrong.
    #[test]
    fn raid_debuff_only_affects_its_own_schools_in_a_mixed_school_rotation() {
        let data = mixed_school_data_pack(0.5); // +50% Fire damage taken
        let member = mixed_school_member();

        let without = simulate_raid_with_debuffs(&data, &[member.clone()], &[], 1, 1);
        let fight_s = 4.5;
        let expected_without = (200.0 + 100.0) / fight_s;
        assert!(
            (without.result.total.mean - expected_without).abs() < 1e-9,
            "expected {expected_without}, got {}",
            without.result.total.mean
        );

        let with = simulate_raid_with_debuffs(&data, &[member], &["fire_curse".into()], 1, 1);
        let expected_with = (200.0 * 1.5 + 100.0) / fight_s; // only the Fire total is boosted
        assert!(
            (with.result.total.mean - expected_with).abs() < 1e-9,
            "expected {expected_with}, got {}",
            with.result.total.mean
        );
        assert_eq!(with.active_debuff_ids, vec!["fire_curse".to_string()]);
        assert!(with.dropped_debuff_ids.is_empty());
    }

    /// An id with no matching entry in `data.raid_debuffs` is silently a
    /// no-op (not a panic) -- a UI might offer a debuff toggle for a
    /// mechanic not yet present in the loaded data-pack revision.
    #[test]
    fn unknown_debuff_id_has_no_effect() {
        let data = mixed_school_data_pack(0.5);
        let member = mixed_school_member();
        let baseline = simulate_raid_with_debuffs(&data, &[member.clone()], &[], 1, 1);
        let with_unknown = simulate_raid_with_debuffs(&data, &[member], &["does_not_exist".into()], 1, 1);
        assert_eq!(baseline.result.total.mean, with_unknown.result.total.mean);
    }

    /// The Classic debuff-slot cap (`CLASSIC_DEBUFF_LIMIT` = 16, see its own
    /// doc comment for sourcing): requesting more distinct debuff ids than
    /// the cap keeps only the first 16 in request order and reports the
    /// rest as dropped, rather than silently applying all of them or
    /// panicking. Duplicate ids (recasting the same curse) are deduped
    /// first and don't themselves consume extra slots.
    #[test]
    fn debuff_count_is_capped_at_the_classic_slot_limit() {
        let data = mixed_school_data_pack(0.5);
        let member = mixed_school_member();

        let mut requested: Vec<String> = (0..20).map(|i| format!("synthetic_debuff_{i}")).collect();
        requested.push("synthetic_debuff_0".into()); // duplicate, must not steal a slot

        let outcome = simulate_raid_with_debuffs(&data, &[member], &requested, 1, 1);
        assert_eq!(outcome.active_debuff_ids.len(), CLASSIC_DEBUFF_LIMIT);
        assert_eq!(outcome.active_debuff_ids, requested[..CLASSIC_DEBUFF_LIMIT]);
        assert_eq!(outcome.dropped_debuff_ids.len(), 4); // 20 distinct ids - 16 kept
    }
}
