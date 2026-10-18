//! Ingestion from thewowdb.com/wow-forever/ (Plan Section 4.5's eighth
//! update): a directly-fetchable, explicitly bot-crawling-welcoming source
//! whose spell pages cite genuine client-data extraction ("Generated from
//! the SpellEffect, SpellMisc, SpellCooldowns and SpellRange records in the
//! WoW Forever beta client"). This replaces the ad-hoc curl+Python process
//! used to source the current production data pack with a tested,
//! repeatable tool.
//!
//! Parsing is deliberately separated from fetching (`parse_spell_page`
//! takes raw HTML, not a URL) so it can be unit-tested against saved
//! fixtures without a network call -- see `tests/fixtures/`.
//!
//! Design principle carried over from CLAUDE.md's placeholder-data policy:
//! a field this parser cannot confidently find is a hard error, not a
//! silent zero. Guessing a number and labeling it `beta-datamine` would be
//! worse than Wowhead's original failure mode, not better.

use regex::Regex;
use wowforever_data::{Confidence, DotDef, School, Source, SpellDef};

/// Strips HTML tags and script/style blocks, decodes a handful of common
/// entities, and collapses whitespace -- enough to make thewowdb.com's
/// prose ("Each cast of X costs N Mana...") regex-matchable, the same
/// approach used (and verified) during this project's manual research.
pub fn strip_tags(html: &str) -> String {
    let no_script = Regex::new(r"(?is)<script.*?</script>").unwrap().replace_all(html, " ").into_owned();
    let no_style = Regex::new(r"(?is)<style.*?</style>").unwrap().replace_all(&no_script, " ").into_owned();
    let no_tags = Regex::new(r"(?s)<[^>]+>").unwrap().replace_all(&no_style, " ").into_owned();
    let decoded = no_tags.replace("&nbsp;", " ").replace("&amp;", "&").replace("&#x27;", "'").replace("&quot;", "\"");
    Regex::new(r"\s+").unwrap().replace_all(&decoded, " ").trim().to_string()
}

#[derive(Debug)]
pub struct ParsedSpell {
    pub spell: SpellDef,
    /// Non-fatal observations worth a human's attention (e.g. an
    /// unrecognized extra effect that wasn't counted as damage).
    pub warnings: Vec<String>,
}

fn find_school(text: &str) -> Option<School> {
    let re = Regex::new(r"is a (Shadow|Fire|Physical|Frost|Arcane|Nature|Holy) spell").unwrap();
    let caps = re.captures(text)?;
    match caps.get(1)?.as_str() {
        "Shadow" => Some(School::Shadow),
        "Fire" => Some(School::Fire),
        "Physical" => Some(School::Physical),
        "Frost" => Some(School::Frost),
        "Arcane" => Some(School::Arcane),
        "Holy" => Some(School::Holy),
        // Nature isn't in this project's School enum yet (no spell sourced
        // so far has needed it) -- reported as unsupported rather than
        // silently mapped to the wrong school.
        _ => None,
    }
}

fn find_cast_time_ms(text: &str) -> Option<u32> {
    if Regex::new(r"with an instant cast").unwrap().is_match(text) {
        return Some(0);
    }
    let re = Regex::new(r"with an? ([\d.]+) sec cast").unwrap();
    re.captures(text).and_then(|c| c.get(1)?.as_str().parse::<f64>().ok()).map(|s| (s * 1000.0).round() as u32)
}

fn find_cooldown_ms(text: &str) -> u32 {
    let re = Regex::new(r"a ([\d.]+) sec cooldown").unwrap();
    re.captures(text).and_then(|c| c.get(1)?.as_str().parse::<f64>().ok()).map(|s| (s * 1000.0).round() as u32).unwrap_or(0)
}

fn find_resource_cost(text: &str) -> Option<f64> {
    let re = Regex::new(r"costs ([\d.]+) Mana").unwrap();
    re.captures(text).and_then(|c| c.get(1)?.as_str().parse::<f64>().ok())
}

struct DirectEffect {
    base: f64,
    coefficient: f64,
}

fn find_direct_effect(text: &str) -> Option<DirectEffect> {
    // The "It scales with N times spell power" clause is **omitted entirely**
    // when a direct-hit effect has zero spell-power scaling -- confirmed on
    // real data (Fireball's rank-11 periodic burn has no scaling clause at
    // all, see find_dot_effect's comment below; the same omission pattern
    // must be assumed possible here too). An earlier version of this regex
    // required the clause unconditionally, which would have returned `None`
    // for any zero-coefficient direct effect instead of correctly reading a
    // coefficient of 0.0 -- made optional, with an explicit default, rather
    // than assumed never to happen.
    //
    // Healing spells use a *different* client effect type -- "Heal (effect
    // type 10)" -- not "School Damage (effect type 2)" at all, found while
    // sourcing Priest data. Same base-value/coefficient phrasing otherwise,
    // so matched with the same regex rather than a separate function; this
    // project's data model (base_min/base_max/coefficient) doesn't
    // distinguish damage from healing at the field level, only at the
    // engine that reads it (crates/heal vs crates/engine), so reusing the
    // extraction logic here is a deliberate match to that, not a shortcut.
    let re = Regex::new(r"(?:School Damage \(effect type 2\)|Heal \(effect type 10\))\.\s*Its base value is ([\d.]+)\.(?:\s*It scales with ([\d.]+) times spell power)?").unwrap();
    let caps = re.captures(text)?;
    Some(DirectEffect {
        base: caps.get(1)?.as_str().parse().ok()?,
        coefficient: caps.get(2).map(|m| m.as_str().parse().unwrap_or(0.0)).unwrap_or(0.0),
    })
}

fn find_dot_effect(text: &str) -> Option<DotDef> {
    // Real counter-example that broke the original unconditional regex:
    // Fireball's level-60 rank (thewowdb.com spell 10151) has a periodic
    // burn described as "Its base value is 14 per tick. It ticks every 2
    // sec..." with **no** "It scales with..." clause in between at all --
    // the site omits that clause rather than writing "scales with 0 times
    // spell power" when the coefficient is zero. The original regex
    // required that clause unconditionally, which would have returned
    // `None` here and silently dropped Fireball's entire DoT component
    // (the caller only errors when *neither* a direct nor a DoT effect is
    // found -- with the direct effect present, the missing DoT would never
    // have surfaced as an error, just a quietly wrong spell). Covered by
    // `fireball_dot_with_no_scaling_clause_is_not_dropped` in
    // tests/parse_fixtures.rs.
    let re = Regex::new(
        r"Apply Aura \(effect type 6\)\.\s*It applies aura type \d+\.\s*Its base value is ([\d.]+) per tick\.(?:\s*It scales with ([\d.]+) times spell power\.)?\s*It ticks every (\d+) sec, (\d+) times over the full (\d+) sec",
    )
    .unwrap();
    let caps = re.captures(text)?;
    Some(DotDef {
        tick_base: caps.get(1)?.as_str().parse().ok()?,
        tick_coefficient: caps.get(2).map(|m| m.as_str().parse().unwrap_or(0.0)).unwrap_or(0.0),
        tick_interval_ms: caps.get(3)?.as_str().parse::<u32>().ok()? * 1000,
        num_ticks: caps.get(4)?.as_str().parse().ok()?,
        // thewowdb.com's own pages never describe a periodic effect as
        // crit-eligible in the prose checked so far, and this matches
        // every DoT already in the production pack -- flagged in the
        // warning returned by the caller, not silently assumed forever.
        can_crit: false,
    })
}

/// Parses one thewowdb.com/wow-forever spell detail page. `id` is this
/// project's own snake_case identifier (the site's numeric spell ID is not
/// reused as our id); `name` and `source_url` are supplied by the caller
/// since they're already known before fetching.
pub fn parse_spell_page(html: &str, id: &str, name: &str, source_url: &str) -> Result<ParsedSpell, String> {
    let text = strip_tags(html);
    let mut warnings = Vec::new();

    let school = find_school(&text).ok_or_else(|| format!("could not determine spell school for `{id}` -- refusing to guess"))?;
    let cast_time_ms = find_cast_time_ms(&text).ok_or_else(|| format!("could not determine cast time for `{id}` -- refusing to guess"))?;
    let cooldown_ms = find_cooldown_ms(&text);
    let resource_cost = find_resource_cost(&text).ok_or_else(|| format!("could not determine mana cost for `{id}` -- refusing to guess"))?;

    let direct = find_direct_effect(&text);
    let dot = find_dot_effect(&text);

    if direct.is_none() && dot.is_none() {
        return Err(format!(
            "found neither a School Damage effect nor a periodic-damage Apply Aura for `{id}` -- this page may use phrasing this parser doesn't recognize yet, refusing to emit a zero-damage spell"
        ));
    }
    if dot.is_none() && Regex::new(r"Apply Aura \(effect type 6\)").unwrap().is_match(&text) {
        warnings.push(format!(
            "`{id}`: page mentions an Apply Aura effect that did not match the periodic-damage pattern (no \"per tick\"/\"ticks every\" phrasing found) -- likely a non-damage aura (e.g. Shadowburn's Soul Shard flag), excluded as intended, but verify manually."
        ));
    }

    let (base_min, base_max, coefficient) = match &direct {
        Some(d) => (d.base, d.base, d.coefficient),
        None => (0.0, 0.0, 0.0),
    };

    let spell = SpellDef {
        id: id.to_string(),
        name: name.to_string(),
        school,
        cast_time_ms,
        cooldown_ms,
        resource_cost,
        base_min,
        base_max,
        coefficient,
        // thewowdb.com's extracted pages only ever cite a spell-power
        // coefficient, never an attack-power one -- 0.0 here is an
        // honest "not applicable to anything this tool extracts," not a
        // guess. Attack-power-scaling abilities (Bloodthirst etc.) are
        // sourced by hand into the data pack, not through this tool.
        ap_coefficient: 0.0,
        dot,
        confidence: Confidence::BetaDatamine,
        source: Source {
            name: format!(
                "thewowdb.com/wow-forever {name} -- 'Generated from the SpellEffect, SpellMisc, SpellCooldowns and SpellRange records in the WoW Forever beta client.'"
            ),
            url: Some(source_url.to_string()),
        },
    };

    Ok(ParsedSpell { spell, warnings })
}
