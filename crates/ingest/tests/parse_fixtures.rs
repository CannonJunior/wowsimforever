//! Tests `parse_spell_page` against real (trimmed) HTML fixtures saved from
//! thewowdb.com/wow-forever during the 2026-10-17 data-sourcing
//! investigation -- not synthetic text, the actual markup the parser has
//! to survive. No network access required.

use std::fs;
use wowforever_data::{Confidence, School};
use wowforever_ingest::parse_spell_page;

fn fixture(name: &str) -> String {
    fs::read_to_string(format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap_or_else(|e| panic!("missing fixture {name}: {e}"))
}

#[test]
fn parses_shadow_bolt_pure_direct_hit() {
    let html = fixture("shadow_bolt.html");
    let parsed = parse_spell_page(&html, "shadow_bolt", "Shadow Bolt", "https://thewowdb.com/wow-forever/spell/shadow-bolt-11661/").expect("should parse");

    assert_eq!(parsed.spell.school, School::Shadow);
    assert_eq!(parsed.spell.base_min, 251.0);
    assert_eq!(parsed.spell.base_max, 251.0);
    assert_eq!(parsed.spell.coefficient, 0.857);
    assert_eq!(parsed.spell.cast_time_ms, 3000);
    assert_eq!(parsed.spell.cooldown_ms, 0);
    assert_eq!(parsed.spell.resource_cost, 370.0);
    assert!(parsed.spell.dot.is_none());
    assert_eq!(parsed.spell.confidence, Confidence::BetaDatamine);
    assert!(parsed.warnings.is_empty());
}

#[test]
fn parses_corruption_pure_dot() {
    let html = fixture("corruption.html");
    let parsed = parse_spell_page(&html, "corruption", "Corruption", "https://thewowdb.com/wow-forever/spell/corruption-25311/").expect("should parse");

    assert_eq!(parsed.spell.school, School::Shadow);
    assert_eq!(parsed.spell.base_min, 0.0);
    assert_eq!(parsed.spell.base_max, 0.0);
    assert_eq!(parsed.spell.coefficient, 0.0);
    assert_eq!(parsed.spell.cast_time_ms, 2000);
    assert_eq!(parsed.spell.resource_cost, 340.0);

    let dot = parsed.spell.dot.expect("Corruption must have a DoT component");
    assert_eq!(dot.tick_base, 73.0);
    assert_eq!(dot.tick_coefficient, 0.2);
    assert_eq!(dot.tick_interval_ms, 3000);
    assert_eq!(dot.num_ticks, 6);
    assert!(!dot.can_crit);
}

#[test]
fn parses_immolate_direct_plus_dot() {
    let html = fixture("immolate.html");
    let parsed = parse_spell_page(&html, "immolate", "Immolate", "https://thewowdb.com/wow-forever/spell/immolate-25309/").expect("should parse");

    assert_eq!(parsed.spell.school, School::Fire);
    assert_eq!(parsed.spell.base_min, 158.0);
    assert_eq!(parsed.spell.base_max, 158.0);
    assert_eq!(parsed.spell.coefficient, 0.2);
    assert_eq!(parsed.spell.resource_cost, 380.0);

    let dot = parsed.spell.dot.expect("Immolate must have a DoT component");
    assert_eq!(dot.tick_base, 55.0);
    assert_eq!(dot.tick_coefficient, 0.13);
    assert_eq!(dot.num_ticks, 5);
}

/// The specific disambiguation this parser exists to get right: Shadowburn
/// has a non-damage Apply Aura (the Soul Shard flag) *and* a School Damage
/// effect. The dot pattern requires "per tick"/"ticks every" phrasing,
/// which the non-damage aura's text doesn't have -- so it must be excluded,
/// with a warning, not silently treated as a second damage component.
#[test]
fn shadowburn_excludes_its_non_damage_aura_and_warns_about_it() {
    let html = fixture("shadowburn.html");
    let parsed = parse_spell_page(&html, "shadowburn", "Shadowburn", "https://thewowdb.com/wow-forever/spell/shadowburn-18871/").expect("should parse");

    assert_eq!(parsed.spell.school, School::Shadow);
    assert_eq!(parsed.spell.base_min, 266.0);
    assert_eq!(parsed.spell.base_max, 266.0);
    assert_eq!(parsed.spell.coefficient, 0.429);
    assert_eq!(parsed.spell.cast_time_ms, 0);
    assert_eq!(parsed.spell.cooldown_ms, 15000);
    assert_eq!(parsed.spell.resource_cost, 365.0);
    assert!(parsed.spell.dot.is_none());

    assert_eq!(parsed.warnings.len(), 1, "expected exactly one warning about the excluded non-damage aura, got {:?}", parsed.warnings);
}

/// The hard-failure case this parser exists to enforce: garbage input must
/// error, never silently produce a zero-damage spell tagged as if it were
/// real data.
#[test]
fn unparseable_html_is_a_hard_error_not_a_silent_zero() {
    let result = parse_spell_page("<html><body>nothing useful here</body></html>", "mystery", "Mystery Spell", "https://example.com/");
    assert!(result.is_err());
}

/// The real bug found while sourcing Mage data: thewowdb.com omits the "It
/// scales with N times spell power" clause entirely when an effect has
/// zero spell-power scaling, rather than writing "scales with 0." The
/// original regex required that clause unconditionally, so Fireball's
/// periodic burn (which has no scaling clause) would have been silently
/// dropped -- with the direct effect still present, the "neither effect
/// found" hard-failure would never have fired, just a quietly incomplete
/// spell. This must not regress.
#[test]
fn fireball_dot_with_no_scaling_clause_is_not_dropped() {
    let html = fixture("fireball.html");
    let parsed = parse_spell_page(&html, "fireball", "Fireball", "https://thewowdb.com/wow-forever/spell/10151").expect("should parse");

    assert_eq!(parsed.spell.base_min, 451.0);
    assert_eq!(parsed.spell.coefficient, 1.0);

    let dot = parsed.spell.dot.expect("Fireball's periodic burn must not be dropped just because it has no scaling clause");
    assert_eq!(dot.tick_base, 14.0);
    assert_eq!(dot.tick_coefficient, 0.0, "a DoT with no scaling clause at all must default to coefficient 0.0, not be absent");
    assert_eq!(dot.num_ticks, 4);
    assert_eq!(dot.tick_interval_ms, 2000);
}
