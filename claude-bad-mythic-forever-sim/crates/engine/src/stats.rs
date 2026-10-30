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
    /// Same as `bonus_shadow_power`, for Holy-school spells (Smite, Holy Fire, Penance) - added
    /// 2026-10-25 for Smite Priest, the first spec this project has that deals Holy-school
    /// damage at all.
    pub bonus_holy_power: f64,
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
    /// Whether to treat Curse of the Elements (+10% magic damage taken) as a standing debuff
    /// on the target for the whole fight, rather than something this caster must cast itself.
    /// Added 2026-10-25 during a rigorous per-ability gap investigation: every Mage validation
    /// fixture's own `raid.debuffs.curseOfElements` field is `true` (confirmed by re-fetching
    /// all three Mage request JSON files directly) even though nothing in a solo Mage trace
    /// ever casts the curse - it's the validation methodology's own "assume a raid Warlock
    /// keeps this up" baseline, independent of which class is actually being measured. Every
    /// Warlock fixture's own `debuffs` block, by contrast, has *no* `curseOfElements` key at
    /// all (confirmed the same way) - Warlock provides its own via `cast_utility`, unaffected
    /// by this field. `false` keeps this struct's older behaviour exactly, which is why no
    /// existing caller needed updating.
    pub standing_curse_of_the_elements: bool,
    /// Spell Piercing (resistance penetration) - see `Fight::spell_vulnerability_bonus`'s own
    /// doc comment for the full real-engine-sourced formula and the mechanic it was added for
    /// (Arcane Subtlety). A generic caster stat, not Mage-specific - any class's own talent,
    /// item or racial that grants it should feed this same field.
    pub spell_piercing: f64,
    /// Whether to treat Judgement of Wisdom (the `raid.debuffs.judgementOfWisdom` setting
    /// every "builder starter" validation fixture this project has checked carries - Warlock's
    /// *and* every Mage spec's own) as a standing debuff on the target: a landed direct hit has
    /// a 50% chance to return 59 mana to its caster, confirmed directly against the real
    /// engine's own `AttachJudgementOfWisdomMana`/`JUDGEMENT_PROC_CHANCE` source. Found
    /// 2026-10-25 while using a new per-cast debug log (added specifically to chase a Fire Mage
    /// cast-count skew) to compare this engine's own mana trajectory against a real one: the
    /// real engine's own Fire Mage fixture barely ever needs Evocation (one cast, at t=124.5,
    /// essentially after the fight already ended); this engine's own identical build needed an
    /// 8-second Evocation around the fight's halfway point, which fully drained the GCDs that
    /// would otherwise have kept Fire Vulnerability refreshed - the real, root explanation for
    /// the Fire Blast/Scorch cast-count skew the debug log was built to chase down. A completely
    /// generic, cross-class mechanic (every class's own melee/spell hits can proc it, not just
    /// Mage's) - this project had simply never modeled it because `judgementOfWisdom` had only
    /// ever been read once, 2026-10-25, as an unidentified key in a `raid.debuffs` block and
    /// never followed up on.
    pub standing_judgement_of_wisdom: bool,
}

impl Default for CasterStats {
    fn default() -> Self {
        CasterStats {
            spell_power: 0.0,
            bonus_shadow_power: 0.0,
            bonus_fire_power: 0.0,
            bonus_arcane_power: 0.0,
            bonus_frost_power: 0.0,
            bonus_holy_power: 0.0,
            spell_hit: 0.0,
            spell_crit: 0.0,
            haste: 1.0,
            mp5: 0.0,
            max_mana: 10000.0,
            spirit: 0.0,
            intellect: 0.0,
            standing_curse_of_the_elements: false,
            spell_piercing: 0.0,
            standing_judgement_of_wisdom: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum School {
    Shadow,
    Fire,
    Arcane,
    Frost,
    Holy,
}
