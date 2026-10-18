//! Minimal gear/itemization mechanism (Plan Section 7's Phase 1
//! simplification: "hand-entered gear only, no item data pack"). This
//! closes the *mechanism* half of that gap honestly: a build's
//! `spell_power`/`crit_chance` can now be computed from a list of
//! equipped items instead of typed in directly.
//!
//! What this deliberately does **not** do: ship a real, sourced item
//! database. Itemizing dozens of slots across every class/armor type is a
//! much larger undertaking than this pass's scope, and this project's own
//! no-placeholder-in-production rule (`CLAUDE.md`) means fabricated item
//! stats cannot simply be invented and shipped as if real. The one example
//! file this crate ships (`data-packs/gear.example.json`) is tagged
//! `Confidence::Placeholder` on every item, exactly like
//! `overrides.example.json` already is for spell data, and is never loaded
//! by default -- only via an explicit path the caller supplies.
//!
//! Also deliberately scoped: no generic "crit rating" stat. Classic-era
//! itemization has no such thing (that's a Burning Crusade+ concept) --
//! instead, a handful of real Classic items grant a direct, stated
//! "Equip: Increases your critical strike chance by X%" bonus, which is
//! what `GearItem::crit_chance_pct` models. Inventing a rating-to-percent
//! conversion formula that didn't exist in this game's actual itemization
//! would be a fabricated mechanic, not a simplification of a real one.

use serde::{Deserialize, Serialize};
use wowforever_data::{Confidence, Source};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GearItem {
    pub id: String,
    pub name: String,
    /// A plain slot label (e.g. "head", "chest", "weapon") -- not an
    /// enum. This mechanism doesn't enforce one-item-per-slot or any
    /// other equip-validity rule; it only aggregates whatever list it's
    /// given. Real slot-conflict validation is future work, not claimed
    /// here.
    pub slot: String,
    #[serde(default)]
    pub spell_power: f64,
    /// Direct critical-strike-chance percentage this item grants (e.g.
    /// `1.0` for a stated "+1% critical strike chance"), see module docs
    /// for why this isn't a generic "rating" stat.
    #[serde(default)]
    pub crit_chance_pct: f64,
    pub confidence: Confidence,
    pub source: Source,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Gear {
    pub items: Vec<GearItem>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GearStats {
    pub spell_power: f64,
    pub crit_chance: f64,
}

impl Gear {
    pub fn total_spell_power(&self) -> f64 {
        self.items.iter().map(|i| i.spell_power).sum()
    }

    pub fn total_crit_chance_pct(&self) -> f64 {
        self.items.iter().map(|i| i.crit_chance_pct).sum()
    }

    /// Computes effective build stats from this gear set plus a
    /// `base_crit_chance` (e.g. a class's innate Intellect-derived crit
    /// before any items) -- Classic itemization doesn't zero out crit
    /// just because no gear happens to grant any, so the base is additive
    /// input, not a default this function invents on its own.
    pub fn stats_with_base_crit(&self, base_crit_chance: f64) -> GearStats {
        GearStats {
            spell_power: self.total_spell_power(),
            crit_chance: base_crit_chance + self.total_crit_chance_pct() / 100.0,
        }
    }

    /// Ids of every item carrying `Confidence::Placeholder`, so a caller
    /// can surface a loud warning (mirrors
    /// `DataPack::apply_overrides`'s placeholder-id return) rather than
    /// silently simulating with fabricated stats.
    pub fn placeholder_item_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| matches!(i.confidence, Confidence::Placeholder))
            .map(|i| i.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, spell_power: f64, crit_chance_pct: f64, confidence: Confidence) -> GearItem {
        GearItem {
            id: id.into(),
            name: id.into(),
            slot: "test".into(),
            spell_power,
            crit_chance_pct,
            confidence,
            source: Source { name: "test".into(), url: None },
        }
    }

    #[test]
    fn empty_gear_contributes_nothing() {
        let gear = Gear::default();
        let stats = gear.stats_with_base_crit(0.20);
        assert_eq!(stats, GearStats { spell_power: 0.0, crit_chance: 0.20 });
    }

    #[test]
    fn spell_power_and_crit_sum_across_multiple_items() {
        let gear = Gear {
            items: vec![
                item("ring", 40.0, 0.0, Confidence::Placeholder),
                item("staff", 120.0, 0.0, Confidence::Placeholder),
                item("trinket", 0.0, 2.0, Confidence::Placeholder), // +2% crit
            ],
        };
        let stats = gear.stats_with_base_crit(0.10); // 10% base crit
        assert_eq!(stats.spell_power, 160.0);
        assert!((stats.crit_chance - 0.12).abs() < 1e-9, "expected 0.12, got {}", stats.crit_chance);
    }

    #[test]
    fn placeholder_item_ids_are_surfaced_not_hidden() {
        let gear = Gear {
            items: vec![
                item("real_item", 10.0, 0.0, Confidence::CommunityReported),
                item("fake_item", 10.0, 0.0, Confidence::Placeholder),
            ],
        };
        assert_eq!(gear.placeholder_item_ids(), vec!["fake_item".to_string()]);
    }
}
