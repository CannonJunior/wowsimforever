#!/usr/bin/env python3
"""Regenerates data/warlock-forever-*.json from a local copy of
sage3648/mythicsim-forever-engine's data/spells.jsonl (MIT licensed; see
research-notes/sources.md for provenance and the cross-check methodology).

This is NOT run automatically - the data pack it produces was hand-reviewed and
cross-checked against foreverdb.net before being committed (see the pack's own
`provenance_note` and `known_conflicts` fields). Re-run it only to regenerate against a newer
client-data dump, and re-do the cross-check afterwards.

Usage:
    curl -s -o /tmp/spells.jsonl \\
        https://raw.githubusercontent.com/sage3648/mythicsim-forever-engine/main/data/spells.jsonl
    python3 tools/mine_warlock_data.py /tmp/spells.jsonl data/warlock-forever-<date>-revN.json
"""
import json
import sys

# highest-rank spell id for each core Warlock ability this project's three spec rotations use.
# See research-notes/priority-lists.md for why exactly these 17 and not the rest of the
# Warlock spellbook (hellfire, rain of fire, death coil, bane of havoc, wrack and drain life
# were mined once but turned out not to appear in any of the three real priority lists this
# project's rotations are based on, so they were left out of the shipped pack).
TARGET_SPELL_IDS = {
    "shadow_bolt": 25307,
    "corruption": 25311,
    "immolate": 25309,
    "incinerate": 1293813,
    "conflagrate": 18932,
    "shadowburn": 18871,
    "searing_pain": 17923,
    "soul_fire": 17924,
    "life_tap": 11689,
    "bane_of_agony": 11713,
    "bane_of_doom": 603,
    "siphon_life": 18881,
    "drain_life": 11700,
    "wrack": 1316697,
    "curse_of_the_elements": 1311680,
    "curse_of_recklessness": 11717,
    "amplify_curse": 18288,
}

ENGINE_SRC = "sage3648/mythicsim-forever-engine (MIT license), data/spells.jsonl, client build {build}"


def direct_effect(row):
    for e in row.get("Effects", []):
        if e.get("Type") == 2:
            return e
    return None


def periodic_effect(row):
    for e in row.get("Effects", []):
        if e.get("Type") == 6 and "PeriodMs" in e and "SPCoef" in e:
            return e
    return None


def mine(spells_jsonl_path, build="1.60.1.70425"):
    by_id = {v: k for k, v in TARGET_SPELL_IDS.items()}
    rows = {}
    with open(spells_jsonl_path) as f:
        for line in f:
            d = json.loads(line)
            sid = d.get("ID")
            if sid in by_id:
                rows[by_id[sid]] = d

    missing = set(TARGET_SPELL_IDS) - set(rows)
    if missing:
        print(f"WARNING: missing spell ids for: {sorted(missing)}", file=sys.stderr)

    spells = {}
    for name, d in rows.items():
        out = {
            "spell_id": d["ID"],
            "rank": d.get("Rank"),
            "school": d.get("School"),
            "cast_time_ms": d.get("CastTimeMs"),
            "gcd_ms": d.get("GCDMs"),
            "cooldown_ms": d.get("CooldownMs") or d.get("CategoryCooldownMs"),
            "mana_cost": (d.get("Powers") or [{}])[0].get("Cost"),
            "max_range": d.get("MaxRange"),
            "duration_ms": d.get("DurationMs"),
        }
        de = direct_effect(d)
        if de:
            out["direct"] = {
                "base_points": de.get("BasePoints"),
                "sp_coefficient": de.get("SPCoef"),
                "variance": de.get("Variance"),
            }
        pe = periodic_effect(d)
        if pe:
            out["periodic"] = {
                "base_points_per_tick": pe.get("BasePoints"),
                "sp_coefficient": pe.get("SPCoef"),
                "period_ms": pe.get("PeriodMs"),
                "num_ticks": (d["DurationMs"] // pe["PeriodMs"])
                if d.get("DurationMs") and pe.get("PeriodMs")
                else None,
            }
        out["confidence"] = "beta-datamine"
        out["source"] = f"{ENGINE_SRC.format(build=build)}, spell id {d['ID']}. NEEDS RE-CROSS-CHECK after regeneration."
        spells[name] = out
    return spells


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    spells = mine(sys.argv[1])
    pack = {
        "schema_version": 1,
        "pack_version": "REGENERATE ME: set a real version string",
        "spells": spells,
    }
    with open(sys.argv[2], "w") as f:
        json.dump(pack, f, indent=2)
    print(f"wrote {len(spells)} spells to {sys.argv[2]}")
