//! Smite Priest single-target core rotation, in the order
//! `validation/2026-10-04-builder-starters/smite-priest--undead.request.json` uses (decoded
//! 2026-10-25): 1. Inner Focus immediately followed by Holy Fire, once Holy Fire's own dot has
//! under 3s remaining and Inner Focus is off cooldown (falls through to a plain Holy Fire
//! refresh if Inner Focus isn't available). 2. Penance, unconditional whenever off cooldown.
//! 3. Shadow Word: Pain, kept up (no remaining-fight-time gate on this item, unlike Shadow
//! Priest's own copy of the same spell). 4. Devouring Plague, kept up (skipped under 12s
//! remaining). 5. Smite (full rank), gated on a real mana-pacing check:
//! `currentManaPercent >= remainingTimePercent * 0.9` - only cast the expensive rank while
//! there's still enough mana left to sustain it for the *rest* of the fight. 6. Smite (rank 2,
//! `591` - a cheap downranked spell), the mana-saving fallback once that check fails.
//!
//! Every mechanic below is read directly from the Rust port's own source
//! (`src/classes/priest/talents/power_in_light.rs`, `src/classes/priest/talents/inner_focus.rs`)
//! and this exact build's own `prepare` output:
//!
//! - **Power in Light** (talent): +10% damage on this build's own Holy-school kit (`multiplier:
//!   1.1` in the real `prepare` output, applied to Smite/Holy Fire/Penance - the exact spell
//!   list this rotation already casts).
//! - **Inner Focus** (talent, shared with Shadow Priest - see `priest/shadow.rs`'s own module
//!   docs for the full citation): makes the next costed cast free and adds +25% crit chance
//!   (this build's own real effect). Spent on Holy Fire's own refresh here, not Mind Blast
//!   (this spec's real priority list never casts Mind Blast at all, despite the spell being
//!   registered/available - trusted exactly as the real priority list states it, not assumed
//!   from what the class *could* cast).
//! - **Twin Disciplines** (talent, shared mechanic with Shadow Priest - see
//!   `SmiteTalents::twin_disciplines`'s own doc comment): +1%/point damage on instant spells
//!   only - Shadow Word: Pain, Devouring Plague and Penance (confirmed instant via this
//!   build's own real log, `Cast Time = 0s`), but not Smite or Holy Fire (both have a real
//!   cast time). Found 2026-10-26 the same way as Shadow Priest's own: this build's own
//!   Shadow Word: Pain line shows `AfterAttackerMods/BaseDamage = 1.0498 ≈ 1.05`, matching
//!   5/5 exactly (this build has neither Shadowform nor Shadow Weaving, so the ratio is purely
//!   this one talent, unlike Shadow Priest's own compound 1.21-times-1.05 reading).
//! - **Penance's own "extra tick"**: a landed cast fires its first bolt *immediately* on cast,
//!   not after the first tick period - `src/classes/priest/spells/holy.rs::apply_penance`'s
//!   own doc comment ("Go `Dot.TickOnce`, an extra tick that leaves the tick count alone"),
//!   confirmed directly against a real log showing three bolts (t=0.00, 1.00, 2.00) for one
//!   cast, not two. Modeled as 3 ticks in the data pack rather than 2 - the exact intra-channel
//!   timing (this engine's own dot-scheduling never fires a tick at offset 0) is a minor
//!   approximation, not a total-damage one.
//!
//! - **Spiritual Guidance** (talent, 3/5 in this build): adds a flat percentage of Spirit
//!   directly to the universal `SpellDamage` stat (see `SmiteTalents::spiritual_guidance`'s
//!   own doc comment for the exact curve and derivation) - found 2026-10-26 to be exactly the
//!   previously-unexplained "509.9 real vs 498 computed" effective-spell-power residual noted
//!   in `research-notes/priest-smite-comparison.md`.
//! - **Judgement of the Crusader** (this fixture's own `raid.debuffs.judgementOfTheCrusader:
//!   true` flag - found and *wrongly dismissed twice* as "probably a crit-chance debuff"
//!   before finally being checked against the real engine's own source: it's a flat
//!   Holy-school damage-*taken* bonus, `+161` at its own max rank, modeled as a flat
//!   `bonus_holy_power` addition in `gear.rs` - see `JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER`'s
//!   own doc comment there for the full derivation). This was the real cause of this spec's
//!   own dominant ~23% `BaseDamage` shortfall on Smite/Holy Fire/Penance.
//! - **Dark Sacrifice** (the Undead racial, not Priest-specific - shared with Shadow Priest,
//!   see `priest/shadow.rs`'s own `step` for the full citation): `1600 + Spirit` mana, once per
//!   fight, the moment the full amount would fit without overflow.
//!
//! Not modeled: Holy Nova (present in this build's own `prepare` output but never cast by the
//! real priority list - an AoE spell irrelevant to a single-target fight) and Searing Light (a
//! healing-cost-reduction mechanic, this project's DPS-only scope excludes healing).

use crate::casting::{cast_direct_with_cost_pct, cast_dot_only, cast_utility, BASE_CRIT_MULTIPLIER};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct SmiteTalents {
    /// Discipline tree: see module docs. +10% on Smite/Holy Fire/Penance.
    pub power_in_light: bool,
    /// Discipline tree, shared mechanic with Shadow Priest - see `priest/shadow.rs`.
    pub inner_focus: bool,
    /// Discipline tree, 0-5, shared mechanic with Shadow Priest (see that module's own doc
    /// comment for the full citation and derivation) - +1%/point damage on *instant* spells.
    /// Shadow Word: Pain and Devouring Plague qualify the same way they do for Shadow Priest;
    /// Penance also qualifies (`src/classes/priest/prepare/talents/discipline.rs`'s own class
    /// mask names it explicitly, and this build's own real log confirms `Cast Time = 0s` for
    /// it) but Smite/Holy Fire do not (both have a real cast time). Found 2026-10-26 by the
    /// same technique as Shadow Priest's own: this build's own real Shadow Word: Pain line
    /// shows `AfterAttackerMods / BaseDamage` = 1.0498 ≈ `1.00 * 1.05` (no Shadowform/Shadow
    /// Weaving on this build at all, unlike Shadow Priest's), matching 5/5 exactly.
    pub twin_disciplines: u8,
    /// Holy tree, 0-5: adds a flat percentage of Spirit directly to the universal `SpellDamage`
    /// stat (`src/classes/priest/prepare/talents/holy.rs::apply_spiritual_guidance`:
    /// `sdm.add_stat_dependency(Stat::Spirit, Stat::SpellDamage, damage)` - a real stat
    /// dependency on the *universal* SpellDamage stat, not a school-scoped `bonus_holy_power`
    /// like the Healing Power mechanic below). The real curve (`data/spell-extras.json`,
    /// spell 14901: damage% = `[1,3,5,6,8]` at ranks 1-5) is exact, not the module doc's
    /// rounded prose. Found 2026-10-26: this build's own `prepare` output gives 3/5 points and
    /// a final `SpellDamage: 509.85` where `509.85 - 237.0 * 0.05 = 498.0` matches this
    /// project's own pre-existing gear-computed total *exactly* - the previously-unexplained
    /// "509.9 real vs 498 computed" SP residual noted in `research-notes/priest-smite-
    /// comparison.md` was this talent the whole time, not a separate, still-open mystery.
    pub spiritual_guidance: u8,
}

/// `data/spell-extras.json`'s own curve for spell 14901 (Spiritual Guidance), damage half only -
/// see `SmiteTalents::spiritual_guidance`'s own doc comment.
const SPIRITUAL_GUIDANCE_DAMAGE_PCT: [f64; 5] = [0.01, 0.03, 0.05, 0.06, 0.08];

const HOLY_SPELLS: [&str; 3] = ["smite", "holy_fire", "penance"];
/// `apply_twin_disciplines`'s own class mask, restricted to this spec's own instant-cast
/// spells that qualify (Smite/Holy Fire both have a real cast time, so neither is in this
/// list - see `SmiteTalents::twin_disciplines`'s own doc comment).
const TWIN_DISCIPLINES_SPELLS: [&str; 3] = ["shadow_word_pain", "devouring_plague", "penance"];
const INNER_FOCUS: &str = "inner_focus";
const INNER_FOCUS_COOLDOWN_MS: i64 = 180_000;
const INNER_FOCUS_DURATION_MS: i64 = 10_000;
const INNER_FOCUS_CRIT_BONUS: f64 = 0.25;

/// A fixed fraction of this build's own full fight duration, captured once at setup, since
/// `Fight` itself only exposes the *current* remaining time, not the original total - see
/// `smite_mana_pacing_ok`'s own doc comment for why this is needed.
pub struct Setup {
    total_duration_ms: i64,
}

fn power_in_light_bonus(talents: &SmiteTalents) -> f64 {
    if talents.power_in_light {
        0.10
    } else {
        0.0
    }
}

/// The real priority list's own mana-pacing condition on the full-rank Smite:
/// `currentManaPercent >= remainingTimePercent * 0.9` - only spend the expensive rank while
/// there's still enough mana to sustain it for the rest of the fight, within a 10% margin.
fn smite_mana_pacing_ok(fight: &Fight, setup: &Setup) -> bool {
    let remaining_time_percent = (fight.end_ms - fight.now_ms) as f64 / setup.total_duration_ms as f64;
    fight.mana_percent() >= remaining_time_percent * 0.9
}

/// Applies this build's static talent modifiers once, before the fight starts.
pub fn apply_talents(fight: &mut Fight, talents: &SmiteTalents) {
    let bonus = power_in_light_bonus(talents);
    if bonus > 0.0 {
        let m = &mut fight.mods;
        for key in HOLY_SPELLS {
            m.direct_pct.push((key.to_string(), bonus));
            m.dot_pct.push((key.to_string(), bonus));
        }
    }
    if talents.twin_disciplines > 0 {
        let pct = 0.01 * talents.twin_disciplines as f64;
        let m = &mut fight.mods;
        for key in TWIN_DISCIPLINES_SPELLS {
            m.direct_pct.push((key.to_string(), pct));
            m.dot_pct.push((key.to_string(), pct));
        }
    }
    if talents.spiritual_guidance > 0 {
        let pct = SPIRITUAL_GUIDANCE_DAMAGE_PCT[(talents.spiritual_guidance - 1) as usize];
        fight.stats.spell_power += fight.stats.spirit * pct;
    }
}

fn log_result(fight: &mut Fight, ability: &str, result: CastResult) {
    let line = match result {
        CastResult::Landed { crit: true, amount } => format!("{ability} Crit for {amount:.1}"),
        CastResult::Landed { crit: false, amount } => format!("{ability} Hit for {amount:.1}"),
        CastResult::Missed => format!("{ability} Miss"),
        CastResult::CouldNotCast => format!("{ability} CouldNotCast"),
    };
    fight.log_event(&line);
}

const HOLY_FIRE_REFRESH_THRESHOLD_MS: i64 = 3_000;

fn holy_fire_needs_refresh(fight: &Fight) -> bool {
    fight.dot_remaining_ms("holy_fire").map_or(true, |remaining| remaining < HOLY_FIRE_REFRESH_THRESHOLD_MS)
}

pub fn step(fight: &mut Fight, talents: &SmiteTalents, setup: &Setup) {
    let mult = BASE_CRIT_MULTIPLIER;
    let crit = fight.stats.spell_crit;

    // Two real, one-time mana items - see `priest/shadow.rs`'s own matching block for the full
    // citation (same build package, confirmed present in this spec's own real log too).
    fight.maybe_use_one_time_mana_item("mana_potion_13444", 1.0, 1800.0);
    fight.maybe_use_one_time_mana_item("conjured_mana_12662", 0.80, 1200.0);

    // Dark Sacrifice (the Undead racial - see `priest/shadow.rs`'s own matching block for the
    // full real-source citation): `1600 + Spirit` mana, once per fight, the moment the full
    // amount would fit without overflow.
    fight.maybe_use_headroom_gated_mana_cooldown("dark_sacrifice_1277328", 1600.0 + fight.stats.spirit);

    // Inner Focus, spent immediately on a Holy Fire refresh - falls through to a plain refresh
    // if Inner Focus isn't off cooldown (see module docs).
    if holy_fire_needs_refresh(fight) {
        if talents.inner_focus && !fight.is_on_cooldown(INNER_FOCUS) && !fight.is_on_cooldown("holy_fire") && cast_utility(fight, "inner_focus") {
            fight.set_cooldown(INNER_FOCUS, INNER_FOCUS_COOLDOWN_MS);
            fight.set_aura(INNER_FOCUS, INNER_FOCUS_DURATION_MS);
            fight.log_event("inner_focus Cast");
            return step(fight, talents, setup);
        }
        if !fight.is_on_cooldown("holy_fire") {
            let inner_focus_active = fight.aura_active(INNER_FOCUS);
            let hf_crit = crit + if inner_focus_active { INNER_FOCUS_CRIT_BONUS } else { 0.0 };
            let cost_pct = if inner_focus_active { -1.0 } else { 0.0 };
            if let Some(result) = cast_direct_with_cost_pct(fight, "holy_fire", "holy_fire", hf_crit, mult, 0.0, cost_pct) {
                if matches!(result, CastResult::Landed { .. }) {
                    fight.apply_dot_unconditional("holy_fire", "holy_fire", hf_crit, mult, 0.0);
                }
                log_result(fight, "holy_fire", result);
                if inner_focus_active {
                    fight.clear_aura(INNER_FOCUS);
                }
                return;
            }
        }
    }

    // Penance - unconditional whenever off cooldown, the real priority list's own default.
    if !fight.is_on_cooldown("penance") {
        let cast_started_at = fight.now_ms;
        let landed = cast_dot_only(fight, "penance", "penance", crit, mult, 0.0);
        if landed {
            fight.set_cooldown_from("penance", cast_started_at, 12_000);
        }
        fight.log_event(if landed { "penance Cast (channel, per-bolt damage logged separately)" } else { "penance Miss" });
        if landed {
            return;
        }
    }

    // Shadow Word: Pain maintenance - no remaining-fight-time gate on this item (unlike Shadow
    // Priest's own copy of the same spell).
    if fight.dot_remaining_ms("shadow_word_pain").is_none() {
        let landed = cast_dot_only(fight, "shadow_word_pain", "shadow_word_pain", crit, mult, 0.0);
        fight.log_event(if landed { "shadow_word_pain Cast" } else { "shadow_word_pain Miss" });
        return;
    }

    // Devouring Plague maintenance - same 12s remaining-time gate as Shadow Priest's own copy.
    if !fight.is_on_cooldown("devouring_plague")
        && fight.dot_remaining_ms("devouring_plague").is_none()
        && fight.end_ms - fight.now_ms >= 12_000
    {
        let cast_started_at = fight.now_ms;
        let landed = cast_dot_only(fight, "devouring_plague", "devouring_plague", crit, mult, 0.0);
        if landed {
            fight.set_cooldown_from("devouring_plague", cast_started_at, 24_000);
        }
        fight.log_event(if landed { "devouring_plague Cast" } else { "devouring_plague Miss" });
        return;
    }

    // Smite - full rank while mana pacing allows it, the cheap downrank otherwise.
    if smite_mana_pacing_ok(fight, setup) {
        if let Some(result) = cast_direct_with_cost_pct(fight, "smite", "smite", crit, mult, 0.0, 0.0) {
            log_result(fight, "smite", result);
            return;
        }
    }
    if let Some(result) = cast_direct_with_cost_pct(fight, "smite_downrank", "smite", crit, mult, 0.0, 0.0) {
        log_result(fight, "smite_downrank", result);
    }
}

pub fn run(stats: crate::stats::CasterStats, duration_ms: i64, seed: u64, talents: &SmiteTalents) -> crate::sim::FightReport {
    run_inner(stats, duration_ms, seed, talents, false).0
}

/// Same as `run`, but also returns a human-readable, timestamped per-decision log - see
/// `mage/fire.rs::run_with_debug_log`'s own doc comment for the full citation.
pub fn run_with_debug_log(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &SmiteTalents,
) -> (crate::sim::FightReport, Vec<String>) {
    run_inner(stats, duration_ms, seed, talents, true)
}

fn run_inner(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &SmiteTalents,
    debug_log: bool,
) -> (crate::sim::FightReport, Vec<String>) {
    let pack = crate::spelldata::embedded_priest_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    if debug_log {
        fight.enable_debug_log();
    }
    super::apply_standing_debuffs(&mut fight);
    apply_talents(&mut fight, talents);
    let setup = Setup { total_duration_ms: duration_ms };
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents, &setup);
        if fight.now_ms == before {
            fight.advance(1000);
        }
    }
    let log = fight.debug_log.take().unwrap_or_default();
    (fight.report, log)
}
