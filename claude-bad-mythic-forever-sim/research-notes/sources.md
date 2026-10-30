# Research trail

How this project's Warlock data and rotations were actually obtained, in order, so a future
reader can re-verify or extend it without redoing the whole investigation.

## What didn't work, and why

The task started as "reverse engineer
`https://mythicsim.com/sims/d53a6fb8-ca3e-4c1d-95ad-a967e5216d65`". That exact URL was never
fetched: `mythicsim.com/robots.txt` disallows `/sims/` (and `/api/`, `/developer`) for every
crawler it names, including `ClaudeBot`, `Claude-User` and `Claude-SearchBot` by name. Per this
repo's own established practice (see the sibling wowsimforever project's CLAUDE.md, which
applied the same rule to a different site), an explicit per-agent disallow is respected even
when the general policy is permissive.

## What worked instead

MythicSim's WoW Forever sim runs on an open-source engine, not a black box:

- **Upstream lineage**: `wowsims/forever` (GitHub org `wowsims`, MIT), sibling to the
  well-known `wowsims/classic`, `tbc`, `wotlk`, `cata`, `sod` sim projects.
- **MythicSim's maintained fork**: `sage3648/mythicsim-forever-engine-go` (Go, the engine
  MythicSim's production Quick Sims actually ran on as of this research).
- **MythicSim's newer engine**: `sage3648/mythicsim-forever-engine` (Rust, MIT, actively
  pushed; this is the one this project's data and rotations were mined from). It carries a
  full `src/classes/warlock/` tree (spells, talents, pets, the class agent) plus a
  client-data extraction (`data/spells.jsonl`, `data/spell-effects.jsonl`) and real
  `validation/*/request.json` fixtures with actual priority-list rotations.

All of the above were verified to actually exist via `api.github.com` (not taken on a search
engine's word) before being used - see the one real wrinkle below.

### One documentation error caught, not a sourcing failure

`sage3648/mythicsim-forever-engine`'s own README cites `github.com/ElliotWood/Forever` as its
"wowsims Forever" upstream. That repo does not exist (404) and that account has no such repo
in its history. The real upstream is `wowsims/forever` (confirmed to exist, matching the
`classic`/`tbc`/`wotlk` sibling pattern). Treated as a stale/incorrect citation in someone
else's README, not as evidence the engine repo itself is fake - the repo's commits, file tree,
license and data files were all independently verified live.

## Mining the data

1. Pulled `src/classes/warlock/**` (spells, talents, pets, agent) via `raw.githubusercontent.com`
   - reading, not executing, this code.
2. Pulled `data/spells.jsonl` (3.3MB, real client-extracted rows - `manifest.json` states
   `"client_build": "1.60.1.70425"`) and extracted the 17 spells this project's rotations use.
3. Cross-checked two of those (Shadow Bolt, Immolate) against `foreverdb.net` (an independent
   WoW Forever client-data site, `robots.txt`-cleared the same way thewowdb.com was in the
   sibling project). Both matched exactly - see `data/warlock-forever-2026-10-23-rev1.json`'s
   per-spell `source` fields for the numbers.
4. Pulled three real `validation/.../*.request.json` fixtures (one per spec) and extracted
   their `priorityList` rotations - see `priority-lists.md` in this folder. This project's
   `destruction.rs`/`affliction.rs`/`demonology.rs` rotations are a direct translation of
   those real priority lists (minus consumable/potion/trinket lines this engine doesn't model).

## Follow-up pass (same day, 2026-10-23): closing the gaps the first pass left open

Two files downloaded in the first pass but never actually opened turned out to hold most of
what was missing:

- **`data/spell-extras.json`** (10KB, downloaded alongside `spells.jsonl` but not inspected
  until now) has a `curves` object mapping a talent's spell ID to its exact per-point value
  ladder - e.g. `"17959": [[20,40,60,80,100]]` for Ruin's 20%/point. Cross-checking every
  talent this project implements against it confirmed most of the code-comment-derived values
  and caught two real bugs: Aftermath is a 5-point talent (this project had it capped at 3),
  and Bane / half of Improved Corruption (both cast-time reductions) were documented in a
  doc-comment but never actually wired into the simulation.
- **`src/classes/warlock/prepare/export.rs`** (740 lines, downloaded but only grepped for
  "DemonicBrand"/"Firebolt" in the first pass, not read) turned out to hold the Imp Firebolt,
  Succubus Lash of Pain and Demonic Brand hit's actual damage formulas as literal, commented
  Go-ported constants (`imp_firebolt_effect()`, `"a Go literal base"`, `"The hit's roll and
  spell power share are Go literals"`). This replaced the first pass's explicitly-labeled
  placeholder (100.0) with a real value (45.0 at level 60) and made Demonic Brand
  implementable at all.
- **`src/prepare/character.rs`** gave `CHARACTER_LEVEL = 60` directly - the first pass had
  guessed 65 from an unrelated field (a spell's own `MaxLevel`) and was wrong.

Lesson for next time: when a research pass downloads a file "just in case" and only greps it,
go back and actually read it before concluding something is unsourceable.

## Third pass (same day, 2026-10-23): the gear system and real proc-window talents

Two more pieces, done the same way as everything above - read the real engine's source for
the formula, then mine real rows to feed it:

- **Gear**: `src/prepare/items.rs` (how an item's and enchant's raw stats sum, and Forever's
  own `unifyGearHitAndCrit` rule that merges melee and spell hit/crit from gear into one
  shared percentage), `src/prepare/stats.rs` (the stat array and its rating-dependency
  machinery), `src/mechanics/haste.rs` (the haste-rating-to-cast-speed formula) and
  `src/prepare/character.rs` (the rating-per-percent constants, all exact: 10/10/14/14/10).
  `data/go-tables.json` (a small file downloaded but not opened in the first pass, same
  lesson as `spell-extras.json` before it) turned out to hold real Warlock-specific
  Intellect-to-crit (0.0165%/point) and per-race base stats, straight from the Go reference
  engine's own tables. `data/items.jsonl` and `data/enchants.jsonl` gave the real 15-item,
  4-enchant equipment list, pulled from the exact validation fixture the rotations already
  came from - not hand-assembled.
  - One formula actively searched for and not found anywhere in the Rust source:
    Intellect-to-mana. A `15.0`-per-point stat dependency does appear, but only inside
    `stats.rs`'s own unit test as an illustrative example of the dependency-manager API - not
    confirmed as a real registered Warlock value. Left unimplemented rather than guessed; see
    `gear.rs`'s docs.
- **Proc-window talents**: real durations for Shadow Vulnerability (Improved Shadow Bolt's
  debuff, 12s), Shadow Trance (Nightfall's buff, 10s) and Shadow and Flame's two buffs (20s
  each) came straight from `data/spells.jsonl`'s `DurationMs` field for each triggered aura's
  spell ID (17794, 17941, 426311, 1293816) - the same file the main spell pack was mined from,
  just different rows. Nightfall's exact 2%/4% proc chance came from `spell-extras.json`'s
  curve for spell 18094. The real trigger mask (`NIGHTFALL_SPELLS` = Corruption | Drain Life |
  Drain Soul | Wrack, from `src/classes/warlock/prepare/masks.rs`, fetched to settle this
  rather than assumed) confirmed Corruption is the only one of those four this engine's
  Affliction rotation actually casts, so that's the only trigger implemented - not a narrowed
  approximation of the real mask, just what's actually in play.

## Fourth pass (2026-10-24): closing the last of the "genuine gaps," and finding the real upstream had what the port didn't

Two formulas (Intellect-to-mana, Spirit-to-mana-regen) were marked unsourced after the third
pass despite a real search, because the search only looked in the Rust port
(`sage3648/mythicsim-forever-engine`). This pass went one level further upstream, to
`wowsims/forever` itself (the real Go engine that port is ported *from*, already confirmed to
exist back in the first pass) - `sim/core/mana.go` and `sim/core/character_constants.go`,
fetched directly via `api.github.com`/`raw.githubusercontent.com` the same way as always.
Both formulas were right there, exactly as real WoW theorycrafting communities have long
described them (base mana ≈ 15/Intellect - 280, spirit regen ≈ 6.25 + Spirit/8 per second) -
confirming the earlier "not found" conclusion was about this *engine's Rust port* specifically,
not that the formulas don't exist anywhere in the project's lineage. Lesson for next time: when
a formula search comes up empty in a fork, check the thing it was forked from before
concluding it's unsourceable.

The same file (`sim/warlock/lifetap.go`, upstream Go - the Rust port doesn't carry this file at
all) gave Life Tap's exact formula directly: `amount := baseAmount + Spirit; AddMana(amount *
improvedLifeTapMult)`.

Everything else this pass touched reused data already mined in earlier passes:
- Succubus's Lash of Pain numbers (base 50.0, coefficient 0.429, 12s cooldown) were already in
  `data/warlock-forever-2026-10-23-rev2.json` from the second pass (mined "for when the
  Succubus is added," which is now).
- Cataclysm's -3/-6/-10% (spell 17778) and Decimation's 35% threshold and 3%/6% damage bonus
  (spell 440870, the third and fourth curve entries) were already confirmed in
  `data/spell-extras.json`'s `curves` table back in the second pass, just not wired into any
  Talents struct until this one.
- Each spell's real school (Shadow=32, Fire=4) was already in the data pack's `school` field
  (mined in the first pass) but never parsed into `SpellRow` or read by the damage formulas
  until this pass.

## What's still unverified

The Imp's Firebolt (spell 11763) and Succubus's Lash of Pain (spell 11780) are pet-only
spells. The client-data table in step 2 above doesn't include pet spell rows at all (the
engine's own comment: "The generator makes no table for pet spells"), and `foreverdb.net`
404s for both IDs. `demonology.rs`'s `FIREBOLT_BASE_DAMAGE_UNVERIFIED` is a placeholder, not a
sourced number - Demonology's DPS output should be read as illustrative only until that's
replaced with something real (a manual tooltip read from a live Forever beta client, or a
community-reported value, would both be better than what's there now).

## Mining script

`tools/mine_warlock_data.py` is the actual extraction script used in step 2-3, kept so the
pack can be regenerated or extended (e.g. against a newer client build) without redoing this
research from scratch.
