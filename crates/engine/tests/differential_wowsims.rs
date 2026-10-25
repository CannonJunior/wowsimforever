//! Differential testing against `wowsims/classic` (Plan Section 0.3).
//!
//! Scope, stated honestly: this does **not** vendor or run the
//! `wowsims/classic` Go binary. Two reasons, found when actually
//! investigating this (2026-10-17) rather than assuming it was feasible:
//!
//! 1. `wowsims/classic` simulates *Classic Season of Discovery*, a
//!    different game variant from WoW Forever. Its Warlock spell data is
//!    for the old, un-reworked Classic numbers -- running its binary and
//!    diffing output against our WoW-Forever-specific data pack
//!    (`forever-beta-2026-10-17.json`) would not validate anything, since
//!    the two are *expected* to disagree on Forever-rebalanced values.
//! 2. The repository is a large (~300MB), real Go codebase. Building and
//!    scripting it for black-box output comparison, just to then discover
//!    its numbers don't even apply to Forever, would be disproportionate
//!    effort for the actual question that needs answering.
//!
//! The question that *is* answerable, and actually useful: does this
//! engine's **damage-formula structure** -- not its Forever-specific
//! numbers -- match an established, independently-implemented, published
//! reference? `wowsims/classic` is MIT-licensed (confirmed via GitHub API,
//! 2026-10-17) and its source is public, so its Go source for a
//! *classic-era* spell is read directly as reference data, the same way
//! Wowhead is read as reference data elsewhere in this project -- not run.
//!
//! Source cross-checked (as fetched 2026-10-17 from `master`):
//! `https://raw.githubusercontent.com/wowsims/classic/master/sim/warlock/shadowbolt.go`
//! defines Shadow Bolt rank 9 (the level-60 Classic rank, `SpellID: 11661`
//! -- the *same* spell ID WoW Forever's Wowhead page uses, see
//! `data-packs/forever-beta-2026-10-17.json`) as: base damage range
//! `{455, 507}`, `BonusCoefficient: 0.857`, mana cost `370`, cast time
//! `3000ms`, required level `60`. Notably the coefficient (`0.857`) is
//! *identical* to what this project independently sourced from Wowhead's
//! WoW Forever page for the same spell ID -- real cross-validation that the
//! coefficient wasn't misread -- while the base damage differs
//! (252 in Forever vs. 455-507 in Classic), consistent with WoW Forever
//! having rebalanced base damage while preserving the spell-power scaling
//! ratio. That discrepancy is expected, not a bug.
//!
//! The formula itself was also read directly, not assumed:
//! `https://raw.githubusercontent.com/wowsims/classic/master/sim/core/spell_result.go`,
//! `Spell.CalcDamage`: `baseDamage := roll(min, max); baseDamage +=
//! spell.BonusCoefficient * spell.GetBonusDamage(target)` -- an additive
//! base-plus-coefficient-times-spell-power model, with no other multiplier
//! applied for a spell with no talent/buff modifiers (`BaseDamageMultiplierAdditive`
//! and `attackerMultiplier` both default to 1). That is exactly this
//! engine's own formula (see `crates/engine/src/lib.rs`'s `run_schedule`:
//! `base + coefficient * spell_power`). This test asserts that structural
//! agreement using their exact classic-era numbers, with crit disabled --
//! crit was *not* independently re-verified against their crit-multiplier
//! constant (its definition wasn't quickly locatable in their source), so
//! this test does not claim to validate crit math, only the base formula.

use wowforever_data::{Confidence, DataPack, School, SpellDef, Source};
use wowforever_engine::{BuildConfig, PetType};

/// Classic-era (not WoW Forever) rank-9 Shadow Bolt, exactly as read from
/// `wowsims/classic`'s `sim/warlock/shadowbolt.go` on 2026-10-17. This is a
/// test-only fixture representing an *old* Classic spell rank for formula
/// cross-validation -- it is never written into a production data pack,
/// since it is not WoW Forever data at all.
fn classic_rank9_shadow_bolt() -> SpellDef {
    SpellDef {
        id: "classic_shadow_bolt_rank9".into(),
        name: "Shadow Bolt (Classic rank 9, cross-reference fixture)".into(),
        school: School::Shadow,
        cast_time_ms: 3000,
        cooldown_ms: 0,
        resource_cost: 370.0,
        base_min: 455.0,
        base_max: 507.0,
        coefficient: 0.857,
        ap_coefficient: 0.0,
        resource_type: wowforever_data::ResourceType::Mana,
        uses_weapon_damage: false,
        mana_coefficient: 0.0,
        stacking_buff: None,
        timed_buff: None,
        dot: None,
        confidence: Confidence::CommunityReported, // read from a third-party (if MIT-licensed, public) source, not Blizzard
        source: Source {
            name: "wowsims/classic sim/warlock/shadowbolt.go, rank 9 (as of 2026-10-17)".into(),
            url: Some("https://raw.githubusercontent.com/wowsims/classic/master/sim/warlock/shadowbolt.go".into()),
        },
    }
}

fn fixture_data_pack(spell: SpellDef) -> DataPack {
    DataPack {
        version: "differential-fixture".into(),
        game_build_reference: "wowsims/classic cross-reference, not a WoW Forever data pack".into(),
        spells: vec![spell],
        disputed_mechanics: vec![],
        dispute_ledger: vec![],
        demonic_sacrifice_effects: vec![],
        raid_debuffs: vec![],
    }
}

fn no_buff_build(rotation: Vec<String>, fight_duration_ms: u32) -> BuildConfig {
    BuildConfig {
        spell_power: 300.0,
        crit_chance: 0.0, // crit intentionally excluded from this comparison -- see module docs
        fight_duration_ms,
        rotation,
        pet_type: PetType::Imp,
        demonic_sacrifice_active: false,
        has_demonic_pact: false,
        resummon_event_ms: None,
        resummon_is_same_pet: false,
        dispute_rule_override: None,
        starting_mana: None,
        mana_regen_per_second: 0.0,
        attack_power: 0.0,
        weapon_min: 0.0,
        weapon_max: 0.0,
        weapon_speed_ms: 0,
        rage_enabled: false,
        is_two_handed: false,
        heroic_strike_spell_id: None,
        max_mana: 0.0,
    }
}

/// The actual differential assertion: this engine's damage output for the
/// exact classic-era numbers wowsims/classic's own source defines must fall
/// within their documented base-damage range plus the coefficient bonus --
/// i.e. this engine reproduces their additive base+coefficient*SP formula,
/// not some other shape (multiplicative, flat, etc.).
#[test]
fn single_cast_damage_matches_wowsims_classic_additive_formula_bounds() {
    let spell = classic_rank9_shadow_bolt();
    let data = fixture_data_pack(spell.clone());
    let mut build = no_buff_build(vec!["classic_shadow_bolt_rank9".into()], 3000);

    let spell_power = 300.0;
    build.spell_power = spell_power;

    let result = wowforever_engine::simulate(&data, &build, 2000, 99);

    let min_expected = spell.base_min + spell.coefficient * spell_power;
    let max_expected = spell.base_max + spell.coefficient * spell_power;

    assert!(
        result.dps.p5 >= min_expected / 3.0 - 1e-6 && result.dps.p95 <= max_expected / 3.0 + 1e-6,
        "damage fell outside wowsims/classic's documented additive-formula bounds: p5={} p95={} expected range [{}, {}] DPS (fight is 3s, one cast)",
        result.dps.p5,
        result.dps.p95,
        min_expected / 3.0,
        max_expected / 3.0
    );
}

/// The stronger, formula-shape-specific property: damage must scale
/// *linearly* in spell power with slope exactly equal to their documented
/// `BonusCoefficient` (0.857) -- this is what would actually catch a future
/// bug like "coefficient applied multiplicatively instead of additively,"
/// which a bounds check alone could miss.
#[test]
fn damage_scales_linearly_with_spell_power_at_wowsims_classic_coefficient() {
    let spell = classic_rank9_shadow_bolt();
    let data = fixture_data_pack(spell.clone());

    let mut low_sp_build = no_buff_build(vec!["classic_shadow_bolt_rank9".into()], 3000);
    low_sp_build.spell_power = 100.0;
    let mut high_sp_build = no_buff_build(vec!["classic_shadow_bolt_rank9".into()], 3000);
    high_sp_build.spell_power = 500.0;

    // Same RNG seed for both: the base-damage roll is drawn identically, so
    // any difference in output is attributable only to the spell-power term.
    let low = wowforever_engine::simulate(&data, &low_sp_build, 1, 7);
    let high = wowforever_engine::simulate(&data, &high_sp_build, 1, 7);

    let observed_slope_per_second = (high.dps.mean - low.dps.mean) / (500.0 - 100.0);
    let expected_slope_per_second = spell.coefficient / 3.0; // one 3000ms cast per 3s fight

    assert!(
        (observed_slope_per_second - expected_slope_per_second).abs() < 1e-9,
        "damage-per-spell-power slope {} did not match wowsims/classic's documented coefficient-derived slope {}",
        observed_slope_per_second,
        expected_slope_per_second
    );
}
