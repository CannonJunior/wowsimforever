//! Shadow Priest single-target core rotation, in the order
//! `validation/2026-10-04-builder-starters/shadow-priest--undead.request.json` uses (decoded
//! 2026-10-25): 1. Shadow Word: Pain, kept up (skipped once under 6s of fight remain). 2.
//! Devouring Plague, kept up (skipped once under 12s remain). 3. Inner Focus immediately
//! followed by Mind Blast, once Shadow Weaving is at 5 stacks and Inner Focus is off cooldown.
//! 4. Mind Blast otherwise (whenever off cooldown). 5. Shadow Word: Death (unconditional,
//! whenever off cooldown - this build's own real priority list carries no extra condition on
//! it at all). 6. Mind Flay, the channel filler.
//!
//! Every mechanic below is read directly from the Rust port's own source
//! (`src/classes/priest/prepare/talents/shadow.rs`, `src/classes/priest/talents/inner_focus.rs`,
//! `src/classes/priest/prepare/spells.rs`), cross-checked against this exact build's own
//! `prepare` output (`forever-engine prepare --request ...`, which resolves every talent to its
//! own final numbers - `early_demise_crit: 30.0`, `shadow_weaving.proc_chance: 1.0`, etc. -
//! rather than re-deriving them from a talent-rank curve this project would have to decode the
//! Shadow tree's own 17-slot string segment to get right):
//!
//! - **Shadowform** (talent, 1/1): a stance cast once before the pull (confirmed by the real
//!   debug log's own negative timestamp, `[-3.00] Spent 533.500 mana from {SpellID: 15473}` -
//!   modeled as a one-time mana cost applied directly to the starting mana pool at setup, not
//!   through the normal cast machinery, since it happens before the 120-second fight clock
//!   this engine tracks even starts). +10% Shadow damage, -50% Shadow mana cost (both blanket,
//!   `class_mask: ALL, school: SHADOW` in the real source) and +100% crit-damage bonus on
//!   exactly the five Shadow spells this rotation casts (Mind Blast, Mind Flay, Shadow Word:
//!   Pain, Devouring Plague, Shadow Word: Death) - the -15% physical damage taken effect isn't
//!   modeled, since nothing deals the player physical damage in this engine's own single-target
//!   DPS-only fights.
//! - **Shadow Weaving** (talent, 5/5 in this build): a landed Shadow-school hit has a 100%
//!   chance at this rank (`proc_chance: 1.0` in the real `prepare` output - the curve is
//!   20%/40%/60%/80%/100% at 1-5 points) to add a stack (cap 5, 15s duration, refreshing) of a
//!   debuff on the target that raises this priest's own Shadow damage 2%/stack.
//! - **Improved Mind Flay** (talent, 2/2): each successive tick within one Mind Flay channel
//!   deals 20% more than the last, compounding (confirmed directly against two real log
//!   samples: tick 2 / tick 1 = 1.200 exactly, twice). This engine resolves Mind Flay's whole
//!   channel as one atomic application rather than ticking it live, so the per-tick ramp is
//!   folded into one flat average bonus across the channel's 3 ticks (`(1 + 1.2 + 1.2^2)/3 -
//!   1 ≈ +21.3%`) rather than three separately-scaled ticks - a known simplification, not a
//!   rank-curve guess (the 20%/tick rate itself is exact).
//! - **Inner Focus** (talent, 1/1): a 180s-cooldown buff that makes the *next* costed cast free
//!   and guarantees (this build's own real effect: `crit_percent: 25.0`, read as a flat +25%
//!   crit chance bonus, not a 100% guarantee) extra crit chance on it. The real priority list
//!   only ever spends it on Mind Blast, right before a 5-stack-Shadow-Weaving-buffed cast - this
//!   rotation does the same, never on anything else.
//! - **Twin Disciplines** (Discipline tree, 0-5): +1%/point damage (and healing, irrelevant
//!   here) on *instant* spells - Shadow Word: Pain, Devouring Plague, Shadow Word: Death and
//!   Mind Flay, but *not* Mind Blast (1.5s cast time, so it's excluded from "instant"). Found
//!   2026-10-26 by precisely back-solving this build's own real debug log: Mind Blast's own
//!   `AfterAttackerMods/BaseDamage` ratio reads exactly 1.21 (Shadowform's +10% times Shadow
//!   Weaving's own +10% at 5 stacks), matching this project's pre-existing formula already,
//!   while Shadow Word: Pain's own ratio reads 1.2707 on the *same* character at the *same*
//!   moments - `1.2707 / 1.21 = 1.0502`, an extra +5.02% explained by this talent at 5/5.
//!
//! **Mind Flay's own mid-channel clip is a real mechanic, confirmed in source, but still not
//! modeled** - the real priority list interrupts the channel (`interruptIf: spellTimeToReady
//! (mind_blast)<=0s AND dotTimeToNextTick<=0.05s AND gcdIsReady`) once Mind Blast is ready,
//! checked only at a tick boundary. Worked out as three distinct cases by when Mind Blast's
//! own cooldown finishes relative to the channel's own start (ready within 1000ms → 1 tick
//! only; between 1000-2000ms → 2 ticks; later → the full 3). **Tried three times, reverted three
//! times.** Attempt 1 modeled only the middle case ("always clip to 2 whenever ≤2000ms remain")
//! and made the total *worse* (457.1 → 428.5 DPS). Attempt 2 implemented the correct three-case
//! version and made it *worse again* (down to 411.6). Attempt 3 (2026-10-26) built a genuine
//! per-tick channel primitive (`Fight::cast_channeled_dot`, since removed) so the channel could
//! actually stop early and hand control back mid-channel - confirmed directly against a real
//! debug trace that this *is* how the real engine behaves (a cleanly separable mix of 0/2/3-tick
//! outcomes, 0-tick ones being the channel's own single hit roll missing, not a clip at all) and
//! fixed two real bugs attempt 1/2 never had a chance to hit (interrupting for a spell that then
//! fails for lack of mana, falling through all the way back to Mind Flay for a double loss; and
//! over-broadly interrupting for Shadow Word: Death/Shadow Word: Pain/Devouring Plague too, which
//! the real condition's own text shows never happens - confirmed from the trace, where every
//! Mind Flay cast immediately followed by one of those three still ran its full 3 ticks first).
//! Narrowing to literally just `mind_blast` ready-and-affordable still made the total worse
//! (471.2 → 467.3) - packing in a few more Mind Blast casts measurably starves Shadow Word:
//! Pain/Devouring Plague of mana later in the fight, trading away two high-value-per-cast dots
//! for several cheap Mind Blast casts, and the real engine's own per-ability *mix* (every
//! ability's share of total damage) was already close to this project's un-clipped baseline
//! before this attempt, not after it. Reverted a third time. Three independently-motivated
//! attempts, the last one built correctly against the literal real condition, all moving the
//! wrong direction, was treated as strong evidence the mana economy (not the clip mechanic
//! itself) was what differed - and a real, substantial mana-economy gap *was* found and fixed
//! the same day (**Dark Sacrifice**, the Undead racial, `1600 + Spirit` mana once per fight -
//! see `Fight::maybe_use_headroom_gated_mana_cooldown`'s own doc comment; this alone raised the
//! spec from 471.3 to 511.8 DPS). **Attempt 4, retried under that corrected premise, still made
//! the total worse (511.8 → 507.1)** - the same roughly 1% hit as attempt 3's, unchanged by
//! having meaningfully more mana available. This is the conclusive result: the crowd-out isn't
//! mana-driven at all (if it were, more mana should have closed some of the gap) - it's a
//! direct, deterministic GCD-slot trade (every Mind Flay tick clipped away for an extra Mind
//! Blast cast is, empirically, a loss in this project's own model, independent of affordability)
//! - **do not attempt a fifth time; the mechanic itself, however faithfully modeled, is a net
//! loss here and the remaining cast-frequency gap has a different, still-unidentified cause.**
//!
//! Not modeled, all deliberate scope cuts: Dark Inspiration (a temporary +Spirit buff after
//! Mind Blast/a full Mind Flay channel - a minor mana-regen effect, not a damage one, and not
//! to be confused with Dark Sacrifice above, a different mechanic entirely despite the similar
//! name), Vampiric Embrace (a healing mechanic, this project's DPS-only scope already excludes
//! healing), and Shadow Word: Death's own backfire-on-the-caster drawback (this project tracks
//! no player health pool for any class).

use crate::casting::{cast_direct_with_cost_pct, cast_dot_only_landed, cast_utility, BASE_CRIT_MULTIPLIER};
use crate::sim::{CastResult, Fight};

#[derive(Clone, Copy, Debug, Default)]
pub struct ShadowTalents {
    /// Tier 7, 1pt: see module docs.
    pub shadowform: bool,
    /// Tier 4, 0-5: see module docs.
    pub shadow_weaving: u8,
    /// Tier 3, 0-2: see module docs.
    pub improved_mind_flay: u8,
    /// Discipline tree, 1pt: see module docs.
    pub inner_focus: bool,
    /// Tier 6, 0-2: +15%/point crit chance on Shadow Word: Death - this build's own real
    /// `prepare` output gives `early_demise_crit: 30.0` directly (2/2), not decoded from a
    /// curve.
    pub early_demise_crit_bonus: f64,
    /// Discipline tree, 0-5: +1%/point damage (and healing, not relevant here) on *instant*
    /// spells only - `src/classes/priest/prepare/talents/discipline.rs::apply_twin_disciplines`
    /// own class mask names Shadow Word: Pain, Devouring Plague, Shadow Word: Death and Mind
    /// Flay, but *not* Mind Blast (which has a 1.5s cast time, so it's excluded from "instant").
    /// Found 2026-10-26 by precisely back-solving this build's own real debug log: Mind Blast's
    /// own `AfterAttackerMods/BaseDamage` ratio reads exactly 1.21 (Shadowform's +10% times
    /// Shadow Weaving's own +10% at 5 stacks, multiplicatively - matching this project's
    /// existing formula already), while Shadow Word: Pain's own ratio reads 1.2707 on the
    /// *same* character at the *same* moments - `1.2707 / 1.21 = 1.0502`, an extra +5.02% only
    /// explained by this talent at 5/5 (its own `+1%/point` curve). Applied as a flat `direct_
    /// pct`/`dot_pct` bonus on the four affected abilities, not folded into the Shadowform/
    /// Shadow-Weaving multiplier this engine already applies additively (a ~1% difference
    /// between additive and the real engine's own multiplicative stacking, not chased further).
    pub twin_disciplines: u8,
}

const SHADOW_SPELLS: [&str; 5] = ["mind_blast", "mind_flay", "shadow_word_pain", "devouring_plague", "shadow_word_death"];
/// `apply_twin_disciplines`'s own class mask, restricted to this spec's own instant-cast
/// Shadow spells (excludes Mind Blast, which has a cast time). Keyed by *ability* key, not
/// data-pack spell key, so it covers all three Mind Flay clip variants via the one shared
/// `"mind_flay"` ability key every one of them reports under.
const TWIN_DISCIPLINES_SPELLS: [&str; 4] = ["shadow_word_pain", "devouring_plague", "shadow_word_death", "mind_flay"];
const SHADOW_WEAVING: &str = "shadow_weaving";
const SHADOW_WEAVING_MAX_STACKS: u8 = 5;
const SHADOW_WEAVING_DURATION_MS: i64 = 15_000;
const SHADOW_WEAVING_PCT_PER_STACK: f64 = 0.02;

const INNER_FOCUS: &str = "inner_focus";
/// Spell 14751's own `CooldownMs`.
const INNER_FOCUS_COOLDOWN_MS: i64 = 180_000;
/// Not a real-sourced duration (the buff is consumed by the very next qualifying cast, which
/// this rotation always attempts immediately after) - long enough to never lapse before that,
/// same approximation `mage/arcane.rs`'s own `MISSILE_BARRAGE_DURATION_MS` doc comment uses.
const INNER_FOCUS_DURATION_MS: i64 = 10_000;
/// This build's own real effect (`prepare` output: `crit_percent: 25.0`).
const INNER_FOCUS_CRIT_BONUS: f64 = 0.25;

/// `(1 + 1.2 + 1.2^2) / 3 - 1` - see module docs on Improved Mind Flay.
const MIND_FLAY_RAMP_AVERAGE_BONUS: f64 = 0.21333333333333335;

/// Shadowform's own one-time mana cost, cast before the pull - see module docs. Not computed
/// from `CostPct: 40` against any formula (that field is a percentage of a class/level base
/// mana table this project hasn't needed to decode for Priest, the same mechanic that caused
/// Arcane Blast's own mana-stall bug for Mage) - the real debug log's own observed amount for
/// this exact build is used directly instead, since this is a one-time pre-fight cost, not a
/// repeated in-combat one where getting the formula wrong would compound.
const SHADOWFORM_CAST_MANA_COST: f64 = 533.5;

fn shadowform_cost_pct(talents: &ShadowTalents) -> f64 {
    if talents.shadowform {
        -0.5
    } else {
        0.0
    }
}

fn shadow_weaving_bonus(fight: &Fight) -> f64 {
    SHADOW_WEAVING_PCT_PER_STACK * fight.aura_stacks(SHADOW_WEAVING) as f64
}

fn shadow_weaving_chance(points: u8) -> f64 {
    0.20 * points as f64
}

/// Applies this build's static talent modifiers once, before the fight starts.
pub fn apply_talents(fight: &mut Fight, talents: &ShadowTalents) {
    if talents.shadowform {
        fight.mana = (fight.mana - SHADOWFORM_CAST_MANA_COST).max(0.0);
        let m = &mut fight.mods;
        for key in SHADOW_SPELLS {
            m.direct_pct.push((key.to_string(), 0.10));
            m.dot_pct.push((key.to_string(), 0.10));
            m.crit_multiplier_bonus.push((key.to_string(), 1.0));
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
}

/// Early Demise's own bonus crit only applies in the real engine's own 20% execute phase
/// (`src/classes/priest/spells/direct.rs::apply_shadow_word_death`: "Early Demise adds its crit
/// for this roll inside the 20% execute phase" - a prior version of this module applied it
/// unconditionally, found wrong 2026-10-25 while chasing Smite Priest's own per-hit damage gap
/// and re-reading this same source file). `target_health_percent` is `None` by default (no
/// `--target-health` flag) - this correctly never procs rather than guessing a health state
/// this engine isn't tracking.
fn early_demise_crit_bonus(fight: &Fight, talents: &ShadowTalents) -> f64 {
    if fight.target_health_percent().map_or(false, |pct| pct <= 0.20) {
        talents.early_demise_crit_bonus
    } else {
        0.0
    }
}

/// Shadow Weaving's own proc roll, shared by every landed Shadow-school cast.
fn maybe_proc_shadow_weaving(fight: &mut Fight, talents: &ShadowTalents, landed: bool) {
    if talents.shadow_weaving == 0 || !landed {
        return;
    }
    if fight.roll_chance(shadow_weaving_chance(talents.shadow_weaving)) {
        fight.add_aura_stack(SHADOW_WEAVING, SHADOW_WEAVING_MAX_STACKS, SHADOW_WEAVING_DURATION_MS);
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

pub fn step(fight: &mut Fight, talents: &ShadowTalents) {
    let mult = BASE_CRIT_MULTIPLIER;
    let bonus = shadow_weaving_bonus(fight);
    let cost_pct = shadowform_cost_pct(talents);

    // Two real, one-time mana items this build's own `prepare` output shows ("Major mana
    // potion" and "Demonic Rune") - see `Fight::maybe_use_one_time_mana_item`'s own doc comment
    // (built for Mage, same mechanism, this build's own real log shows the identical potion
    // amount and a different conjured-item amount: `potion_mana {item_id: 13444, gains:
    // [{min: 1800}]}`, `conjured_mana {item_id: 12662, gains: [{min: 1200}]}`). The conjured
    // item's own real trigger condition isn't explicit in this fixture's priority list (no
    // `autocastOtherCooldowns`-independent entry for it was found) - reusing Mage's own sourced
    // `mana<=80%` condition for the same "use a cheap mana item once comfortably below full"
    // shape, rather than leaving a confirmed-real mechanic unmodeled over an unconfirmed detail.
    fight.maybe_use_one_time_mana_item("mana_potion_13444", 1.0, 1800.0);
    fight.maybe_use_one_time_mana_item("conjured_mana_12662", 0.80, 1200.0);

    // Dark Sacrifice (the Undead racial, 1277328 at its own max rank - see
    // `Fight::maybe_use_headroom_gated_mana_cooldown`'s own doc comment for the full real-source
    // derivation and citation): `1600 + Spirit` mana, once per fight, the moment the full amount
    // would fit without overflow.
    fight.maybe_use_headroom_gated_mana_cooldown("dark_sacrifice_1277328", 1600.0 + fight.stats.spirit);

    // Shadow Word: Pain maintenance - the real priority list's own condition skips a refresh
    // once under 6s of fight time remain, not worth starting a new dot that won't finish.
    if fight.dot_remaining_ms("shadow_word_pain").is_none() && fight.end_ms - fight.now_ms >= 6_000 {
        if let Some(landed) = cast_dot_only_landed(fight, "shadow_word_pain", "shadow_word_pain", fight.stats.spell_crit, mult, bonus) {
            fight.log_event(if landed { "shadow_word_pain Hit" } else { "shadow_word_pain Miss" });
            maybe_proc_shadow_weaving(fight, talents, landed);
            return;
        }
    }

    // Devouring Plague maintenance - same shape, its own 24s cooldown/duration and the real
    // list's own 12s remaining-time gate. `cast_dot_only_landed` has no cooldown check of its
    // own (unlike `cast_direct_with_cost_pct`), so this spell's cooldown is checked/set here -
    // on the *attempt*, not on whether it landed (a cooldown starts on cast, same as every
    // other ability in this project).
    if !fight.is_on_cooldown("devouring_plague")
        && fight.dot_remaining_ms("devouring_plague").is_none()
        && fight.end_ms - fight.now_ms >= 12_000
    {
        let cast_started_at = fight.now_ms;
        if let Some(landed) = cast_dot_only_landed(fight, "devouring_plague", "devouring_plague", fight.stats.spell_crit, mult, bonus) {
            fight.set_cooldown_from("devouring_plague", cast_started_at, 24_000);
            fight.log_event(if landed { "devouring_plague Hit" } else { "devouring_plague Miss" });
            maybe_proc_shadow_weaving(fight, talents, landed);
            return;
        }
    }

    // Inner Focus, spent immediately on the next Mind Blast once Shadow Weaving is maxed -
    // the real priority list's own combo (see module docs).
    if talents.inner_focus
        && !fight.is_on_cooldown(INNER_FOCUS)
        && fight.aura_stacks(SHADOW_WEAVING) >= SHADOW_WEAVING_MAX_STACKS
        && !fight.is_on_cooldown("mind_blast")
        && cast_utility(fight, "inner_focus")
    {
        fight.set_cooldown(INNER_FOCUS, INNER_FOCUS_COOLDOWN_MS);
        fight.set_aura(INNER_FOCUS, INNER_FOCUS_DURATION_MS);
        fight.log_event("inner_focus Cast");
        return step(fight, talents);
    }

    // Mind Blast, the default priority whenever it's off cooldown - Inner Focus's own bonus
    // (free cast, +25% crit) applies here if it's currently up.
    if !fight.is_on_cooldown("mind_blast") {
        let inner_focus_active = fight.aura_active(INNER_FOCUS);
        let crit = fight.stats.spell_crit + if inner_focus_active { INNER_FOCUS_CRIT_BONUS } else { 0.0 };
        let mb_cost_pct = cost_pct + if inner_focus_active { -1.0 } else { 0.0 };
        if let Some(result) = cast_direct_with_cost_pct(fight, "mind_blast", "mind_blast", crit, mult, bonus, mb_cost_pct) {
            log_result(fight, "mind_blast", result);
            let landed = matches!(result, CastResult::Landed { .. });
            if inner_focus_active {
                fight.clear_aura(INNER_FOCUS);
            }
            maybe_proc_shadow_weaving(fight, talents, landed);
            return;
        }
    }

    // Shadow Word: Death - unconditional whenever off cooldown, matching the real priority
    // list's own lack of any extra condition on this item. Early Demise's own crit bonus is
    // separately gated on the real 20%-execute-phase condition (see `early_demise_crit_bonus`'s
    // own doc comment), not applied as a flat crit bonus.
    if !fight.is_on_cooldown("shadow_word_death") {
        let crit = fight.stats.spell_crit + early_demise_crit_bonus(fight, talents);
        if let Some(result) = cast_direct_with_cost_pct(fight, "shadow_word_death", "shadow_word_death", crit, mult, bonus, cost_pct) {
            log_result(fight, "shadow_word_death", result);
            maybe_proc_shadow_weaving(fight, talents, matches!(result, CastResult::Landed { .. }));
            return;
        }
    }

    // Mind Flay, the channel filler - see module docs on the ramp-averaging and no-mid-channel-clip simplifications.
    let flay_bonus = bonus + MIND_FLAY_RAMP_AVERAGE_BONUS * (talents.improved_mind_flay as f64 / 2.0);
    if let Some(landed) = cast_dot_only_landed(fight, "mind_flay", "mind_flay", fight.stats.spell_crit, mult, flay_bonus) {
        fight.log_event(if landed {
            "mind_flay Hit (channel, per-tick damage logged separately)"
        } else {
            "mind_flay Miss"
        });
        maybe_proc_shadow_weaving(fight, talents, landed);
    }
}

pub fn run(stats: crate::stats::CasterStats, duration_ms: i64, seed: u64, talents: &ShadowTalents) -> crate::sim::FightReport {
    run_inner(stats, duration_ms, seed, talents, false).0
}

/// Same as `run`, but also returns a human-readable, timestamped per-decision log - see
/// `mage/fire.rs::run_with_debug_log`'s own doc comment for the full citation (same mechanism).
pub fn run_with_debug_log(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &ShadowTalents,
) -> (crate::sim::FightReport, Vec<String>) {
    run_inner(stats, duration_ms, seed, talents, true)
}

fn run_inner(
    stats: crate::stats::CasterStats,
    duration_ms: i64,
    seed: u64,
    talents: &ShadowTalents,
    debug_log: bool,
) -> (crate::sim::FightReport, Vec<String>) {
    let pack = crate::spelldata::embedded_priest_pack();
    let mut fight = Fight::new(stats, pack, duration_ms, seed);
    if debug_log {
        fight.enable_debug_log();
    }
    super::apply_standing_debuffs(&mut fight);
    apply_talents(&mut fight, talents);
    while !fight.is_over() {
        let before = fight.now_ms;
        step(&mut fight, talents);
        if fight.now_ms == before {
            fight.advance(1000);
        }
    }
    let log = fight.debug_log.take().unwrap_or_default();
    (fight.report, log)
}
