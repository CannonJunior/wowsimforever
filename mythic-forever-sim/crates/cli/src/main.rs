//! Runs the Warlock engine over many iterations (Monte Carlo, since hit/crit/variance are all
//! randomized) and prints average DPS with a per-ability breakdown. This is a research/dev
//! tool, not a polished UI - see the workspace README for the project's actual status.

use mythic_forever_engine::gear::{
    embedded_mage_gear_pack, embedded_warlock_gear_pack, equipment_to_stats, example_build_equipment,
    BuilderStarterBuffs, GearPack,
};
use mythic_forever_engine::mage::{arcane, fire, frost};
use mythic_forever_engine::sim::FightReport;
use mythic_forever_engine::stats::CasterStats;
use mythic_forever_engine::warlock::{affliction, demonology, destruction};
use std::collections::HashMap;

fn parse_args() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut args = std::env::args().skip(1).peekable();
    while let Some(key) = args.next() {
        if let Some(stripped) = key.strip_prefix("--") {
            let value = args.next().unwrap_or_else(|| "true".to_string());
            map.insert(stripped.to_string(), value);
        }
    }
    map
}

fn get_f64(args: &HashMap<String, String>, key: &str, default: f64) -> f64 {
    args.get(key)
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn get_u8(args: &HashMap<String, String>, key: &str, default: u8) -> u8 {
    args.get(key)
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(default)
}

fn get_bool(args: &HashMap<String, String>, key: &str, default: bool) -> bool {
    args.get(key)
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default)
}

fn parse_sacrificed_pet(args: &HashMap<String, String>) -> Option<mythic_forever_engine::warlock::SacrificedPet> {
    use mythic_forever_engine::warlock::SacrificedPet;
    match args.get("sacrifice").map(String::as_str) {
        None | Some("none") => None,
        Some("imp") => Some(SacrificedPet::Imp),
        Some("succubus") => Some(SacrificedPet::Succubus),
        Some(other) => {
            eprintln!("unknown --sacrifice `{other}`, expected imp|succubus|none");
            std::process::exit(1);
        }
    }
}

fn parse_active_pet(args: &HashMap<String, String>, default: mythic_forever_engine::warlock::PetChoice) -> mythic_forever_engine::warlock::PetChoice {
    use mythic_forever_engine::warlock::PetChoice;
    match args.get("pet").map(String::as_str) {
        None => default,
        Some("imp") => PetChoice::Imp,
        Some("succubus") => PetChoice::Succubus,
        Some(other) => {
            eprintln!("unknown --pet `{other}`, expected imp|succubus");
            std::process::exit(1);
        }
    }
}

fn parse_optional_pet(args: &HashMap<String, String>) -> Option<mythic_forever_engine::warlock::PetChoice> {
    use mythic_forever_engine::warlock::PetChoice;
    match args.get("pet").map(String::as_str) {
        None | Some("none") => None,
        Some("imp") => Some(PetChoice::Imp),
        Some("succubus") => Some(PetChoice::Succubus),
        Some(other) => {
            eprintln!("unknown --pet `{other}`, expected imp|succubus|none");
            std::process::exit(1);
        }
    }
}

fn print_report(spec: &str, report: &FightReport, iterations: u32) {
    println!("=== {spec} (avg of {iterations} runs) ===");
    println!(
        "DPS: {:.1}  (total damage {:.0} over {:.1}s)",
        report.dps(),
        report.total_damage,
        report.duration_ms as f64 / 1000.0
    );
    println!(
        "{:<18} {:>8} {:>8} {:>6} {:>6} {:>10} {:>8}",
        "ability", "casts", "ticks", "hits", "crits", "damage", "dps%"
    );
    // NOTE: `t.damage` below is summed across all iterations (never divided by `iterations`),
    // so the percentage denominator must be on that same raw-sum scale, not `report.total_damage`
    // (which `average_reports` already divided down to a per-run average) - mixing the two
    // scales was this function's first bug, caught by a sanity-check run showing >100% shares.
    // `casts`/`ticks` are now separate (found while comparing this breakdown against
    // mythicsim.com's own, 2026-10-24: `casts` used to silently include dot ticks, inflating a
    // dot's own cast count ~10x over its real one - see `AbilityTotals::casts`'s doc comment).
    let total_summed: f64 = report.totals.values().map(|t| t.damage).sum::<f64>().max(1.0);
    for (key, t) in &report.totals {
        println!(
            "{:<18} {:>8.1} {:>8.1} {:>6.1} {:>6.1} {:>10.0} {:>7.1}%",
            key,
            t.casts as f64 / iterations as f64,
            t.ticks as f64 / iterations as f64,
            t.hits as f64 / iterations as f64,
            t.crits as f64 / iterations as f64,
            t.damage / iterations as f64,
            100.0 * t.damage / total_summed
        );
    }
    println!();
}

fn average_reports(reports: Vec<FightReport>) -> FightReport {
    let n = reports.len() as f64;
    let duration_ms = reports[0].duration_ms;
    let mut merged = FightReport {
        duration_ms,
        ..FightReport::default()
    };
    for r in &reports {
        merged.total_damage += r.total_damage / n;
        for (key, t) in &r.totals {
            let e = merged.totals.entry(key.clone()).or_default();
            e.casts += t.casts;
            e.ticks += t.ticks;
            e.hits += t.hits;
            e.crits += t.crits;
            e.misses += t.misses;
            e.damage += t.damage;
        }
    }
    merged
}

fn main() {
    let args = parse_args();
    let class = args.get("class").cloned().unwrap_or_else(|| "warlock".to_string());
    let default_spec = match class.as_str() {
        "mage" => "fire",
        "warlock" => "destruction",
        other => {
            eprintln!("unknown --class `{other}`, expected warlock|mage");
            std::process::exit(1);
        }
    };
    let spec = args.get("spec").cloned().unwrap_or_else(|| default_spec.to_string());
    let iterations = args.get("iterations").and_then(|v| v.parse::<u32>().ok()).unwrap_or(2000);
    let duration_ms = (get_f64(&args, "duration", 180.0) * 1000.0) as i64;
    let base_seed = args.get("seed").and_then(|v| v.parse::<u64>().ok()).unwrap_or(42);

    let stats = if let Some(gear_name) = args.get("gear") {
        let pack: GearPack = match class.as_str() {
            "mage" => embedded_mage_gear_pack(),
            _ => embedded_warlock_gear_pack(),
        };
        let build = pack.example_builds.get(gear_name).unwrap_or_else(|| {
            let known: Vec<_> = pack.example_builds.keys().cloned().collect();
            eprintln!("unknown --gear `{gear_name}`, known builds: {known:?}");
            std::process::exit(1);
        });
        let race = args.get("race").cloned().unwrap_or_else(|| build.race.clone());
        let equipment = example_build_equipment(build);
        let builder_starter_buffs = match args.get("buffs").map(String::as_str) {
            None | Some("none") => BuilderStarterBuffs::None,
            // The packages differ only in the school elixir (and, for Mage, a weapon imbue
            // Warlock's own package doesn't carry) - see `gear.rs`'s own doc comment on
            // `BuilderStarterBuffs` for the full citation. Mage itself splits further: Fire's
            // own fixture uses a different school elixir than Arcane/Frost's shared one, so
            // `--buffs builder-starter` picks the right variant from `--class` *and* `--spec`.
            Some("builder-starter") => match (class.as_str(), spec.as_str()) {
                ("mage", "arcane") | ("mage", "frost") => BuilderStarterBuffs::MageArcaneFrost,
                ("mage", _) => BuilderStarterBuffs::MageFire,
                _ => BuilderStarterBuffs::Warlock,
            },
            Some(other) => {
                eprintln!("unknown --buffs `{other}`, expected none|builder-starter");
                std::process::exit(1);
            }
        };
        let mut computed = equipment_to_stats(&pack, &race, &equipment, builder_starter_buffs)
            .unwrap_or_else(|e| {
                eprintln!("gear error: {e}");
                std::process::exit(1);
            });
        // Explicit flags still override individual fields on top of the gear-computed ones
        // (e.g. to layer in buffs/consumables this engine doesn't itself model).
        if args.contains_key("spell-power") {
            computed.spell_power = get_f64(&args, "spell-power", computed.spell_power);
        }
        if args.contains_key("shadow-power") {
            computed.bonus_shadow_power = get_f64(&args, "shadow-power", computed.bonus_shadow_power);
        }
        if args.contains_key("fire-power") {
            computed.bonus_fire_power = get_f64(&args, "fire-power", computed.bonus_fire_power);
        }
        if args.contains_key("arcane-power") {
            computed.bonus_arcane_power = get_f64(&args, "arcane-power", computed.bonus_arcane_power);
        }
        if args.contains_key("frost-power") {
            computed.bonus_frost_power = get_f64(&args, "frost-power", computed.bonus_frost_power);
        }
        if args.contains_key("intellect") {
            computed.intellect = get_f64(&args, "intellect", computed.intellect);
        }
        if args.contains_key("hit") {
            computed.spell_hit = get_f64(&args, "hit", 0.0) / 100.0;
        }
        if args.contains_key("crit") {
            computed.spell_crit = get_f64(&args, "crit", 0.0) / 100.0;
        }
        if args.contains_key("haste") {
            computed.haste = 1.0 + get_f64(&args, "haste", 0.0) / 100.0;
        }
        if args.contains_key("mp5") {
            computed.mp5 = get_f64(&args, "mp5", computed.mp5);
        }
        if args.contains_key("max-mana") {
            computed.max_mana = get_f64(&args, "max-mana", computed.max_mana);
        }
        eprintln!(
            "gear: `{gear_name}` ({race}) -> spell_power={:.0} hit={:.1}% crit={:.1}% haste={:.1}% \
             max_mana={:.0} spirit={:.0}\n",
            computed.spell_power,
            computed.spell_hit * 100.0,
            computed.spell_crit * 100.0,
            (computed.haste - 1.0) * 100.0,
            computed.max_mana,
            computed.spirit,
        );
        computed
    } else {
        CasterStats {
            spell_power: get_f64(&args, "spell-power", 450.0),
            bonus_shadow_power: get_f64(&args, "shadow-power", 0.0),
            bonus_fire_power: get_f64(&args, "fire-power", 0.0),
            bonus_arcane_power: get_f64(&args, "arcane-power", 0.0),
            bonus_frost_power: get_f64(&args, "frost-power", 0.0),
            spell_hit: get_f64(&args, "hit", 99.0) / 100.0,
            spell_crit: get_f64(&args, "crit", 25.0) / 100.0,
            haste: 1.0 + get_f64(&args, "haste", 0.0) / 100.0,
            mp5: get_f64(&args, "mp5", 80.0),
            max_mana: get_f64(&args, "max-mana", 8000.0),
            spirit: get_f64(&args, "spirit", 100.0),
            intellect: get_f64(&args, "intellect", 0.0),
        }
    };
    let target_max_health = args.get("target-health").and_then(|v| v.parse::<f64>().ok());

    if spec == "demonology" {
        eprintln!(
            "NOTE: Demonology's Imp Firebolt, Demonic Brand and (unused) Lash of Pain numbers \
             come from a Go-literal source (export.rs), not the main auto-generated data table \
             the rest of this pack uses, and were not independently cross-checked against a \
             second site (see crates/engine/src/warlock/demonology.rs module docs). Sourced, \
             but on thinner ground than the other two specs.\n"
        );
    }

    let mut reports = Vec::with_capacity(iterations as usize);
    for i in 0..iterations {
        let seed = base_seed.wrapping_add(i as u64 * 2654435761);
        let report = match spec.as_str() {
            "destruction" => {
                let talents = destruction::DestructionTalents {
                    ruin: get_u8(&args, "ruin", 5),
                    improved_shadow_bolt: get_u8(&args, "improved-shadow-bolt", 5),
                    shadow_and_flame: get_u8(&args, "shadow-and-flame", 5),
                    agonizing_flames: get_u8(&args, "agonizing-flames", 3),
                    fire_and_brimstone: get_u8(&args, "fire-and-brimstone", 3),
                    aftermath: get_u8(&args, "aftermath", 5),
                    bane: get_u8(&args, "bane", 5),
                    incinerate: get_bool(&args, "incinerate", true),
                    improved_life_tap: get_u8(&args, "improved-life-tap", 0),
                    cataclysm: get_u8(&args, "cataclysm", 3),
                    sacrificed_pet: parse_sacrificed_pet(&args),
                    suppression: get_u8(&args, "suppression", 5),
                    improved_corruption: get_u8(&args, "improved-corruption", 2),
                };
                destruction::run(stats, duration_ms, seed, &talents)
            }
            "affliction" => {
                let pet = args
                    .get("pet")
                    .map(|_| parse_optional_pet(&args))
                    .unwrap_or(Some(mythic_forever_engine::warlock::PetChoice::Succubus));
                let talents = affliction::AfflictionTalents {
                    improved_corruption: get_u8(&args, "improved-corruption", 5),
                    improved_life_tap: get_u8(&args, "improved-life-tap", 2),
                    improved_bane_of_agony: get_u8(&args, "improved-bane-of-agony", 2),
                    pandemic: get_u8(&args, "pandemic", 3),
                    malevolence: get_u8(&args, "malevolence", 5),
                    shadow_mastery: get_u8(&args, "shadow-mastery", 5),
                    nightfall: get_u8(&args, "nightfall", 2),
                    // Default was `true`, a leftover guess from before this project ever
                    // decoded the real "builder starter" talent string - that string (checked
                    // directly via wowsims/forever's own talent tree layout, 2026-10-24 while
                    // comparing a per-ability breakdown against mythicsim.com's) has 0 points
                    // in Siphon Life. Fixed; `--siphon-life true` still opts a different build
                    // in.
                    siphon_life: get_bool(&args, "siphon-life", false),
                    keep_immolate: get_bool(&args, "keep-immolate", true),
                    amplify_curse: get_bool(&args, "amplify-curse", true),
                    ruin: get_u8(&args, "ruin", 5),
                    improved_shadow_bolt: get_u8(&args, "improved-shadow-bolt", 5),
                    bane: get_u8(&args, "bane", 5),
                    agonizing_flames: get_u8(&args, "agonizing-flames", 3),
                    sacrificed_pet: parse_sacrificed_pet(&args),
                    pet,
                    suppression: get_u8(&args, "suppression", 5),
                };
                affliction::run(stats, duration_ms, seed, &talents)
            }
            "demonology" => {
                let pet = parse_active_pet(&args, mythic_forever_engine::warlock::PetChoice::Imp);
                let talents = demonology::DemonologyTalents {
                    pet,
                    improved_imp: get_u8(&args, "improved-imp", 3),
                    improved_sayaad: get_u8(&args, "improved-sayaad", 3),
                    master_demonologist: get_u8(&args, "master-demonologist", 5),
                    demonic_knowledge: get_u8(&args, "demonic-knowledge", 3),
                    demonic_brand: get_u8(&args, "demonic-brand", 3),
                    decimation: get_u8(&args, "decimation", 2),
                    improved_life_tap: get_u8(&args, "improved-life-tap", 0),
                    ruin: get_u8(&args, "ruin", 5),
                    improved_shadow_bolt: get_u8(&args, "improved-shadow-bolt", 5),
                    bane: get_u8(&args, "bane", 5),
                    agonizing_flames: get_u8(&args, "agonizing-flames", 3),
                    sacrificed_pet: parse_sacrificed_pet(&args),
                    unholy_power: get_u8(&args, "unholy-power", 5),
                    soul_link: get_bool(&args, "soul-link", true),
                    suppression: get_u8(&args, "suppression", 2),
                };
                demonology::run(stats, duration_ms, seed, &talents, target_max_health)
            }
            "fire" => {
                let talents = fire::FireTalents {
                    improved_scorch: get_u8(&args, "improved-scorch", 3),
                    improved_fireball: get_u8(&args, "improved-fireball", 5),
                    critical_mass: get_u8(&args, "critical-mass", 5),
                    fire_power: get_u8(&args, "fire-power-talent", 3),
                    incineration: get_u8(&args, "incineration", 3),
                    // Default was 4, a real bug (the talent only has 3 ranks) - found while
                    // doing the per-ability breakdown comparison, 2026-10-25 - see
                    // `FireTalents::elemental_precision`'s own doc comment.
                    elemental_precision: get_u8(&args, "elemental-precision", 3),
                    ignite: get_u8(&args, "ignite", 5),
                    heating_up_pyroblast: get_bool(&args, "heating-up-pyroblast", true),
                    combustion: get_bool(&args, "combustion", true),
                    arcane_concentration: get_u8(&args, "arcane-concentration", 5),
                };
                fire::run(stats, duration_ms, seed, &talents)
            }
            "arcane" => {
                let talents = arcane::ArcaneTalents {
                    arcane_blast: get_bool(&args, "arcane-blast", true),
                    missile_barrage: get_u8(&args, "missile-barrage", 1),
                    arcane_concentration: get_u8(&args, "arcane-concentration", 5),
                    arcane_impact: get_u8(&args, "arcane-impact", 3),
                    arcane_mind: get_u8(&args, "arcane-mind", 5),
                    arcane_instability: get_u8(&args, "arcane-instability", 3),
                };
                arcane::run(stats, duration_ms, seed, &talents)
            }
            "frost" => {
                let talents = frost::FrostTalents {
                    improved_frostbolt: get_u8(&args, "improved-frostbolt", 5),
                    elemental_precision: get_u8(&args, "elemental-precision", 3),
                    ice_shards: get_u8(&args, "ice-shards", 5),
                    fingers_of_frost: get_u8(&args, "fingers-of-frost", 1),
                    shatter: get_u8(&args, "shatter", 1),
                    missile_barrage: get_u8(&args, "missile-barrage", 1),
                    arcane_concentration: get_u8(&args, "arcane-concentration", 5),
                };
                frost::run(stats, duration_ms, seed, &talents)
            }
            other => {
                eprintln!(
                    "unknown --spec `{other}` for --class {class}, expected {} \
                     (warlock: destruction|affliction|demonology; mage: fire|arcane|frost)",
                    default_spec
                );
                std::process::exit(1);
            }
        };
        reports.push(report);
    }

    let averaged = average_reports(reports);
    print_report(&spec, &averaged, iterations);
}
