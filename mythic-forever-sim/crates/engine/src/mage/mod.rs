//! Mage spec rotations - the second class (2026-10-25), begun after the Warlock's three specs
//! reached a stable, cross-checked state. Same discipline as `warlock/`: each spec module
//! implements the *exact* single-target priority order mined from mythicsim-forever-engine's
//! own validation fixtures, translated from its declarative condition tree into plain Rust
//! `if`/`else`; generic mana/cast-time/cooldown mechanics live in `crate::casting`, shared with
//! Warlock rather than duplicated (see that module's own doc comment for why).
//!
//! **Fire, Arcane and Frost are modeled; Frostfire is not started.** See each spec's own module
//! docs for exactly what's modeled and what's a deliberate scope cut.

pub mod arcane;
pub mod fire;
pub mod frost;

/// Evocation (spell 12051): "While channeling this spell, your mana regeneration is active and
/// increased by 1500%. Lasts 8 sec." - confirmed from foreverdb.net's own tooltip text rather
/// than this engine's three raw `Effects` on that spell row, which this project didn't fully
/// decode in the time available (see the data pack's own `source` field on this entry for the
/// full citation). Modeled the same way Life Tap is for Warlock: a flat formula computed here
/// in Rust, not through the data pack's generic direct/periodic scheme, since "the normal regen
/// rate, 16x, for 8 seconds" isn't a damage effect at all.
///
/// `mp5` is the caster's own already-computed mp5 stat (includes Spirit-derived regen and any
/// flat gear MP5) - this engine doesn't model the 5-second-rule's casting/not-casting split
/// (see the README's "Scope"), so "the normal regen rate" here is simply that same stat,
/// continuously, the same simplification every other mana-regen calculation in this project
/// already makes.
pub fn evocation_mana(mp5: f64) -> f64 {
    const EVOCATION_DURATION_S: f64 = 8.0;
    const EVOCATION_REGEN_MULTIPLIER: f64 = 16.0; // "+1500%" = 1.0 + 15.0
    EVOCATION_DURATION_S * (mp5 / 5.0) * EVOCATION_REGEN_MULTIPLIER
}
