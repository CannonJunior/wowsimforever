//! Priest spec rotations - the third class (2026-10-25), begun after Warlock and Mage both
//! reached a stable, cross-checked state. Same discipline as `mage/`: each spec module
//! implements the *exact* single-target priority order mined from mythicsim-forever-engine's
//! own validation fixtures, translated from its declarative condition tree into plain Rust
//! `if`/`else`; generic mana/cast-time/cooldown mechanics live in `crate::casting`, shared with
//! every other class rather than duplicated.
//!
//! **Shadow and Smite are modeled.** See each spec's own module docs for exactly what's
//! modeled and what's a deliberate scope cut.

pub mod shadow;
pub mod smite;

/// Seeds the standing-raid-debuff assumptions `CasterStats` carries (Curse of the Elements)
/// onto a freshly-constructed `Fight` - see `mage/mod.rs::apply_standing_debuffs`'s own doc
/// comment for the full reasoning (identical mechanic, duplicated per this project's own
/// "duplicate small things" style rather than hoisted into a shared module for two callers).
/// Both Priest "builder starter" fixtures' own `raid.debuffs` blocks carry `curseOfElements:
/// true` too, confirmed 2026-10-25.
pub(crate) fn apply_standing_debuffs(fight: &mut crate::sim::Fight) {
    if fight.stats.standing_curse_of_the_elements {
        fight.set_aura("curse_of_the_elements", fight.end_ms - fight.now_ms);
    }
}
