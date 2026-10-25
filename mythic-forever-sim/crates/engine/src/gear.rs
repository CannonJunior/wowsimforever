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

pub const EMBEDDED_WARLOCK_GEAR_PACK_JSON: &str =
    include_str!("../../../data/warlock-gear-2026-10-24-rev3.json");

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
/// 0.0165 - the whole reason this struct's fields dropped their "warlock_" prefix).
pub const EMBEDDED_MAGE_GEAR_PACK_JSON: &str =
    include_str!("../../../data/mage-gear-2026-10-25-rev2.json");

pub fn embedded_mage_gear_pack() -> GearPack {
    GearPack::load_str(EMBEDDED_MAGE_GEAR_PACK_JSON).expect("embedded mage gear pack must parse")
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
/// directly in both the Warlock and Mage request JSON, 2026-10-24/25). The Warlock package's
/// request JSON also lists a `spellPowerElixirId` (Greater Arcane Elixir, item 13454: +35
/// universal spell damage) - confirmed via `wowsims/forever`'s own `ExclusivePerStat` rule
/// (`sim/common/shared/consumable_stats.go`) that only the single highest buff of the *same*
/// stat applies, the elixir's own +35 is strictly inert once the flask (150) is up, so it's
/// deliberately not added separately for either class below.
const BUFF_SPELL_POWER: f64 = 150.0;

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
}

/// Elixir of Shadow Power (item 9264, Warlock's package's own `schoolElixirId`): +40
/// Shadow-specific spell damage, 30 min - a *different* stat from the flask's (school-specific,
/// not universal), so `ExclusivePerStat` doesn't touch it; it stacks on top.
const WARLOCK_BUFF_SHADOW_POWER: f64 = 40.0;

/// Mage's own package consumables, different from Warlock's:
/// - Elixir of Fire Power (item 6373, this package's own `schoolElixirId` - a different, lower
///   item than Warlock's Shadow Power one): +10 Fire-specific spell damage, 30 min. A real,
///   smaller number for a genuinely different item - not a mistake to double check against
///   Warlock's +40.
/// - Brilliant Wizard Oil (item 20749, a weapon imbue - Warlock's own package carries none at
///   all): +36 universal spell damage and +1% spell crit, 30 min. The spell-damage half is the
///   *same* stat as the Flask's own (both universal `SpellDamage`), so by the same
///   `ExclusivePerStat` rule the flask (150) already wins over Greater Arcane Elixir's 35, it
///   should also strictly dominate this oil's 36 - not added separately here, matching that
///   rule, not an oversight. The crit half is a different stat (not spell damage), and a weapon
///   imbue isn't one of the elixir-vs-elixir or elixir-vs-raid-buff categories `consumable_stats.go`'s
///   own exclusivity rules describe, so it's modeled as additive on top of Moonkin Aura's own
///   3% - an assumption, not independently confirmed from source, flagged as such.
const MAGE_BUFF_FIRE_POWER: f64 = 10.0;
const MAGE_BUFF_SPELL_CRIT_PCT: f64 = 1.0;

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
/// Blessing of Wisdom (spell 25918): +40 mp5, 1h, on top.
///
/// **Not modeled from this same package** (documented rather than silently applied or guessed
/// at): Mana Spring Totem's own mp5 tick (spell 10494 - its real numeric tick value wasn't in
/// any static tooltip text this research pass could find; a sustain-only stat regardless, see
/// "Scope" in the README on why mp5-only gaps don't move this engine's reported DPS under
/// comfortable mana). Judgement of Wisdom's mana-on-hit, the Major Mana Potion and the Demonic
/// Rune (both combat-use items, not standing pre-pull buffs - this engine still doesn't
/// simulate consumable *usage* mid-fight, only standing buffs, which is why those two differ
/// from the flask/elixirs/food/potion-as-buff above).
const BUFF_MP5: f64 = 12.0 + 40.0;

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
        BuilderStarterBuffs::MageFire | BuilderStarterBuffs::MageArcaneFrost
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
    const INTELLECT_TO_MANA_PER_POINT: f64 = 15.0;
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

    let mut spell_power = totals.spell_damage;
    let mut bonus_shadow_power = totals.shadow_damage;
    let mut bonus_fire_power = totals.fire_damage;
    let bonus_arcane_power = totals.arcane_damage;
    let mut bonus_frost_power = totals.frost_damage;
    let mut mp5 = totals.mp5 + spirit_regen_per_5s;
    if apply_buffs {
        spell_power += BUFF_SPELL_POWER;
        mp5 += BUFF_MP5;
    }
    match buffs {
        BuilderStarterBuffs::Warlock => bonus_shadow_power += WARLOCK_BUFF_SHADOW_POWER,
        BuilderStarterBuffs::MageFire => bonus_fire_power += MAGE_BUFF_FIRE_POWER,
        BuilderStarterBuffs::MageArcaneFrost => bonus_frost_power += MAGE_BUFF_FROST_POWER,
        BuilderStarterBuffs::None => {}
    }

    Ok(CasterStats {
        spell_power,
        bonus_shadow_power,
        bonus_fire_power,
        bonus_arcane_power,
        bonus_frost_power,
        spell_hit: spell_hit_percent / 100.0,
        spell_crit: spell_crit_percent / 100.0,
        haste: 1.0 + haste_percent / 100.0,
        mp5,
        max_mana: race_raw.mana + totals.mana + intellect_mana,
        spirit: total_spirit,
        intellect: total_intellect,
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
