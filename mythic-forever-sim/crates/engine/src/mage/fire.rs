//! Fire Mage single-target core rotation, in the order
//! `validation/2026-10-04-builder-starters/fire-mage--undead.request.json` uses (decoded
//! 2026-10-25, the same way every Warlock rotation already in this project was):
//!
//! 1. Evocation below 10% mana. 2. Combustion when it's up and Fire Vulnerability is either
//! not applied yet or already maxed. 3. Pyroblast once Heating Up is fully stacked. 4. Keep
//! Fire Vulnerability stacked (Scorch, refreshed before it drops). 5. Fire Blast whenever off
//! cooldown. 6. Fireball filler.
//!
//! **2026-10-25, second pass**: Ignite, Heating Up/Pyroblast, Combustion and Arcane
//! Concentration/Clearcasting - the four real mechanics the first pass explicitly deferred -
//! are modeled now, each read directly from the Rust port's own talent-mechanic source
//! (`src/classes/mage/talents/heating_up.rs`, `spells/combustion.rs`, `talents/ignite.rs`,
//! `talents/arcane_concentration.rs`), not guessed at from a tooltip's rounded summary:
//!
//! - **Ignite** (talent, 5/5 in the real decoded build): a Fire-school crit rolls `share`
//!   (8/16/24/32/40% at 1-5 points) of its own already-fully-modified damage into a 2-tick/4s
//!   dot. A second crit while the dot is still ticking doesn't overwrite it - it rolls in
//!   whatever that dot still owes (`Fight::dot_owed_damage`) so nothing is lost, the same way
//!   the real source does it. The dot's own ticks never crit and never reroll Curse of the
//!   Elements (`Fight::apply_fixed_dot`'s own doc comment).
//! - **Heating Up / Pyroblast** (talents, 1/1 each): a *non-periodic* crit from Fireball,
//!   Scorch or Fire Blast adds a stack (cap 3, 20s duration) to a buff that cuts Pyroblast's
//!   cast time by 25%/stack (so 1.5s at 3/3, down from 6s - not literally instant, a real
//!   number read straight from the buff's own data row, not Classic/live WoW's different "Hot
//!   Streak" mechanic). Fire Blast's own *cast* (regardless of whether it crit) also adds a
//!   stack on top - the real source's own comment: "every Fire Blast cast adds a stack on top
//!   of its crit's". Casting Pyroblast at a cast time above 0 (always true here - this engine
//!   never models Presence of Mind) spends every stack.
//! - **Combustion** (talent, 1/1): a cooldown whose buff adds +10% Fire crit chance per stack,
//!   where a stack is added by *any* landed Fire-school hit (not just crits) while it's up, up
//!   to 10 stacks - but the buff itself ends (and only then does its real 3-minute cooldown
//!   start - this engine's own `Fight::set_cooldown` call for it happens on buff end, not on
//!   cast) once 3 of those hits have been crits, whichever stack count that happens at.
//! - **Arcane Concentration / Clearcasting** (talent, 5/5): a landed damaging cast has a
//!   2/4/6/8/10%-at-1-5-points chance (gated by a 1s internal cooldown) to grant Clearcasting -
//!   the next costed cast is free, via the exact same `cost_pct` mechanism Destruction
//!   Warlock's Cataclysm already uses (-1.0 = -100%), not a new one.
//!
//! Mana Ruby (a consumable) and `autocastOtherCooldowns` (trinkets) remain out of scope - the
//! same category of scope cut every Warlock rotation in this project already makes. Arcane
//! Missiles/Missile Barrage are also still out (the real priority list's own Arcane Missiles
//! line references an aura ID, 44404, that doesn't resolve to any real spell this project could
//! find - see the first pass's own research notes).

use crate::casting::{cast_direct_with_cost_pct, cast_utility, BASE_CRIT_MULTIPLIER};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct FireTalents {
    /// Tier 4, 0-3: a landed Scorch has a 33/67/100% chance to add a stack of Fire
    /// Vulnerability (spell 11095's own curve: `[[33,67,100]]`).
    pub improved_scorch: u8,
    /// Tier 3, 0-5: -100ms/point Fireball cast time (spell 11069's curve:
    /// `[[-100,-200,-300,-400,-500]]`).
    pub improved_fireball: u8,
    /// Tier 7, 0-5: +2%/point spell crit chance (spell 11115's curve: `[[2,4,6,8,10]]`).
    pub critical_mass: u8,
    /// Tier 8, 0-3: +1%/point Fire damage (spell 11124's curve: `[[1,2,3]]`).
    pub fire_power: u8,
    /// Tier 5, 0-3: +2%/point crit damage bonus on Fire spells (spell 18459's curve:
    /// `[[2,4,6]]`).
    pub incineration: u8,
    /// Frost tree tier 2, 0-3: +2%/point spell hit (spell 29438's curve: `[[2,4,6]]` - a real
    /// 3-point max, confirmed directly from the real engine's own talent-string validator
    /// (`mage talents "...": Elemental Precision has 3 ranks, the string gives 4`) while doing
    /// the per-ability breakdown comparison, 2026-10-25. The first pass's own talent-tree UI
    /// data (`ui/sim/talents/trees/mage.json`'s `maxPoints: 4`) was simply wrong for this one
    /// talent - a genuine discrepancy between that UI metadata and the actual game data the
    /// engine validates against, not a guess this project made up. The real decoded request's
    /// own talent string has 4 in this slot regardless (the same string the validation
    /// corpus's own logged entry carries too) - both are themselves invalid by the current
    /// engine's own rules, part of the larger repo-drift this comparison pass found (see
    /// research-notes/mage-fire-comparison.md).
    pub elemental_precision: u8,
    /// Tier 6, 0-5: a Fire crit rolls this much of its own damage into a burning dot - see
    /// module docs. Spell 11119's curve: `[[8,16,24,32,40]]`.
    pub ignite: u8,
    /// Tier 9/10, 1pt each: unlocks the Heating Up -> fast Pyroblast mechanic (see module
    /// docs). Two separate talents in the real tree (`heatingUp`/`pyroblast`) collapsed into
    /// one flag here since this engine doesn't cast Pyroblast at all without both.
    pub heating_up_pyroblast: bool,
    /// Tier 11, 1pt: unlocks the Combustion cooldown (see module docs).
    pub combustion: bool,
    /// Arcane tree tier 4, 0-5: Clearcasting's own proc chance (see module docs). Spell
    /// 11213's curve: `[[2,4,6,8,10]]`.
    pub arcane_concentration: u8,
}

const FIRE_VULNERABILITY: &str = "fire_vulnerability";
const FIRE_VULNERABILITY_MAX_STACKS: u8 = 5;
/// Spell 22959's own `DurationMs`.
const FIRE_VULNERABILITY_DURATION_MS: i64 = 30_000;
const FIRE_VULNERABILITY_PCT_PER_STACK: f64 = 0.03;
/// The real priority list's own refresh threshold (`auraRemainingTime <= 5s`).
const FIRE_VULNERABILITY_REFRESH_THRESHOLD_MS: i64 = 5_000;

/// Which casts can add a Heating Up stack on crit - `src/classes/mage/talents/heating_up.rs`'s
/// own `STACKERS` constant (Frostfire Bolt excluded: this engine doesn't have a Frost spec).
const HEATING_UP_STACKERS: [&str; 3] = ["fireball", "scorch", "fire_blast"];
const HEATING_UP: &str = "heating_up";
const HEATING_UP_MAX_STACKS: u8 = 3;
/// Spell 400625's own `DurationMs`.
const HEATING_UP_DURATION_MS: i64 = 20_000;
/// Spell 400625's own `A_ADD_PCT_MODIFIER`/`SPELLMOD_CASTING_TIME` effect, confirmed via the
/// Rust port's own `export.rs` field name (`cast_time_per_stack`) and the raw row's
/// `BasePoints: -25`.
const HEATING_UP_CAST_TIME_PCT_PER_STACK: f64 = -0.25;

const COMBUSTION: &str = "combustion";
/// Spell 28682's own `MaxStack`.
const COMBUSTION_MAX_STACKS: u8 = 10;
/// Spell 28682's own effect: `BasePoints: 10` (+10% crit per stack).
const COMBUSTION_CRIT_PCT_PER_STACK: f64 = 0.10;
/// `Combustion::max_crits` in the real source - 3 crits end the buff.
const COMBUSTION_CRITS_TO_END: i32 = 3;
/// Spell 11129's own `CategoryCooldownMs` - applied manually when the buff ends, not on cast
/// (see module docs and `Fight::EMBEDDED_MAGE_PACK_JSON`'s own `combustion` entry).
const COMBUSTION_COOLDOWN_MS: i64 = 180_000;
/// Not a real game duration (Combustion's own buff never expires on a timer, only on crit
/// count - `DurationMs: -1` on the real row) - just long enough to never lapse mid-fight by
/// accident, since this engine's aura map needs *some* expiry to track "is this active at all".
const COMBUSTION_NOMINAL_DURATION_MS: i64 = 3_600_000;
const COMBUSTION_CRIT_CHARGES_KEY: &str = "combustion_crits_remaining";

const CLEARCASTING: &str = "clearcasting";
/// Spell 12536's own `DurationMs`.
const CLEARCASTING_DURATION_MS: i64 = 15_000;
/// Spell 11213's own `ICDMs`.
const CLEARCASTING_ICD_MS: i64 = 1_000;
const CLEARCASTING_ICD_KEY: &str = "clearcasting_icd";

const IGNITE_NUM_TICKS: i64 = 2;
/// Ignite's dot (spell 412538): `DurationMs: 4000`, one periodic effect with `PeriodMs: 2000` -
/// 4000/2000 = 2 ticks.
const IGNITE_PERIOD_MS: i64 = 2_000;

fn improved_scorch_chance(points: u8) -> f64 {
    match points {
        1 => 0.33,
        2 => 0.67,
        3 => 1.00,
        _ => 0.0,
    }
}

fn arcane_concentration_chance(points: u8) -> f64 {
    0.02 * points as f64
}

fn ignite_share(points: u8) -> f64 {
    0.08 * points as f64
}

fn fire_vulnerability_bonus(fight: &Fight) -> f64 {
    FIRE_VULNERABILITY_PCT_PER_STACK * fight.aura_stacks(FIRE_VULNERABILITY) as f64
}

fn combustion_crit_bonus(fight: &Fight) -> f64 {
    COMBUSTION_CRIT_PCT_PER_STACK * fight.aura_stacks(COMBUSTION) as f64
}

/// -1.0 (free) if Clearcasting is up, 0.0 otherwise - the same `cost_pct` shape
/// `cast_direct_with_cost_pct` already uses for Destruction Warlock's Cataclysm.
fn clearcasting_cost_pct(fight: &Fight) -> f64 {
    if fight.aura_active(CLEARCASTING) {
        -1.0
    } else {
        0.0
    }
}

/// Applies this build's static talent modifiers once, before the fight starts. Fire
/// Vulnerability's and Combustion's own live bonuses are read fresh each cast instead, since
/// they depend on the current stack count.
pub fn apply_talents(fight: &mut Fight, talents: &FireTalents) {
    if talents.elemental_precision > 0 {
        fight.stats.spell_hit += 0.02 * talents.elemental_precision as f64;
    }
    if talents.critical_mass > 0 {
        fight.stats.spell_crit += 0.02 * talents.critical_mass as f64;
    }
    let m = &mut fight.mods;
    let fire_spells = ["fireball", "scorch", "fire_blast", "pyroblast"];
    if talents.fire_power > 0 {
        let pct = 0.01 * talents.fire_power as f64;
        for key in fire_spells {
            m.direct_pct.push((key.to_string(), pct));
            m.dot_pct.push((key.to_string(), pct));
        }
    }
    if talents.incineration > 0 {
        let pct = 0.02 * talents.incineration as f64;
        for key in fire_spells {
            m.crit_multiplier_bonus.push((key.to_string(), pct));
        }
    }
    if talents.improved_fireball > 0 {
        m.cast_time_flat_ms
            .push(("fireball".to_string(), -100 * talents.improved_fireball as i64));
    }
}

fn fire_vulnerability_needs_refresh(fight: &Fight) -> bool {
    let stacks = fight.aura_stacks(FIRE_VULNERABILITY);
    if stacks < FIRE_VULNERABILITY_MAX_STACKS {
        return true;
    }
    fight
        .aura_remaining_ms(FIRE_VULNERABILITY)
        .map_or(true, |remaining| remaining <= FIRE_VULNERABILITY_REFRESH_THRESHOLD_MS)
}

/// Ignite's own merge formula (`talents/ignite.rs`'s `on_spell_hit_dealt`): roll `share` of
/// `crit_damage` into whatever the dot still owes, spread back over its full tick count.
fn apply_ignite(fight: &mut Fight, crit_damage: f64, share: f64) {
    let owed = fight.dot_owed_damage("ignite");
    let tick_amount = crit_damage.mul_add(share, owed) / IGNITE_NUM_TICKS as f64;
    fight.apply_fixed_dot("ignite", tick_amount, IGNITE_PERIOD_MS, IGNITE_NUM_TICKS);
}

/// Everything that can react to a resolved Fire-school cast: Ignite, Heating Up's stacking,
/// Combustion's stacking/ending, and Clearcasting's own proc roll and consumption. Called after
/// every damaging cast in `step` below. `is_stacker` is whether this ability can add a Heating
/// Up stack on crit (Fire Blast's own unconditional stack, regardless of crit, is handled by
/// its caller separately, since it isn't gated on landing a hit at all).
fn on_fire_cast_resolved(fight: &mut Fight, talents: &FireTalents, result: CastResult) {
    let landed = matches!(result, CastResult::Landed { .. });
    let crit = matches!(result, CastResult::Landed { crit: true, .. });

    // Clearcasting: consumed by the cast that just spent it, then re-rolled for the next one.
    if landed && fight.aura_active(CLEARCASTING) {
        fight.clear_aura(CLEARCASTING);
    }
    if landed && talents.arcane_concentration > 0 && !fight.is_on_cooldown(CLEARCASTING_ICD_KEY) {
        fight.set_cooldown(CLEARCASTING_ICD_KEY, CLEARCASTING_ICD_MS);
        if fight.roll_chance(arcane_concentration_chance(talents.arcane_concentration)) {
            fight.set_aura(CLEARCASTING, CLEARCASTING_DURATION_MS);
        }
    }

    // Combustion: any landed Fire hit adds a stack; every 3rd crit ends the buff and starts
    // its real cooldown (see module docs on why the cooldown is set here, not on cast).
    if landed && fight.aura_active(COMBUSTION) {
        fight.add_aura_stack(COMBUSTION, COMBUSTION_MAX_STACKS, COMBUSTION_NOMINAL_DURATION_MS);
        if crit && fight.consume_charge(COMBUSTION_CRIT_CHARGES_KEY) {
            if fight.charges_remaining(COMBUSTION_CRIT_CHARGES_KEY) == 0 {
                fight.clear_aura(COMBUSTION);
                fight.set_cooldown(COMBUSTION, COMBUSTION_COOLDOWN_MS);
            }
        }
    }

    if crit {
        if talents.ignite > 0 {
            if let CastResult::Landed { amount, .. } = result {
                apply_ignite(fight, amount, ignite_share(talents.ignite));
            }
        }
    }
}

/// Fire Blast's own Heating Up stack is unconditional on *casting* it (not on landing or
/// critting) - `heating_up.rs`'s own `on_fire_blast_cast`, separate from the crit-gated
/// stacking every other stacker spell only gets.
fn maybe_add_heating_up_stack_for_cast(fight: &mut Fight, talents: &FireTalents, ability_key: &str, result: CastResult) {
    if !talents.heating_up_pyroblast {
        return;
    }
    if ability_key == "fire_blast" {
        fight.add_aura_stack(HEATING_UP, HEATING_UP_MAX_STACKS, HEATING_UP_DURATION_MS);
    }
    let crit = matches!(result, CastResult::Landed { crit: true, .. });
    if crit && HEATING_UP_STACKERS.contains(&ability_key) {
        fight.add_aura_stack(HEATING_UP, HEATING_UP_MAX_STACKS, HEATING_UP_DURATION_MS);
    }
}

pub fn step(fight: &mut Fight, talents: &FireTalents) {
    let mult = BASE_CRIT_MULTIPLIER;
    let bonus = fire_vulnerability_bonus(fight);
    let cost_pct = clearcasting_cost_pct(fight);

    if fight.mana_percent() < 0.10 {
        if cast_utility(fight, "evocation") {
            fight.add_mana(super::evocation_mana(fight.stats.mp5));
            return;
        }
    }

    if talents.combustion
        && !fight.aura_active(COMBUSTION)
        && !fight.is_on_cooldown(COMBUSTION)
        && (fight.aura_stacks(FIRE_VULNERABILITY) == 0
            || fight.aura_stacks(FIRE_VULNERABILITY) >= FIRE_VULNERABILITY_MAX_STACKS)
        && cast_utility(fight, "combustion")
    {
        fight.add_aura_stack(COMBUSTION, COMBUSTION_MAX_STACKS, COMBUSTION_NOMINAL_DURATION_MS);
        fight.set_charges(COMBUSTION_CRIT_CHARGES_KEY, COMBUSTION_CRITS_TO_END, COMBUSTION_NOMINAL_DURATION_MS);
        return;
    }

    if talents.heating_up_pyroblast && fight.aura_stacks(HEATING_UP) >= HEATING_UP_MAX_STACKS {
        let crit = fight.stats.spell_crit + combustion_crit_bonus(fight);
        let stacks = fight.aura_stacks(HEATING_UP) as f64;
        fight.mods.cast_time_pct.push(("pyroblast".to_string(), HEATING_UP_CAST_TIME_PCT_PER_STACK * stacks));
        let result = cast_direct_with_cost_pct(fight, "pyroblast", "pyroblast", crit, mult, bonus, cost_pct);
        fight.mods.cast_time_pct.pop();
        if let Some(result) = result {
            // Pyroblast has its own dot (53/tick x4 over 12s) from the same cast - missed
            // entirely until this per-ability breakdown comparison caught it (the exact same
            // bug this engine's own Fireball had in the first pass, before that one got
            // caught and fixed).
            if matches!(result, CastResult::Landed { .. }) {
                fight.apply_dot_unconditional("pyroblast", "pyroblast", crit, mult, bonus);
            }
            fight.clear_aura(HEATING_UP);
            on_fire_cast_resolved(fight, talents, result);
            return;
        }
    }

    if fire_vulnerability_needs_refresh(fight) {
        let crit = fight.stats.spell_crit + combustion_crit_bonus(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "scorch", "scorch", crit, mult, bonus, cost_pct) {
            let landed = matches!(result, CastResult::Landed { .. });
            if landed && talents.improved_scorch > 0 {
                let proc_chance = improved_scorch_chance(talents.improved_scorch);
                if proc_chance >= 1.0 || fight.roll_chance(proc_chance) {
                    fight.add_aura_stack(FIRE_VULNERABILITY, FIRE_VULNERABILITY_MAX_STACKS, FIRE_VULNERABILITY_DURATION_MS);
                }
            }
            maybe_add_heating_up_stack_for_cast(fight, talents, "scorch", result);
            on_fire_cast_resolved(fight, talents, result);
            return;
        }
    }

    {
        let crit = fight.stats.spell_crit + combustion_crit_bonus(fight);
        if let Some(result) = cast_direct_with_cost_pct(fight, "fire_blast", "fire_blast", crit, mult, bonus, cost_pct) {
            maybe_add_heating_up_stack_for_cast(fight, talents, "fire_blast", result);
            on_fire_cast_resolved(fight, talents, result);
            return;
        }
    }

    let crit = fight.stats.spell_crit + combustion_crit_bonus(fight);
    if let Some(result) = cast_direct_with_cost_pct(fight, "fireball", "fireball", crit, mult, bonus, cost_pct) {
        if matches!(result, CastResult::Landed { .. }) {
            fight.apply_dot_unconditional("fireball", "fireball", crit, mult, bonus);
        }
        maybe_add_heating_up_stack_for_cast(fight, talents, "fireball", result);
        on_fire_cast_resolved(fight, talents, result);
    }
}

pub fn run(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &FireTalents,
) -> crate::sim::FightReport {
    let pack = crate::spelldata::embedded_mage_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    apply_talents(&mut fight, talents);
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents);
        if fight.now_ms == before {
            // Safety: see warlock/destruction.rs::run for why this guard exists.
            fight.advance(1000);
        }
    }
    fight.report
}
