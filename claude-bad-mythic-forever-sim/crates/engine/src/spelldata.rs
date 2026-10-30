//! Loader for this project's own data pack (`data/warlock-forever-*.json`). See that file's
//! `provenance_note` for sourcing. Deserializes loosely (most fields `Option`) because the raw
//! client rows this was mined from don't populate every field for every spell (e.g. a pure
//! debuff has no `direct` or `periodic` block).

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
pub struct DataPack {
    pub pack_version: String,
    pub spells: BTreeMap<String, SpellRow>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpellRow {
    pub spell_id: i64,
    /// The client's raw school bitmask value (32 = Shadow, 4 = Fire among this pack's 17
    /// spells - every one of them is one or the other). Added in the fourth
    /// research/implementation pass to let `gear`'s school-specific ShadowDamage/FireDamage
    /// stats apply to the right spells - see `school()`.
    pub school: Option<i64>,
    pub cast_time_ms: Option<i64>,
    pub gcd_ms: Option<i64>,
    pub cooldown_ms: Option<i64>,
    pub mana_cost: Option<f64>,
    pub duration_ms: Option<i64>,
    pub direct: Option<DirectEffect>,
    pub periodic: Option<PeriodicEffect>,
    pub confidence: String,
}

impl SpellRow {
    /// Maps this pack's raw client school value to `crate::stats::School`. Shadow (32) and
    /// Fire (4) are Warlock/Fire Mage's; Frost (16) and Arcane (64) were added for Frost/Arcane
    /// Mage (confirmed directly off Frostbolt/Ice Lance's and Arcane Blast/Arcane Missiles' own
    /// `School` fields in `data/spells.jsonl`); Holy (2) was added for Smite Priest (confirmed
    /// off Smite/Holy Fire/Penance's own `School` field). A sixth value would mean a spell this
    /// engine doesn't actually expect, so this panics rather than silently defaulting (the same
    /// "fail loud, don't guess" discipline the rest of this project uses for missing data).
    pub fn school(&self) -> crate::stats::School {
        match self.school {
            Some(32) => crate::stats::School::Shadow,
            Some(4) => crate::stats::School::Fire,
            Some(16) => crate::stats::School::Frost,
            Some(64) => crate::stats::School::Arcane,
            Some(2) => crate::stats::School::Holy,
            other => panic!(
                "spell id {} has an unrecognized school {other:?} (expected 2=Holy, 4=Fire, 16=Frost, 32=Shadow or 64=Arcane)",
                self.spell_id
            ),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct DirectEffect {
    pub base_points: f64,
    pub sp_coefficient: f64,
    /// Fractional total width of the damage roll around `base_points`, e.g. 0.11 means the
    /// roll spans base*(1-0.055) to base*(1+0.055). `None` for spells with a fixed, nonrandom
    /// value (not observed among the 17 spells this pack mined, but the type allows it).
    pub variance: Option<f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PeriodicEffect {
    pub base_points_per_tick: f64,
    pub sp_coefficient: f64,
    pub period_ms: i64,
    pub num_ticks: Option<i64>,
}

impl DataPack {
    pub fn load_str(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("failed to parse data pack: {e}"))
    }

    pub fn spell(&self, key: &str) -> &SpellRow {
        self.spells
            .get(key)
            .unwrap_or_else(|| panic!("data pack has no entry for spell `{key}` (checked {} entries)", self.spells.len()))
    }
}

/// Embeds the project's own data pack at compile time so the CLI needs no `--data` flag for
/// the common case. This is the real, sourced pack (see data/*.json provenance notes) -
/// unlike wowsimforever's hard rule on this point, this project has no placeholder-data tier
/// yet because every spell it implements has a sourced, cross-checked row.
pub const EMBEDDED_WARLOCK_PACK_JSON: &str =
    include_str!("../../../data/warlock-forever-2026-10-23-rev2.json");

pub fn embedded_warlock_pack() -> DataPack {
    DataPack::load_str(EMBEDDED_WARLOCK_PACK_JSON).expect("embedded data pack must parse")
}

/// Mage's own spell pack (2026-10-25, beginning this project's Mage support) - same loader,
/// same `DataPack`/`SpellRow` shapes, a different embedded JSON. Nothing about this loader was
/// Warlock-specific to begin with.
pub const EMBEDDED_MAGE_PACK_JSON: &str =
    include_str!("../../../data/mage-forever-2026-10-25-rev6.json");

pub fn embedded_mage_pack() -> DataPack {
    DataPack::load_str(EMBEDDED_MAGE_PACK_JSON).expect("embedded mage data pack must parse")
}

/// Priest's own spell pack (2026-10-25, beginning this project's Priest support) - same loader,
/// same `DataPack`/`SpellRow` shapes, a different embedded JSON. rev2 (2026-10-26) adds
/// `mind_flay_clip1`/`mind_flay_clip2`, the real engine's own mid-channel-interrupted Mind Flay
/// variants - see `priest/shadow.rs`'s own doc comment for the three-case timing derivation.
pub const EMBEDDED_PRIEST_PACK_JSON: &str =
    include_str!("../../../data/priest-forever-2026-10-26-rev2.json");

pub fn embedded_priest_pack() -> DataPack {
    DataPack::load_str(EMBEDDED_PRIEST_PACK_JSON).expect("embedded priest data pack must parse")
}
