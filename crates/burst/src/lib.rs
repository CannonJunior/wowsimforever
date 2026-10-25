//! Burst Damage Rotation Calculator (Plan Section 4.6): the maximum damage a
//! character can land in a short, fixed window by stacking cooldowns --
//! distinct from the steady-state DPS engine (`wowforever-engine`), which
//! answers a different question (long-run average rotation DPS).
//!
//! **Revised 2026-10-17** after the user correctly challenged this module's
//! original "no Monte Carlo needed" design. That reasoning rested on a
//! false premise -- that this data pack's damage values have no real
//! min/max range -- which turned out to be an artifact of how spell data
//! was being sourced (see `data-packs/README.md` / `docs/PLAN.md` for the
//! investigation), not a property of the actual game data. Real WoW spells
//! like Searing Pain do have damage ranges; the extraction pipeline was
//! failing to capture them. The design is corrected as follows:
//!
//! - **The search still uses exact closed-form expected value**, and this
//!   part of the original reasoning was correct and stays: by linearity of
//!   expectation, `E[sum of casts] = sum of E[each cast]` holds exactly
//!   regardless of how wide each cast's damage range is -- no sampling
//!   needed to rank candidate sequences by expected damage. The closed-form
//!   EV in `spell_contribution` is unchanged.
//! - **Final reporting now uses real Monte Carlo sampling of the chosen
//!   sequence**, not a naive "every cast rolls its extreme simultaneously"
//!   floor. That naive floor (which the original version of this module
//!   actually computed incorrectly anyway -- see below) is an overly
//!   pessimistic, unrealistic bound once there's more than one cast in the
//!   sequence: the probability every single roll lands at the minimum at
//!   once is far lower than a real 5th-percentile outcome. `evaluate_sequence`
//!   samples the fixed, already-chosen sequence many times and reports a
//!   real `DpsStats` distribution (mean/stdev/p5/p50/p95), using the same
//!   shared `wowforever-stats` the DPS engine uses.
//! - **A real bug this revision fixes**: the previous version's "floor"
//!   statistic summed the same EV-midpoint `base` value used for expected
//!   damage, not `base_min` -- so it wasn't a floor at all, just expected
//!   damage again. `guaranteed_floor` now correctly uses `base_min` with no
//!   crit, and is explicitly labeled as a theoretical bound (every cast
//!   rolling its worst case with no crits at all), not a realistic
//!   "sometimes you'll see this" number -- that's what `monte_carlo.p5` is
//!   for.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wowforever_data::{DataPack, PetType, School, SpellDef};
use wowforever_stats::{stats_from, DpsStats};

const GCD_MS: u32 = 1500;
const CRIT_MULTIPLIER: f64 = 1.5;
const PASS: &str = "--pass--";

// `#[serde(default)]` on every resist field below: this struct has grown a
// new field twice now (Frost/Arcane, then Holy) as `School` grew, and both
// times every existing JSON caller (Rust test fixtures *and* web/index.html's
// JS) broke with a hard deserialization error until manually patched. A
// missing resist field defaulting to "no resistance configured" (0.0) is a
// reasonable, honest default -- not a silently-wrong guess -- so the next
// school this project adds won't require hunting down every call site again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpponentProfile {
    pub name: String,
    #[serde(default)]
    pub shadow_resist_pct: f64,
    #[serde(default)]
    pub fire_resist_pct: f64,
    #[serde(default)]
    pub physical_resist_pct: f64,
    #[serde(default)]
    pub frost_resist_pct: f64,
    #[serde(default)]
    pub arcane_resist_pct: f64,
    #[serde(default)]
    pub holy_resist_pct: f64,
    #[serde(default)]
    pub nature_resist_pct: f64,
}

impl OpponentProfile {
    fn resist_for(&self, school: School) -> f64 {
        match school {
            School::Shadow => self.shadow_resist_pct,
            School::Fire => self.fire_resist_pct,
            School::Physical => self.physical_resist_pct,
            School::Frost => self.frost_resist_pct,
            School::Arcane => self.arcane_resist_pct,
            School::Holy => self.holy_resist_pct,
            School::Nature => self.nature_resist_pct,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackerOpener {
    pub starting_mana: f64,
    /// Recorded per the plan's spec; no mechanical effect on this Warlock-only
    /// Phase 1 slice (see module docs).
    pub opened_from_stealth_or_cc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurstBuildConfig {
    pub spell_power: f64,
    pub crit_chance: f64,
    pub window_ms: u32,
    pub available_spell_ids: Vec<String>,
    pub pet_type: PetType,
    pub demonic_sacrifice_active: bool,
    pub opponent: OpponentProfile,
    pub opener: AttackerOpener,
    /// Explicit search budget (Plan 4.6's correction): the beam width cap.
    pub search_beam_width: usize,
}

fn demonic_sacrifice_boost_pct(data: &DataPack, pet: PetType, school: School) -> f64 {
    let Some(effect) = data.demonic_sacrifice_effect(pet) else {
        return 0.0;
    };
    match school {
        School::Shadow => effect.shadow_damage_pct,
        School::Fire => effect.fire_damage_pct,
        // Demonic Sacrifice's documented pet effects (Plan 2.4) only ever
        // boost Shadow or Fire damage (Imp/Succubus) -- every other school
        // correctly has no bonus, not an unhandled gap.
        School::Physical | School::Frost | School::Arcane | School::Holy | School::Nature => 0.0,
    }
}

/// Exact expected damage contributed by casting `spell` at `cast_time_ms`,
/// counting only the portion (direct hit + any DoT ticks) that lands
/// strictly before `window_ms`. Exact regardless of range width: by
/// linearity of expectation, using the midpoint of a uniform range gives
/// the true expected value of that roll, no sampling required.
fn spell_expected_contribution(
    spell: &SpellDef,
    cast_time_ms: u32,
    window_ms: u32,
    buff_boost_pct: f64,
    resist_pct: impl Fn(School) -> f64,
    crit_chance: f64,
    spell_power: f64,
) -> f64 {
    let crit_ev_mult = 1.0 + crit_chance * (CRIT_MULTIPLIER - 1.0);
    let resist = 1.0 - resist_pct(spell.school);
    let mut ev = 0.0;

    let has_direct = !(spell.base_min == 0.0 && spell.base_max == 0.0 && spell.coefficient == 0.0);
    if has_direct && cast_time_ms < window_ms {
        let midpoint = (spell.base_min + spell.base_max) / 2.0;
        let base = (midpoint + spell.coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
        ev += base * crit_ev_mult;
    }

    if let Some(dot) = &spell.dot {
        for k in 1..=dot.num_ticks {
            let tick_time = cast_time_ms + k * dot.tick_interval_ms;
            if tick_time >= window_ms {
                break;
            }
            let base = (dot.tick_base + dot.tick_coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
            let mult = if dot.can_crit { crit_ev_mult } else { 1.0 };
            ev += base * mult;
        }
    }

    ev
}

/// The theoretical worst case for one cast: every roll lands at `base_min`,
/// nothing crits. A real bound (useful as a "this can never be worse than
/// X" guarantee), explicitly *not* a realistic percentile -- see module docs.
fn spell_guaranteed_floor_contribution(
    spell: &SpellDef,
    cast_time_ms: u32,
    window_ms: u32,
    buff_boost_pct: f64,
    resist_pct: impl Fn(School) -> f64,
    spell_power: f64,
) -> f64 {
    let resist = 1.0 - resist_pct(spell.school);
    let mut floor = 0.0;

    let has_direct = !(spell.base_min == 0.0 && spell.base_max == 0.0 && spell.coefficient == 0.0);
    if has_direct && cast_time_ms < window_ms {
        floor += (spell.base_min + spell.coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
    }
    if let Some(dot) = &spell.dot {
        for k in 1..=dot.num_ticks {
            let tick_time = cast_time_ms + k * dot.tick_interval_ms;
            if tick_time >= window_ms {
                break;
            }
            // DoT ticks in this data model have a single fixed `tick_base`,
            // not a range (consistent with how Classic-style DoTs are
            // itemized -- periodic ticks are fixed per rank, unlike direct
            // "nuke" hits). can_crit is always false for every DoT in the
            // current data pack, so there is no crit term to drop here.
            floor += (dot.tick_base + dot.tick_coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
        }
    }

    floor
}

/// One sampled realization of a single cast's damage, for the Monte Carlo
/// evaluation pass -- mirrors `wowforever-engine`'s `run_schedule` sampling
/// approach (uniform roll + Bernoulli crit), duplicated here rather than
/// shared because `wowforever-burst` is deliberately independent of
/// `wowforever-engine` (a distinct simulation mode, per the plan).
fn sample_spell_damage(
    spell: &SpellDef,
    cast_time_ms: u32,
    window_ms: u32,
    buff_boost_pct: f64,
    resist_pct: impl Fn(School) -> f64,
    crit_chance: f64,
    spell_power: f64,
    rng: &mut impl Rng,
) -> f64 {
    let resist = 1.0 - resist_pct(spell.school);
    let mut total = 0.0;

    let has_direct = !(spell.base_min == 0.0 && spell.base_max == 0.0 && spell.coefficient == 0.0);
    if has_direct && cast_time_ms < window_ms {
        let roll = if spell.base_max <= spell.base_min { spell.base_min } else { rng.gen_range(spell.base_min..=spell.base_max) };
        let mut dmg = (roll + spell.coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
        if rng.gen::<f64>() < crit_chance {
            dmg *= CRIT_MULTIPLIER;
        }
        total += dmg;
    }

    if let Some(dot) = &spell.dot {
        for k in 1..=dot.num_ticks {
            let tick_time = cast_time_ms + k * dot.tick_interval_ms;
            if tick_time >= window_ms {
                break;
            }
            let mut dmg = (dot.tick_base + dot.tick_coefficient * spell_power) * (1.0 + buff_boost_pct) * resist;
            if dot.can_crit && rng.gen::<f64>() < crit_chance {
                dmg *= CRIT_MULTIPLIER;
            }
            total += dmg;
        }
    }

    total
}

#[derive(Debug, Clone)]
struct SearchState {
    time_ms: u32,
    mana: f64,
    cooldown_until: HashMap<String, u32>,
    dot_active_until: HashMap<String, u32>,
    ev: f64,
    sequence: Vec<(u32, String)>,
}

#[derive(Debug, Clone)]
pub struct BurstSearchResult {
    pub sequence: Vec<(u32, String)>,
    /// Exact closed-form expected damage of this sequence -- what the
    /// search optimized for.
    pub expected_damage: f64,
    pub search_budget: usize,
    pub states_expanded: usize,
    /// True iff the beam never had to prune -- every reachable state was
    /// kept, so this result is provably optimal, not merely best-found.
    pub provably_optimal: bool,
}

/// Searches for the expected-damage-maximizing ability sequence within
/// `build.window_ms`, via beam search over a GCD-quantized timeline. See
/// module docs: this part of the design is unchanged and was never the
/// problem -- expected value is exact here regardless of range width.
pub fn find_best_burst_sequence(data: &DataPack, build: &BurstBuildConfig) -> BurstSearchResult {
    let resist_pct = |school: School| build.opponent.resist_for(school);
    let buff_boost_pct_for = |school: School| {
        if build.demonic_sacrifice_active {
            demonic_sacrifice_boost_pct(data, build.pet_type, school)
        } else {
            0.0
        }
    };

    let initial =
        SearchState { time_ms: 0, mana: build.opener.starting_mana, cooldown_until: HashMap::new(), dot_active_until: HashMap::new(), ev: 0.0, sequence: Vec::new() };

    let mut beam = vec![initial];
    let mut provably_optimal = true;
    let mut states_expanded = 0usize;

    loop {
        let (finished, active): (Vec<_>, Vec<_>) = beam.into_iter().partition(|s| s.time_ms >= build.window_ms);
        if active.is_empty() {
            beam = finished;
            break;
        }

        let mut successors = finished;
        for state in &active {
            states_expanded += 1;
            let mut any_branch = false;

            for id in &build.available_spell_ids {
                let spell = data.spell(id).unwrap_or_else(|| panic!("burst rotation references unknown spell id `{id}`"));
                let dot_ready = state.time_ms >= *state.dot_active_until.get(id).unwrap_or(&0);
                let cd_ready = state.time_ms >= *state.cooldown_until.get(id).unwrap_or(&0);
                let affordable = state.mana >= spell.resource_cost;
                if !(dot_ready && cd_ready && affordable) {
                    continue;
                }
                any_branch = true;

                let ev_delta = spell_expected_contribution(
                    spell,
                    state.time_ms,
                    build.window_ms,
                    buff_boost_pct_for(spell.school),
                    resist_pct,
                    build.crit_chance,
                    build.spell_power,
                );

                let mut next = state.clone();
                next.mana -= spell.resource_cost;
                if spell.cooldown_ms > 0 {
                    next.cooldown_until.insert(id.clone(), state.time_ms + spell.cooldown_ms);
                }
                if let Some(dot) = &spell.dot {
                    next.dot_active_until.insert(id.clone(), state.time_ms + dot.num_ticks * dot.tick_interval_ms);
                }
                next.ev += ev_delta;
                next.sequence.push((state.time_ms, id.clone()));
                next.time_ms += spell.cast_time_ms.max(GCD_MS);
                successors.push(next);
            }

            if !any_branch || build.available_spell_ids.is_empty() {
                let mut next = state.clone();
                next.time_ms += GCD_MS;
                next.sequence.push((state.time_ms, PASS.to_string()));
                successors.push(next);
            }
        }

        if successors.len() > build.search_beam_width {
            successors.sort_by(|a, b| b.ev.partial_cmp(&a.ev).unwrap());
            successors.truncate(build.search_beam_width);
            provably_optimal = false;
        }
        beam = successors;
    }

    let best = beam
        .into_iter()
        .max_by(|a, b| a.ev.partial_cmp(&b.ev).unwrap())
        .expect("beam must be non-empty: the initial state alone always survives to the final partition");

    BurstSearchResult {
        sequence: best.sequence,
        expected_damage: best.ev,
        search_budget: build.search_beam_width,
        states_expanded,
        provably_optimal,
    }
}

#[derive(Debug, Clone)]
pub struct SequenceEvaluation {
    /// Theoretical worst case: every cast rolls its minimum, nothing crits.
    /// A real bound, not a realistic outcome -- see `monte_carlo.p5` for that.
    pub guaranteed_floor: f64,
    /// Real sampled distribution of this exact sequence's total damage.
    pub monte_carlo: DpsStats,
}

/// Evaluates one fixed, already-chosen sequence (e.g. the output of
/// `find_best_burst_sequence`): computes its theoretical floor exactly, and
/// its realistic damage distribution via Monte Carlo sampling. Separated
/// from the search itself so the (cheap, exact) search isn't slowed down by
/// (comparatively expensive) sampling it doesn't need.
pub fn evaluate_sequence(
    data: &DataPack,
    build: &BurstBuildConfig,
    sequence: &[(u32, String)],
    iterations: u32,
    seed: u64,
) -> SequenceEvaluation {
    let resist_pct = |school: School| build.opponent.resist_for(school);
    let buff_boost_pct_for = |school: School| {
        if build.demonic_sacrifice_active {
            demonic_sacrifice_boost_pct(data, build.pet_type, school)
        } else {
            0.0
        }
    };

    let cast_events: Vec<(&SpellDef, u32)> = sequence
        .iter()
        .filter(|(_, id)| id != PASS)
        .map(|(t, id)| (data.spell(id).unwrap_or_else(|| panic!("unknown spell id `{id}` in sequence")), *t))
        .collect();

    let guaranteed_floor: f64 = cast_events
        .iter()
        .map(|(spell, t)| spell_guaranteed_floor_contribution(spell, *t, build.window_ms, buff_boost_pct_for(spell.school), resist_pct, build.spell_power))
        .sum();

    let mut samples = Vec::with_capacity(iterations as usize);
    for i in 0..iterations {
        let mut rng = Pcg32::seed_from_u64(seed.wrapping_add(i as u64));
        let total: f64 = cast_events
            .iter()
            .map(|(spell, t)| {
                sample_spell_damage(spell, *t, build.window_ms, buff_boost_pct_for(spell.school), resist_pct, build.crit_chance, build.spell_power, &mut rng)
            })
            .sum();
        samples.push(total);
    }

    SequenceEvaluation { guaranteed_floor, monte_carlo: stats_from(&mut samples) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowforever_data::{Confidence, Source};

    fn demo_data_pack() -> DataPack {
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![
                SpellDef {
                    id: "shadow_bolt".into(),
                    name: "Shadow Bolt".into(),
                    school: School::Shadow,
                    cast_time_ms: 3000,
                    cooldown_ms: 0,
                    resource_cost: 370.0,
                    base_min: 180.0,
                    base_max: 215.0, // a genuine range, per the user's correction
                    coefficient: 0.857,
                    ap_coefficient: 0.0,
                    resource_type: wowforever_data::ResourceType::Mana,
                    uses_weapon_damage: false,
                    mana_coefficient: 0.0,
                    stacking_buff: None,
                    timed_buff: None,
                    dot: None,
                    confidence: Confidence::CommunityReported,
                    source: Source { name: "test".into(), url: None },
                },
                SpellDef {
                    id: "shadowburn".into(),
                    name: "Shadowburn".into(),
                    school: School::Shadow,
                    cast_time_ms: 0,
                    cooldown_ms: 15000,
                    resource_cost: 105.0,
                    base_min: 50.0,
                    base_max: 84.0, // a genuine range
                    coefficient: 0.429,
                    ap_coefficient: 0.0,
                    resource_type: wowforever_data::ResourceType::Mana,
                    uses_weapon_damage: false,
                    mana_coefficient: 0.0,
                    stacking_buff: None,
                    timed_buff: None,
                    dot: None,
                    confidence: Confidence::CommunityReported,
                    source: Source { name: "test".into(), url: None },
                },
            ],
            disputed_mechanics: vec![],
            dispute_ledger: vec![],
            demonic_sacrifice_effects: vec![],
            raid_debuffs: vec![],
        }
    }

    fn base_build() -> BurstBuildConfig {
        BurstBuildConfig {
            spell_power: 350.0,
            crit_chance: 0.0,
            window_ms: 9000,
            available_spell_ids: vec!["shadow_bolt".into(), "shadowburn".into()],
            pet_type: PetType::Imp,
            demonic_sacrifice_active: false,
            opponent: OpponentProfile {
                name: "test".into(),
                shadow_resist_pct: 0.0,
                fire_resist_pct: 0.0,
                physical_resist_pct: 0.0,
                frost_resist_pct: 0.0,
                arcane_resist_pct: 0.0,
                holy_resist_pct: 0.0,
                nature_resist_pct: 0.0,
            },
            opener: AttackerOpener { starting_mana: 10_000.0, opened_from_stealth_or_cc: false },
            search_beam_width: 256,
        }
    }

    #[test]
    fn provably_optimal_for_a_small_ability_pool() {
        let data = demo_data_pack();
        let result = find_best_burst_sequence(&data, &base_build());
        assert!(result.provably_optimal);
    }

    /// The bug this revision fixes: with a genuine range, the guaranteed
    /// floor must be strictly less than expected damage (since it uses
    /// base_min instead of the midpoint) -- the old version summed the same
    /// midpoint-based value for both, so floor == expected always, which
    /// was the bug.
    #[test]
    fn guaranteed_floor_is_strictly_below_expected_damage_when_range_is_real() {
        let data = demo_data_pack();
        let build = base_build();
        let search = find_best_burst_sequence(&data, &build);
        let eval = evaluate_sequence(&data, &build, &search.sequence, 1000, 1);

        assert!(
            eval.guaranteed_floor < search.expected_damage,
            "guaranteed_floor ({}) must be strictly less than expected_damage ({}) when base_min < base_max",
            eval.guaranteed_floor,
            search.expected_damage
        );
    }

    /// The Monte Carlo mean of a real sequence with crit disabled and
    /// uniform ranges must converge close to the exact closed-form expected
    /// value (law of large numbers) -- cross-validates the two independent
    /// code paths (exact EV formula vs. sampling) against each other.
    #[test]
    fn monte_carlo_mean_converges_to_closed_form_expected_value() {
        let data = demo_data_pack();
        let build = base_build();
        let search = find_best_burst_sequence(&data, &build);
        let eval = evaluate_sequence(&data, &build, &search.sequence, 20_000, 42);

        let relative_error = (eval.monte_carlo.mean - search.expected_damage).abs() / search.expected_damage;
        assert!(
            relative_error < 0.01,
            "Monte Carlo mean {} should be within 1% of exact expected value {} at 20,000 iterations, got {:.4}% error",
            eval.monte_carlo.mean,
            search.expected_damage,
            relative_error * 100.0
        );
    }

    /// With crit enabled, the Monte Carlo p5/p95 band must be meaningfully
    /// wider than a degenerate point -- this is the actual property the
    /// user's critique demanded: realistic variance must show up in the
    /// reported distribution, not just in an unused RNG code path.
    #[test]
    fn monte_carlo_distribution_has_real_spread_with_range_and_crit() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.crit_chance = 0.3;
        let search = find_best_burst_sequence(&data, &build);
        let eval = evaluate_sequence(&data, &build, &search.sequence, 20_000, 7);

        assert!(eval.monte_carlo.p95 > eval.monte_carlo.p5 * 1.05, "expected a real spread between p5 and p95, got p5={} p95={}", eval.monte_carlo.p5, eval.monte_carlo.p95);
    }

    #[test]
    fn insufficient_mana_forces_a_pass_instead_of_an_illegal_cast() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.opener.starting_mana = 50.0;
        build.window_ms = 3000;

        let search = find_best_burst_sequence(&data, &build);
        assert_eq!(search.expected_damage, 0.0);
        assert!(search.sequence.iter().all(|(_, id)| id == PASS));
    }

    #[test]
    fn tight_search_budget_disables_the_optimality_guarantee() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.search_beam_width = 1;
        build.window_ms = 12000;

        let result = find_best_burst_sequence(&data, &build);
        assert!(!result.provably_optimal);
    }
}
