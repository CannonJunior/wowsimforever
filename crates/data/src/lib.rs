//! Versioned, confidence-tagged game data (Plan Section 3, Principles 1 & 2).
//!
//! A `DataPack` is the artifact the simulation engine reads; it is never
//! hand-coded into the engine itself. Every fact in it carries a
//! `Confidence` tag and a `Source`, so the engine and UI can refuse to treat
//! a beta guess with the same authority as a Blizzard-confirmed value.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// Confirmed against a live client / live combat logs.
    LiveConfirmed,
    /// Posted by a Blizzard developer or community manager (e.g. Kaivax dev notes).
    DevBluePost,
    /// Extracted or observed from the beta client directly.
    BetaDatamine,
    /// Reported consistently by community/third-party sources, not yet primary-verified.
    CommunityReported,
    /// Synthetic example value used only to exercise the engine during
    /// bring-up; not sourced from anywhere and must never be treated as a
    /// real game value (Plan Section 0.1).
    Placeholder,
    /// Two or more credible sources disagree on the governing rule (Plan 2.4, Principle 3).
    CommunityDisputed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum School {
    Shadow,
    Fire,
    Physical,
    Frost,
    Arcane,
    Holy,
    Nature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PetType {
    Imp,
    Succubus,
    Voidwalker,
    Felhunter,
}

/// A pet's Demonic Sacrifice effect (Plan 2.4). Previously these percentages
/// were hard-coded directly in engine code with no source attached -- an
/// unsourced magic number is strictly worse than a tagged placeholder, since
/// it isn't even flagged as suspect. Moved here so it lives in the
/// versioned, confidence-tagged data pack like everything else (Principle 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemonicSacrificeEffect {
    pub pet_type: PetType,
    pub shadow_damage_pct: f64,
    pub fire_damage_pct: f64,
    pub mana_regen_pct_per_tick: f64,
    pub health_regen_pct_per_tick: f64,
    pub confidence: Confidence,
    pub source: Source,
}

/// A damage-over-time component attached to a spell (e.g. Corruption, Immolate's burn).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DotDef {
    pub tick_interval_ms: u32,
    pub num_ticks: u32,
    pub tick_base: f64,
    pub tick_coefficient: f64,
    /// Classic-style DoTs typically do not benefit from crit; kept explicit rather than assumed.
    pub can_crit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpellDef {
    pub id: String,
    pub name: String,
    pub school: School,
    pub cast_time_ms: u32,
    pub cooldown_ms: u32,
    pub resource_cost: f64,
    pub base_min: f64,
    pub base_max: f64,
    pub coefficient: f64,
    /// Attack-power scaling coefficient for this spell's direct component
    /// (parallel to `coefficient`'s spell-power scaling) -- real melee/
    /// ranged special abilities (e.g. Bloodthirst: "45% of Attack Power
    /// plus 48") scale with attack power, not spell power. `0.0` for
    /// every spell-power-scaling spell, which is every spell sourced
    /// before this field existed.
    #[serde(default)]
    pub ap_coefficient: f64,
    pub dot: Option<DotDef>,
    pub confidence: Confidence,
    pub source: Source,
}

/// One candidate rule for a disputed, state-dependent mechanic (Plan 2.4, Principle 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateRule {
    pub id: String,
    pub label: String,
    pub source: Source,
    pub description: String,
    pub confidence: Confidence,
}

/// A conditional mechanic with two or more candidate governing rules on record.
/// Demonic Sacrifice x Demonic Pact (Plan 2.4) is the flagship instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputedMechanic {
    pub id: String,
    pub name: String,
    pub description: String,
    pub candidate_rules: Vec<CandidateRule>,
    pub default_rule_id: String,
}

impl DisputedMechanic {
    pub fn default_rule(&self) -> &CandidateRule {
        self.candidate_rules
            .iter()
            .find(|r| r.id == self.default_rule_id)
            .expect("default_rule_id must reference a listed candidate_rule")
    }

    pub fn rule(&self, id: &str) -> Option<&CandidateRule> {
        self.candidate_rules.iter().find(|r| r.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisputeStatus {
    Open,
    Resolved,
}

/// A raid-wide debuff applied to the encounter target that increases every
/// raid member's damage taken of certain schools (Plan Section 8: "shared
/// buffs/debuffs with caps... synergy effects between members"). The
/// flagship real instance shipped with this: Warlock's Curse of the
/// Elements, confirmed via web research 2026-10-17 (wowforever.wclbox.com,
/// corroborating with the known Classic-era value) to exist in WoW Forever
/// at max rank, increasing magic damage taken by 10% -- see
/// docs/PLAN.md's Phase 3 update for the exact sourcing. A single
/// Warlock casting this benefits every *other* raid member whose rotation
/// deals one of `schools_affected`, which is the actual cross-character
/// synergy this struct exists to represent -- not just a bonus to its own
/// caster, unlike Demonic Sacrifice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaidDebuffDef {
    pub id: String,
    pub name: String,
    pub schools_affected: Vec<School>,
    pub damage_taken_pct_bonus: f64,
    pub confidence: Confidence,
    pub source: Source,
}

/// One row of the public dispute ledger (Plan Section 4.7, Principle 13).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeLedgerEntry {
    pub mechanic_id: String,
    pub owner: String,
    pub falsifiable_test: String,
    pub target_resolution_date: String,
    pub status: DisputeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPack {
    /// e.g. "forever-beta-2026-10-16-kaivax-dev-notes" — tagged to the build/patch that produced it.
    pub version: String,
    pub game_build_reference: String,
    pub spells: Vec<SpellDef>,
    pub disputed_mechanics: Vec<DisputedMechanic>,
    pub dispute_ledger: Vec<DisputeLedgerEntry>,
    #[serde(default)]
    pub demonic_sacrifice_effects: Vec<DemonicSacrificeEffect>,
    #[serde(default)]
    pub raid_debuffs: Vec<RaidDebuffDef>,
}

impl DataPack {
    pub fn spell(&self, id: &str) -> Option<&SpellDef> {
        self.spells.iter().find(|s| s.id == id)
    }

    pub fn mechanic(&self, id: &str) -> Option<&DisputedMechanic> {
        self.disputed_mechanics.iter().find(|m| m.id == id)
    }

    pub fn demonic_sacrifice_effect(&self, pet: PetType) -> Option<&DemonicSacrificeEffect> {
        self.demonic_sacrifice_effects.iter().find(|e| e.pet_type == pet)
    }

    pub fn raid_debuff(&self, id: &str) -> Option<&RaidDebuffDef> {
        self.raid_debuffs.iter().find(|d| d.id == id)
    }

    /// Looks up a mechanic's current status in the public dispute ledger
    /// (Plan Section 4.7). `None` means the mechanic has no ledger entry at
    /// all (never flagged as disputed in the first place).
    pub fn dispute_status(&self, mechanic_id: &str) -> Option<DisputeStatus> {
        self.dispute_ledger.iter().find(|e| e.mechanic_id == mechanic_id).map(|e| e.status)
    }

    pub fn from_json_str(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }

    /// Merges an overrides pack's spells into this one, replacing by id (or
    /// adding if new). This is the *only* sanctioned way placeholder data
    /// can reach the engine (see CLAUDE.md): the caller must have loaded
    /// `overrides` from an explicit, required command-line argument, never
    /// a default path. Returns the ids of every overridden/added spell that
    /// carries `Confidence::Placeholder`, so the caller can surface a loud
    /// warning rather than silently proceeding.
    pub fn apply_overrides(&mut self, overrides: DataPack) -> Vec<String> {
        let mut placeholder_ids = Vec::new();
        for spell in overrides.spells {
            if matches!(spell.confidence, Confidence::Placeholder) {
                placeholder_ids.push(spell.id.clone());
            }
            if let Some(existing) = self.spells.iter_mut().find(|s| s.id == spell.id) {
                *existing = spell;
            } else {
                self.spells.push(spell);
            }
        }
        placeholder_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disputed_mechanic_resolves_default_and_named_rules() {
        let mechanic = DisputedMechanic {
            id: "demonic_sacrifice_x_demonic_pact".into(),
            name: "Demonic Sacrifice interaction with Demonic Pact".into(),
            description: "test".into(),
            candidate_rules: vec![
                CandidateRule {
                    id: "cancels_on_summon".into(),
                    label: "Buff cancels if any demon is summoned".into(),
                    source: Source { name: "wowforever.space".into(), url: None },
                    description: "test".into(),
                    confidence: Confidence::CommunityReported,
                },
                CandidateRule {
                    id: "persists_through_pact".into(),
                    label: "Buff persists through a pet swap with Demonic Pact".into(),
                    source: Source { name: "andycarlson13/foreversims".into(), url: None },
                    description: "test".into(),
                    confidence: Confidence::CommunityReported,
                },
            ],
            default_rule_id: "cancels_on_summon".into(),
        };

        assert_eq!(mechanic.default_rule().id, "cancels_on_summon");
        assert_eq!(mechanic.rule("persists_through_pact").unwrap().label, "Buff persists through a pet swap with Demonic Pact");
        assert!(mechanic.rule("nonexistent").is_none());
    }
}
