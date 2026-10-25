//! Generic cast-resolution helpers shared by every class's spec modules - mana spend, cast
//! time/GCD advance, cooldown bookkeeping, and the direct/dot/utility cast shapes built on top
//! of `sim.rs`'s lower-level `resolve_direct`/`apply_dot`/etc. None of this is Warlock-specific
//! (it operates purely on `Fight`/`DataPack`/`CastResult`); extracted out of `warlock/mod.rs`
//! into its own module (2026-10-25, beginning Mage support) once a second class needed the
//! exact same mechanisms - this project's own stated style is "duplicate small things, share
//! real mechanisms," and mana/cast-time/cooldown handling is a real mechanism, not a small
//! thing. `warlock/mod.rs` re-exports these so every existing Warlock spec file keeps working
//! unchanged.

use crate::sim::{CastResult, Fight};

/// The spell crit multiplier every direct/periodic hit starts from before talents add their
/// own flat bonus on top via `Fight::mods.crit_multiplier_bonus`. Standard WoW spell crit bonus
/// of this era: +50% damage - not Warlock-specific, every caster class uses the same value.
pub const BASE_CRIT_MULTIPLIER: f64 = 1.5;

pub(crate) fn pay_and_advance(fight: &mut Fight, spell_key: &str, instant: bool) -> bool {
    pay_and_advance_with_cost_pct(fight, spell_key, instant, 0.0)
}

/// Same as `pay_and_advance`, but `cost_pct` (e.g. Cataclysm's -3/-6/-10%) scales the spell's
/// mana cost before it's spent - an additive percentage, so -0.10 means 90% of normal cost.
pub(crate) fn pay_and_advance_with_cost_pct(
    fight: &mut Fight,
    spell_key: &str,
    instant: bool,
    cost_pct: f64,
) -> bool {
    let row = fight.pack.spell(spell_key).clone();
    let mana_cost = row.mana_cost.unwrap_or(0.0) * (1.0 + cost_pct);
    if !fight.spend_mana(mana_cost) {
        return false;
    }
    let cast_ms = if instant {
        0
    } else {
        fight.modified_cast_ms(spell_key, row.cast_time_ms.unwrap_or(0))
    };
    let gcd_ms = fight.haste_scale(row.gcd_ms.unwrap_or(1500)).max(1000);
    fight.advance(cast_ms.max(gcd_ms));
    true
}

/// A direct-damage spell with no dot. Checks the ability's own cooldown (if the data pack has
/// one, or if a talent-driven cooldown was set separately under `ability_key`), pays mana,
/// advances the clock through the cast, then resolves the hit. `extra_pct` is a live, per-cast
/// damage bonus (a proc-window buff's current state - see `sim.rs::resolve_direct`'s docs);
/// pass 0.0 when there isn't one. Returns `None` if the cast couldn't happen at all
/// (cooldown/mana), `Some` with the resolved hit (landed/crit/missed) otherwise, so callers can
/// react to a crit or a landed hit.
pub fn cast_direct(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
) -> Option<CastResult> {
    cast_direct_full(fight, spell_key, ability_key, crit_chance, crit_multiplier, extra_pct, false, 0.0)
}

/// Same as `cast_direct`, but if `instant` is true the cast time is skipped (only the GCD
/// applies) - e.g. a proc that makes the next cast instant.
pub fn cast_direct_maybe_instant(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
    instant: bool,
) -> Option<CastResult> {
    cast_direct_full(fight, spell_key, ability_key, crit_chance, crit_multiplier, extra_pct, instant, 0.0)
}

/// Same as `cast_direct`, but `cost_pct` (e.g. Cataclysm's -3/-6/-10%) scales the mana cost -
/// see `pay_and_advance_with_cost_pct`.
pub fn cast_direct_with_cost_pct(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
    cost_pct: f64,
) -> Option<CastResult> {
    cast_direct_full(fight, spell_key, ability_key, crit_chance, crit_multiplier, extra_pct, false, cost_pct)
}

fn cast_direct_full(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
    instant: bool,
    cost_pct: f64,
) -> Option<CastResult> {
    if fight.is_on_cooldown(ability_key) {
        return None;
    }
    let cooldown_ms = fight.pack.spell(spell_key).cooldown_ms;
    if !pay_and_advance_with_cost_pct(fight, spell_key, instant, cost_pct) {
        return None;
    }
    if let Some(cd) = cooldown_ms {
        fight.set_cooldown(ability_key, cd);
    }
    Some(fight.resolve_direct(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct))
}

/// Same as `cast_direct_with_cost_pct`, but with an extra flat spell-power bonus added on top
/// of the caster's own effective spell power before the cast resolves - for a buff that adds
/// raw spell power dynamically rather than a static `CasterStats` field (Arcane Blast's own
/// stacking buff: 15% of Intellect per stack as spell damage - see `mage/arcane.rs` and
/// `sim.rs::resolve_direct_with_bonus_power`'s own doc comment).
pub fn cast_direct_with_bonus_power(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
    cost_pct: f64,
    bonus_power: f64,
) -> Option<CastResult> {
    if fight.is_on_cooldown(ability_key) {
        return None;
    }
    let cooldown_ms = fight.pack.spell(spell_key).cooldown_ms;
    if !pay_and_advance_with_cost_pct(fight, spell_key, false, cost_pct) {
        return None;
    }
    if let Some(cd) = cooldown_ms {
        fight.set_cooldown(ability_key, cd);
    }
    Some(fight.resolve_direct_with_bonus_power(
        spell_key,
        ability_key,
        crit_chance,
        crit_multiplier,
        extra_pct,
        bonus_power,
    ))
}

/// A pure dot application: no direct hit, just the cast and the snapshot. `extra_pct` is
/// snapshotted into the dot's ticks, Classic-style (see `sim.rs::apply_dot`'s docs) - pass 0.0
/// when there isn't a live proc-window bonus active.
pub fn cast_dot_only(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
) -> bool {
    if !pay_and_advance(fight, spell_key, false) {
        return false;
    }
    fight.apply_dot(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct);
    true
}

/// A direct hit and its own dot from the same cast (Immolate, Fireball). `cost_pct` as in
/// `cast_direct_with_cost_pct`. One hit roll covers both halves (`resolve_direct`'s) - the dot
/// only applies when that roll landed, via `apply_dot_unconditional` rather than `apply_dot`'s
/// own separate roll, matching how a single real cast either lands as a whole or misses as a
/// whole.
pub fn cast_direct_and_dot(
    fight: &mut Fight,
    spell_key: &str,
    ability_key: &str,
    crit_chance: f64,
    crit_multiplier: f64,
    extra_pct: f64,
    cost_pct: f64,
) -> bool {
    if !pay_and_advance_with_cost_pct(fight, spell_key, false, cost_pct) {
        return false;
    }
    let result = fight.resolve_direct(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct);
    if matches!(result, CastResult::Landed { .. }) {
        fight.apply_dot_unconditional(spell_key, ability_key, crit_chance, crit_multiplier, extra_pct);
    }
    true
}

/// A no-damage utility cast (Life Tap, Evocation, Curse of the Elements): pays the mana and
/// GCD/cast time but has no hit roll of its own. Returns whether it was actually cast so the
/// caller can apply its effect (mana gain, aura, etc.) only on success.
pub fn cast_utility(fight: &mut Fight, spell_key: &str) -> bool {
    pay_and_advance(fight, spell_key, false)
}
