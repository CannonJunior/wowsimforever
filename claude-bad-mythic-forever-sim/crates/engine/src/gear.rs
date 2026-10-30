//! The gear/item system, reverse-engineered the same way the spell data was: read the real
//! engine's source for the formulas (`src/prepare/items.rs`, `src/prepare/stats.rs`,
//! `src/mechanics/haste.rs`, `src/prepare/character.rs`), then mine real item/enchant/race-stat
//! rows from its `data/items.jsonl`, `data/enchants.jsonl` and `data/go-tables.json`. See the
//! embedded pack's own `provenance_note` (`EMBEDDED_WARLOCK_GEAR_PACK_JSON` below) for the
//! exact citations.
//!
//! **What this replaces**: before this pass, a caller had to hand-compute final stat totals
//! (spell power, hit%, crit%, haste%) themselves. `equipment_to_stats` now does that from a
//! race and a list of equipped items/enchants, using the real conversion formulas:
//!
//! - Rating-to-percent: 10 spell/melee hit rating per 1%, 14 spell/melee crit rating per 1%,
//!   10 spell haste rating per 1% (`src/prepare/character.rs`'s constants, `src/mechanics/haste.rs`).
//! - **Forever's own gear rule** (`forever_rules.go unifyGearHitAndCrit`, read via
//!   `src/prepare/items.rs`): a gear set's melee and spell hit ratings are summed into one
//!   percentage and credited to *both* - a trinket with melee hit rating genuinely raises a
//!   Warlock's spell hit here too, faithfully, not by mistake.
//! - Intellect-to-crit: 0.0165%/point for `ClassWarlock` specifically, from `data/go-tables.json`
//!   (itself mined from the Go reference engine's own per-class table - Mages and Rogues have
//!   different values in that same table, so this isn't a generic "casters get X" constant).
//! - Race base stats (Intellect, Spirit, Stamina, Mana, base SpellCritPercent) for the five
//!   Warlock-playable races, same source.
//!
//! **What this actually applies now** (both were still listed as "not yet done" below as late
//! as rev2 of the data pack - that was stale, not current behaviour; fixed in rev3, 2026-10-24):
//! - Items' school-specific ShadowDamage/FireDamage stats *are* summed separately into
//!   `CasterStats.bonus_shadow_power`/`bonus_fire_power` (not folded into the universal
//!   `spell_power`) and applied by whichever school each spell actually belongs to - see
//!   `stats.rs`'s `School` enum and `sim.rs`'s `resolve_direct_raw`.
//! - Bonus mana from Intellect *is* derived: `wowsims/forever`'s own `sim/core/mana.go`
//!   (`AddStat(Mana, -280)` + `AddStatDependency(Intellect, Mana, 15)`) - confirmed directly in
//!   that source file, not guessed. Spirit-derived mana regen is derived the same way (same
//!   file's `spiritRegenBase`/`spiritRegenPerSpirit` constants).
//!
//! **What this does not (yet) do** - see the data pack's own `unimplemented_but_parsed` field
//! for the full citation of each, re-audited 2026-10-24 directly against the source engine's
//! `data/items.jsonl` (not just this pack's own prior claims):
//! - `pseudo_stats` (an alternate percentage-based stat representation some items/enchants
//!   carry) and item set bonuses (`set_id`/`set_name`) are real client-data concepts this
//!   pack's own mining/flattening step drops before they ever reach this JSON - confirmed
//!   inert for every item and build actually in this pack (none carry a nonzero pseudo_stat;
//!   no build equips more than one piece of the one set, Stormcloth Regalia, that appears
//!   here), but `ItemRow` has no field for either, so re-mining with them carried through
//!   would also need a `gear.rs` change to apply them.
//! - `random_suffix`/`rand_prop_points` ("of the Whatever" randomly-enchanted items): three of
//!   this pack's 17 mined items have a nonzero `rand_prop_points`, but none of the real
//!   "builder starter" fixtures these builds come from ever chose a suffix for them - inert
//!   today, but `ItemRow` has no field to represent one if a future item needs it.
//! - `item_effects` (on-equip procs / on-use trinket effects): only one mined item has any
//!   (Robe of the Void's "Heal Pet" on-use, a pet-health utility effect with no damage
//!   component - outside this DPS-only engine's scope regardless), so no damage proc is
//!   silently dropped today, but `ItemRow` has no field for this either.
//! - Gems or sockets (the source engine's own item database has none - not a gap in this
//!   mining pass, a real property of the data: `item.rs`'s `new_item` hard-refuses any
//!   non-zero gem slot with "the database has none of").

use serde::Deserialize;
use std::collections::BTreeMap;

use crate::stats::CasterStats;

/// Generalized to any class (2026-10-25, beginning Mage support): this struct's field names
/// used to say "warlock_" even though nothing about `equipment_to_stats` actually is - the
/// crit-per-Intellect rate and race base stats are both genuinely per-class (confirmed directly
/// from `go-tables.json`'s own `crit_per_int_max_level`/`base_stats`, both keyed by class), so
/// each class just needs its own `GearPack` with its own values here, not a different struct.
#[derive(Debug, Deserialize)]
pub struct GearPack {
    pub pack_version: String,
    pub intellect_to_spell_crit_percent: f64,
    pub race_base_stats: BTreeMap<String, RaceBaseStats>,
    pub items: BTreeMap<String, ItemRow>,
    pub enchants: BTreeMap<String, EnchantRow>,
    pub example_builds: BTreeMap<String, ExampleBuild>,
}

// The pack's race rows use the client's own capitalized stat names directly as JSON keys
// (e.g. `{"Intellect": 108, "Spirit": 120, ...}`), so each field below is renamed to match
// rather than relying on serde's default snake_case-insensitive matching, which wouldn't hit
// these at all.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct RaceBaseStats {
    #[serde(default, rename = "Intellect")]
    pub intellect: f64,
    #[serde(default, rename = "Spirit")]
    pub spirit: f64,
    #[serde(default, rename = "Stamina")]
    pub stamina: f64,
    #[serde(default, rename = "Mana")]
    pub mana: f64,
    #[serde(default, rename = "SpellCritPercent")]
    pub spell_crit_percent: f64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ItemRow {
    pub name: String,
    pub slot: String,
    #[serde(default)]
    pub stats: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EnchantRow {
    pub name: String,
    #[serde(default)]
    pub stats: BTreeMap<String, f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ExampleBuild {
    pub race: String,
    pub equipment: Vec<EquipmentSlot>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EquipmentSlot {
    pub item: i64,
    pub enchant: Option<i64>,
    pub slot: String,
}

/// rev4 (2026-10-26) adds every item's own `HealingPower` stat, dropped during earlier
/// revisions' own mining - found to matter for damage, not just healing, while chasing
/// Priest's own per-hit gap - see `equipment_to_stats`'s own `/ 3.0` doc comment.
pub const EMBEDDED_WARLOCK_GEAR_PACK_JSON: &str =
    include_str!("../../../data/warlock-gear-2026-10-26-rev4.json");

impl GearPack {
    pub fn load_str(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("failed to parse gear pack: {e}"))
    }
}

pub fn embedded_warlock_gear_pack() -> GearPack {
    GearPack::load_str(EMBEDDED_WARLOCK_GEAR_PACK_JSON).expect("embedded gear pack must parse")
}

/// Mage's own gear pack (2026-10-25, beginning this project's Mage support) - same `GearPack`
/// shape and the same `equipment_to_stats` function, a different embedded JSON with Mage's own
/// race base stats and Intellect-to-crit rate (0.0168, confirmed different from Warlock's
/// 0.0165 - the whole reason this struct's fields dropped their "warlock_" prefix). rev3
/// (2026-10-26) adds every item's own `HealingPower` stat, dropped during rev1/rev2's own
/// mining - found to matter for damage, not just healing, while chasing Priest's own per-hit
/// gap - see `equipment_to_stats`'s own `/ 3.0` doc comment.
pub const EMBEDDED_MAGE_GEAR_PACK_JSON: &str =
    include_str!("../../../data/mage-gear-2026-10-26-rev3.json");

pub fn embedded_mage_gear_pack() -> GearPack {
    GearPack::load_str(EMBEDDED_MAGE_GEAR_PACK_JSON).expect("embedded mage gear pack must parse")
}

/// Priest's own gear pack (2026-10-25, beginning this project's Priest support) - same shape,
/// Priest's own race base stats and Intellect-to-crit rate (0.0168, confirmed from
/// `go-tables.json`'s own `crit_per_int_max_level.ClassPriest` - same numeric value as Mage's,
/// confirmed independently rather than assumed identical). rev2 (2026-10-26) adds every item's
/// own `HealingPower` stat, dropped during rev1's own mining - see `BUFF_SPELL_POWER`-adjacent
/// doc comment on `/ 3.0` healing-to-damage conversion in `equipment_to_stats` for why that
/// stat turned out to matter for this class's own damage output, not just its healing.
pub const EMBEDDED_PRIEST_GEAR_PACK_JSON: &str =
    include_str!("../../../data/priest-gear-2026-10-26-rev2.json");

pub fn embedded_priest_gear_pack() -> GearPack {
    GearPack::load_str(EMBEDDED_PRIEST_GEAR_PACK_JSON).expect("embedded priest gear pack must parse")
}

/// Running stat totals in the real engine's own units (ratings, not percentages yet) before
/// `finalize` converts them. Kept separate from `CasterStats` because the conversions
/// (rating-to-percent, the hit/crit unification rule, Intellect-to-crit) only make sense once
/// every item has been summed in.
#[derive(Default)]
struct RawTotals {
    intellect: f64,
    spirit: f64,
    stamina: f64,
    spell_damage: f64,
    shadow_damage: f64,
    fire_damage: f64,
    arcane_damage: f64,
    frost_damage: f64,
    holy_damage: f64,
    healing_power: f64,
    spell_piercing: f64,
    spell_hit_rating: f64,
    melee_hit_rating: f64,
    spell_crit_rating: f64,
    melee_crit_rating: f64,
    spell_haste_rating: f64,
    mana: f64,
    mp5: f64,
}

fn add_named_stats(totals: &mut RawTotals, stats: &BTreeMap<String, f64>) {
    for (name, value) in stats {
        match name.as_str() {
            "Intellect" => totals.intellect += value,
            "Spirit" => totals.spirit += value,
            "Stamina" => totals.stamina += value,
            "SpellDamage" => totals.spell_damage += value,
            "ShadowDamage" => totals.shadow_damage += value,
            "FireDamage" => totals.fire_damage += value,
            "ArcaneDamage" => totals.arcane_damage += value,
            "FrostDamage" => totals.frost_damage += value,
            "HolyDamage" => totals.holy_damage += value,
            "HealingPower" => totals.healing_power += value,
            "SpellPiercing" => totals.spell_piercing += value,
            "SpellHitRating" => totals.spell_hit_rating += value,
            "MeleeHitRating" => totals.melee_hit_rating += value,
            "SpellCritRating" => totals.spell_crit_rating += value,
            "MeleeCritRating" => totals.melee_crit_rating += value,
            "SpellHasteRating" => totals.spell_haste_rating += value,
            "Mana" => totals.mana += value,
            "MP5" => totals.mp5 += value,
            _ => {}
        }
    }
}

/// Go `sim/core/mana.go`'s own `AddStatDependency(Intellect, Mana, 15)` - +15 max mana per
/// point of *total* Intellect. `pub(crate)` (not just a `finalize`-local constant) so
/// `sim.rs::Fight::multiply_intellect` can apply the exact same rate when a talent raises
/// Intellect *after* this module's own gear/buff total already fed `CasterStats.max_mana` -
/// see that method's own doc comment for the bug this duplication-avoidance fixed.
pub(crate) const INTELLECT_TO_MANA_PER_POINT: f64 = 15.0;

/// Rating-to-percent constants, from `src/prepare/character.rs`.
const SPELL_HIT_RATING_PER_PERCENT: f64 = 10.0;
const MELEE_HIT_RATING_PER_PERCENT: f64 = 10.0;
const SPELL_CRIT_RATING_PER_PERCENT: f64 = 14.0;
const MELEE_CRIT_RATING_PER_PERCENT: f64 = 14.0;
/// From `src/mechanics/haste.rs`'s `SPELL_HASTE_RATING_PER_PERCENT_TIMES_100 = 1000.0`.
const SPELL_HASTE_RATING_PER_PERCENT: f64 = 10.0;

/// The exact raid-buff + consumable package `mythicsim-forever-engine`'s own "builder starter"
/// validation fixtures use (confirmed identical across all three Warlock specs' own request
/// JSON, re-fetched directly 2026-10-24 while chasing why this engine's DPS still ran ~1.7-2.2x
/// below mythicsim's own numbers for those same fixtures - raid buffs and consumables, not
/// melee auto-attack, turned out to be the real dominant cause: this package alone, applied on
/// top of the *same* gear/talents already modeled, closes most of the remaining gap - see
/// `research-notes/mythicsim-comparison.md`). This is deliberately that one real, reproducible
/// loadout, not a general buff system - each number below is sourced from its own real item or
/// spell page (`foreverdb.net`, checked against its own `robots.txt` first, same bar every
/// other source in this project is held to):
///
/// - Arcane Brilliance (spell 23028): +31 Intellect, 1h.
/// - Gift of the Wild Rank 2 (spell 21850): +16 to all attributes - Intellect and Spirit are
///   the two this DPS-only engine has any use for.
/// - Well Fed from Runn Tum Tuber Surprise (item 18254): +15 Intellect, 15 min.
/// - Zanza's potion, Cerebral Cortex Compound (item 8423): +25 Intellect, 60 min - a real
///   Classic-style exception to normal consumable-stacking rules, not an oversight to include
///   alongside the flask/elixirs below.
/// - Prayer of Spirit Rank 1 (spell 27681): +40 Spirit, 1h.
const BUFF_INTELLECT: f64 = 31.0 + 16.0 + 15.0 + 25.0;
const BUFF_SPIRIT: f64 = 16.0 + 40.0;

/// Greater Blessing of Kings (spell 25898): +10% to the warlock's total attributes - applied
/// here to the Intellect/Spirit totals above (Stamina/Strength/Agility aren't modeled by this
/// engine, so there's nothing else of this package's to apply it to).
const BUFF_STATS_PCT: f64 = 0.10;

/// Flask of Supreme Power (item 13512): +150 universal spell damage, 2h - the one consumable
/// identical across every class's own version of this package (same item ID, confirmed
/// directly in both the Warlock and Mage request JSON, 2026-10-24/25).
///
/// **Corrected 2026-10-25, after a per-ability comparison found a real, reproducible 71-point
/// effective-spell-power gap in BOTH the Frost and Fire Mage comparisons** (the same absolute
/// 71 points on two builds with different totals, which is what first made this look like a
/// missing flat additive term rather than a missing percentage). Every request JSON this
/// project has checked (Warlock's three specs and all three Mage specs) *also* lists a
/// `spellPowerElixirId` (Greater Arcane Elixir, item 13454: +35 universal spell damage) - a
/// prior pass had assumed this was inert once the flask (150, the *same* `SpellDamage` stat)
/// was up, citing `wowsims/forever`'s own `ExclusivePerStat` rule. That assumption was never
/// actually checked against the real Rust engine's own registration code and was wrong: a
/// direct read of `sage3648/mythicsim-forever-engine`'s `src/prepare/consumes.rs`
/// (`register_consumes`) shows `flaskId`, `spellPowerElixirId` and `schoolElixirId` are all fed
/// through the *same* unconditional `add_consumable_stats` call, with no exclusivity check in
/// that path at all - the function's own comment even says so: "Classic buffs that stack
/// *beside* the elixirs: ... the school power ... elixirs." Confirmed directly against a real
/// engine debug log for the Frost Mage fixture: computing this project's own gear/buff total by
/// hand landed at 427 effective Frost spell power; the real engine's own per-cast debug line
/// (`SP: 498.0`) was 71 higher - and 35 (this elixir) + 36 (Brilliant Wizard Oil's own
/// `SpellDamage` component, see `MAGE_BUFF_WIZARD_OIL_SPELL_POWER` below) accounts for that 71
/// exactly. Since every one of this project's own "builder starter" packages carries this same
/// elixir, the fix is applied here, unconditionally for every non-`None` variant, not duplicated
/// per class - the whole point of finding a *formula* bug instead of patching one spec's number.
const BUFF_SPELL_POWER: f64 = 150.0;
/// Greater Arcane Elixir (item 13454, every package's own `spellPowerElixirId`): +35 universal
/// spell damage, 1h - real and additive alongside the flask (see `BUFF_SPELL_POWER`'s own doc
/// comment for the full citation), not inert.
const BUFF_SPELL_POWER_ELIXIR: f64 = 35.0;

/// Which class/spec's own "builder starter" raid-buff + consumable package to apply on top of
/// gear (see `equipment_to_stats`'s own doc comment) - `None` keeps the function's older,
/// buff-less behaviour exactly. The *raid buffs* (Intellect/Spirit/Kings%/Moonkin crit%/
/// Wisdom+Mageblood mp5/the Flask above) are identical across every package checked so far
/// (confirmed by re-fetching all three Mage request fixtures directly, 2026-10-25, and finding
/// the exact same raid buff spell IDs active in each's own debug log as Warlock's) - only the
/// *school* elixir (a different item per spec, since it's school-specific) differs.
/// `MageFire` and `MageArcaneFrost` are two separate variants, not one `Mage` variant, because
/// the real fixtures' own `schoolElixirId` genuinely differs between them: Fire Mage's fixture
/// uses Elixir of Fire Power (item 6373, +10 Fire), but Arcane Mage's *and* Frost Mage's
/// fixtures both use Elixir of Frost Power (item 17708, +15 Frost) - confirmed byte-identical
/// `consumables` blocks between the Arcane and Frost request JSON, 2026-10-25, so one shared
/// variant covers both rather than three near-duplicates. (Yes, this means the real Arcane Mage
/// validation fixture runs with a Frost-school elixir, not an Arcane one - not a mining error,
/// just what that fixture's own package happens to equip; faithfully reproducing it is the
/// point, not second-guessing it.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BuilderStarterBuffs {
    #[default]
    None,
    Warlock,
    MageFire,
    MageArcaneFrost,
    /// Both Priest "builder starter" fixtures (Shadow and Smite) carry byte-identical
    /// `consumables` blocks, confirmed 2026-10-25 - including `schoolElixirId: 9264`, the exact
    /// same Elixir of Shadow Power Warlock's own package uses (a real fixture quirk for Smite,
    /// whose own damage is Holy-school and so gets nothing from it - faithfully reproduced, not
    /// second-guessed, same principle as `MageArcaneFrost`'s own doc comment above). One shared
    /// variant covers both specs.
    Priest,
}

/// Elixir of Shadow Power (item 9264, Warlock's package's own `schoolElixirId`): +40
/// Shadow-specific spell damage, 30 min - a different stat from the flask's (school-specific,
/// not universal), so it was always modeled as additive; unaffected by the `BUFF_SPELL_POWER`
/// correction above.
const WARLOCK_BUFF_SHADOW_POWER: f64 = 40.0;

/// Mage's own package consumables, different from Warlock's:
/// - Elixir of Fire Power (item 6373, this package's own `schoolElixirId` - a different, lower
///   item than Warlock's Shadow Power one): +10 Fire-specific spell damage, 30 min. A real,
///   smaller number for a genuinely different item - not a mistake to double check against
///   Warlock's +40.
/// - Brilliant Wizard Oil (item 20749, a weapon imbue - Warlock's own package carries none at
///   all): confirmed directly in `consumes.rs`'s own `register_static_imbue` match arm (`25122
///   | 20749`) to add three *separate, unconditional* stats - `Stat::SpellDamage, 36.0`,
///   `Stat::HealingPower, 36.0` and `Stat::SpellCritPercent, 1.0` - via plain `env.sim.add_stat`
///   calls with no exclusivity check at all. The +36 `SpellDamage` half was previously assumed
///   inert under the same mistaken `ExclusivePerStat` reasoning `BUFF_SPELL_POWER`'s own doc
///   comment corrects; it is not, and is the other half of that correction's 71-point gap
///   (35 + 36 = 71). The +36 `HealingPower` half was, in turn, assumed *untracked* rather than
///   inert - wrong the same way, found 2026-10-26 while chasing Priest's own per-hit damage
///   gap: Healing Power contributes a third of its value as universal spell damage in this
///   client (see `equipment_to_stats`'s own `/ 3.0` doc comment), so this +36 is worth +12
///   effective spell power, same as the `SpellDamage` half just scaled down.
const MAGE_BUFF_FIRE_POWER: f64 = 10.0;
const MAGE_BUFF_WIZARD_OIL_SPELL_POWER: f64 = 36.0;
const MAGE_BUFF_WIZARD_OIL_HEALING_POWER: f64 = 36.0;
const MAGE_BUFF_SPELL_CRIT_PCT: f64 = 1.0;

/// Judgement of the Crusader (real spell 20303, Rank 6 - the client's own max rank, the only
/// one `mythicsim-forever-engine`'s own `judgement_of_the_crusader_max_rank()` ever resolves):
/// a raid debuff on the target, +161 Holy-school "spell power against this target"
/// (`BasePoints: 161` on the raw spell row, `Misc: 2` = Holy - confirmed directly against
/// `data/spells.jsonl` in the real engine's own clone). Modeled as a flat `bonus_holy_power`
/// addition, the same shape as every other school-scoped bonus this project already has - this
/// is a target-side debuff rather than a caster-side stat in the real engine (`sim.unit_mut
/// (target).pseudo_stats.school_bonus_spell_damage[holy] += bonus`), but since this project
/// only ever simulates a single attacker against a single target, "the target takes +161 more
/// Holy spell power" and "the caster deals +161 more effective Holy spell power" are the same
/// number either way. **Found 2026-10-26**: this was the real cause of Smite/Holy Fire/
/// Penance's own previously-unexplained ~23% `BaseDamage` shortfall (`research-notes/priest-
/// smite-comparison.md`) - three prior passes had ruled out Twin Disciplines, Holy
/// Specialization and Divine Fury as the cause without ever revisiting this fixture's own
/// `raid.debuffs.judgementOfTheCrusader: true` flag, which an earlier pass had found but
/// wrongly guessed was a crit-chance debuff (the classic-WoW shape) and left unmodeled; the
/// real Forever-client row shows it's a flat damage-taken bonus instead, confirmed directly: a
/// real Smite cast's own `SP: 509.9, BaseDamage: 655.6` line matches this formula exactly
/// (`170 + 0.714 * (509.9 + 161) = 649.6`, the small residual being this spell's own `Variance:
/// 0.112` roll) with zero other unexplained factor needed.
const JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER: f64 = 161.0;

/// Elixir of Frost Power (item 17708, the Arcane Mage *and* Frost Mage fixtures' own shared
/// `schoolElixirId` - see `BuilderStarterBuffs::MageArcaneFrost`'s own doc comment for why one
/// item covers both specs): +15 Frost-specific spell damage, 30 min (`foreverdb.net`,
/// 2026-10-25: "Increases spell frost damage by up to 15 for 30 min").
const MAGE_BUFF_FROST_POWER: f64 = 15.0;

/// Moonkin Aura (spell 24907): +3% spell crit chance - not the 5% a Classic/TBC-familiar reader
/// might expect; confirmed directly from this client's own tooltip (`foreverdb.net`,
/// 2026-10-24): "Increases spell Critical Strike chance by 3%."
const BUFF_SPELL_CRIT_PCT: f64 = 3.0;

/// Mageblood Elixir (item 20007, this package's `guardianElixirId` - a different slot/category
/// from the flask and school elixir above, so it stacks with both): +12 mp5, 1h. Greater
/// Blessing of Wisdom (spell 25918): +40 mp5, 1h, on top. **Mana Spring Totem (spell 10494,
/// rank 4): +25 mp5** (its real tick is `10 mana every 2000ms` on the raw client row,
/// `10 * (5/2) = 25` mp5-equivalent) - a prior pass had explicitly flagged this as "not
/// modeled... a sustain-only stat regardless... mp5-only gaps don't move this engine's reported
/// DPS under comfortable mana." That assumption held for Warlock and Fire Mage (mana was never
/// the bottleneck there) but was wrong in general: found and fixed 2026-10-25 while diffing
/// Arcane Mage's own mana trajectory against a real debug trace - this engine's own mana
/// declined to 15% by t=78 in a 120s fight the real engine barely dents (first Evocation at
/// t=124.5), and the real engine's own natural-regen tick (`Gained 97.050 mana from {OtherID:
/// 2}` every 2s = 242.625 mp5-equivalent) was *exactly* 25 higher than this engine's own
/// computed mp5 (218.0) for the identical build - solving precisely for Mana Spring's own real
/// tick value, not a coincidence. A standing raid buff present in every fixture checked so
/// far (Fire's and Frost's included, confirmed by the same `SpellID: 10494` aura-gained line),
/// so applied alongside Mageblood/Wisdom here rather than per-class.
const BUFF_MP5: f64 = 12.0 + 40.0 + 25.0;

/// Computes final caster stats for a race plus a list of equipped (item key, optional enchant
/// key) pairs, using `pack`'s real items/enchants/race data. Item and enchant keys are the
/// string keys this pack's `items`/`enchants` maps use (their real client IDs, as strings).
/// `buffs` applies the named class's exact raid-buff + consumable package documented above
/// (`BUFF_*`/`WARLOCK_BUFF_*`/`MAGE_BUFF_*` constants) on top of the gear -
/// `BuilderStarterBuffs::None` keeps this function's older, buff-less behaviour exactly, which
/// is why every existing caller keeps working unchanged.
pub fn equipment_to_stats(
    pack: &GearPack,
    race: &str,
    equipment: &[(String, Option<String>)],
    buffs: BuilderStarterBuffs,
) -> Result<CasterStats, String> {
    let race_raw = pack
        .race_base_stats
        .iter()
        .find(|(name, _)| name.as_str() == race)
        .map(|(_, v)| v)
        .ok_or_else(|| format!("no base stats for race `{race}`"))?;

    let mut totals = RawTotals::default();
    for (item_key, enchant_key) in equipment {
        let item = pack
            .items
            .get(item_key)
            .ok_or_else(|| format!("no item with key `{item_key}`"))?;
        add_named_stats(&mut totals, &item.stats);
        if let Some(enchant_key) = enchant_key {
            let enchant = pack
                .enchants
                .get(enchant_key)
                .ok_or_else(|| format!("no enchant with key `{enchant_key}`"))?;
            add_named_stats(&mut totals, &enchant.stats);
        }
    }

    // Forever's unifyGearHitAndCrit: melee and spell hit/crit from gear are summed into one
    // percentage and credited to both pools - see module docs.
    let gear_hit_percent = totals.melee_hit_rating / MELEE_HIT_RATING_PER_PERCENT
        + totals.spell_hit_rating / SPELL_HIT_RATING_PER_PERCENT;
    let gear_crit_percent = totals.melee_crit_rating / MELEE_CRIT_RATING_PER_PERCENT
        + totals.spell_crit_rating / SPELL_CRIT_RATING_PER_PERCENT;

    let apply_buffs = buffs != BuilderStarterBuffs::None;
    let mut total_intellect = race_raw.intellect + totals.intellect;
    let mut total_spirit = race_raw.spirit + totals.spirit;
    if apply_buffs {
        total_intellect = (total_intellect + BUFF_INTELLECT) * (1.0 + BUFF_STATS_PCT);
        total_spirit = (total_spirit + BUFF_SPIRIT) * (1.0 + BUFF_STATS_PCT);
    }
    let intellect_crit_percent = total_intellect * pack.intellect_to_spell_crit_percent;

    let mut spell_crit_percent = race_raw.spell_crit_percent + gear_crit_percent + intellect_crit_percent;
    if apply_buffs {
        spell_crit_percent += BUFF_SPELL_CRIT_PCT;
    }
    if matches!(
        buffs,
        BuilderStarterBuffs::MageFire | BuilderStarterBuffs::MageArcaneFrost | BuilderStarterBuffs::Priest
    ) {
        spell_crit_percent += MAGE_BUFF_SPELL_CRIT_PCT;
    }
    // Base spell miss chance vs a level-63 target (the standard raid-boss level, +3 over a
    // level-60 character) is 17% - confirmed directly in wowsims/forever's own
    // sim/core/attack_table_test.go base-chance table (the row for level 63: `spellMiss:
    // 0.17`), found and fixed in the same research pass as the mythicsim.com DPS comparison
    // this constant was needed for. Gear/talent hit rating is additive on top of this base,
    // not a replacement for it - an earlier version of this function computed only the gear
    // delta (4-5% for a typical starter build) and fed that alone into `CasterStats.spell_hit`,
    // which meant ~95% of casts missed. Not level-aware (always assumes a level-63 target);
    // flagged as a simplification rather than threading a target level through this function.
    const BASE_SPELL_HIT_VS_LEVEL_63_PERCENT: f64 = 83.0;
    let spell_hit_percent = (BASE_SPELL_HIT_VS_LEVEL_63_PERCENT + gear_hit_percent).min(100.0);
    let haste_percent = totals.spell_haste_rating / SPELL_HASTE_RATING_PER_PERCENT;

    let _ = totals.stamina; // parsed (real gear stat), not consumed by this DPS-only engine

    // Intellect-to-mana (wowsims/forever sim/core/mana.go EnableManaBarWithModifier, read
    // directly in the fourth research/implementation pass): AddStat(Mana, 20-15*20) then
    // AddStatDependency(Intellect, Mana, 15) - i.e. -280 flat, +15/point of TOTAL Intellect.
    // `race_raw.mana` is the pre-dependency base_stats entry (confirmed by tables.rs's own
    // doc comment: "core.BaseStats... the nonzero stats by Go stat name" - a raw starting
    // point, not a finalized total), so this formula still needs to run on top of it.
    const INTELLECT_TO_MANA_FLAT_OFFSET: f64 = -280.0;
    let intellect_mana = INTELLECT_TO_MANA_FLAT_OFFSET + INTELLECT_TO_MANA_PER_POINT * total_intellect;

    // Spirit-to-mana-regen (same file: `spiritRegenBase, spiritRegenPerSpirit = 6.25, 1.0/8`,
    // Warlock uses the class default - confirmed via warlock.go calling plain `EnableManaBar()`
    // with no `SetSpiritManaRegen` override). This engine doesn't model the 5-second-rule
    // (regen is throttled while casting in the real game - see README "Scope"), so this uses
    // the real "while not casting" rate continuously, an overestimate under heavy casting.
    const SPIRIT_REGEN_BASE_PER_SECOND: f64 = 6.25;
    const SPIRIT_REGEN_PER_SPIRIT_PER_SECOND: f64 = 1.0 / 8.0;
    let spirit_regen_per_5s =
        5.0 * (SPIRIT_REGEN_BASE_PER_SECOND + total_spirit * SPIRIT_REGEN_PER_SPIRIT_PER_SECOND);

    // Healing Power contributes a third of its value as bonus damage *on Holy-school spells
    // specifically* - a real, Forever-specific mechanic (classicwow.gg's own Forever guide,
    // "Healing gear now provides one-third as much bonus damage"), not a Classic-era rule this
    // project assumed, and not a universal bonus to every school either. Found 2026-10-26
    // while chasing Smite Priest's own per-hit damage gap: a real debug line gave Smite's own
    // effective spell power as needing roughly 170 points more than this engine computed from
    // gear+buffs alone. **A first version of this fix added it to the universal `spell_power`
    // field (affecting every school) - wrong, caught by checking Shadow Word: Pain's own real
    // debug line on the *same* character: its Shadow-school `SP:`/`BaseDamage` matched this
    // project's own pre-existing formula exactly, with no healing-power bonus needed at all,
    // while the Holy-school spells on the identical character still needed one - proving the
    // bonus is Holy-school-scoped, like `bonus_shadow_power`/`bonus_fire_power` already are,
    // not a `spell_power` one.** This build's own 15 items each carry an *equal* HealingPower
    // value alongside their SpellDamage one (confirmed item-by-item against `data/items.jsonl`'s
    // own stat index 4 - every single piece, not a coincidence), summing to exactly the same
    // 277 total as this build's own SpellDamage. `/ 3.0` of that (92.3) closes most, not all,
    // of the Holy-school gap - this project doesn't yet know which consumables/raid buffs also
    // add Healing Power (the real engine's own `prepare` output gives this build's *buffed*
    // total as 348.55, higher than the 277 gear-only figure this fix uses, though the Brilliant
    // Wizard Oil fix below accounts for part of that difference), so a smaller residual
    // remains, honestly left open rather than guessed at. Any class whose gear carries
    // HealingPower sums it the same way, but it only ever changes `bonus_holy_power` - a class
    // with no Holy-school spells (Mage, Warlock) computes the same total and simply never
    // reads it, by construction, not by a separate check.
    let mut spell_power = totals.spell_damage;
    let mut bonus_shadow_power = totals.shadow_damage;
    let mut bonus_fire_power = totals.fire_damage;
    let bonus_arcane_power = totals.arcane_damage;
    let mut bonus_frost_power = totals.frost_damage;
    let mut bonus_holy_power = totals.holy_damage + totals.healing_power / 3.0;
    let mut mp5 = totals.mp5 + spirit_regen_per_5s;
    if apply_buffs {
        // Flask of Supreme Power + Greater Arcane Elixir, additive - see `BUFF_SPELL_POWER`'s
        // own doc comment for why both apply, not just the higher one.
        spell_power += BUFF_SPELL_POWER + BUFF_SPELL_POWER_ELIXIR;
        mp5 += BUFF_MP5;
    }
    match buffs {
        BuilderStarterBuffs::Warlock => bonus_shadow_power += WARLOCK_BUFF_SHADOW_POWER,
        BuilderStarterBuffs::MageFire => {
            bonus_fire_power += MAGE_BUFF_FIRE_POWER;
            spell_power += MAGE_BUFF_WIZARD_OIL_SPELL_POWER;
            bonus_holy_power += MAGE_BUFF_WIZARD_OIL_HEALING_POWER / 3.0;
        }
        BuilderStarterBuffs::MageArcaneFrost => {
            bonus_frost_power += MAGE_BUFF_FROST_POWER;
            spell_power += MAGE_BUFF_WIZARD_OIL_SPELL_POWER;
            bonus_holy_power += MAGE_BUFF_WIZARD_OIL_HEALING_POWER / 3.0;
        }
        // Same two items, same amounts, as Warlock's/Mage's own packages respectively - see
        // `BuilderStarterBuffs::Priest`'s own doc comment for why this reuses Warlock's shadow
        // elixir constant rather than a new Priest-specific one.
        BuilderStarterBuffs::Priest => {
            bonus_shadow_power += WARLOCK_BUFF_SHADOW_POWER;
            spell_power += MAGE_BUFF_WIZARD_OIL_SPELL_POWER;
            bonus_holy_power += MAGE_BUFF_WIZARD_OIL_HEALING_POWER / 3.0;
            // Judgement of the Crusader (`raid.debuffs.judgementOfTheCrusader: true` - confirmed
            // present in Smite's own fixture, absent from Shadow's own - see
            // `JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER`'s own doc comment). Applied unconditionally
            // here, same reasoning as the Healing Power fix just above: Shadow Priest never
            // reads `bonus_holy_power` for any of its own (Shadow-school) spells, so adding it
            // to both builds rather than threading a new per-spec flag through this function is
            // a no-op for Shadow and correct for Smite.
            bonus_holy_power += JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER;
        }
        BuilderStarterBuffs::None => {}
    }

    Ok(CasterStats {
        spell_power,
        bonus_shadow_power,
        bonus_fire_power,
        bonus_arcane_power,
        bonus_frost_power,
        bonus_holy_power,
        spell_hit: spell_hit_percent / 100.0,
        spell_crit: spell_crit_percent / 100.0,
        haste: 1.0 + haste_percent / 100.0,
        mp5,
        max_mana: race_raw.mana + totals.mana + intellect_mana,
        spirit: total_spirit,
        intellect: total_intellect,
        // See `CasterStats::standing_curse_of_the_elements`'s own doc comment: true for every
        // Mage "builder starter" fixture (confirmed via each one's own `raid.debuffs` block),
        // false for Warlock's (which casts its own curse instead, unaffected either way). Both
        // Priest fixtures' own `raid.debuffs` blocks carry it too (confirmed 2026-10-25).
        standing_curse_of_the_elements: matches!(
            buffs,
            BuilderStarterBuffs::MageFire | BuilderStarterBuffs::MageArcaneFrost | BuilderStarterBuffs::Priest
        ),
        spell_piercing: totals.spell_piercing,
        // Judgement of Wisdom (see `CasterStats::standing_judgement_of_wisdom`'s own doc
        // comment): every "builder starter" fixture checked so far - Warlock's own *and* every
        // Mage spec's - carries `raid.debuffs.judgementOfWisdom: true`, so (unlike Curse of
        // the Elements, which only Mage's own fixtures assume) this applies to every class.
        standing_judgement_of_wisdom: apply_buffs,
    })
}

/// Resolves a named example build (e.g. `"destruction_reference"`) from the pack into the race
/// and equipment list `equipment_to_stats` wants.
pub fn example_build_equipment(build: &ExampleBuild) -> Vec<(String, Option<String>)> {
    build
        .equipment
        .iter()
        .map(|slot| (slot.item.to_string(), slot.enchant.map(|e| e.to_string())))
        .collect()
}
