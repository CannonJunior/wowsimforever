//! Phase 1 MVP simulation engine (Plan Section 7): Warlock, single-target only.
//!
//! Scope deliberately narrowed per the plan's Phase 1 cut: one simplified
//! rotation-priority list rather than the full APL grammar envisioned in
//! Section 2.2/Principle driven design (that richer grammar is a later-phase
//! investment, not re-derived here), hand-entered gear only (no item data
//! pack), and no resource/mana modeling (assumed unconstrained for Phase 1 -
//! this is a known simplification, not a claim of final accuracy).
//!
//! The one mechanic given full first-class treatment is the Demonic
//! Sacrifice x Demonic Pact mechanic (Plan 2.4, Principle 3, Principle 13).
//! Research on 2026-10-17 (wofwforever.com, classicwowforever.com) converged
//! on a more precise rule than the original two-source framing: Demonic
//! Pact exempts summoning a *different* demon from canceling the buff, but
//! resummoning the *same* (sacrificed) pet still cancels it regardless. That
//! same/different-pet distinction is still not confirmed by a primary
//! Blizzard source (the 2026-10-16 Kaivax dev notes don't mention either
//! talent at all), so it remains a tracked, open dispute (see the data
//! pack's dispute_ledger) -- just a narrower one than originally modeled:
//! the two candidate rules now only disagree on same-pet resummons while
//! Demonic Pact is known.

use rand::{Rng, SeedableRng};
use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wowforever_data::{DataPack, School};
pub use wowforever_data::PetType;
pub use wowforever_stats::{stats_from, DpsStats};

pub const DEMONIC_SACRIFICE_MECHANIC_ID: &str = "demonic_sacrifice_x_demonic_pact";

/// The better-sourced candidate rule (wofwforever.com / classicwowforever.com,
/// 2026-10-17 research): Demonic Pact exempts a *different*-pet resummon only.
pub const RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY: &str = "pact_exempts_different_pet_only";
/// The original, cruder candidate rule (andycarlson13/foreversims README):
/// once Demonic Pact is known, no resummon cancels the buff at all.
pub const RULE_PACT_EXEMPTS_ANY_RESUMMON: &str = "pact_exempts_any_resummon";

/// Fixed Classic-style global cooldown. A simplification for Phase 1: a real
/// engine would read this from the data pack too, since it is itself a
/// patchable value, not a law of nature.
const GCD_MS: u32 = 1500;

/// Classic-style spell crit multiplier (bonus damage = 0.5x base). Treated as
/// a documented simplification, not a WoW-Forever-verified constant -- it
/// belongs in the data pack in a later iteration, not hard-coded here
/// permanently.
const CRIT_MULTIPLIER: f64 = 1.5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub spell_power: f64,
    pub crit_chance: f64,
    pub fight_duration_ms: u32,
    /// Ordered spell-id priority list: simplified APL (see module docs).
    pub rotation: Vec<String>,
    pub pet_type: PetType,
    /// Whether Demonic Sacrifice was cast pre-pull (the pet is already sacrificed).
    pub demonic_sacrifice_active: bool,
    /// Whether the build has taken the Demonic Pact talent at all. Without
    /// it, any demon summon cancels the buff -- undisputed, confirmed by
    /// Demonic Sacrifice's own base tooltip (classicwowforever.com).
    pub has_demonic_pact: bool,
    /// A mid-fight "a demon is summoned" event (e.g. the pet died and was
    /// resummoned). `None` means the mechanic is never challenged by this
    /// build at all.
    pub resummon_event_ms: Option<u32>,
    /// Whether that resummon is the *same* pet type that was originally
    /// sacrificed (vs. a different demon). This is the dimension the two
    /// candidate rules actually disagree on -- a different-pet resummon
    /// persists under both rules once Demonic Pact is known, so it is not
    /// disputed. Meaningless when `resummon_event_ms` is `None`.
    pub resummon_is_same_pet: bool,
    /// Force a specific candidate rule id instead of the data pack's
    /// default (Principle 13's "expose the alternative as a toggle").
    pub dispute_rule_override: Option<String>,
    /// Mana/resource constraint (a Phase 1 simplification, now partially
    /// closed -- see `build_schedule`'s doc comment). `None` preserves
    /// this engine's original, historical behavior: unconstrained
    /// casting, `resource_cost` tracked in the data pack but never
    /// enforced. `Some(m)` starts the fight with `m` mana and actually
    /// enforces each cast's `resource_cost`, regenerating at
    /// `mana_regen_per_second` while no rotation entry is affordable,
    /// capped at `m` (the starting pool doubles as the max pool -- this
    /// engine doesn't separately model gear-granted mana-pool increases).
    #[serde(default)]
    pub starting_mana: Option<f64>,
    /// Only meaningful when `starting_mana` is `Some`. A flat per-second
    /// regen rate -- a real, documented simplification, not Classic's
    /// actual spirit-based regen-with-the-5-second-rule mechanics (which
    /// halve/zero regen for 5s after any cast); modeling that precisely is
    /// future work, not claimed here.
    #[serde(default)]
    pub mana_regen_per_second: f64,
    /// Attack power (Hunter/Rogue/Warrior extension, Plan Section 7's
    /// seventeenth update): these three classes deal 100%
    /// attack-power/weapon-damage-scaling damage in Classic-era
    /// itemization, with zero spell-power-scaling abilities -- this is a
    /// parallel scaling stat to `spell_power`, hand-entered the same way
    /// (a build-configuration input, not a sourced game fact). `0.0` is
    /// inert for every pre-existing caster build.
    #[serde(default)]
    pub attack_power: f64,
    /// Hand-entered weapon damage range and swing speed. `weapon_speed_ms:
    /// 0` means "no weapon equipped" -- no auto-attack events are
    /// scheduled at all, which is what every pre-existing caster build
    /// gets by default. Auto-attack swings are scheduled on their own
    /// independent timeline (real WoW mechanic: melee swings continue
    /// regardless of what spells/abilities are being cast, not gated by
    /// the global cooldown the `rotation` loop uses).
    #[serde(default)]
    pub weapon_min: f64,
    #[serde(default)]
    pub weapon_max: f64,
    #[serde(default)]
    pub weapon_speed_ms: u32,
}

#[derive(Debug, Clone)]
struct DamageEvent {
    time_ms: u32,
    school: School,
    base_min: f64,
    base_max: f64,
    coefficient: f64,
    /// Attack-power scaling coefficient (parallel to `coefficient`'s
    /// spell-power scaling). Real auto-attack swings use the classic
    /// `attack_power / 14 * weapon_speed_seconds` formula, expressed here
    /// as `ap_coefficient = weapon_speed_seconds / 14` so the same
    /// `base + coefficient * spell_power + ap_coefficient * attack_power`
    /// formula covers both scaling kinds uniformly. Always 0.0 for every
    /// spell-power-scaling event.
    ap_coefficient: f64,
    can_crit: bool,
    /// Which spell this event came from (`"auto_attack"` for melee
    /// swings, which have no `SpellDef`) -- carried through so a detailed
    /// per-run breakdown (`simulate_detail`) can group/label events
    /// without re-deriving which spell produced them.
    source_id: String,
    kind: EventKind,
}

/// What kind of damage instance an event represents -- distinguishes a
/// spell's own direct hit from its DoT's periodic ticks (e.g. Immolate
/// has both, and real players see them reported separately) and from
/// auto-attack swings (which have no `SpellDef` at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Direct,
    Tick,
    AutoAttack,
}

/// Builds the deterministic cast/tick schedule for the fight. Deliberately
/// has no RNG dependency, and does not depend on which disputed-mechanic
/// rule is in effect -- both facts are what let `simulate_dispute_delta`
/// replay the identical schedule under both rules with common random
/// numbers. Mana consumption is also deterministic (`resource_cost` is a
/// fixed per-cast amount, not randomized), so enforcing it belongs here
/// too, not in the RNG-dependent damage-rolling step.
fn build_schedule(data: &DataPack, build: &BuildConfig) -> Vec<DamageEvent> {
    let mut time: u32 = 0;
    let mut dot_active_until: HashMap<String, u32> = HashMap::new();
    let mut cooldown_until: HashMap<String, u32> = HashMap::new();
    let mut events = Vec::new();
    let mana_capacity = build.starting_mana.unwrap_or(f64::INFINITY);
    let mut mana = mana_capacity;

    // An empty rotation is legitimate for a pure-auto-attack build (no
    // sourced special abilities yet, e.g. Hunter/Rogue/Warrior's current
    // scope) -- skip the GCD-gated loop entirely rather than looping
    // forever at time=0 or panicking; the auto-attack timeline below is
    // independent of this loop and still runs regardless.
    while !build.rotation.is_empty() && time < build.fight_duration_ms {
        let mut chosen = None;
        for id in &build.rotation {
            let spell = data
                .spell(id)
                .unwrap_or_else(|| panic!("rotation references unknown spell id `{id}`"));
            // Eligible only once both the DoT-reapplication window (if any)
            // and the spell's own cooldown (if any, e.g. Shadowburn's 15s)
            // have elapsed -- a prior version of this check ignored
            // `cooldown_ms` for any spell without a DoT, which silently
            // ignored cooldowns on spells like Shadowburn. Also only
            // eligible if affordable -- with `starting_mana: None` this is
            // always true (mana is infinite), preserving every pre-mana-
            // modeling build's exact behavior.
            let dot_ready = time >= *dot_active_until.get(id).unwrap_or(&0);
            let cd_ready = time >= *cooldown_until.get(id).unwrap_or(&0);
            let affordable = mana >= spell.resource_cost;
            if dot_ready && cd_ready && affordable {
                chosen = Some(spell);
                break;
            }
        }
        let Some(spell) = chosen else {
            // Nothing in the rotation is eligible right now -- every gate
            // (cooldown, DoT-reapplication window, mana) is always
            // temporary, so the correct real-play answer is the same one
            // mana-starvation already used: wait one GCD, let cooldowns/
            // DoT windows/mana regen advance, and retry. This covers a
            // real, legitimate build shape this project didn't have until
            // Bloodthirst was sourced: a rotation that's *entirely*
            // cooldown-gated abilities (no spell with neither a cooldown
            // nor a DoT) -- a real Warrior keeps auto-attacking (modeled
            // on its own independent timeline below, unaffected by this
            // wait) while waiting for Bloodthirst to come off cooldown,
            // rather than this being a rotation-design bug. Bounded by the
            // outer `while time < fight_duration_ms` -- time strictly
            // advances every iteration, so this cannot loop forever even
            // if nothing ever becomes eligible again.
            time += GCD_MS;
            mana = (mana + build.mana_regen_per_second * (GCD_MS as f64 / 1000.0)).min(mana_capacity);
            continue;
        };
        mana -= spell.resource_cost;
        if spell.cooldown_ms > 0 {
            cooldown_until.insert(spell.id.clone(), time + spell.cooldown_ms);
        }

        let has_direct_component = !(spell.base_min == 0.0
            && spell.base_max == 0.0
            && spell.coefficient == 0.0
            && spell.ap_coefficient == 0.0);
        if has_direct_component {
            events.push(DamageEvent {
                time_ms: time,
                school: spell.school,
                base_min: spell.base_min,
                base_max: spell.base_max,
                coefficient: spell.coefficient,
                ap_coefficient: spell.ap_coefficient,
                can_crit: true,
                source_id: spell.id.clone(),
                kind: EventKind::Direct,
            });
        }

        if let Some(dot) = &spell.dot {
            for k in 1..=dot.num_ticks {
                events.push(DamageEvent {
                    time_ms: time + k * dot.tick_interval_ms,
                    school: spell.school,
                    base_min: dot.tick_base,
                    base_max: dot.tick_base,
                    coefficient: dot.tick_coefficient,
                    ap_coefficient: 0.0,
                    can_crit: dot.can_crit,
                    source_id: spell.id.clone(),
                    kind: EventKind::Tick,
                });
            }
            dot_active_until.insert(spell.id.clone(), time + dot.num_ticks * dot.tick_interval_ms);
        }

        time += spell.cast_time_ms.max(GCD_MS);
    }

    // Auto-attack (Hunter/Rogue/Warrior extension): an independent
    // timeline, not gated by the rotation loop's GCD at all -- real melee
    // swings happen on their own weapon-speed timer regardless of what
    // spells/abilities are being cast. `weapon_speed_ms: 0` (every
    // pre-existing caster build's default) means no weapon equipped, so
    // no swings are scheduled and this is a no-op for them.
    if build.weapon_speed_ms > 0 {
        let ap_coefficient = (build.weapon_speed_ms as f64 / 1000.0) / 14.0; // classic AP/14 * swing_time formula
        let mut swing_time = 0u32;
        while swing_time < build.fight_duration_ms {
            events.push(DamageEvent {
                time_ms: swing_time,
                school: School::Physical,
                base_min: build.weapon_min,
                base_max: build.weapon_max,
                coefficient: 0.0,
                ap_coefficient,
                can_crit: true,
                source_id: "auto_attack".to_string(),
                kind: EventKind::AutoAttack,
            });
            swing_time += build.weapon_speed_ms;
        }
    }

    events
}

/// Whether the Demonic Sacrifice buff is active at time `t`. `persists` is
/// the pre-resolved outcome (see `effective_persists`) of "does this build,
/// under this candidate rule, keep the buff past its resummon event."
fn buff_active_at(t: u32, initial_active: bool, resummon_ms: Option<u32>, persists: bool) -> bool {
    if !initial_active {
        return false;
    }
    match resummon_ms {
        None => true,
        Some(r) => t < r || persists,
    }
}

/// Resolves whether the buff survives the build's resummon event under one
/// candidate rule (`rule_exempts_any_resummon` = true selects
/// `RULE_PACT_EXEMPTS_ANY_RESUMMON`; false selects the better-sourced
/// different-pet-only exemption). Without Demonic Pact this is always
/// `false` -- undisputed base behavior.
fn effective_persists(build: &BuildConfig, rule_exempts_any_resummon: bool) -> bool {
    if !build.has_demonic_pact {
        return false;
    }
    if build.resummon_is_same_pet {
        rule_exempts_any_resummon
    } else {
        true // both candidate rules agree: a different-pet resummon persists once Demonic Pact is known
    }
}

fn roll_base(rng: &mut impl Rng, min: f64, max: f64) -> f64 {
    if max <= min {
        min
    } else {
        rng.gen_range(min..=max)
    }
}

/// Per-school damage boost from an active Demonic Sacrifice buff, looked up
/// from the data pack (Principle 1: no hard-coded game values in engine
/// code). Returns 0.0 for schools/pets with no damage effect (e.g.
/// Voidwalker's mana regen isn't a damage boost and is out of this pure-DPS
/// engine's scope) or when the data pack has no entry for this pet at all.
fn boost_pct_for(data: &DataPack, pet: PetType, school: School) -> f64 {
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

/// Runs one simulated iteration and returns raw (not yet divided by fight
/// duration) damage totals broken down **by school**. Broken down rather
/// than collapsed to a single scalar so that raid-level code
/// (`wowforever-raid`) can apply a school-targeted raid debuff (e.g. Curse
/// of the Elements, which only increases Fire/Frost/Arcane/Shadow damage
/// taken, not Physical or Holy) correctly for builds whose rotation mixes
/// schools -- a single post-hoc multiplier on the grand total would be
/// wrong for e.g. a Warlock whose Immolate (Fire) is affected but whose
/// Shadow Bolt is not. `simulate_samples` below just sums this map back
/// down to the scalar every other caller already expects.
fn run_schedule(
    data: &DataPack,
    events: &[DamageEvent],
    build: &BuildConfig,
    rng: &mut impl Rng,
    persists: bool,
) -> HashMap<School, f64> {
    let mut totals: HashMap<School, f64> = HashMap::new();
    for ev in events {
        if ev.time_ms >= build.fight_duration_ms {
            continue;
        }
        let base = roll_base(rng, ev.base_min, ev.base_max);
        let mut dmg = base + ev.coefficient * build.spell_power + ev.ap_coefficient * build.attack_power;

        if buff_active_at(ev.time_ms, build.demonic_sacrifice_active, build.resummon_event_ms, persists) {
            dmg *= 1.0 + boost_pct_for(data, build.pet_type, ev.school);
        }

        if ev.can_crit && rng.gen::<f64>() < build.crit_chance {
            dmg *= CRIT_MULTIPLIER;
        }

        *totals.entry(ev.school).or_insert(0.0) += dmg;
    }
    totals
}

/// Resolves which candidate rule this build's own (non-dispute-delta) run
/// should use: the user's override wins, falling back to the data pack's
/// recorded default (Principle 11), then resolves that rule id to an
/// effective persists/cancels outcome for this specific build.
fn resolve_persists(data: &DataPack, build: &BuildConfig) -> bool {
    let rule_id = build.dispute_rule_override.clone().unwrap_or_else(|| {
        data.mechanic(DEMONIC_SACRIFICE_MECHANIC_ID)
            .map(|m| m.default_rule_id.clone())
            .unwrap_or_else(|| RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY.to_string())
    });
    effective_persists(build, rule_id == RULE_PACT_EXEMPTS_ANY_RESUMMON)
}

/// Whether this build's configuration mechanically exercises the mechanic at
/// all: Demonic Pact known, a resummon happens, and specifically a
/// *same*-pet resummon -- the one case the two candidate rules disagree on
/// (see module docs). This is independent of whether the mechanic has since
/// been resolved; it only asks "would these two rules give different
/// answers for this build."
fn exercises_the_mechanic(build: &BuildConfig) -> bool {
    build.demonic_sacrifice_active
        && build.has_demonic_pact
        && build.resummon_event_ms.is_some()
        && build.resummon_is_same_pet
}

/// Whether this build should actually warn the user with a live "disputed
/// mechanic" flag: mechanically relevant *and* still open in the data
/// pack's public dispute ledger (Plan Section 4.7). Once a mechanic is
/// confirmed by a primary source and marked resolved there, builds that
/// exercise it no longer need the warning -- the comparison in
/// `simulate_dispute_delta` remains available for historical reference, but
/// `SimOutput::touches_disputed_mechanic` should stop firing.
fn touches_open_dispute(data: &DataPack, build: &BuildConfig) -> bool {
    exercises_the_mechanic(build) && data.dispute_status(DEMONIC_SACRIFICE_MECHANIC_ID) == Some(wowforever_data::DisputeStatus::Open)
}

#[derive(Debug, Clone)]
pub struct SimOutput {
    pub dps: DpsStats,
    pub data_pack_version: String,
    /// Surfaces to the UI per Section 5's confidence-tag treatment: this
    /// result's validity depends on an unresolved community dispute.
    pub touches_disputed_mechanic: bool,
}

/// The raw per-iteration DPS samples, before being collapsed into
/// `DpsStats`. Exposed (not just internal to `simulate`) because correctly
/// aggregating *multiple* characters' DPS into a raid total (Plan Section
/// 8) requires summing same-iteration samples across characters, then
/// taking percentiles of the sums -- summing each character's
/// already-computed p5/p50/p95 independently would be wrong, since
/// percentiles aren't additive even though means are. See
/// `wowforever-raid`, which is the actual consumer of this.
pub fn simulate_samples(data: &DataPack, build: &BuildConfig, iterations: u32, seed: u64) -> Vec<f64> {
    let schedule = build_schedule(data, build);
    let persists = resolve_persists(data, build);
    let fight_s = build.fight_duration_ms as f64 / 1000.0;

    (0..iterations)
        .map(|i| {
            let mut rng = Pcg32::seed_from_u64(seed.wrapping_add(i as u64));
            let totals = run_schedule(data, &schedule, build, &mut rng, persists);
            totals.values().sum::<f64>() / fight_s
        })
        .collect()
}

/// Same as `simulate_samples`, but keeps each iteration's damage broken
/// down by school instead of summing it. Exposed for raid-level code
/// (`wowforever-raid`) that needs to apply a school-targeted raid debuff
/// (e.g. Curse of the Elements) correctly across mixed-school rotations
/// -- see `run_schedule`'s doc comment for why a single scalar isn't
/// enough for that.
pub fn simulate_samples_by_school(
    data: &DataPack,
    build: &BuildConfig,
    iterations: u32,
    seed: u64,
) -> Vec<HashMap<School, f64>> {
    let schedule = build_schedule(data, build);
    let persists = resolve_persists(data, build);
    let fight_s = build.fight_duration_ms as f64 / 1000.0;

    (0..iterations)
        .map(|i| {
            let mut rng = Pcg32::seed_from_u64(seed.wrapping_add(i as u64));
            let totals = run_schedule(data, &schedule, build, &mut rng, persists);
            totals.into_iter().map(|(school, v)| (school, v / fight_s)).collect()
        })
        .collect()
}

/// One rolled damage instance from a single representative simulated run
/// (see `simulate_detail`) -- a real event, not an average: its `damage`
/// and `is_crit` are this specific seed's actual roll, the same way a
/// real combat log entry would be.
#[derive(Debug, Clone, Serialize)]
pub struct DetailEvent {
    pub time_ms: u32,
    pub source_id: String,
    pub kind: EventKind,
    pub school: School,
    pub damage: f64,
    pub is_crit: bool,
}

/// Aggregated per-ability statistics over one representative run (not an
/// average across iterations -- see `simulate_detail`'s doc comment for
/// why a single real run, not a cross-iteration average, is what this
/// project reports here). `(source_id, kind)` is the grouping key so a
/// spell's direct hit and its DoT ticks are reported as separate lines,
/// matching how a real player would read a combat log (e.g. "Immolate"
/// the direct hit vs. "Immolate" the burn are different line items).
#[derive(Debug, Clone, Serialize)]
pub struct AbilityBreakdown {
    pub source_id: String,
    pub kind: EventKind,
    pub casts: u32,
    pub total_damage: f64,
    /// How many of this ability's hits in this run *could* crit (DoT
    /// ticks often cannot) -- the denominator for a meaningful crit %;
    /// using `casts` instead would understate crit rate for an ability
    /// that's a mix of crittable and non-crittable hits.
    pub crit_eligible: u32,
    pub crits: u32,
}

/// A buff or debuff's active window during one representative run. This
/// project currently models exactly one such mechanic (Demonic
/// Sacrifice) -- a build with none active correctly produces an empty
/// list, shown as "no buffs/debuffs modeled for this build" rather than
/// a fabricated one.
#[derive(Debug, Clone, Serialize)]
pub struct BuffWindow {
    pub name: String,
    pub start_ms: u32,
    pub end_ms: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimDetail {
    pub fight_duration_ms: u32,
    /// Chronologically sorted (schedule construction interleaves a
    /// spell's own direct hit and DoT ticks with other casts' events in
    /// push order, not time order -- sorted here purely for a readable
    /// timeline, which does not change RNG draw order above).
    pub events: Vec<DetailEvent>,
    /// Sorted by `total_damage` descending -- the highest-damage ability
    /// first, matching how every real sim tool's breakdown table reads.
    pub abilities: Vec<AbilityBreakdown>,
    pub buffs: Vec<BuffWindow>,
}

/// Simulates exactly **one** representative run (not a cross-iteration
/// average) and returns full per-event detail: a chronological event
/// list, a per-ability damage/crit breakdown, and any buff/debuff
/// windows. This is a deliberately different kind of output from
/// `simulate`'s Monte Carlo summary stats -- real sim tools (e.g.
/// mythicsim.com) show both a many-iteration aggregate *and* a sample
/// log/timeline from one real run side by side, because a literal
/// event-by-event trace of "the average run" doesn't exist (you can't
/// average *which* rolls crit across 10,000 independent coin flips into
/// one coherent timeline) -- only a specific run can be shown as a
/// timeline, so this is explicitly one, clearly labeled as such by every
/// caller, not implied to be "the" output of the fight.
pub fn simulate_detail(data: &DataPack, build: &BuildConfig, seed: u64) -> SimDetail {
    let schedule = build_schedule(data, build);
    let persists = resolve_persists(data, build);
    let mut rng = Pcg32::seed_from_u64(seed);

    let mut events = Vec::with_capacity(schedule.len());
    let mut breakdown: HashMap<(String, EventKind), AbilityBreakdown> = HashMap::new();

    for ev in &schedule {
        if ev.time_ms >= build.fight_duration_ms {
            continue;
        }
        let base = roll_base(&mut rng, ev.base_min, ev.base_max);
        let mut dmg = base + ev.coefficient * build.spell_power + ev.ap_coefficient * build.attack_power;

        if buff_active_at(ev.time_ms, build.demonic_sacrifice_active, build.resummon_event_ms, persists) {
            dmg *= 1.0 + boost_pct_for(data, build.pet_type, ev.school);
        }

        let is_crit = ev.can_crit && rng.gen::<f64>() < build.crit_chance;
        if is_crit {
            dmg *= CRIT_MULTIPLIER;
        }

        events.push(DetailEvent {
            time_ms: ev.time_ms,
            source_id: ev.source_id.clone(),
            kind: ev.kind,
            school: ev.school,
            damage: dmg,
            is_crit,
        });

        let entry = breakdown.entry((ev.source_id.clone(), ev.kind)).or_insert(AbilityBreakdown {
            source_id: ev.source_id.clone(),
            kind: ev.kind,
            casts: 0,
            total_damage: 0.0,
            crit_eligible: 0,
            crits: 0,
        });
        entry.casts += 1;
        entry.total_damage += dmg;
        if ev.can_crit {
            entry.crit_eligible += 1;
        }
        if is_crit {
            entry.crits += 1;
        }
    }

    events.sort_by_key(|e| e.time_ms);

    let mut abilities: Vec<AbilityBreakdown> = breakdown.into_values().collect();
    abilities.sort_by(|a, b| b.total_damage.partial_cmp(&a.total_damage).unwrap_or(std::cmp::Ordering::Equal));

    let mut buffs = Vec::new();
    if build.demonic_sacrifice_active {
        let end_ms = match build.resummon_event_ms {
            None => build.fight_duration_ms,
            Some(r) => {
                if persists {
                    build.fight_duration_ms
                } else {
                    r.min(build.fight_duration_ms)
                }
            }
        };
        buffs.push(BuffWindow { name: "Demonic Sacrifice".to_string(), start_ms: 0, end_ms });
    }

    SimDetail { fight_duration_ms: build.fight_duration_ms, events, abilities, buffs }
}

pub fn simulate(data: &DataPack, build: &BuildConfig, iterations: u32, seed: u64) -> SimOutput {
    let mut samples = simulate_samples(data, build, iterations, seed);

    SimOutput {
        dps: stats_from(&mut samples),
        data_pack_version: data.version.clone(),
        touches_disputed_mechanic: touches_open_dispute(data, build),
    }
}

#[derive(Debug, Clone)]
pub struct DisputeDeltaOutput {
    pub mechanic_id: String,
    pub rule_a_label: String,
    pub rule_a_dps: DpsStats,
    pub rule_b_label: String,
    pub rule_b_dps: DpsStats,
    pub mean_dps_delta: f64,
    pub pct_swing: f64,
}

/// Implements Principle 13: compute and surface the DPS delta between the
/// two candidate rules for the Demonic Sacrifice x Demonic Pact mechanic,
/// rather than merely stating that both are representable. Returns `None`
/// when the build never exercises the mechanic at all (no Demonic Pact, no
/// resummon event, or a different-pet resummon that both rules already
/// agree on) -- the delta is then zero by construction and not worth
/// reporting. Available regardless of the mechanic's ledger status: even
/// once resolved, this remains a useful historical "how much did this used
/// to matter" comparison (see `touches_open_dispute` for the separate,
/// resolution-aware "should the UI still warn about this" check).
pub fn simulate_dispute_delta(
    data: &DataPack,
    build: &BuildConfig,
    iterations: u32,
    seed: u64,
) -> Option<DisputeDeltaOutput> {
    if !exercises_the_mechanic(build) {
        return None;
    }
    let mechanic = data.mechanic(DEMONIC_SACRIFICE_MECHANIC_ID)?;
    let schedule = build_schedule(data, build);
    let fight_s = build.fight_duration_ms as f64 / 1000.0;

    let persists_a = effective_persists(build, false); // RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY
    let persists_b = effective_persists(build, true); // RULE_PACT_EXEMPTS_ANY_RESUMMON

    let mut samples_a = Vec::with_capacity(iterations as usize);
    let mut samples_b = Vec::with_capacity(iterations as usize);
    for i in 0..iterations {
        let seed_i = seed.wrapping_add(i as u64);
        // Common random numbers: identical seed and identical schedule means
        // identical draw order, so only the disputed rule differs.
        let mut rng_a = Pcg32::seed_from_u64(seed_i);
        let mut rng_b = Pcg32::seed_from_u64(seed_i);
        samples_a.push(run_schedule(data, &schedule, build, &mut rng_a, persists_a).values().sum::<f64>() / fight_s);
        samples_b.push(run_schedule(data, &schedule, build, &mut rng_b, persists_b).values().sum::<f64>() / fight_s);
    }

    let stats_a = stats_from(&mut samples_a);
    let stats_b = stats_from(&mut samples_b);
    let delta = stats_b.mean - stats_a.mean;
    let pct_swing = delta / stats_a.mean * 100.0;

    Some(DisputeDeltaOutput {
        mechanic_id: mechanic.id.clone(),
        rule_a_label: mechanic.rule(RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY).map(|r| r.label.clone()).unwrap_or_default(),
        rule_a_dps: stats_a,
        rule_b_label: mechanic.rule(RULE_PACT_EXEMPTS_ANY_RESUMMON).map(|r| r.label.clone()).unwrap_or_default(),
        rule_b_dps: stats_b,
        mean_dps_delta: delta,
        pct_swing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowforever_data::{
        CandidateRule, Confidence, DemonicSacrificeEffect, DisputeLedgerEntry, DisputeStatus, DisputedMechanic,
        DotDef, SpellDef, Source,
    };

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
                    resource_cost: 0.0,
                    base_min: 100.0,
                    base_max: 100.0, // fixed for a hand-computable unit test
                    coefficient: 0.5,
                    ap_coefficient: 0.0,
                    dot: None,
                    confidence: Confidence::CommunityReported,
                    source: Source { name: "test".into(), url: None },
                },
                SpellDef {
                    id: "corruption".into(),
                    name: "Corruption".into(),
                    school: School::Shadow,
                    cast_time_ms: 0,
                    cooldown_ms: 0,
                    resource_cost: 0.0,
                    base_min: 0.0,
                    base_max: 0.0,
                    coefficient: 0.0,
                    ap_coefficient: 0.0,
                    dot: Some(DotDef {
                        tick_interval_ms: 3000,
                        num_ticks: 6,
                        tick_base: 50.0,
                        tick_coefficient: 0.1,
                        can_crit: false,
                    }),
                    confidence: Confidence::CommunityReported,
                    source: Source { name: "test".into(), url: None },
                },
            ],
            disputed_mechanics: vec![DisputedMechanic {
                id: DEMONIC_SACRIFICE_MECHANIC_ID.into(),
                name: "Demonic Sacrifice interaction with Demonic Pact".into(),
                description: "test fixture".into(),
                candidate_rules: vec![
                    CandidateRule {
                        id: RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY.into(),
                        label: "Demonic Pact exempts a different-pet resummon only".into(),
                        source: Source { name: "wofwforever.com".into(), url: None },
                        description: "test".into(),
                        confidence: Confidence::BetaDatamine,
                    },
                    CandidateRule {
                        id: RULE_PACT_EXEMPTS_ANY_RESUMMON.into(),
                        label: "Demonic Pact exempts any resummon, including the same pet".into(),
                        source: Source { name: "andycarlson13/foreversims".into(), url: None },
                        description: "test".into(),
                        confidence: Confidence::CommunityReported,
                    },
                ],
                default_rule_id: RULE_PACT_EXEMPTS_DIFFERENT_PET_ONLY.into(),
            }],
            dispute_ledger: vec![],
            demonic_sacrifice_effects: vec![DemonicSacrificeEffect {
                pet_type: PetType::Imp,
                shadow_damage_pct: 0.15,
                fire_damage_pct: 0.0,
                mana_regen_pct_per_tick: 0.0,
                health_regen_pct_per_tick: 0.0,
                confidence: Confidence::BetaDatamine,
                source: Source { name: "test".into(), url: None },
            }],
            raid_debuffs: vec![],
        }
    }

    fn base_build() -> BuildConfig {
        BuildConfig {
            spell_power: 200.0,
            crit_chance: 0.0, // disabled for hand-computable tests
            fight_duration_ms: 9000,
            rotation: vec!["corruption".into(), "shadow_bolt".into()],
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
        }
    }

    /// Hand-computed unit test (Plan Section 0.3): a single Shadow Bolt with
    /// crit disabled and no Demonic Sacrifice buff must equal exactly
    /// base + coefficient * spell_power = 100 + 0.5*200 = 200.
    #[test]
    fn shadow_bolt_direct_damage_matches_hand_computed_formula() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 3000; // exactly one cast, no room for Corruption's priority slot to matter
        build.rotation = vec!["shadow_bolt".into()];

        let schedule = build_schedule(&data, &build);
        let mut rng = Pcg32::seed_from_u64(1);
        let total: f64 = run_schedule(&data, &schedule, &build, &mut rng, false).values().sum();

        assert_eq!(total, 200.0);
    }

    /// Without Demonic Pact, any resummon cancels the buff -- undisputed, so
    /// no dispute to report regardless of same/different pet.
    #[test]
    fn dispute_delta_is_none_without_demonic_pact() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.demonic_sacrifice_active = true;
        build.has_demonic_pact = false;
        build.resummon_event_ms = Some(3000);
        build.resummon_is_same_pet = true;

        assert!(simulate_dispute_delta(&data, &build, 100, 42).is_none());
    }

    /// With Demonic Pact, a *different*-pet resummon persists under both
    /// candidate rules -- not disputed, so no delta to report.
    #[test]
    fn dispute_delta_is_none_for_different_pet_resummon_with_pact() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.demonic_sacrifice_active = true;
        build.has_demonic_pact = true;
        build.resummon_event_ms = Some(3000);
        build.resummon_is_same_pet = false;

        assert!(simulate_dispute_delta(&data, &build, 100, 42).is_none());
    }

    /// With Demonic Pact and a *same*-pet resummon, the two candidate rules
    /// genuinely disagree: rule A (better-sourced) cancels, rule B (cruder,
    /// older framing) persists. This is the one case the dispute ledger
    /// still has open, and Principle 13 calls for the DPS delta to be
    /// computed and surfaced, not just "both representable."
    #[test]
    fn dispute_delta_favors_any_resummon_rule_for_same_pet_resummon_with_pact() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 15000;
        build.demonic_sacrifice_active = true;
        build.has_demonic_pact = true;
        build.resummon_event_ms = Some(6000);
        build.resummon_is_same_pet = true;

        let result = simulate_dispute_delta(&data, &build, 500, 7).expect("dispute should be exercised");
        assert_eq!(result.mechanic_id, DEMONIC_SACRIFICE_MECHANIC_ID);
        assert!(
            result.mean_dps_delta > 0.0,
            "the any-resummon rule should out-damage the different-pet-only rule once a same-pet resummon occurs mid-fight, got delta = {}",
            result.mean_dps_delta
        );
        assert!(result.rule_b_dps.mean > result.rule_a_dps.mean);
    }

    fn exercising_build() -> BuildConfig {
        let mut build = base_build();
        build.fight_duration_ms = 15000;
        build.demonic_sacrifice_active = true;
        build.has_demonic_pact = true;
        build.resummon_event_ms = Some(6000);
        build.resummon_is_same_pet = true;
        build
    }

    /// The UI-facing "disputed mechanic" warning must fire while the ledger
    /// says a mechanically-relevant mechanic is still open (Plan 4.7).
    #[test]
    fn touches_disputed_mechanic_is_true_while_ledger_entry_is_open() {
        let mut data = demo_data_pack();
        data.dispute_ledger.push(DisputeLedgerEntry {
            mechanic_id: DEMONIC_SACRIFICE_MECHANIC_ID.into(),
            owner: "test".into(),
            falsifiable_test: "test".into(),
            target_resolution_date: "test".into(),
            status: DisputeStatus::Open,
        });

        let result = simulate(&data, &exercising_build(), 50, 1);
        assert!(result.touches_disputed_mechanic);
    }

    /// Once the ledger marks the mechanic resolved (e.g. a primary source
    /// confirmed it), the same build must stop triggering the live warning
    /// -- Principle 13's "visibly converges toward resolved," not a
    /// permanently-sticky flag.
    #[test]
    fn touches_disputed_mechanic_is_false_once_ledger_entry_is_resolved() {
        let mut data = demo_data_pack();
        data.dispute_ledger.push(DisputeLedgerEntry {
            mechanic_id: DEMONIC_SACRIFICE_MECHANIC_ID.into(),
            owner: "test".into(),
            falsifiable_test: "test".into(),
            target_resolution_date: "test".into(),
            status: DisputeStatus::Resolved,
        });

        let result = simulate(&data, &exercising_build(), 50, 1);
        assert!(!result.touches_disputed_mechanic);

        // The comparison itself must still be available for historical reference.
        assert!(simulate_dispute_delta(&data, &exercising_build(), 50, 1).is_some());
    }

    /// Regression/golden-style test (Plan Section 0.3): the same seed must
    /// always reproduce the exact same result. A future data-pack or engine
    /// change that silently alters this is exactly the kind of regression
    /// this test class exists to catch.
    #[test]
    fn same_seed_is_fully_deterministic() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.crit_chance = 0.3;

        let out1 = simulate(&data, &build, 50, 12345);
        let out2 = simulate(&data, &build, 50, 12345);
        assert_eq!(out1.dps.mean, out2.dps.mean);
        assert_eq!(out1.dps.p50, out2.dps.p50);
    }

    /// A data pack with a single flat-damage, nonzero-cost, no-dot,
    /// no-cooldown spell -- isolates mana-constraint behavior from DoT
    /// scheduling and crit variance, so cast counts are hand-computable.
    fn mana_test_data_pack() -> DataPack {
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![SpellDef {
                id: "costly_bolt".into(),
                name: "Costly Bolt".into(),
                school: School::Shadow,
                cast_time_ms: 1500, // == GCD_MS
                cooldown_ms: 0,
                resource_cost: 100.0,
                base_min: 50.0,
                base_max: 50.0,
                coefficient: 0.0,
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

    fn mana_test_build(starting_mana: Option<f64>, mana_regen_per_second: f64, fight_duration_ms: u32) -> BuildConfig {
        BuildConfig {
            spell_power: 0.0,
            crit_chance: 0.0, // disabled: hand-computable totals
            fight_duration_ms,
            rotation: vec!["costly_bolt".into()],
            pet_type: PetType::Imp,
            demonic_sacrifice_active: false,
            has_demonic_pact: false,
            resummon_event_ms: None,
            resummon_is_same_pet: false,
            dispute_rule_override: None,
            starting_mana,
            mana_regen_per_second,
            attack_power: 0.0,
            weapon_min: 0.0,
            weapon_max: 0.0,
            weapon_speed_ms: 0,
        }
    }

    /// `starting_mana: None` must behave exactly as this engine always
    /// did before mana modeling existed: a rotation with no always-ready
    /// filler panics rather than silently producing a wrong answer.
    /// `costly_bolt` has no dot/cooldown gating it, so this specifically
    /// isolates that the *old* unconstrained path still panics only when
    /// there's genuinely no eligible spell for dot/cooldown reasons, not
    /// mana -- here it never even reaches that state because mana is
    /// infinite, so this build must run forever-affordable and NOT panic.
    #[test]
    fn unconstrained_mana_never_blocks_an_otherwise_always_ready_spell() {
        let data = mana_test_data_pack();
        let build = mana_test_build(None, 0.0, 9000); // 6 GCDs
        let result = simulate(&data, &build, 10, 1);
        let expected_dps = (6.0 * 50.0) / 9.0; // 6 casts, 50 dmg each, over 9s
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }

    /// Core mana-constraint correctness, zero regen: starting mana 250,
    /// cost 100/cast affords exactly 2 casts (200 spent), then the 3rd
    /// cast is permanently unaffordable (50 left, needs 100, never
    /// regens) for the rest of a 9000ms fight. Must NOT panic (the old
    /// "rotation exhausted" panic would have fired here pre-mana-modeling
    /// logic if mana starvation weren't distinguished from a genuine
    /// no-eligible-spell rotation bug), and must report exactly 2 casts'
    /// worth of damage, not fewer (premature stop) or more (constraint
    /// silently ignored).
    #[test]
    fn mana_starved_build_waits_out_the_fight_instead_of_panicking() {
        let data = mana_test_data_pack();
        let build = mana_test_build(Some(250.0), 0.0, 9000);
        let result = simulate(&data, &build, 10, 1);
        let expected_dps = (2.0 * 50.0) / 9.0; // only 2 casts ever affordable
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }

    /// Regen correctness: starting mana 250, cost 100/cast, zero regen
    /// affords 2 casts as above; with a nonzero regen rate, a 3rd cast
    /// becomes affordable again partway through the same fight (hand
    /// trace: cast@0 mana 250->150, cast@1500 mana 150->50, unaffordable
    /// at t=3000, wait to t=4500 regenerating 100*1.5=150 -> mana
    /// 50+150=200, affordable again, cast@4500 mana 200->100; fight ends
    /// at t=6000). This is the regression test for the wait-then-retry
    /// branch actually re-enabling casting, not just avoiding a panic.
    #[test]
    fn mana_regen_enables_an_additional_cast_later_in_the_fight() {
        let data = mana_test_data_pack();
        let build = mana_test_build(Some(250.0), 100.0, 6000);
        let result = simulate(&data, &build, 10, 1);
        let expected_dps = (3.0 * 50.0) / 6.0; // 3 casts once regen kicks in
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }

    /// Regen must cap at the starting pool, not accumulate past it --
    /// `starting_mana` doubles as max mana (see `BuildConfig::starting_mana`
    /// doc comment). With starting mana 100 == cost, and a large regen
    /// rate (200/s) applied over repeated 1500ms waits, each wait
    /// physically computes 0 + 200*1.5 = 300, which *must* be clamped
    /// back down to 100 before the next affordability check -- if the cap
    /// were missing or broken, excess mana would bank across waits and
    /// eventually permit back-to-back casts with no wait between them,
    /// which this hand-traced 6000ms fight would show as 3 casts instead
    /// of the correctly-capped 2 (see the test body for the full trace).
    #[test]
    fn mana_regen_is_capped_at_the_starting_pool_not_banked_unbounded() {
        let data = mana_test_data_pack();
        // Capped trace: cast@0 (100->0); t=1500 unaffordable, wait->t=3000,
        // mana = min(0 + 200*1.5, 100) = 100; cast@3000 (100->0); t=4500
        // unaffordable, wait->t=6000, mana = min(0+300, 100) = 100; loop
        // ends (time 6000 not < 6000). Total: 2 casts.
        // Uncapped (what a regression would produce): cast@0 (100->0);
        // wait->mana=300 (no cap); cast@3000 (300->200, affordable
        // immediately, no further wait needed); cast@4500 (200->100).
        // Total: 3 casts. The two predictions are decisively different.
        let build = mana_test_build(Some(100.0), 200.0, 6000);
        let result = simulate(&data, &build, 10, 1);
        let expected_dps_if_capped = (2.0 * 50.0) / 6.0;
        assert!(
            (result.dps.mean - expected_dps_if_capped).abs() < 1e-6,
            "expected the capped prediction {expected_dps_if_capped} (2 casts), got {} -- \
             if this matches 3 casts (25.0) instead, the starting_mana cap on regen has regressed",
            result.dps.mean
        );
    }

    /// A minimal build with no spells at all (empty rotation) and an
    /// optional weapon -- isolates auto-attack behavior (Hunter/Rogue/
    /// Warrior extension) from everything spell-power-related.
    fn melee_build(attack_power: f64, weapon_min: f64, weapon_max: f64, weapon_speed_ms: u32, fight_duration_ms: u32) -> BuildConfig {
        BuildConfig {
            spell_power: 0.0,
            crit_chance: 0.0, // disabled: hand-computable totals
            fight_duration_ms,
            rotation: vec![],
            pet_type: PetType::Imp,
            demonic_sacrifice_active: false,
            has_demonic_pact: false,
            resummon_event_ms: None,
            resummon_is_same_pet: false,
            dispute_rule_override: None,
            starting_mana: None,
            mana_regen_per_second: 0.0,
            attack_power,
            weapon_min,
            weapon_max,
            weapon_speed_ms,
        }
    }

    /// An empty rotation used to be unreachable without a panic (every
    /// pre-existing build always had at least one spell). Hunter/Rogue/
    /// Warrior's current scope (auto-attack only, no sourced special
    /// abilities yet) legitimately has no spells at all, and with no
    /// weapon equipped either this must produce exactly zero damage, not
    /// a panic -- the empty-rotation skip and the `weapon_speed_ms: 0`
    /// no-op path are two separate conditions that both need to hold.
    #[test]
    fn empty_rotation_with_no_weapon_produces_zero_dps_not_a_panic() {
        let data = demo_data_pack();
        let build = melee_build(0.0, 0.0, 0.0, 0, 10_000);
        let result = simulate(&data, &build, 10, 1);
        assert_eq!(result.dps.mean, 0.0);
    }

    /// Core auto-attack correctness, hand-traced: weapon_min == weapon_max
    /// == 50 (no roll variance), attack_power = 1000, weapon_speed_ms =
    /// 2800 -> per-swing attack-power bonus = (2.8s / 14) * 1000 = 200,
    /// per-swing damage = 50 + 200 = 250. A 56000ms fight fits exactly 20
    /// swings (0, 2800, ..., 53200; the next at 56000 is not < 56000), so
    /// total damage = 20 * 250 = 5000 over 56s.
    #[test]
    fn auto_attack_dps_matches_hand_computed_classic_ap_formula() {
        let data = demo_data_pack();
        let build = melee_build(1000.0, 50.0, 50.0, 2800, 56_000);
        let result = simulate(&data, &build, 10, 1);
        let expected_dps = 5000.0 / 56.0;
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }

    /// Attack power must have zero effect on a spell-power build and vice
    /// versa -- the two scaling stats are additive and independent
    /// (`ap_coefficient` is 0.0 on every spell-power event and
    /// `coefficient` is 0.0 on every auto-attack event), not a shared or
    /// confused stat. Gives a caster build a large attack_power value
    /// with no weapon equipped (so it can't matter) and confirms the DPS
    /// is identical to the same build with attack_power at 0.
    #[test]
    fn attack_power_does_not_affect_a_pure_spell_power_build() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 3000;
        build.rotation = vec!["shadow_bolt".into()];

        let mut with_ap = build.clone();
        with_ap.attack_power = 99_999.0; // large, but weapon_speed_ms stays 0 -- must be inert

        let result_without = simulate(&data, &build, 10, 1);
        let result_with = simulate(&data, &with_ap, 10, 1);
        assert_eq!(result_without.dps.mean, result_with.dps.mean);
    }

    /// A direct-only spell (Shadow Bolt, no dot) with crit disabled should
    /// produce exactly one `Direct`-kind ability line whose `casts` and
    /// `total_damage` are hand-computable, and every event in the
    /// chronological list should carry the exact same damage value (no
    /// variance -- base_min == base_max here).
    #[test]
    fn simulate_detail_groups_a_direct_only_spell_into_one_ability_line() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 9000; // 3 casts at 3000ms cast time
        build.rotation = vec!["shadow_bolt".into()];

        let detail = simulate_detail(&data, &build, 1);
        assert_eq!(detail.abilities.len(), 1);
        let ab = &detail.abilities[0];
        assert_eq!(ab.source_id, "shadow_bolt");
        assert_eq!(ab.kind, EventKind::Direct);
        assert_eq!(ab.casts, 3);
        let per_cast = 100.0 + 0.5 * 200.0; // base + coefficient * spell_power
        assert!((ab.total_damage - per_cast * 3.0).abs() < 1e-9);
        assert_eq!(ab.crit_eligible, 3);
        assert_eq!(ab.crits, 0); // crit_chance is 0.0 in base_build()

        assert_eq!(detail.events.len(), 3);
        for ev in &detail.events {
            assert!((ev.damage - per_cast).abs() < 1e-9);
            assert!(!ev.is_crit);
        }
    }

    /// A DoT (Corruption) must report as a `Tick`-kind ability line,
    /// separate from any direct-hit ability sharing the rotation --
    /// Corruption needs a filler here (same documented "rotation
    /// exhausted" constraint as every other DoT-only-rotation test in
    /// this project), so Shadow Bolt fills the gaps between its 3000ms
    /// ticks; this test asserts on Corruption's own line specifically,
    /// not on there being exactly one ability. Also confirms events come
    /// back chronologically sorted even though a DoT's ticks are pushed
    /// as a single contiguous block by `build_schedule`, interleaved
    /// with the filler's casts only once sorted for display.
    #[test]
    fn simulate_detail_reports_a_dot_spell_as_tick_kind_not_direct() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 18_001; // just past Corruption's 6th tick at 18000ms
        build.rotation = vec!["corruption".into(), "shadow_bolt".into()];

        let detail = simulate_detail(&data, &build, 1);
        let corruption = detail.abilities.iter().find(|a| a.source_id == "corruption").expect("corruption must appear");
        assert_eq!(corruption.kind, EventKind::Tick);
        assert_eq!(corruption.casts, 6);
        assert_eq!(corruption.crit_eligible, 0); // Corruption's dot has can_crit: false

        for i in 1..detail.events.len() {
            assert!(detail.events[i].time_ms >= detail.events[i - 1].time_ms, "events must be chronologically sorted");
        }
    }

    /// With `crit_chance: 1.0`, every crit-eligible event in a single
    /// representative run must actually crit -- a direct, deterministic
    /// check that `is_crit` and the per-ability `crits` counter reflect
    /// real RNG outcomes, not a hardcoded flag.
    #[test]
    fn simulate_detail_marks_every_roll_as_crit_when_crit_chance_is_certain() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.crit_chance = 1.0;
        build.fight_duration_ms = 9000;
        build.rotation = vec!["shadow_bolt".into()];

        let detail = simulate_detail(&data, &build, 1);
        assert_eq!(detail.abilities[0].crits, detail.abilities[0].crit_eligible);
        assert!(detail.events.iter().all(|e| e.is_crit));
    }

    /// Demonic Sacrifice's buff window must reflect the same
    /// persists-or-cancels resolution `simulate`/`simulate_dispute_delta`
    /// already use, not a separately-reimplemented (and potentially
    /// inconsistent) rule. `exercising_build()`'s default candidate rule
    /// (different-pet-only exemption) means a *same*-pet resummon cancels
    /// the buff, so the window must end exactly at `resummon_event_ms`
    /// (6000), not run to the full fight duration (15000).
    #[test]
    fn simulate_detail_buff_window_ends_at_resummon_when_the_buff_does_not_persist() {
        let data = demo_data_pack();
        let build = exercising_build();

        let detail = simulate_detail(&data, &build, 1);
        assert_eq!(detail.buffs.len(), 1);
        assert_eq!(detail.buffs[0].name, "Demonic Sacrifice");
        assert_eq!(detail.buffs[0].start_ms, 0);
        assert_eq!(detail.buffs[0].end_ms, 6000);
    }

    /// A build with no Demonic Sacrifice (every non-Warlock build, and
    /// most Warlock builds) must report zero buff windows -- an honest
    /// empty list, not a fabricated entry.
    #[test]
    fn simulate_detail_reports_no_buffs_when_none_are_active() {
        let data = demo_data_pack();
        let mut build = base_build();
        build.fight_duration_ms = 9000;
        build.rotation = vec!["shadow_bolt".into()];
        assert!(!build.demonic_sacrifice_active);

        let detail = simulate_detail(&data, &build, 1);
        assert!(detail.buffs.is_empty());
    }

    /// Auto-attack swings must report under the literal `"auto_attack"`
    /// source id with `EventKind::AutoAttack`, matching the label this
    /// project's frontend uses for Hunter/Rogue/Warrior's demo builds.
    #[test]
    fn simulate_detail_labels_auto_attack_swings_correctly() {
        let data = demo_data_pack();
        let build = melee_build(0.0, 50.0, 50.0, 2800, 5600); // exactly 2 swings

        let detail = simulate_detail(&data, &build, 1);
        assert_eq!(detail.abilities.len(), 1);
        assert_eq!(detail.abilities[0].source_id, "auto_attack");
        assert_eq!(detail.abilities[0].kind, EventKind::AutoAttack);
        assert_eq!(detail.abilities[0].casts, 2);
    }

    /// A data pack with one real-shaped attack-power-scaling special
    /// ability (Bloodthirst's actual sourced shape: 45% Attack Power +
    /// 48 flat, instant, 6s cooldown) -- isolates `ap_coefficient`
    /// scaling and the cooldown-only-rotation wait behavior from
    /// anything spell-power-related.
    fn ap_scaling_data_pack() -> DataPack {
        DataPack {
            version: "test-fixture".into(),
            game_build_reference: "test".into(),
            spells: vec![SpellDef {
                id: "bloodthirst_like".into(),
                name: "Bloodthirst-like".into(),
                school: School::Physical,
                cast_time_ms: 0,
                cooldown_ms: 6000,
                resource_cost: 30.0,
                base_min: 48.0,
                base_max: 48.0,
                coefficient: 0.0,
                ap_coefficient: 0.45,
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

    /// Hand-traced: cast@0 (cooldown gates until 6000), fight ends at
    /// 3000ms before the cooldown ever clears again, so exactly one cast
    /// happens. Damage = 48 + 0.45*1000 = 498 over 3s = 166.0 DPS.
    #[test]
    fn ap_coefficient_scales_a_direct_spell_with_attack_power() {
        let data = ap_scaling_data_pack();
        let mut build = melee_build(1000.0, 0.0, 0.0, 0, 3000); // no weapon equipped -- isolates the special ability
        build.rotation = vec!["bloodthirst_like".into()];

        let result = simulate(&data, &build, 10, 1);
        let expected_dps = (48.0 + 0.45 * 1000.0) / 3.0;
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }

    /// The real-world case this test exists for: a rotation that is
    /// *entirely* cooldown-gated abilities (no spell with neither a
    /// cooldown nor a DoT) used to hit the "rotation exhausted" panic,
    /// even though a real character just keeps auto-attacking while
    /// waiting -- not a rotation-design bug. Hand-traced: casts land
    /// exactly on the 6000ms cooldown boundary each time (0, 6000, 12000
    /// -- the GCD-stepped wait always lands exactly on it here since
    /// 6000 is a multiple of the 1500ms GCD), giving 3 casts over an
    /// 18000ms fight: 3 * 498 = 1494 total, 1494/18 = 83.0 DPS. Must not
    /// panic.
    #[test]
    fn cooldown_only_rotation_waits_instead_of_panicking() {
        let data = ap_scaling_data_pack();
        let mut build = melee_build(1000.0, 0.0, 0.0, 0, 18_000);
        build.rotation = vec!["bloodthirst_like".into()];

        let result = simulate(&data, &build, 10, 1);
        let expected_dps = (3.0 * 498.0) / 18.0;
        assert!((result.dps.mean - expected_dps).abs() < 1e-6, "expected {expected_dps}, got {}", result.dps.mean);
    }
}
