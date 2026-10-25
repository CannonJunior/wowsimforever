//! Caster stat inputs to the simulation. Kept deliberately small: this engine's first
//! milestone is single-target core-rotation DPS, not full gear/itemization modeling (see
//! README "Scope" section). A caller supplies final, already-computed totals (e.g. "542 spell
//! power after gear and buffs") rather than raw gear pieces.

#[derive(Clone, Copy, Debug)]
pub struct CasterStats {
    /// Flat spell power, applied to every school this engine models (Shadow and Fire) - the
    /// universal "SpellDamage" gear stat. `bonus_shadow_power`/`bonus_fire_power` below add
    /// school-specific power on top of this for the one school each applies to (real items
    /// carry both kinds - see `gear.rs`'s module docs for a concrete example build with +60
    /// ShadowDamage split across three items. An earlier version of this engine claimed gear
    /// was "school-agnostic" and didn't have these two fields at all - wrong, fixed in the
    /// fourth research/implementation pass).
    pub spell_power: f64,
    /// School-specific spell power on top of `spell_power`, applied only to Shadow-school
    /// spells (Shadow Bolt, Corruption, Bane of Agony, Siphon Life, Shadowburn).
    pub bonus_shadow_power: f64,
    /// Same as `bonus_shadow_power`, for Fire-school spells (Immolate, Incinerate,
    /// Conflagrate, Searing Pain, Soul Fire).
    pub bonus_fire_power: f64,
    /// Same as `bonus_shadow_power`, for Arcane-school spells (Arcane Blast, Arcane Missiles) -
    /// added 2026-10-25 for Arcane Mage, which is the first spec this project has that actually
    /// carries Arcane-specific gear stats (`ArcaneDamage`, e.g. Wand of Arcane Potency).
    pub bonus_arcane_power: f64,
    /// Same as `bonus_shadow_power`, for Frost-school spells (Frostbolt; Ice Lance has no spell
    /// power coefficient at all, so this never actually applies to it - see `mage/frost.rs`).
    pub bonus_frost_power: f64,
    /// Spell hit chance, 0.0-1.0. Forever's warlock spell hit cap against a raid boss (level
    /// 60/63) is 17% (see mythicsim-forever-engine `prepare/warlock` notes on the hit cap);
    /// this engine does not enforce the cap, it simulates whatever is supplied.
    pub spell_hit: f64,
    /// Spell crit chance, 0.0-1.0, already including base caster crit, Intellect-derived crit
    /// and talent/gear bonuses the caller has computed.
    pub spell_crit: f64,
    /// Haste as a multiplier on cast time and GCD, e.g. 1.10 for 10% haste. 1.0 = no haste.
    pub haste: f64,
    /// Mana regenerated per 5 seconds while not casting. This engine does not model the 5%
    /// "five-second rule" interruption; mp5 is applied continuously for simplicity (see README).
    pub mp5: f64,
    /// Starting mana pool.
    pub max_mana: f64,
    /// Spirit, used only by Life Tap's mana-per-Spirit component.
    pub spirit: f64,
    /// Total Intellect (race + gear + buffs, Arcane Mind's own +%/rank already folded in by
    /// `mage/arcane.rs::apply_talents` - see that module). Added 2026-10-25 for Arcane Mage's
    /// own Arcane Blast buff, which needs the caster's *raw* Intellect (15% of it as spell
    /// damage per stack) - no other mechanic in this project needed Intellect itself rather
    /// than something already derived from it (crit%, mana), which is why this field didn't
    /// exist before.
    pub intellect: f64,
}

impl Default for CasterStats {
    fn default() -> Self {
        CasterStats {
            spell_power: 0.0,
            bonus_shadow_power: 0.0,
            bonus_fire_power: 0.0,
            bonus_arcane_power: 0.0,
            bonus_frost_power: 0.0,
            spell_hit: 0.0,
            spell_crit: 0.0,
            haste: 1.0,
            mp5: 0.0,
            max_mana: 10000.0,
            spirit: 0.0,
            intellect: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum School {
    Shadow,
    Fire,
    Arcane,
    Frost,
}
