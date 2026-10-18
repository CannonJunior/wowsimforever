//! Phase 1 bring-up CLI: loads a data pack and runs either the steady-state
//! DPS simulator (default) or the burst damage rotation calculator
//! (`--mode burst`) against a demo Warlock build.
//!
//! Placeholder-data policy (CLAUDE.md): the default data pack must contain
//! only sourced data. Placeholder entries can only enter the simulation via
//! an explicit `--overrides <path>` argument -- there is no default
//! overrides path and no environment-variable fallback.
//!
//! Historical storage (Plan Section 4.3): pass `--history <path>` to persist
//! this run and surface a regression check against the most recent prior
//! run of the *same* build under a *different* data-pack version. Pass
//! `--show-history <path>` to print a trend view of everything recorded
//! there without running a new simulation. (DPS mode only -- burst results
//! aren't persisted yet.)

use std::env;
use std::fs;
use wowforever_burst::{evaluate_sequence, find_best_burst_sequence, AttackerOpener, BurstBuildConfig, OpponentProfile};
use wowforever_data::DataPack;
use wowforever_engine::{simulate, simulate_dispute_delta, BuildConfig, PetType};
use wowforever_raid::{simulate_raid_with_debuffs, RaidMember};
use wowforever_recommend::{diff_data_packs, find_affected_builds};
use wowforever_storage::{build_comparisons, ExternalResult, ExternalResultStore, RunStore};

const DEFAULT_DATA_PACK: &str = "data-packs/forever-beta-2026-10-17-rev9.json";

#[derive(PartialEq)]
enum Mode {
    Dps,
    Burst,
    Recommend,
    AddExternal,
    CompareExternal,
    Raid,
    Heal,
}

struct Args {
    mode: Mode,
    data_pack: String,
    overrides: Option<String>,
    history: Option<String>,
    show_history: Option<String>,
    from: Option<String>,
    to: Option<String>,
    resimulate: bool,
    external: Option<String>,
    build_fingerprint: Option<String>,
    source: Option<String>,
    source_version: Option<String>,
    dps: Option<f64>,
    notes: Option<String>,
    class: Option<String>,
    gear: Option<String>,
}

fn parse_args() -> Args {
    let mut mode = Mode::Dps;
    let mut data_pack = DEFAULT_DATA_PACK.to_string();
    let mut overrides = None;
    let mut history = None;
    let mut show_history = None;
    let mut from = None;
    let mut to = None;
    let mut resimulate = false;
    let mut external = None;
    let mut build_fingerprint = None;
    let mut source = None;
    let mut source_version = None;
    let mut dps = None;
    let mut notes = None;
    let mut class = None;
    let mut gear = None;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--mode" => {
                mode = match args.next().expect("--mode requires an argument (dps|burst|recommend|add-external|compare-external|raid|heal)").as_str() {
                    "dps" => Mode::Dps,
                    "burst" => Mode::Burst,
                    "recommend" => Mode::Recommend,
                    "add-external" => Mode::AddExternal,
                    "compare-external" => Mode::CompareExternal,
                    "raid" => Mode::Raid,
                    "heal" => Mode::Heal,
                    other => panic!("unknown --mode `{other}` (expected dps, burst, recommend, add-external, compare-external, raid, or heal)"),
                };
            }
            "--data-pack" => data_pack = args.next().expect("--data-pack requires a path argument"),
            "--overrides" => overrides = Some(args.next().expect("--overrides requires a path argument")),
            "--history" => history = Some(args.next().expect("--history requires a path argument")),
            "--show-history" => show_history = Some(args.next().expect("--show-history requires a path argument")),
            "--from" => from = Some(args.next().expect("--from requires a path argument")),
            "--to" => to = Some(args.next().expect("--to requires a path argument")),
            "--resimulate" => resimulate = true,
            "--external" => external = Some(args.next().expect("--external requires a path argument")),
            "--build" => build_fingerprint = Some(args.next().expect("--build requires a build-fingerprint argument")),
            "--source" => source = Some(args.next().expect("--source requires a value, e.g. wowforeversim.com")),
            "--source-version" => source_version = Some(args.next().expect("--source-version requires a value")),
            "--dps" => dps = Some(args.next().expect("--dps requires a numeric value").parse::<f64>().expect("--dps must be a number")),
            "--notes" => notes = Some(args.next().expect("--notes requires a value")),
            "--class" => class = Some(args.next().expect("--class requires a value, e.g. warlock or mage")),
            "--gear" => gear = Some(args.next().expect("--gear requires a path argument, e.g. data-packs/gear.example.json")),
            other => panic!(
                "unknown argument `{other}` (expected --mode, --data-pack, --overrides, --history, --show-history, --from, --to, --resimulate, --external, --build, --source, --source-version, --dps, --notes, --class, and/or --gear)"
            ),
        }
    }
    Args { mode, data_pack, overrides, history, show_history, from, to, resimulate, external, build_fingerprint, source, source_version, dps, notes, class, gear }
}

fn load_data_pack(path: &str) -> DataPack {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read data pack `{path}`: {e}"));
    DataPack::from_json_str(&raw).unwrap_or_else(|e| panic!("failed to parse data pack `{path}`: {e}"))
}

fn print_history(path: &str) {
    let store = RunStore::open(path);
    let runs = store.load_all().unwrap_or_else(|e| panic!("failed to read history `{path}`: {e}"));
    if runs.is_empty() {
        println!("No history recorded yet at {path}.");
        return;
    }
    println!("History ({} run(s)) from {path}:", runs.len());
    for run in &runs {
        let dispute_note = run
            .dispute_delta
            .as_ref()
            .map(|d| format!(" [dispute swing {:+.2}%]", d.pct_swing))
            .unwrap_or_default();
        println!(
            // Full fingerprint, not truncated -- this is what --mode add-external's
            // --build argument needs verbatim; a truncated display here would be a
            // copy-paste trap (silently matching nothing, or the wrong build).
            "  [{}] data pack {} -- build {} -- DPS mean={:.1} p50={:.1}{}",
            run.recorded_at_unix, run.data_pack_version, run.build_fingerprint, run.dps.mean, run.dps.p50, dispute_note
        );
    }
}

fn load_data_pack_with_overrides(args: &Args) -> DataPack {
    let mut data = load_data_pack(&args.data_pack);
    println!("Loaded production data pack: {} ({})", data.version, args.data_pack);

    if let Some(overrides_path) = &args.overrides {
        let overrides = load_data_pack(overrides_path);
        let placeholder_ids = data.apply_overrides(overrides);
        println!("Loaded overrides from explicit --overrides {overrides_path}");
        for id in &placeholder_ids {
            println!("  [!] PLACEHOLDER override active for spell `{id}` -- not a real game value. Never run this without --overrides in a real result.");
        }
    }

    println!("Game build reference: {}\n", data.game_build_reference);
    for spell in &data.spells {
        if matches!(spell.confidence, wowforever_data::Confidence::Placeholder) {
            println!(
                "  [!] {} uses PLACEHOLDER data -- only reachable because --overrides was passed explicitly ({})",
                spell.name, spell.source.name
            );
        }
    }
    println!();
    data
}

/// The flagship demo build for the mana-constraint feature (Phase 1
/// simplifications pass, see `wowforever_engine::BuildConfig::starting_mana`
/// doc comment): 5000 starting mana and a 15 mana/s flat regen are
/// illustrative demo inputs, not sourced game facts -- exactly like
/// `spell_power: 350.0` above has always been a build-configuration input,
/// never a `Confidence`-tagged value. At this rotation's real cost (340 +
/// 380 + 370 mana per cast of Corruption/Immolate/Shadow Bolt), this
/// build genuinely goes OOM partway through a 60s fight and has to wait
/// out real regen -- a deliberate choice so the feature is visibly
/// exercised by the default demo, not just reachable in theory.
fn warlock_demo_build() -> BuildConfig {
    BuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["corruption".into(), "immolate".into(), "shadow_bolt".into()],
        pet_type: PetType::Imp,
        demonic_sacrifice_active: true,
        has_demonic_pact: true,
        resummon_event_ms: Some(30_000), // pet dies and is resummoned halfway through the fight
        resummon_is_same_pet: true,      // resummoning another Imp -- the one case the dispute used to be open on
        dispute_rule_override: None,
        starting_mana: Some(5000.0),
        mana_regen_per_second: 15.0,
        attack_power: 0.0,
        weapon_min: 0.0,
        weapon_max: 0.0,
        weapon_speed_ms: 0,
    }
}

/// Mage demo build (Plan Section 7's "remaining classes/specs," proving the
/// engine generalizes beyond Warlock -- see docs/PLAN.md's REV4 update).
/// Scope note: Fireball's minor periodic burn means this engine's
/// maintenance-style rotation logic (re-cast only once a spell's own DoT
/// expires -- the same mechanism Corruption/Immolate already use) treats
/// Fireball like a maintained DoT with a direct component, recasting it
/// only every ~8s rather than every cast as real play would. This is an
/// honest consequence of Phase 1's simplified, non-APL rotation model
/// (Plan Section 2.2/3), not a new limitation introduced for Mage -- it's
/// the same simplification Immolate already has, just more visible here.
/// Demonic Sacrifice fields are irrelevant to Mage and left at their
/// inert defaults (no pet, no Warlock mechanic) rather than omitted, since
/// `BuildConfig` is shared across classes in this Phase 1 engine.
fn mage_demo_build() -> BuildConfig {
    BuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["fireball".into(), "frostbolt".into()],
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

/// Shaman demo build (REV7's class-sourcing pass). Lightning Bolt has
/// neither a DoT nor a cooldown, so unlike Moonfire it's safely spammable
/// as a single-spell rotation with no filler needed -- no "rotation
/// exhausted" risk the way an unfilled DoT-only rotation would have.
fn shaman_demo_build() -> BuildConfig {
    BuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["lightning_bolt".into()],
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

/// Druid demo build (REV8). Moonfire alone hit the "rotation exhausted"
/// constraint (its DoT gates the only rotation entry with nothing to fall
/// back to) -- fixed properly by sourcing Wrath, a real filler with
/// neither a DoT nor a cooldown, the same maintenance-DoT-plus-filler
/// pattern every other class's demo build already uses.
fn druid_demo_build() -> BuildConfig {
    BuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["moonfire".into(), "wrath".into()],
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

/// Hunter/Rogue/Warrior demo builds (the attack-power/weapon-damage engine
/// extension, Plan Section 7's seventeenth update revisited): these three
/// classes deal zero spell-power-scaling damage in Classic-era
/// itemization, so `rotation` is empty -- no special abilities have been
/// sourced with a real attack-power coefficient yet, honestly. What *is*
/// real: auto-attack, scheduled on its own independent weapon-speed
/// timer, using the classic `attack_power / 14 * swing_time` formula. The
/// attack_power/weapon_min/weapon_max/weapon_speed_ms values below are
/// hand-entered build-configuration inputs, exactly like `spell_power:
/// 350.0` already is for every caster demo build above -- illustrative,
/// not sourced game facts, and not subject to the no-placeholder-data
/// rule (that rule is about fabricated *game* data, not demo build stats).
// Recalibrated 2026-10-18: the original numbers (AP 400-500) produced an
// unrealistically *low* illustrative baseline (DPS in the low 40s-60s) --
// technically correct for those inputs, but a raid-buffed Classic-era
// level 60 character routinely reaches ~1000-2000+ attack power
// (web-research cross-check: AQ40-geared warriors averaged ~499 *total*
// DPS including abilities; ~2000 AP buffed is cited as a realistic
// well-geared breakpoint). Auto-attack-only DPS for these three classes
// should still read as honestly lower than the caster demo builds (real
// melee/ranged output leans heavily on special abilities this engine
// hasn't modeled yet), just not implausibly so.
fn hunter_demo_build() -> BuildConfig {
    melee_demo_build(vec![], 850.0, 50.0, 90.0, 2800)
}
fn rogue_demo_build() -> BuildConfig {
    melee_demo_build(vec![], 900.0, 35.0, 65.0, 1700)
}
fn warrior_demo_build() -> BuildConfig {
    // REV9: Bloodthirst on top of auto-attack -- this project's first
    // sourced attack-power-scaling special ability. Still a partial
    // rotation (real Warriors use more than one rage-spending ability),
    // stated honestly in the web UI's warning banner for this class.
    melee_demo_build(vec!["bloodthirst".into()], 1000.0, 80.0, 140.0, 3300)
}

fn melee_demo_build(rotation: Vec<String>, attack_power: f64, weapon_min: f64, weapon_max: f64, weapon_speed_ms: u32) -> BuildConfig {
    BuildConfig {
        spell_power: 0.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation,
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

/// Priest healer demo build (Plan Section 8's healer-output half -- see
/// crates/heal's module docs for exactly what is and isn't modeled).
/// Renew (HoT) maintained, Flash Heal as filler -- the same
/// maintenance-spell-plus-filler pattern the Warlock/Mage demo builds use.
fn priest_demo_build() -> wowforever_heal::HealBuildConfig {
    wowforever_heal::HealBuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["renew".into(), "flash_heal".into()],
    }
}

/// Paladin healer demo build (REV7). Holy Light has neither a HoT nor a
/// cooldown, same safely-spammable shape as Lightning Bolt above.
fn paladin_demo_build() -> wowforever_heal::HealBuildConfig {
    wowforever_heal::HealBuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        fight_duration_ms: 60_000,
        rotation: vec!["holy_light".into()],
    }
}

fn run_heal_mode(args: &Args) {
    let data = load_data_pack_with_overrides(args);
    let build = match args.class.as_deref() {
        None | Some("priest") => priest_demo_build(),
        Some("paladin") => paladin_demo_build(),
        Some(other) => panic!("unknown --class `{other}` for --mode heal (expected priest or paladin)"),
    };
    let result = wowforever_heal::simulate_hps(&data, &build, 10_000, 1);
    println!(
        "HPS (data pack {}): mean={:.1} stdev={:.1} p5={:.1} p50={:.1} p95={:.1}",
        result.data_pack_version, result.hps.mean, result.hps.stdev, result.hps.p5, result.hps.p50, result.hps.p95
    );

    // ILLUSTRATIVE ONLY -- see wowforever_heal's module docs and
    // SyntheticIncomingDamageProfile's doc comment. There is no real WoW
    // Forever encounter-damage data to source this from (the game hasn't
    // launched), so this specific number is a stated assumption for demo
    // purposes, not a researched game value: "a 5-person group splits a
    // boss's 1500 raid-wide DPS evenly across 5 healing assignments" is
    // this crate's author's own made-up scenario, not Blizzard's.
    let profile = wowforever_heal::SyntheticIncomingDamageProfile {
        name: "ILLUSTRATIVE, NOT SOURCED: a 5-person group's boss deals 1500 raid-wide DPS, split evenly across 5 healing assignments (300/s each)".into(),
        raid_damage_per_second: 300.0,
    };
    let sufficiency = wowforever_heal::evaluate_healing_sufficiency(&data, &build, &profile, 10_000, 1);
    println!("\nHealing sufficiency vs. a SYNTHETIC incoming-damage profile ({}):", sufficiency.incoming_profile_name);
    println!(
        "  Assumed incoming damage: {:.1}/s -- mean deficit: {:+.1}/s ({})",
        sufficiency.raid_damage_per_second,
        sufficiency.mean_deficit_per_second,
        if sufficiency.mean_deficit_per_second < 0.0 { "surplus on average" } else { "shortfall on average" }
    );
    println!("  Sufficient in {:.1}% of simulated iterations.", sufficiency.pct_iterations_sufficient);
    println!("  [!] The incoming-damage number above is a stated assumption for this demo, not WoW Forever encounter data -- no such data exists for an unlaunched game. Treat this mode's sufficiency verdict as only as good as whatever profile you supply.");
}

fn run_dps_mode(args: &Args) {
    let data = load_data_pack_with_overrides(args);

    let mut build = match args.class.as_deref() {
        None | Some("warlock") => warlock_demo_build(),
        Some("mage") => mage_demo_build(),
        Some("shaman") => shaman_demo_build(),
        Some("druid") => druid_demo_build(),
        Some("hunter") => hunter_demo_build(),
        Some("rogue") => rogue_demo_build(),
        Some("warrior") => warrior_demo_build(),
        Some(other) => panic!("unknown --class `{other}` (expected warlock, mage, shaman, druid, hunter, rogue, or warrior)"),
    };

    // Gear/itemization mechanism (Plan Section 7's Phase 1 simplification,
    // see wowforever_gear's module docs): --gear replaces the demo build's
    // hand-typed spell_power/crit_chance with stats computed from an
    // equipped item list. The demo build's own crit_chance is treated as
    // the "innate, pre-gear" base (Classic itemization doesn't zero crit
    // out just because no gear grants any).
    if let Some(gear_path) = &args.gear {
        let raw = fs::read_to_string(gear_path).unwrap_or_else(|e| panic!("failed to read gear file `{gear_path}`: {e}"));
        let gear: wowforever_gear::Gear =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse gear file `{gear_path}`: {e}"));
        let placeholder_ids = gear.placeholder_item_ids();
        let stats = gear.stats_with_base_crit(build.crit_chance);
        println!(
            "Applied gear from {gear_path}: {} item(s), total spell_power={:.1}, effective crit_chance={:.1}% (base {:.1}% + gear {:.1}%)",
            gear.items.len(), stats.spell_power, stats.crit_chance * 100.0, build.crit_chance * 100.0, gear.total_crit_chance_pct()
        );
        for id in &placeholder_ids {
            println!("  [!] PLACEHOLDER item `{id}` -- not a real game value. Never use --gear with placeholder items for a real result.");
        }
        build.spell_power = stats.spell_power;
        build.crit_chance = stats.crit_chance;
    }

    let result = simulate(&data, &build, 10_000, 1);
    println!(
        "DPS (data pack {}): mean={:.1} stdev={:.1} p5={:.1} p50={:.1} p95={:.1}",
        result.data_pack_version, result.dps.mean, result.dps.stdev, result.dps.p5, result.dps.p50, result.dps.p95
    );
    if result.touches_disputed_mechanic {
        println!("  [DISPUTED MECHANIC] This result depends on an unresolved community dispute -- see the delta below.");
    }

    let dispute = simulate_dispute_delta(&data, &build, 10_000, 1);
    if let Some(delta) = &dispute {
        let is_open = matches!(
            data.dispute_ledger.iter().find(|e| e.mechanic_id == delta.mechanic_id).map(|e| &e.status),
            Some(wowforever_data::DisputeStatus::Open)
        );
        let swing_label = if is_open { "swing on an unresolved rule" } else { "historical swing -- now resolved, kept for reference" };
        println!("\nMechanic comparison: {}", delta.mechanic_id);
        println!("  Rule A -- {}: mean DPS = {:.1}", delta.rule_a_label, delta.rule_a_dps.mean);
        println!("  Rule B -- {}: mean DPS = {:.1}", delta.rule_b_label, delta.rule_b_dps.mean);
        println!("  Delta: {:+.1} DPS ({:+.2}% {swing_label})", delta.mean_dps_delta, delta.pct_swing);
    }

    println!("\nDispute ledger:");
    for entry in &data.dispute_ledger {
        println!(
            "  [{:?}] {} -- owner: {} -- target: {}",
            entry.status, entry.mechanic_id, entry.owner, entry.target_resolution_date
        );
    }

    if let Some(history_path) = &args.history {
        let store = RunStore::open(history_path);
        let prior = store
            .append(&data.version, &build, &result.dps, result.touches_disputed_mechanic, dispute.as_ref())
            .unwrap_or_else(|e| panic!("failed to append to history `{history_path}`: {e}"));
        println!("\nRecorded this run to {history_path}.");
        match prior {
            Some(p) => {
                let delta = result.dps.mean - p.dps.mean;
                let pct = delta / p.dps.mean * 100.0;
                println!(
                    "  [REGRESSION CHECK] Same build previously ran under data pack {} at mean DPS {:.1} -- this run: {:.1} ({:+.2}%).",
                    p.data_pack_version, p.dps.mean, result.dps.mean, pct
                );
            }
            None => println!("  [REGRESSION CHECK] No prior run of this exact build under a different data-pack version yet."),
        }
    }
}

fn run_burst_mode(args: &Args) {
    let data = load_data_pack_with_overrides(args);

    let build = BurstBuildConfig {
        spell_power: 350.0,
        crit_chance: 0.20,
        window_ms: 15_000, // a short PvP burst window, per Plan Section 4.6
        available_spell_ids: vec!["shadowburn".into(), "immolate".into(), "corruption".into(), "shadow_bolt".into()],
        pet_type: PetType::Imp,
        demonic_sacrifice_active: true,
        opponent: OpponentProfile {
            name: "Generic PvP target, no resistance gear".into(),
            shadow_resist_pct: 0.0,
            fire_resist_pct: 0.0,
            physical_resist_pct: 0.0,
            frost_resist_pct: 0.0,
            arcane_resist_pct: 0.0,
                holy_resist_pct: 0.0,
            nature_resist_pct: 0.0,
        },
        opener: AttackerOpener { starting_mana: 2500.0, opened_from_stealth_or_cc: false },
        search_beam_width: 500,
    };

    println!("Burst window: {} ms against {}", build.window_ms, build.opponent.name);
    let search = find_best_burst_sequence(&data, &build);
    let eval = evaluate_sequence(&data, &build, &search.sequence, 10_000, 1);

    println!("\nExact expected damage (what the search optimized): {:.1}", search.expected_damage);
    println!(
        "Monte Carlo of that same sequence (10,000 samples): mean={:.1} stdev={:.1} p5={:.1} p50={:.1} p95={:.1}",
        eval.monte_carlo.mean, eval.monte_carlo.stdev, eval.monte_carlo.p5, eval.monte_carlo.p50, eval.monte_carlo.p95
    );
    println!(
        "Theoretical guaranteed floor (every roll at minimum, no crits -- a bound, not a realistic outcome): {:.1}",
        eval.guaranteed_floor
    );
    println!(
        "Search: beam width {}, {} states expanded, {}",
        search.search_budget,
        search.states_expanded,
        if search.provably_optimal { "PROVABLY OPTIMAL (beam never had to prune)" } else { "best found under this budget, not guaranteed optimal" }
    );
    println!("\nSequence:");
    for (t, id) in &search.sequence {
        println!("  t={t:>6}ms  {id}");
    }
}

fn run_recommend_mode(args: &Args) {
    let from_path = args.from.as_ref().expect("--mode recommend requires --from <old data pack path>");
    let to_path = args.to.as_ref().map(String::as_str).unwrap_or(DEFAULT_DATA_PACK);
    let history_path = args.history.as_ref().expect("--mode recommend requires --history <path>");

    let old_pack = load_data_pack(from_path);
    let new_pack = load_data_pack(to_path);
    println!("Diffing {} ({}) -> {} ({})\n", old_pack.version, from_path, new_pack.version, to_path);

    let diff = diff_data_packs(&old_pack, &new_pack);
    if diff.is_empty() {
        println!("No changes found between these two data packs.");
        return;
    }

    if !diff.added_spell_ids.is_empty() {
        println!("Added spells: {}", diff.added_spell_ids.join(", "));
    }
    if !diff.removed_spell_ids.is_empty() {
        println!("Removed spells: {}", diff.removed_spell_ids.join(", "));
    }
    for change in &diff.changed_spells {
        println!("Changed: {}", change.spell_id);
        for c in &change.changes {
            println!("    {c}");
        }
    }
    for m in &diff.mechanic_status_changes {
        println!("Mechanic status changed: {} ({:?} -> {:?})", m.mechanic_id, m.old_status, m.new_status);
    }

    let store = RunStore::open(history_path);
    let history = store.load_all().unwrap_or_else(|e| panic!("failed to read history `{history_path}`: {e}"));
    let affected = find_affected_builds(&history, &diff);

    if affected.is_empty() {
        println!("\nNo historical builds reference any changed spell -- nothing to re-test.");
        return;
    }

    println!("\n{} build(s) worth re-testing:", affected.len());
    for build in &affected {
        println!(
            "  build {} -- last ran under {} at mean DPS {:.1} -- touches changed spell(s): {}",
            build.build_fingerprint.as_str(),
            build.last_known_data_pack_version,
            build.last_known_dps_mean,
            build.matching_spell_ids.join(", ")
        );
    }

    if !args.resimulate {
        println!("\nPass --resimulate to actually re-run these builds under {} and record the deltas.", new_pack.version);
        return;
    }

    println!("\nRe-simulating under {} ...", new_pack.version);
    for affected_build in &affected {
        let run = history
            .iter()
            .filter(|r| r.build_fingerprint == affected_build.build_fingerprint)
            .max_by_key(|r| r.recorded_at_unix)
            .expect("affected build must come from a real history entry");

        let result = simulate(&new_pack, &run.build, 10_000, 1);
        let dispute = simulate_dispute_delta(&new_pack, &run.build, 10_000, 1);
        let prior = store
            .append(&new_pack.version, &run.build, &result.dps, result.touches_disputed_mechanic, dispute.as_ref())
            .unwrap_or_else(|e| panic!("failed to append to history `{history_path}`: {e}"));

        let delta = match prior {
            Some(p) => result.dps.mean - p.dps.mean,
            None => 0.0,
        };
        println!(
            "  build {} -- new mean DPS {:.1} ({:+.1} vs. last recorded {:.1})",
            affected_build.build_fingerprint.as_str(),
            result.dps.mean,
            delta,
            affected_build.last_known_dps_mean
        );
    }
}

fn run_add_external_mode(args: &Args) {
    let external_path = args.external.as_ref().expect("--mode add-external requires --external <path>");
    let build_fingerprint = args.build_fingerprint.clone().expect("--mode add-external requires --build <fingerprint> -- get this from --show-history");
    let source_name = args.source.clone().expect("--mode add-external requires --source <name>, e.g. wowforeversim.com");
    let reported_dps_mean = args.dps.expect("--mode add-external requires --dps <value> -- the number you read off the other site yourself");

    let result = ExternalResult {
        entry_id: String::new(),
        entered_at_unix: 0,
        build_fingerprint: build_fingerprint.clone(),
        source_name: source_name.clone(),
        source_talent_version: args.source_version.clone(),
        reported_dps_mean,
        notes: args.notes.clone().unwrap_or_default(),
    };

    let store = ExternalResultStore::open(external_path);
    store.append(result).unwrap_or_else(|e| panic!("failed to append to external-results store `{external_path}`: {e}"));
    println!("Recorded external result: build {build_fingerprint} -- {source_name} reports mean DPS {reported_dps_mean:.1}");
    println!("This value is never merged into this app's own simulation output (Plan Principle 10) -- see --mode compare-external to view it alongside our own result.");
}

fn run_compare_external_mode(args: &Args) {
    let history_path = args.history.as_ref().expect("--mode compare-external requires --history <path>");
    let external_path = args.external.as_ref().expect("--mode compare-external requires --external <path>");

    let run_store = RunStore::open(history_path);
    let external_store = ExternalResultStore::open(external_path);
    let comparisons = build_comparisons(&run_store, &external_store).unwrap_or_else(|e| panic!("failed to build comparisons: {e}"));

    if comparisons.is_empty() {
        println!("No external results recorded yet at {external_path}. Use --mode add-external to record one.");
        return;
    }

    for cmp in &comparisons {
        println!("\nBuild {}:", cmp.build_fingerprint.as_str());
        match &cmp.our_latest {
            Some(run) => println!("  Our result    -- data pack {}: mean DPS = {:.1}", run.data_pack_version, run.dps.mean),
            None => println!("  Our result    -- no run of this build recorded yet"),
        }
        for ext in &cmp.external {
            let version_note = ext.source_talent_version.as_deref().map(|v| format!(" (talent version: {v})")).unwrap_or_default();
            println!("  {:<13} -- reports mean DPS = {:.1}{version_note}", ext.source_name, ext.reported_dps_mean);
            if let Some(run) = &cmp.our_latest {
                let delta = ext.reported_dps_mean - run.dps.mean;
                let pct = delta / run.dps.mean * 100.0;
                println!("                    delta vs. our result: {delta:+.1} ({pct:+.2}%)");
            }
            if !ext.notes.is_empty() {
                println!("                    notes: {}", ext.notes);
            }
        }
    }
}

/// Phase 3 (Plan Section 8): a toy 2-person "raid" (Warlock + Mage demo
/// builds) demonstrating both the correctly-aggregated raid total and the
/// real cross-character synergy added this phase -- the Warlock's own
/// Curse of the Elements (`crates/raid`'s one sourced `RaidDebuffDef`)
/// boosts the *Mage's* Frostbolt/Fireball damage too, not just its own
/// caster's. Printed as a before/after so that synergy is visible, not
/// just asserted. See crates/raid's module docs for what's still not
/// modeled (raid buffs, healing-vs-incoming-damage).
fn run_raid_mode(args: &Args) {
    let data = load_data_pack_with_overrides(args);

    let members = vec![
        RaidMember { name: "Warlock".into(), build: warlock_demo_build() },
        RaidMember { name: "Mage".into(), build: mage_demo_build() },
    ];

    let baseline = simulate_raid_with_debuffs(&data, &members, &[], 10_000, 1);
    let cursed = simulate_raid_with_debuffs(&data, &members, &["curse_of_the_elements".into()], 10_000, 1);

    println!("Raid DPS (data pack {}), no raid debuffs active:", data.version);
    for m in &baseline.result.members {
        let dispute_note = if m.touches_disputed_mechanic { "  [DISPUTED MECHANIC]" } else { "" };
        println!("  {:<10} mean={:.1} stdev={:.1} p5={:.1} p50={:.1} p95={:.1}{}", m.name, m.dps.mean, m.dps.stdev, m.dps.p5, m.dps.p50, m.dps.p95, dispute_note);
    }
    println!(
        "  {:<10} mean={:.1} stdev={:.1} p5={:.1} p50={:.1} p95={:.1}  (correctly aggregated per-iteration, not summed percentiles)",
        "TOTAL", baseline.result.total.mean, baseline.result.total.stdev, baseline.result.total.p5, baseline.result.total.p50, baseline.result.total.p95
    );

    println!("\nWith Curse of the Elements active (one Warlock curse, benefits the whole raid -- Plan Section 8's cross-character synergy):");
    for m in &cursed.result.members {
        println!("  {:<10} mean={:.1} (was {:.1}, {:+.1}%)", m.name, m.dps.mean,
            baseline.result.members.iter().find(|b| b.name == m.name).unwrap().dps.mean,
            (m.dps.mean / baseline.result.members.iter().find(|b| b.name == m.name).unwrap().dps.mean - 1.0) * 100.0);
    }
    println!(
        "  {:<10} mean={:.1} (was {:.1}, {:+.1}%)",
        "TOTAL", cursed.result.total.mean, baseline.result.total.mean, (cursed.result.total.mean / baseline.result.total.mean - 1.0) * 100.0
    );
    println!(
        "  Active debuffs: {:?}  Dropped (over the {}-slot cap): {:?}",
        cursed.active_debuff_ids, wowforever_raid::CLASSIC_DEBUFF_LIMIT, cursed.dropped_debuff_ids
    );
    println!("\nNote: this still only models the Curse of the Elements debuff and the Warlock's own Demonic Sacrifice buff -- no raid-wide *buffs* (stat boosts shared across members) and no healing-vs-incoming-damage model yet (Plan Section 8 scope, see crates/raid and crates/heal module docs).");
}

fn main() {
    let args = parse_args();

    if let Some(show_history_path) = &args.show_history {
        print_history(show_history_path);
        return;
    }

    match args.mode {
        Mode::Dps => run_dps_mode(&args),
        Mode::Burst => run_burst_mode(&args),
        Mode::Recommend => run_recommend_mode(&args),
        Mode::AddExternal => run_add_external_mode(&args),
        Mode::CompareExternal => run_compare_external_mode(&args),
        Mode::Raid => run_raid_mode(&args),
        Mode::Heal => run_heal_mode(&args),
    }
}
