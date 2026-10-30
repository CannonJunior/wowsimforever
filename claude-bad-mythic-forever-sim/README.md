# mythic-forever-sim

A from-scratch WoW Forever DPS simulator, started as Warlock-only and now beginning to cover
the game's other classes (Mage second, Priest third, from 2026-10-25). Built by
reverse-engineering [MythicSim](https://mythicsim.com/wow-forever)'s own sims - see
`research-notes/sources.md` for exactly how, since the obvious path (fetching MythicSim's own
sim page) turned out to be blocked by that site's own `robots.txt` and a better path existed.
Warlock (all three specs), Mage (Fire, Arcane and Frost) and Priest (Shadow and Smite) have all
been compared ability-by-ability against the real engine's own trace, not just an aggregate DPS
number - see "Mage"/"Priest" below and `CLAUDE.md`'s hard rule for why that comparison is
mandatory for every class/spec this project adds from here on.

**This project shares no code, data files, or build configuration with this repository's other
project (`wowsimforever` / `crates/`, `web/`, `data-packs/`, etc.)** It is its own Cargo
workspace (`mythic-forever-sim/Cargo.toml`, not a member of the root workspace) with its own
data pack, mined independently. The only relationship is that both happen to live in this same
git checkout.

## Status

Single-target, core-rotation DPS for all three Warlock specs (Affliction, Demonology,
Destruction). That phrase is doing real work - see "Scope" below for what it deliberately
excludes, and "Known gaps" for what's in-scope but not yet trustworthy.

**Compared against mythicsim.com, then the gap closed (2026-10-24)** - see
`research-notes/mythicsim-comparison.md` for the full methodology (the live site's `/sims/`
and `/api/` are robots.txt-disallowed the same way they were for the original reverse-engineering
task, so this used mythicsim-forever-engine's own checked-in Go/Rust validation corpus instead,
which is arguably better ground truth: exact, versioned, reproducible). The comparison caught
and fixed a real bug first - `gear::equipment_to_stats` was dropping the ~83% base spell hit
chance every character has before gear, feeding only the 4-5% gear bonus into the sim and
missing ~95% of casts. A follow-up pass then implemented every specific, named talent the
comparison's own build-vs-struct diff turned up as missing (Ruin, Improved Shadow Bolt, Bane
and Agonizing Flames for Affliction/Demonology; Demonic Sacrifice, Unholy Power and Soul Link
for Demonology; a Demonic Brand/Master Demonologist pet-gating bug; and a second real bug in
Destruction's own Agonizing Flames). Together these closed the gap from ~4-6x down to
~1.7-2.2x. A third follow-up pass (same day) gave Affliction the pet simulation it was missing
entirely, closing its gap further to 1.71x, and audited the gear system against the real
engine's item mechanics for completeness (`pseudo_stats`, random suffixes, item effects, item
set bonuses - all checked and confirmed inert for this pack's actual items, documented rather
than silently dropped). A fourth pass ("analyze why the gap is still large, then address it",
same day) found the real dominant cause had never actually been pet melee auto-attack: all
three "builder starter" fixtures run with a full raid-buff + consumable package
(`--buffs builder-starter`) this engine never applied, plus Curse of the Elements' own +10%
damage-taken effect was a complete no-op despite every rotation casting it, plus Demonology's
own rotation was missing two of its real actions (Curse of the Elements and Immolate) entirely.
Fixing all three closed most of the gap. A fifth pass ("produce a per-ability breakdown in both
sims and analyze the differences", same day) built mythicsim-forever-engine's own Rust CLI
locally (MIT-licensed, the same repo this whole comparison treats as ground truth) and ran it
against the identical request files to get a genuine per-ability trace from the real engine,
not just its total DPS - and found a real damage-calculation bug (dot-only spells never rolled
their own hit chance), a reporting bug (dot ticks were counted as "casts"), a wrong CLI default
(Affliction's real build has 0 points in Siphon Life), and a whole missing cross-tree talent
(Suppression, +1%/point hit, present in all three builds, modeled in none) plus one more
missing field (Destruction's own Improved Corruption). That pass left Demonology's gap (1.35x)
measured but unfixed: ~71 DPS of it was the Succubus's unmodeled melee auto-attack. A sixth
pass ("fix the differences in Demonology", same day) built it for real - weapon damage, attack
power, glancing blows and melee crit all read from the real engine's own source, with the two
numbers no general formula was found for in time (the Succubus's innate attack power and base
crit) taken directly from that same engine's own debug combat log for this comparison's exact
fixture, not guessed. Also found and fixed a real scheduling bug the new melee timeline made
impossible to miss: the pet's spell-cast and melee timers both silently lost ground whenever
another actor's turn came first, instead of keeping their own fixed cadence. Final:
**0.99x for Demonology, 0.99x for Destruction** (both essentially matched), **0.92x for
Affliction** (now running a few percent ahead, not behind). See
`research-notes/mythicsim-comparison.md` for the full per-ability tables and analysis.

## Running it

By hand-specified stats:

```sh
cd mythic-forever-sim
cargo run -p mythic-forever-cli -- --spec destruction --spell-power 450 --crit 25 --hit 99 --haste 10 --duration 180 --iterations 3000
cargo run -p mythic-forever-cli -- --spec affliction  --spell-power 450 --crit 25 --hit 99 --haste 10
cargo run -p mythic-forever-cli -- --spec demonology  --spell-power 450 --crit 25 --hit 99 --haste 10
cargo test
```

`--class` picks the class (`warlock`, the default, or `mage` - see "Mage" below); `--spec` is
within that class and defaults to a sensible first choice (`destruction` for Warlock, `fire`
for Mage) if left out:

```sh
cargo run -p mythic-forever-cli -- --class mage --spell-power 450 --crit 25 --hit 99
```

Or from real gear (see "Gear system" below):

```sh
cargo run -p mythic-forever-cli -- --spec destruction --gear destruction_reference
```

Demonology can run either pet, Affliction defaults to a summoned Succubus (the real "builder
starter" build's own choice) but can run either pet or none, and Decimation needs an explicit
target health to ever matter:

```sh
cargo run -p mythic-forever-cli -- --spec demonology --pet succubus --spell-power 450 --crit 25 --hit 99
cargo run -p mythic-forever-cli -- --spec affliction --pet none --spell-power 450 --crit 25 --hit 99
cargo run -p mythic-forever-cli -- --spec demonology --target-health 50000 --spell-power 450 --crit 25 --hit 99
```

`--gear` can be layered with `--buffs builder-starter` to also apply the exact raid-buff +
consumable package the mythicsim.com comparison's own fixtures use (flask, elixirs, food, raid
buffs - see "Gear system" below for the full sourced list):

```sh
cargo run -p mythic-forever-cli -- --spec affliction --gear affliction_starter_undead --race RaceUndead --pet succubus --buffs builder-starter
```

All flags are optional (see `crates/cli/src/main.rs` for the full list and defaults); `--spec`
is the main one. `--hit`/`--crit` are percentages (e.g. `25` = 25%), `--haste` is also a
percentage (e.g. `10` = +10% haste), `--duration` is seconds. Any of `--spell-power`/
`--shadow-power`/`--fire-power`/`--hit`/`--crit`/`--haste`/`--mp5`/`--max-mana` passed alongside
`--gear` overrides that one computed field (useful for layering in anything `--buffs` doesn't
cover, like a different buff loadout than the one named preset).

## Scope

What this models: the three specs' real single-target rotations (mined from MythicSim's own
engine's validation fixtures - see `research-notes/priority-lists.md`), their core damage
spells and the talents that materially change those rotations' output (now including
Decimation and Cataclysm - see "Known gaps"), hit/crit/variance rolls, Classic-style dot
snapshotting, mana management via Life Tap with its real Spirit-scaling formula, real
proc-window talents with their actual durations and trigger conditions (Nightfall, Improved
Shadow Bolt, Shadow and Flame), either pet for Demonology or Affliction (Imp or Succubus,
or no pet at all - `--pet`) including the Succubus's own melee auto-attack (weapon damage,
attack power, glancing blows, melee crit - see "Known gaps" below, eighth pass, for the exact
mechanics and their sourcing), Curse of the Elements' real +10% magic-damage-taken effect (not
just its cast/refresh timing), computing final stats from real gear including school-specific
spell power and real Intellect/Spirit-derived mana and regen, and optionally the exact
raid-buff + consumable package the mythicsim.com comparison's own fixtures use
(`--buffs builder-starter` - see "Gear system" below).

What it deliberately does not model, as a first-milestone scope cut, not an oversight:
multi-target/AoE (Hellfire, Rain of Fire, Bane of Havoc), missile travel time, the 5-second-rule
mana regen interruption (mana regen is otherwise now a real formula - see "Gear system"),
partial resists, racial abilities (the Undead's own Touch of the Grave, measured directly from
the real engine's trace at ~6 DPS for both specs that can run a Succubus), and consumables
*used* mid-fight (potions, the Demonic Rune - only standing pre-pull buffs are modeled, see
"Gear system").

## Gear system

Added in the third research/implementation pass (2026-10-23), reverse-engineered from
`src/prepare/items.rs`, `src/prepare/stats.rs`, `src/mechanics/haste.rs` and
`src/prepare/character.rs` (formulas) plus `data/items.jsonl`, `data/enchants.jsonl` and
`data/go-tables.json` (real item/enchant/race-stat rows) in the same source repository. Full
citations in `gear.rs`'s module docs and `data/warlock-gear-2026-10-24-rev3.json`'s
`provenance_note`.

`gear::equipment_to_stats(pack, race, equipment)` computes a `CasterStats` from a race and a
list of equipped (item, enchant) pairs using the real formulas: rating-to-percent conversions
(10 spell/melee hit rating per 1%, 14 spell/melee crit rating per 1%, 10 spell haste rating per
1%), Forever's own gear rule that unifies melee and spell hit/crit from gear into one shared
percentage credited to both, Intellect-to-crit at the Warlock-specific rate (0.0165%/point,
different from other classes' rates in the same source table), the five Warlock-playable
races' real base stats, and the real 83% base spell hit chance vs a level-63 target that every
character has before any gear at all (confirmed via `wowsims/forever`'s own
`attack_table_test.go` - added after the mythicsim.com comparison caught it missing entirely;
see `research-notes/mythicsim-comparison.md`).

The pack ships one real example build (`destruction_reference`): the exact 15-item,
4-enchant equipment list from the same validation fixture the rotations were mined from, not
hand-assembled. `gear_sanity.rs`'s tests pin the computed totals against hand-checked numbers.

**Fixed in the fourth research/implementation pass (2026-10-24)**, by reading the real
upstream Go engine (`wowsims/forever`, not just its Rust port - `sim/core/mana.go` and
`sim/core/character_constants.go`, fetched directly after the Rust port's own source turned up
nothing):
- ~~Items' school-specific ShadowDamage/FireDamage stats are parsed but not applied~~ - fixed.
  `CasterStats` now has `bonus_shadow_power`/`bonus_fire_power` alongside the universal
  `spell_power`, and the engine looks up each spell's real client school (Shadow/Fire) to add
  the right one. Also fixed `resolve_direct_raw` (the Imp's Firebolt, the Succubus's Lash of
  Pain, and the Demonic Brand hit, none of which have a data-pack row to read a school off of)
  to take the caller-supplied bonus directly.
- ~~No Intellect-to-mana bonus~~ - fixed, with the real formula this time:
  `mana.go`'s `EnableManaBarWithModifier` does `AddStat(Mana, 20-15*20)` then
  `AddStatDependency(Intellect, Mana, 15)` - i.e. -280 flat, +15/point of total Intellect, on
  top of the race's raw `base_stats` entry. (The earlier "only a unit test's illustrative
  example" read was of the *Rust port*; the real upstream Go registers it for real, for every
  mana-using character including Warlock.)
- ~~No Spirit-derived mana regen~~ - fixed. Same file: `spiritRegenBase, spiritRegenPerSpirit =
  6.25, 1.0/8`, confirmed as Warlock's actual rate (its `warlock.go` calls plain
  `EnableManaBar()`, no class override). `mp5` is now items' flat MP5 stat plus
  `5 * (6.25 + total_Spirit * 0.125)` - a real formula, though still applied continuously since
  this engine doesn't model the 5-second-rule's casting/not-casting split (see "Scope").

**Audited for completeness in the "simulation differences + gear" pass (2026-10-24)**: the data
pack's own `unimplemented_but_parsed` field still claimed ShadowDamage/FireDamage and
Intellect-to-mana were unapplied, which was stale (both were implemented earlier in this same
pass, just never documented as such) - fixed, and re-fetched the source engine's own
`data/items.jsonl` directly to check for anything else a real item/enchant can carry that this
pack's mining step might silently drop:
- `pseudo_stats` (an alternate percentage-based stat representation, present on 23 rows of the
  source's full item database) and item set bonuses (`set_id`/`set_name` - this pack's own
  10038 Stormcloth Shoulders is part of a 6-piece set) are real concepts, but this pack's own
  flattening step (item rows reduced to name/slot/stats) drops them before they reach the JSON
  at all - confirmed neither is live for any item/build actually in this pack (no mined item
  has a nonzero pseudo_stat; no build equips more than the one Stormcloth piece).
- `rand_prop_points` (a nonzero value marks an item that can roll "of the Whatever"): three of
  the 17 mined items have one, but none of the real fixtures these builds come from ever chose
  a suffix - inert today.
- `item_effects` (on-equip procs / on-use effects): only one mined item has any (Robe of the
  Void's "Heal Pet" on-use, a pet-health utility with no damage component, outside this DPS-only
  engine's scope regardless) - no damage proc is silently dropped by anything in this pack.

None of the four are wrong today, but `ItemRow` has no field for any of them - a future item
that actually needs one would need a `gear.rs` change, not just a re-mine. See `gear.rs`'s
module docs and the rev3 pack's `unimplemented_but_parsed` field for the full citations.

**Still not modeled**:
- No gems/sockets - not a gap in this mining pass, a real property of the source data: its own
  item database has none, and its own code hard-refuses an equipped gem.

**Raid buffs + consumables, added in the sixth pass (2026-10-24, "analyze and address the DPS
gap")**: `equipment_to_stats`'s new `builder_starter_buffs: bool` parameter
(`--buffs builder-starter`) applies the exact raid-buff + consumable package mythicsim.com's own
"builder starter" fixtures use - found while analyzing *why* the DPS gap against those same
fixtures was still so large; it turned out raid buffs/consumables, not pet melee auto-attack,
were the dominant missing piece (see `research-notes/mythicsim-comparison.md`). Not a general
buff system, deliberately - this one real, reproducible loadout, each number sourced from its
own item/spell page (`gear.rs`'s `BUFF_*` constants have the full citations): Arcane Brilliance,
Gift of the Wild, Well Fed food, Zanza's potion and Prayer of Spirit (flat Intellect/Spirit),
Greater Blessing of Kings (+10% to that total), Flask of Supreme Power (+150 universal spell
damage - its package-mate Greater Arcane Elixir's own +35 is real but inert once the flask is
up, confirmed via the source engine's own `ExclusivePerStat` same-stat-dedup rule, so it's
deliberately not added separately), Elixir of Shadow Power (+40 Shadow-specific, a different
stat so it does stack), Moonkin Aura (+3% crit, not Classic's 5%), Mageblood Elixir + Greater
Blessing of Wisdom (+52 mp5 combined). Not modeled from this same package: Mana Spring Totem's
own mp5 tick (no sourced numeric value found), Judgement of Wisdom's mana-on-hit, and the Major
Mana Potion/Demonic Rune (both mid-fight *usage*, not standing buffs - this engine still doesn't
simulate consumable usage, only pre-pull buffs).

## Known gaps

**Resolved in the eighth pass (2026-10-24, "fix the differences in Demonology"):**

- ~~The Succubus's melee auto-attack isn't modeled~~ - fixed for real. A new melee combat
  system (`warlock/mod.rs`'s `pet_melee_swing`, `sim.rs`'s `Fight::resolve_melee_swing`):
  weapon damage, attack power conversion, glancing blows and melee crit all read directly from
  `mythicsim-forever-engine`'s own source; the Succubus's innate attack power and base crit
  (no general formula found for either in the time available) read instead from that same
  engine's own debug combat log for this comparison's exact fixture - a lower sourcing tier,
  flagged the same way this project already flags its other Go-literal pet constants, but a
  real number from the real engine either way, not a guess. Demonic Brand now triggers from
  melee hits too, not just Lash of Pain (confirmed via the same log: 181267 melee swings vs.
  31435 Lash of Pain casts for the one pet). Raised Demonology from 451.3 to 618.2 DPS against
  the real engine's 609.5 (1.35x -> **0.99x**) and Affliction from 514.8 to 589.4 against 554.2
  (1.08x -> **0.92x**).
- ~~A scheduling bug lost real cast/swing frequency whenever another actor's turn came first~~
  - fixed. `pet_ready_at`/`pet_melee_ready_at` used to rebase their next scheduled time off
  `fight.now_ms`, silently losing ground every time a tie-break delayed that actor's turn -
  small and easy to miss for Lash of Pain's 12s cadence (~12% shortfall), large and obvious
  once the new 2.0s melee timeline was added (~35% shortfall: ~38 swings over 120s instead of
  ~60). Fixed by self-incrementing from the previous schedule instead, in both
  `demonology.rs::run` and `affliction.rs::run`.

See `research-notes/mythicsim-comparison.md`'s "Fixing Demonology's gap: a melee combat
system" section for the full sourcing and the exact DPS deltas.

**Resolved in the seventh pass (2026-10-24, "produce a per-ability breakdown and analyze the
differences"):**

- ~~`AbilityTotals.casts` silently counted dot ticks too~~ - fixed. Split into separate
  `casts`/`ticks` fields (`sim.rs`); found because Bane of Agony's own "casts" (51.9 over 120s)
  was ~10x its real-engine counterpart's (~5.5) despite both using a 24s duration - a reporting
  bug, not a damage one.
- ~~Dot-only casts never rolled their own hit chance~~ - fixed, a real one this time:
  `apply_dot` always landed regardless of `spell_hit` for Corruption/Bane of Agony/Siphon Life,
  while every direct-damage spell correctly rolled. A direct+dot spell's dot (Immolate) now
  shares its direct half's one hit roll rather than rolling a second, independent time.
- ~~`AfflictionTalents.siphon_life` defaulted to `true`~~ - fixed to `false`; the real "builder
  starter" talent string has 0 points in it, confirmed by decoding it directly.
- ~~Suppression (+1%/point spell hit) wasn't modeled for any spec~~ - fixed, added to all three
  `Talents` structs; every one of this comparison's three builds spends real points here
  (2-5).
- ~~Destruction had no `improved_corruption` field~~ - fixed (the other two specs already did).

Found by building `mythicsim-forever-engine`'s own Rust CLI locally and running it against the
exact request files this comparison already uses, with `--rotation-stats` for a genuine
per-ability trace - not estimating melee auto-attack's size as the sixth pass had to, but
measuring it directly (~62-71 DPS, depending on spec). See
`research-notes/mythicsim-comparison.md`'s "Per-ability breakdown" section for the full tables.

**Resolved in the sixth pass (2026-10-24, "analyze and address the DPS gap"):**

- ~~Raid buffs and consumables aren't modeled at all~~ - fixed; see "Gear system" above. This
  was the single largest remaining cause of the mythicsim.com gap, not pet melee auto-attack as
  the fifth pass's "Still open" guessed - that guess was never actually checked against the
  numbers until this pass did.
- ~~Curse of the Elements is cast and refreshed but does nothing~~ - fixed. All three rotations
  already tracked it as an aura purely for refresh timing; its real effect (+10% magic damage
  taken, spell 1311680) was never applied to any damage calculation. Added as
  `Fight::curse_of_the_elements_bonus()` in `sim.rs`, checked inside `resolve_direct`,
  `resolve_direct_raw` and `apply_dot` alongside every other modifier those functions already
  apply.
- ~~`demonology.rs`'s rotation was missing two of its own real actions~~ - fixed. Dumping its
  real priority list in full (not just the Curse of the Elements line) found Immolate (spell
  25309) entirely absent - Demonology never dealt any Immolate damage before this pass - on top
  of the same missing Curse of the Elements cast. Also extended Ruin/Bane/Agonizing Flames
  (previously Shadow-Bolt-only in `demonology.rs`) to cover Immolate too, the same way
  `affliction.rs`'s `keep_immolate` case already did, since those talents' real class-masks
  reach it regardless of spec.

**Resolved in the fifth pass (2026-10-24, "simulation differences + gear"):**

- ~~Affliction has no pet simulation at all~~ - fixed. `PetChoice`/`pet_cast` (the Firebolt/
  Lash of Pain/Demonic Brand mechanics) moved out of `demonology.rs` into `warlock/mod.rs` as
  shared code; `AfflictionTalents` got the same `pet: Option<PetChoice>` field and
  player/pet-interleaving loop `demonology::run` already had. Defaults to a summoned Succubus
  on the CLI (`--pet none|imp|succubus` to change it), matching the real "builder starter"
  build; library callers still default to no pet unless they opt in. Raised the
  `affliction_starter_undead` mythicsim.com comparison from 300.3 to 316.8 DPS (1.80x -> 1.71x
  gap) - see `research-notes/mythicsim-comparison.md`.
- ~~Gear's completeness against the real engine's item mechanics was never actually audited~~ -
  fixed. See "Gear system" above for what was checked (`pseudo_stats`, random suffixes, item
  effects, item set bonuses) and found inert for this pack's actual items; also fixed two stale
  doc claims (`gear.rs`'s module doc and the data pack's own `unimplemented_but_parsed` field
  both still said ShadowDamage/FireDamage and Intellect-to-mana were unapplied, when both had
  actually been implemented earlier in the fourth pass).

**Resolved in the fourth pass (2026-10-24):**

- ~~Succubus/Lash of Pain isn't simulated~~ - fixed. `--pet succubus` (or
  `DemonologyTalents { pet: PetChoice::Succubus, .. }`) simulates her Lash of Pain the same way
  the Imp's Firebolt already was. Her melee auto-attack is *not* modeled (would need a
  melee hit/crit/weapon-damage system this engine has nowhere else either) - a real,
  deliberate scope cut, so a Succubus build's true DPS is higher than this engine shows.
- ~~Decimation and Cataclysm aren't modeled~~ - fixed. Cataclysm's -3/6/10% Destruction mana
  cost is wired into every relevant cast and is only reportable as different numbers under
  real mana pressure (see `cataclysm_reduces_real_spell_casts_lost_to_life_tapping` in
  `data_pack_sanity.rs`) - it's always correctly near-invisible at `--mp5 80`-style comfortable
  mana, which is expected, not evidence it's a no-op. Decimation's Shadow Bolt/Searing Pain
  damage bonus is gated on a *new engine feature*, `Fight::set_target_max_health` /
  `target_health_percent` (`--target-health` on the CLI) - without it, Decimation can never
  tell the target is below 35% and simply never fires, which is correct behavior, not a bug.
  Its Soul Fire cast-time/cooldown halves are implemented as constants but have no observable
  effect since this rotation doesn't cast Soul Fire (faithful to the real priority-list
  fixture - see `demonology.rs`'s module docs).
- ~~Life Tap's exact Spirit-scaling formula is still not sourced~~ - fixed, with the real
  formula this time: `wowsims/forever`'s `sim/warlock/lifetap.go`, read directly (the upstream
  Go, not the Rust port, which doesn't carry this file at all): `amount = 420 + Spirit;
  mana = amount * (1 + ImprovedLifeTap%)`. The old flat-420 version is gone.
- ~~Gear's own known gaps~~ (Intellect-to-mana, Spirit-to-regen, school-specific power) - all
  fixed; see "Gear system" above for the real formulas, which came from the same upstream Go
  source as Life Tap's.

**Resolved across the second and third passes (2026-10-23):**

- ~~Demonology's pet damage number is not sourced~~ - fixed (second pass). `export.rs` in the
  same source repo carries the Imp's Firebolt (and Succubus's Lash of Pain, Demonic Brand's
  hit) as literal Go-ported constants with their own provenance comments, found by reading
  that file directly rather than relying on the auto-generated table (which has no pet-spell
  rows at all). `demonology.rs` uses the real average (45.0 at level 60, not the old
  placeholder's 100.0) - still thinner sourcing than the main pack (no second-site
  cross-check; foreverdb.net 404s for both pet spell IDs).
- ~~Demonic Brand isn't implemented~~ - fixed (second pass). Required merging Demonology's pet
  loop onto the player's own fight timeline, since Brand is a genuine interaction: the
  warlock's Searing Pain has to land before the pet's hits can spend its charges.
- ~~A handful of talent percentages come from code comments, not an independent cross-check~~ -
  upgraded (second pass) for the ones that mattered for DPS, via `spell-extras.json`'s `curves`
  table (downloaded in the first pass, not opened until the second) - caught two real bugs:
  Aftermath is a 5-point talent, not 3, and Bane/half of Improved Corruption (cast-time
  reductions) were documented but never wired in.
- ~~The assumed Warlock level cap~~ - was wrong (65, guessed from an unrelated field); fixed
  (second pass) to the real `CHARACTER_LEVEL = 60` from `character.rs`.
- ~~No gear/item system~~ - implemented (third pass). See "Gear system" above.
- ~~Proc-window talents (Nightfall, Improved Shadow Bolt, Shadow and Flame) were approximated
  as flat always-on bonuses~~ - fixed (third pass). All three are now real timed auras with
  sourced durations (Shadow Vulnerability 12s from spell 17794, Shadow Trance 10s from spell
  17941, Shadow and Flame's Fire/Shadow buffs 20s each from spells 426311/1293816) and real
  trigger conditions (Nightfall: 2%/4% per Corruption tick, from `spell-extras.json`'s curve
  for spell 18094; Improved Shadow Bolt: a Shadow Bolt crit; Shadow and Flame: a landed
  Conflagrate or Shadowburn). `data_pack_sanity.rs` has regression tests proving the uptime is
  genuinely partial, not flat.

**Resolved in the "close the DPS gap" pass (2026-10-24, same day as the comparison above):**

- ~~`AfflictionTalents`/`DemonologyTalents` have no fields for Ruin, Improved Shadow Bolt,
  Bane, or Agonizing Flames~~ - fixed, reusing `destruction.rs`'s existing proc-window
  machinery rather than rebuilding it. (Shadow and Flame wasn't needed - neither real build's
  decoded talents actually included it.)
- ~~Demonic Brand gated to `PetChoice::Imp`~~ and ~~Master Demonologist gated to the Imp~~ -
  both fixed; either pet's landed hit can spend/trigger a brand charge now, and both demons'
  matching-school halves of Master Demonologist apply.
- ~~Demonic Sacrifice's own damage buff~~ - fixed, as `Option<SacrificedPet>` on all three
  Talents structs. Confirmed Forever reverses Classic's school pairing (Imp's sacrifice buffs
  *Shadow*, Succubus's buffs *Fire*) directly from the client rows, not assumed.
  Also closes Demonic Pact's practical effect for free (sacrifice is modeled independent of
  which pet is actively simulated).
- Also found and fixed while re-checking the Demonology build's full talent list: **Unholy
  Power** and **Soul Link** weren't modeled at all (not previously listed as a known gap -
  found directly in this pass), plus a second real bug in `destruction.rs`'s own Agonizing
  Flames (it was missing Immolate's direct hit and dot entirely from the bonus).
- Demonic Pact itself (as a distinct mechanic) and genuinely exhaustive cross-tree talent
  coverage beyond what these two specific real builds needed remain unimplemented in general -
  see `research-notes/mythicsim-comparison.md` for exactly what's covered.

**Still open:**

- The real Searing Pain/Demonic Brand refresh condition isn't decoded exactly -
  `demonology.rs`'s check is a single flat threshold (`charges_remaining_ms <= 3500`) where the
  real priority list's own line has a second, more specific condition (an `auraIsKnown`/
  `dotRemainingTime` combination) this project's translation of the rotation simplified away.
  Now that melee also triggers Demonic Brand (eighth pass), this engine's own brand-trigger
  rate runs a bit *higher* than the real engine's measured one, roughly canceling against
  Shadow Bolt running a bit low - the total lands almost exactly on target anyway (Demonology
  0.99x), so this is a real, named imprecision, not something the current numbers are hiding.
- Two of the Succubus's own melee constants (her innate attack power, 258.0, and her own base
  physical crit on top of her owner's inherited one, 4.25%) were read directly off the real
  engine's debug combat log for this comparison's one fixture rather than independently
  derived from a general Strength/Agility-to-AP or crit-rating formula - the same lower
  sourcing tier this project already uses for its other Go-literal pet constants, flagged the
  same way. Likely why Affliction's own melee share now overshoots slightly (0.92x) rather
  than landing as close to exact as Demonology's did (0.99x, the build these two constants were
  actually calibrated against).
- `ItemRow`/`EnchantRow` have no fields for `pseudo_stats`, random-suffix `rand_prop_points`,
  or `item_effects` - confirmed inert for every item in this pack today, but a future item that
  needs one would need a `gear.rs` change, not just a re-mine. See "Gear system" above.
- Mana Spring Totem's own mp5 tick and Judgement of Wisdom's mana-on-hit (both part of the
  buffs package added in the sixth pass) weren't modeled - no sourced numeric value was found
  for the totem's tick, and JoW's proc-on-hit mechanic isn't something this engine tracks.
  Sustain-only either way, and neither comparison run here ever actually Life Taps, so this
  shouldn't move DPS in practice.
- Decimation's Soul Fire cast-time/cooldown halves have no observable effect since this
  rotation doesn't cast Soul Fire - see this section's fourth-pass entry above.
- Master Demonologist's Shadow half (for a Succubus build) isn't modeled, only its Fire half
  (for an Imp build) - `demonology.rs`'s `DemonologyTalents` doc comment flags this inline.
- `tools/mine_warlock_data.py` only reproduces the original 17-spell pack; the pet/brand
  additions (rev2) and the entire gear pack were one-off manual extractions never folded into
  a reusable script.
- No multi-target, missile travel time, the 5-second-rule's casting/not-casting regen split,
  or partial resists - all still deliberate first-milestone scope cuts, not bugs (see "Scope"
  above).

## Mage

Begun 2026-10-25, Fire only so far, following the exact same discipline the Warlock specs were
built with: the real single-target rotation decoded directly from
`validation/2026-10-04-builder-starters/fire-mage--undead.request.json` (same validation corpus
this whole project already uses), real client spell data read from a local clone of
`sage3648/mythicsim-forever-engine`, real talent curves from that same repo's own
`spell-extras.json`, and a real gear pack mined the same way the Warlock one was (reusing
several item/enchant entries byte-for-byte where the real item IDs are identical - caster gear
a Mage and a Warlock can equally equip).

```sh
cargo run -p mythic-forever-cli -- --class mage --spec fire --gear fire_mage_starter_undead --race RaceUndead
cargo run -p mythic-forever-cli -- --class mage --spell-power 450 --crit 25 --hit 99
```

**What Fire models**: Fireball (the main filler, direct hit + its own 8s dot), Scorch
(maintains the Fire Vulnerability debuff - +3%/stack fire damage taken, up to 5 stacks, 30s
duration, via a new stack-aware aura mechanism in `sim.rs` added for this), Fire Blast (an
instant nuke on an 8s cooldown), Pyroblast (a slow nuke whose cast time Heating Up cuts
dramatically - see below), Evocation (mana restore below 10% mana, modeled the same way Life
Tap is for Warlock), and all nine real talents that materially change the rotation's output:
Improved Scorch, Improved Fireball, Critical Mass, Fire Power, Incineration, Elemental
Precision, **Ignite, Heating Up/Pyroblast, Combustion, and Arcane Concentration/Clearcasting**
(the four added in a second pass, 2026-10-25, closing every gap the first pass had left open).
The generic mana/cast-time/cooldown mechanics (`cast_direct`, `cast_direct_and_dot`,
`cast_utility`, etc.) moved out of `warlock/mod.rs` into a new `crate::casting` module shared
by both classes, rather than being duplicated - the first real test of whether this project's
engine (not just its data) would generalize past one class, and it did with almost no changes
to `sim.rs`'s core damage resolution (two small, genuinely reusable additions: stack-counted
auras for Fire Vulnerability/Heating Up/Combustion, and a "tick amount already computed by the
caller" dot variant for Ignite, whose tick damage depends on the triggering crit's own damage,
not a flat data-pack formula).

**The four mechanics the first pass deferred, modeled now** - each read from the Rust port's
own talent-mechanic source files (`src/classes/mage/talents/heating_up.rs`,
`spells/combustion.rs`, `talents/ignite.rs`, `talents/arcane_concentration.rs`), not guessed at
from a tooltip's rounded summary:
- **Ignite** (5/5 in the real build): a Fire crit rolls 8/16/24/32/40% (at 1-5 points) of its
  own damage into a 2-tick/4s dot - a second crit while it's still ticking rolls in whatever it
  still owed rather than overwriting it, matching the real source exactly.
- **Heating Up / Pyroblast** (1/1 each): a non-periodic crit from Fireball/Scorch/Fire Blast
  adds a stack (cap 3, 20s) that cuts Pyroblast's cast time 25%/stack - 1.5s at 3/3, down from
  6s, not literally instant (a real number read from the buff's own data row, different from
  live WoW's unrelated "Hot Streak"). Fire Blast's own cast adds a stack regardless of crit, on
  top of its crit's own.
- **Combustion** (1/1): a cooldown whose buff adds +10% Fire crit per stack - any landed Fire
  hit adds a stack (cap 10), but the buff itself (and its real 3-minute cooldown) only ends
  once 3 of those hits have been crits, whichever stack count that happens at.
- **Arcane Concentration / Clearcasting** (5/5): a landed damaging cast has a
  2/4/6/8/10%-at-1-5-points chance (1s internal cooldown) to make the next costed cast free,
  via the same `cost_pct` mechanism Destruction Warlock's Cataclysm already uses.

Still out of scope for Fire specifically: `autocastOtherCooldowns` (general trinket autocast)
and a worn trinket's own randomized mana-restore amount - the same category of scope cut every
Warlock rotation already makes. **Missile Barrage/Arcane Missiles and two real one-time mana
items (a flat-amount item and a potion) *are* now modeled** - a 2026-10-25 per-ability
investigation found this section's own prior claim ("Fire's rotation simply never casts Arcane
Missiles... aura id 44404 doesn't resolve to any real spell") was simply wrong; see "Compared
against the real engine" below for the full fix.

**Compared against the real engine's own per-ability trace (2026-10-25)** - see
`research-notes/mage-fire-comparison.md` for the full tables. That comparison found and fixed
three real bugs the first two passes' "does the total look plausible" check had missed
entirely: Elemental Precision's real max is 3 points, not the 4 this project had decoded
(the talent-tree UI metadata used to decode it was simply wrong for this one talent, confirmed
against the engine's own talent-string validator); `--buffs builder-starter` was never actually
passed for this spec's own comparison runs (the same mistake the first Warlock comparison made,
now fixed with a Mage-specific buff package - its consumables genuinely differ from Warlock's:
Elixir of Fire Power instead of Shadow Power, plus a weapon imbue Warlock's package doesn't
carry); and Pyroblast's own dot was never applied (the same class of bug Fireball had in the
first pass, reintroduced when Pyroblast was added in the second). Fixing all three raised this
build from ~330 DPS to 495.6 against the real engine's own 704.9 for the identical fixture
(1.42x gap at the time). **A fourth pass, same day, found and fixed the exact root cause of
that 71-point effective-spell-power gap this doc's own "found, quantified, but not-yet-
resolved" section had left open**: two consumables (Greater Arcane Elixir +35, Brilliant
Wizard Oil +36) wrongly modeled as inert once the Flask was up, which a direct read of the
real engine's own `consumes.rs` showed are both unconditionally additive - plus a project-wide
crit-multiplier formula bug (`1.5 + bonus%` instead of the real `1.0 + 0.5*(1+bonus%)`,
confirmed against the real engine's own `src/mechanics/damage.rs`). Both fixes are in
`gear.rs`/`sim.rs`, not per-spec, so they raise Warlock's own DPS slightly too. **A further
pass, same day, found two more generalizable bugs by reading the real engine's own four-stage
debug pipeline end-to-end** (`BaseDamage→AfterAttackerMods→AfterResistances→AfterTargetMods→
AfterOutcome`): a whole missing combat mechanic, Spell Piercing/resistance penetration (every
Mage build has 2/2 Arcane Subtlety, whose *second* effect - previously assumed threat-only -
grants it; modeled generically as `Fight::spell_vulnerability_bonus`, confirmed against the
real engine's own `spellVulnerabilityMultiplier`), and an Intellect→max-mana sequencing bug
(any talent that raises Intellect after gear/buffs are already totaled silently never reached
the mana pool - fixed with a new generic `Fight::multiply_intellect`). This raised Fire to
**606.5 DPS (0.86x)** - see `research-notes/mage-fire-comparison.md`'s own "Resolved" section
and `mage-arcane-comparison.md`'s two root-cause-pass sections for the full methodology (and
confirmation, via web research, that `sage3648/mythicsim-forever-engine` is itself built and
checked against the official `wowsims/forever` Go engine, the right thing to be treating as
ground truth). **A fifth pass found two more real condition bugs** by comparing per-ability
cast counts against the real trace and re-reading the real priority list's own exact condition
tree: Combustion's own condition had been misread (it should only ever fire once Fire
Vulnerability hits 5 stacks, never at the very start of the fight before it's ever landed -
confirmed by the real debug log's own first Combustion cast landing at t=9.00, the exact moment
Fire Vulnerability first maxes), and Fire Blast was missing a real `mana >= 30% AND Clearcasting
not active` gate entirely. Both fixed in `mage/fire.rs`, but neither closed the specific
Fire-Blast-under-cast/Scorch-over-cast skew (605.5 DPS, 0.86x - essentially unchanged) - kept
anyway since both are verified correct against source, not reverted just because they didn't
move the number being chased. **A sixth pass added real per-cast debug logging** (`Fight::
debug_log`, `--debug-log`) specifically to close that skew, and found the actual cause: two
more off-GCD/timing bugs (Combustion's own real `GCD = 0s` had been defaulting to the normal
1500ms, then floored to 1000ms, both now fixed), and - the real driver - **two entire missing
mana-return mechanics** found by diffing every real `Gained X mana` log line against this
engine's own: Judgement of Wisdom (`raid.debuffs.judgementOfWisdom: true`, every fixture
checked so far including Warlock's - a landed direct hit has a flat 50% chance to return 59
mana, fixed generically in `sim.rs`) and Master of Elements (a Fire Mage talent, 3/3 in this
build, refunding 10%/point of a crit's own base mana cost). Without them, this engine's own
Fire Mage needed an 8-second, DPS-dead Evocation around the fight's halfway point; the real one
needs its first and only Evocation at t=124.50, essentially after the fight is already over.
614.7 DPS (0.87x) at that point. **A sixth pass** added the same cooldown-timing fix and Mana
Spring Totem's own missing mp5 found via Arcane Mage's own debug log (Fire doesn't talent
Arcane Blast, so that spec's later buff-sharing fix doesn't apply here) for 619.5 DPS (0.88x).

**A seventh pass (2026-10-25, prompted by "0.88x is unacceptably low") found two more real
bugs via a direct per-ability investigation.** First: this project's own first-pass module doc
had flatly claimed "Fire's own real priority list never casts Arcane Missiles at all" - wrong.
The real per-ability breakdown shows a genuine 29.9 DPS Arcane Missiles contribution (~4% of
the build), and the real priority list's own item #7 (between Fire Blast and the Fireball
fallback) casts it whenever Missile Barrage is active - the same buff already modeled in
`mage/arcane.rs`, never checked against Fire's own wrong claim. Fixed: added `FireTalents::
missile_barrage`, Fireball as its own half-rate trigger spell (confirmed against the real
engine's own `register_missile_barrage` trigger list), and the priority-list slot itself.
Second: diffing every real `Gained X mana` log line found two "Major cooldown used" one-time
mana items - `ItemID: 8008` (+1100, gated on the real priority list's own explicit
`currentManaPercent <= 80%` condition) and `ItemID: 13444` (+1800, a mana potion) - reliably
present with the *identical* amount in all three Mage specs' own real logs, unlike a third item
seen the same way (`ItemID: 14152`, a worn trinket) whose own amount varies across every build
checked (593.8/547.2/599.2) with no clean pattern, consistent with an unsourced randomized
range and deliberately *not* added. The first two are real, exactly-sourced, and reliable
enough that this project's own "no consumable-use modeling" scope note doesn't really cover
them - fixed generically (`Fight::maybe_use_one_time_mana_item` in `sim.rs`), called from all
three Mage specs. **Final: 646.6 → 669.8 DPS (0.95x)** - the closest Fire Mage comparison yet.
One per-ability laggard remains: Fire Blast (0.74x) - this engine's own mana still crashes
below the 30%-mana Fire-Blast-skip threshold and needs a mid-fight Evocation in at least some
seeds, while the real engine's own trace never needs one inside the 120-second window; the
still-unmodeled trinket's ~550-600 mana would very plausibly close most of this, but isn't
confidently sourced to add. The Undead racial Touch of the Grave (3.6 DPS) remains unmodeled,
same as every other spec's own comparison.

**Every class/spec this project adds from here gets this same treatment before it's considered
done** - see `CLAUDE.md`'s hard rule, written after this comparison demonstrated (again) that a
plausible-looking aggregate DPS number hides real, fixable bugs a per-ability trace catches
immediately.

## Mage: Arcane and Frost

Begun 2026-10-25, same day as Fire's second and third passes, same discipline: real talent
strings and priority lists decoded from `arcane-mage--undead.request.json` and
`frost-mage--undead.request.json` (same validation corpus), real spell/talent-curve data from
the same `sage3648/mythicsim-forever-engine` clone, compared ability-by-ability against the
real engine's own trace before being considered done (see `CLAUDE.md`'s hard rule).

```sh
cargo run -p mythic-forever-cli -- --class mage --spec arcane --gear arcane_mage_starter_undead --race RaceUndead
cargo run -p mythic-forever-cli -- --class mage --spec frost --gear frost_mage_starter_undead --race RaceUndead
```

**A real talent-string decode issue found while adding these two.** Both fixtures' talent
strings decoded Elemental Precision to 5 points (max is 3 - the same discrepancy Fire's own
comparison already found and fixed), and Frost Mage's own Frost-tree segment had one too many
characters for its 18-talent tree, with a second stale value (Ice Barrier, decoded to 5
against a real max of 1). Capped both to their real current max per talent, the same
"trust the validator, not a guess" fix Fire's own `elemental_precision` got - every other
talent slot in both fixtures decoded clean, so this is validation-corpus/engine version drift
on a handful of talents, not a bug in the decode method itself. Full writeup in
`research-notes/mage-arcane-comparison.md`'s own "A talent-string decode bug" section.

**What Arcane models**: Arcane Blast (the main nuke - each cast stacks a buff, up to 4 times,
giving 15% of the caster's Intellect as spell damage and raising its own mana cost 175% per
stack, non-refreshing for 15s from the first stack), Frostbolt (the mana-conserving fallback
once stacks hit 3 or mana drops below 40%), Arcane Missiles (free + double-speed whenever
Missile Barrage procs), Evocation, and Missile Barrage/Arcane Concentration/Arcane
Impact/Arcane Mind/Arcane Instability. Two new generic engine primitives came out of this:
`Fight::add_aura_stack_no_refresh` (a stacking buff whose timer starts once and never resets -
Arcane Blast's own, unlike every other stacking aura this project had modeled before it) and
`Fight::register_dynamic_spell_power_aura` (a stacking aura whose current stack count
contributes a live `stacks * intellect_multiplier * Intellect` bonus to *every* cast's
effective spell power, not just a bonus local to the triggering ability's own cast - see
"Resolved" below for why this replaced an earlier, narrower primitive). `CasterStats` gained a
new `intellect` field for this - no earlier mechanic needed raw Intellect itself. **Arcane
Power** (+30% damage/mana cost, 15s, 180s cooldown) *is* modeled, despite an initial pass
bucketing it with Presence of Mind as `autocastOtherCooldowns`-driven and out of scope - a real
debug log showed a clean 15s window of a flat +30% multiplier only this talent explains, so it
was added using the same cooldown/timed-aura primitives Combustion already established. Out of
scope: Presence of Mind (only changes cast time, not damage) and the Undead racial Touch of the
Grave.

**Compared against the real engine (2026-10-25, then again after a rigorous root-cause pass
the same day)**: first landed at 244.9 DPS against the real engine's own 497.8 (0.49x). A
follow-up investigation - prompted by being asked to find *why*, ability by ability, and fix it
with formulas that generalize to every class - found and fixed three real, project-wide bugs
(none Arcane-specific): a crit-multiplier formula error (confirmed against the real engine's
own `src/mechanics/damage.rs`), two consumables wrongly modeled as inert that account for a
flat 71-point spell-power gap found identically in both this spec and Frost's (confirmed
against the real engine's own `consumes.rs`), and an Arcane-Missiles-under-Missile-Barrage
tick-count bug that was *inflating* this spec's own numbers. **A further pass found two more
generalizable bugs**: Arcane Subtlety's own missing Spell Piercing effect (every Mage build has
2/2, previously assumed threat-only - confirmed against the real engine's own
`spellVulnerabilityMultiplier`) and Arcane Mind's own Intellect boost never reaching the mana
pool it should have fed (a 553-point `max_mana` gap, confirmed against a real debug line - any
Intellect-boosting talent for any class could hit the same sequencing bug, fixed with a new
generic `Fight::multiply_intellect`). Net result at that point: 305.2 DPS (0.61x). **The
mana-pacing stall itself was then closed outright**: extracting every real cast timestamp
showed the real engine never idles in this fight at all (largest gap 3.0s, Frostbolt's own
cast time), confirming this engine's own ~43% idle time was a real bug - traced to Arcane
Blast's own mana cost being computed as 15% of this *caster's* buffed max mana (~1000+)
instead of 15% of a flat class/level base (181.0, solved exactly from six real
`Spent X mana` log lines fitting `cost = base*(1+1.75*stacks+0.30*arcane_power)`). `CostPct`
costs are a percentage of a class/level base-mana table, not the player's own current pool -
a real, generalizable WoW mechanic this project had simply never encountered before Arcane
Blast. Fixed in `mage/arcane.rs`. 397.8 DPS (0.80x) at that point - Arcane Blast's own cast count
alone roughly doubled. **A sixth pass** (prompted by closing Fire Mage's own cast-count skew
with new per-cast debug logging) found Judgement of Wisdom - a landed direct hit has a flat 50%
chance to return 59 mana, a real mechanic every "builder starter" fixture checked so far
carries, not just Mage's - fixed generically in `sim.rs`, applying here with no Arcane-specific
code. 400.7 DPS (0.81x) at that point. **A seventh pass** (asked explicitly to look at per-
ability damage, biggest first) found three more generic bugs shared across classes (a
cooldown-timing bug, `Fight::set_cooldown_from`; Mana Spring Totem's own missing +25 mp5; and
Missile Barrage's own self-sustaining per-tick proc chain, not just a per-cast one) for 416.1
DPS (0.84x), then the real cause of the remaining gap: **Arcane Blast's own stacking buff turned
out to be a standing increase to the caster's general SpellDamage, not a bonus local to its own
cast** - a real debug log showed Frostbolt's own `SP:` field alternating between 480.0 and 665.4
over one fight with no gear/talent change, exactly matching `480 + 3*0.15*Intellect(412)`, every
time Arcane Blast's buff gained or lost a stack. Every Frostbolt/Arcane Missiles cast landed
during the buff's ~85%-uptime window was silently missing this bonus. Fixed generically
(`Fight::register_dynamic_spell_power_aura`, read live by `effective_spell_power` rather than
mutated imperatively, since a stacking aura can expire with no method call firing - any class's
"N stacks of X aura add a flat Intellect fraction to SpellDamage" could reuse it) - the earlier,
narrower `arcane_blast_bonus_power`/`cast_direct_with_bonus_power`/`resolve_direct_with_bonus_
power` path was removed, not left dead. 437.5 DPS (0.88x) at that point - Frostbolt alone moved
from 0.95x to 1.005x, confirming the fix rather than a lucky aggregate.

**An eighth pass (2026-10-25, same "0.88x is unacceptably low" push) added the same two real,
sourced one-time mana items found via Fire Mage's own investigation** (full citation in
`mage-fire-comparison.md`): `ItemID: 8008` (+1100, gated on `currentManaPercent <= 80%`) and
`ItemID: 13444` (+1800, a mana potion) - both confirmed present with the identical amount in
this build's own real log too, fixed generically (`Fight::maybe_use_one_time_mana_item`).
**Final: 449.4 DPS (0.90x)**. One gap remains open: Arcane Blast's own cast frequency
(18.8/120s real vs. ~13.5 here, Frostbolt picking up the slack in the other direction) - not
chased further this pass. Full methodology, source citations, and the residual gap in
`research-notes/mage-arcane-comparison.md`.

**What Frost models**: Frostbolt (the filler), Ice Lance (consumes Fingers of Frost charges for
a x3.0 frozen-target multiplier and Shatter's bonus crit), Arcane Missiles (same Missile
Barrage mechanism as Arcane's), Evocation, and Improved Frostbolt/Elemental
Precision/Ice Shards/Fingers of Frost/Shatter/Missile Barrage/Arcane Concentration. Fingers of
Frost uses `Fight`'s existing charge-counter primitives (`set_charges`/`consume_charge`) rather
than a stacking aura, since "N charges, decremented on use" is a better fit than a stack count
with no decrement operation. Winter's Chill (2/5 real points in this build) is deliberately
unmodeled - its only damage effect buffs Cone of Cold, which this rotation never casts, confirmed
inert rather than skipped by oversight. Out of scope: a mana-potion item action and the Undead
racial Touch of the Grave.

**Compared against the real engine (2026-10-25, then again after the same rigorous root-cause
pass described under Arcane above)**: first landed at 413.4 DPS against the real engine's own
475.6 (0.87x). The same project-wide crit-multiplier and spell-power-elixir/weapon-imbue fixes
applied here too (Frost's own 71-point spell-power gap was the *other* data point, alongside
Fire's, that pinned those fixes to exactly 35+36 rather than a guess), and the Arcane-Missiles
tick-count fix flipped this spec's own 1.44x *over*-performance to 0.94x - tracking Frostbolt's
own ratio almost exactly, confirming it was the right call. The same Spell Piercing and
Intellect→max-mana fixes found under Arcane above apply here too (Frost's own debug log showed
the identical ~4.76% unexplained Spell Piercing bonus first, in fact). 452.0 DPS (0.95x) at that
point. **A sixth pass** then added Judgement of Wisdom (see Fire's own section above for the
full citation - found via Fire's new per-cast debug logging, applies generically) for 466.3 DPS
(0.98x). **A seventh pass** added the same cooldown-timing fix, Mana Spring Totem's own missing
mp5, and Missile Barrage's own per-tick proc chain found via Fire/Arcane's own debug logs (Frost
uses Missile Barrage too, so this one applies directly, not just by inheritance) for 471.8 DPS
(0.99x). Arcane's own Arcane-Blast-buff-sharing fix does *not* apply here - Frost has no Arcane
Blast talent, confirmed by scanning this spec's own real debug log for `SP:` fluctuation and
finding none. **An eighth pass** added the same two real one-time mana items (`ItemID: 8008`/
`+1100`, `ItemID: 13444`/`+1800` potion - see `mage-fire-comparison.md`) for a **final 471.9
DPS (0.99x, unchanged)** - Frost's own mana economy was already comfortable enough that the
extra income doesn't move the number, added for consistency since the mechanic is real and
cross-spec. One gap remains open: Ice Lance (0.73x vs Frostbolt's ~1.0x) - still most likely this
project's flat, non-rank-scaled Fingers of Frost proc-chance reading not being the whole
picture, not independently re-confirmed this pass. Full writeup in
`research-notes/mage-frost-comparison.md`.

Not started yet: Frostfire (Mage); every other class besides Priest (Warrior, Paladin, Hunter,
Rogue, Shaman, Druid) - all have real "builder starter" validation fixtures already available
in the same corpus, the same way every Mage spec's did, whenever this project picks up the
next one.

## Priest: Shadow and Smite

Begun 2026-10-25, the third class. Same discipline as Mage: real talent strings and priority
lists decoded from `shadow-priest--undead.request.json` and `smite-priest--undead.request.json`
(the same validation corpus), real spell data from the same `sage3648/mythicsim-forever-engine`
clone - plus, new for this class, the real engine's own `forever-engine prepare` subcommand,
which resolves every talent to its own final numbers (e.g. `early_demise_crit: 30.0`,
`shadow_weaving.proc_chance: 1.0`, and this exact build's own computed caster stats) without
needing to decode either spec's own multi-tier talent-string segment by hand.

```sh
cargo run -p mythic-forever-cli -- --class priest --spec shadow --gear shadow_priest_starter_undead --race RaceUndead
cargo run -p mythic-forever-cli -- --class priest --spec smite --gear smite_priest_starter_undead --race RaceUndead
```

**What Shadow models**: Shadow Word: Pain and Devouring Plague (both kept up, skipped near the
fight's end), Mind Blast (the main nuke, 8s cooldown), Shadow Word: Death (unconditional
whenever off cooldown - the real priority list carries no extra condition on it at all), Mind
Flay (the channel filler), Shadowform (a pre-pull stance cast with its own one-time mana cost,
+10% Shadow damage, -50% Shadow cost, +100% crit-damage bonus on the five Shadow spells this
rotation casts), Shadow Weaving (a self-stacking Shadow-damage debuff, 5/5 in this build =
100% proc chance per the real `prepare` output), Improved Mind Flay (a 20%-per-tick compounding
ramp, folded into one flat average bonus across the channel since this engine resolves Mind
Flay atomically rather than ticking it live), Inner Focus (a free-cast + crit buff spent on
Mind Blast once Shadow Weaving hits 5 stacks), and Early Demise (+30% crit on Shadow Word:
Death, correctly gated on the real engine's own 20%-execute-phase condition - this engine
tracks no target health pool by default, so the bonus simply never procs rather than guessing).
A new generic engine primitive, `Fight::maybe_use_one_time_mana_item`, already built for Mage's
own one-time mana potion/item fix, applies here too (this build's own real log shows the
identical potion amount plus a different conjured item, "Demonic Rune").

**What Smite models**: Smite (the main nuke, with a real mana-pacing throttle - the full rank
only casts while `currentManaPercent >= remainingTimePercent * 0.9`, falling back to a cheap
downranked Smite otherwise), Holy Fire (a direct hit plus its own dot, refreshed via an Inner
Focus combo the same shape as Shadow's own Mind Blast combo), Penance (a 3-bolt channel - the
real engine's own "extra tick that leaves the tick count alone" on landing, found 2026-10-26),
Shadow Word: Pain and Devouring Plague (shared mechanics with Shadow Priest), and Power in
Light (+10% on this spec's own Holy-school kit). A new `School::Holy` variant and
`CasterStats.bonus_holy_power` field were added for this - the first spec in this project that
deals Holy-school damage at all.

**Compared against the real engine (2026-10-25)**: Shadow landed at 457.1 DPS against the real
engine's own 667.5 (**0.68x**). Effective spell power matches the real engine's own `SP: 538.0`
exactly, and every ability sits in a tight, uniform 0.66-0.72x band - pointing at one shared
cast-frequency shortfall rather than a per-ability damage-formula bug. A full-fight debug trace
found the decision clock is actually 100% saturated (no idle time anywhere), ruling out a mana
stall; the real priority list's own Mind Flay mid-channel clip is the likely remaining cause,
but a first attempt at modeling it made the total *worse*, not better, and was reverted rather
than kept as a half-right mechanic (see the comparison doc's own write-up). Smite landed at
284.2 DPS against the real engine's own 514.3 (**0.55x**) at first - a real per-hit damage gap
on Smite and Penance specifically that neither effective spell power nor any talent explained.

**A sixth pass (2026-10-26), prompted by being told both specs were "woefully inaccurate" and
asked for a *general* fix with web research**: a WoW Forever community guide
(classicwow.gg) states plainly, in its Priest gearing section, that *"Healing gear now provides
one-third as much bonus damage"* - a real, Forever-specific mechanic this project had never
modeled for *any* class. Re-checking found every Priest (and Mage, and Warlock) item this
project's own gear packs track carries a `HealingPower` value - mined from `data/items.jsonl`
alongside `SpellDamage` from the start, then silently dropped every time, on the wrong
assumption that a DPS-only engine had no use for a "healing" stat. Brilliant Wizard Oil (item
20749, already modeled for its own `+36 SpellDamage`) carries the identical `+36 HealingPower`
this project's own doc comment had explicitly (and wrongly) called "untracked." **Found to be
Holy-school-scoped, not universal**, by checking Shadow Word: Pain's own real debug line on the
same (Holy-geared) Smite character: it matched this project's pre-existing Shadow-school
formula exactly, with zero healing-power bonus needed, proving the mechanic belongs on
`bonus_holy_power` (the same shape as `bonus_shadow_power`/`bonus_fire_power`), not the
universal `spell_power` field a first attempt wrongly added it to (which overshot every other
class - Fire Mage to 1.02x, Destruction Warlock to 1.14x - since their own gear carries
HealingPower too, confirmed the same item-by-item way, and a Shadow/Fire-school spell has no
business reading it). **Smite: 284.2 → 309.9 DPS (0.60x)** - Holy Fire alone moved from 0.74x to
0.83x.

**A seventh pass (2026-10-26, told again that 0.68x/0.60x were still unacceptable)** found the
single biggest remaining lever by comparing two *different* abilities' own real debug lines on
the *same* character: Mind Blast's own `AfterAttackerMods/BaseDamage` ratio reads exactly 1.21
(matching this project's existing Shadowform-times-Shadow-Weaving formula precisely), while
Shadow Word: Pain's own ratio, on the identical Shadow Priest character, reads 1.2707 instead -
`1.2707 / 1.21 = 1.0502`. **Twin Disciplines** (Discipline tree, +1%/point damage on *instant*
spells - its own class mask names Shadow Word: Pain/Devouring Plague/Shadow Word: Death/Mind
Flay, explicitly excluding Mind Blast, which has a cast time) explains it exactly at 5/5. The
same talent, independently confirmed at 5/5 on the *Smite* character too (whose own Shadow
Word: Pain line reads `1.05` - no Shadowform/Shadow Weaving to compound it there), also reaches
Penance (confirmed instant, `Cast Time = 0s`, in that talent's own class mask). **Shadow:
457.1 → 471.2 DPS (0.68x → 0.71x)**; **Smite: 309.9 → 332.6 DPS (0.60x → 0.65x)** (combined
with the Penance 3-tick fix above). **Mind Flay's own mid-channel clip was tried a second time
this pass**, this time modeling the real condition's full three cases (clip to 1/2/3 ticks
depending on exactly when Mind Blast's cooldown finishes relative to a tick boundary) rather
than the first pass's single-case approximation - it made Shadow's own total *worse again*
(down to 411.6, below even the first attempt's 428.5), confirming the cast-frequency gap isn't
explained by channel-clipping at all, in either form; reverted both times.

**An eighth pass (2026-10-26, same day, told again both specs were "woefully inaccurate" and
asked for a comprehensive web-researched pass)** tried Mind Flay's clip a *third* time, this
time building a real per-tick channel primitive so the channel could genuinely stop early
(the first two attempts never had that capability at all - `cast_dot_only`'s one fixed-size
`advance()` call can't return control mid-channel, so no precomputed case-table could have
worked). This version fixed two real bugs the prior attempts never got the chance to hit (an
interrupt firing for a spell that then fails for lack of mana, and over-broadly interrupting
for abilities the real condition's own text doesn't name) and matched the real engine's own
literal interrupt condition exactly (Mind Blast only). **It still made the total worse (471.2 →
467.3)** - packing in more Mind Blast casts measurably starves Shadow Word: Pain/Devouring
Plague of mana later in the fight, and the real engine's own per-ability *mix* was already
closer to this project's un-clipped baseline than to the clipped version. Reverted a third time
- three independently-motivated attempts, the last one free of every bug the first two had,
all moving the wrong direction, is strong evidence the mana economy itself (not the clip
mechanic) is what differs here, not a sign a fourth attempt would succeed. Shadow's own final
number this pass: unchanged, **471.3 DPS (0.71x)**.

**The same pass's comprehensive re-check found two major fixes for Smite instead** - a full
re-read of this build's own real `BaseDamage` log line against the real engine's actual source,
rather than inferring from the log alone, found: (1) **Spiritual Guidance** (3/5 points, a flat
5% of Spirit added to the *universal* `SpellDamage` stat, not school-scoped) exactly explains
the previously-"unexplained" `SP: 509.9` real vs `498` computed residual; (2) **Judgement of
the Crusader** - this fixture's own `raid.debuffs.judgementOfTheCrusader: true` flag had been
found twice before and dismissed both times as "probably the classic-WoW crit-chance debuff" -
never actually checked against source. It isn't: the real engine's own
`judgement_of_the_crusader_aura` is a flat **damage-taken** bonus (`+161` at its own max rank,
Holy-school-scoped), which turned out to be the real cause of Smite/Holy Fire/Penance's own
dominant 23% `BaseDamage` shortfall that three prior passes had each ruled out specific talent
candidates for without ever finding the real one. **Smite: 332.6 → 379.3 DPS (0.65x → 0.74x)** -
Holy Fire alone moved to an essentially exact **0.99x**, confirming the per-hit damage formula
itself is now correct; Smite's own ability (0.52x) is the one remaining named laggard, tied to
its own real mana-pacing rank-switch condition rather than a new damage-formula bug.

**A ninth pass (2026-10-26, same day, asked explicitly to address the mana economy, the Smite
mana-pacing condition, and whether any already-modeled Mage/Warlock mana mechanic
generalizes)** found the single largest missing mana source either Priest spec has had:
**Dark Sacrifice**, the *Undead racial* (not a Priest talent, a buff item, or a gear stat - a
category this project had only ever modeled passively before) grants `1600 + Spirit` mana once
per fight, confirmed exactly from two independent real traces (`1839.0` gained at `Spirit:
239`; `1837.0` at `Spirit: 237`) - roughly 27% of either build's own mana pool. Checked two ways
whether this generalizes to Mage/Warlock (both also Undead in this project's own reference
builds): by source structure (only `src/classes/priest/` registers it anywhere in the real
engine) and empirically (a fresh real Destruction Warlock trace shows zero occurrences) - it
doesn't, confirmed, not guessed; adding it there would be a regression. Every other
already-modeled Mage/Warlock mana mechanic (Judgement of Wisdom, Master of Elements, the
Arcane Blast base-mana-cost fix, the two shared one-time items) was re-checked against this
pass's own Priest trace and found to generalize exactly where it should and nowhere else.
**Shadow: 471.3 → 511.8 DPS (0.71x → 0.77x). Smite: 379.3 → 395.1 DPS (0.74x → 0.77x)** - both
specs' single biggest jump since Judgement of the Crusader. The Smite mana-pacing condition
itself was re-verified against the real priority list's own raw JSON (not a prose paraphrase)
and confirmed exactly correct as implemented - its own remaining gap isn't a pacing bug.

**Mind Flay's clip was retried a fourth time under the now-corrected mana economy - and still
failed, conclusively.** If the three prior attempts' own "it's the mana economy, not the
mechanic" conclusion were right, more mana should have let the clip succeed. It didn't: **511.8
→ 507.1**, worse by almost exactly the same ~1% margin as the third attempt's failure, despite
meaningfully more mana now available. A margin that doesn't shrink when the resource it was
blamed on becomes more abundant means the resource was never the real constraint - clipping
trades Mind Flay ticks for Mind Blast casts at a fixed, deterministic loss in this project's own
model, not a mana-gated one. Four independent attempts now, the last one specifically designed
to test (and refute) the leading theory from the attempt before it - **this closes the Mind Flay
investigation; do not attempt a fifth time.** Full methodology, every mechanic's own source
citation, and the remaining open cast-frequency gap (real, confirmed not mana-driven, cause
still unidentified) in `research-notes/priest-shadow-comparison.md` and `research-notes/
priest-smite-comparison.md`.

## Data pack

Two revisions, kept side by side rather than overwritten - once a pack's been used for a test
run or a result, it's never edited in place (the sibling `wowsimforever` project learned this
the hard way; adopting the same discipline here from the start).

- `data/warlock-forever-2026-10-23-rev1.json` - the original 17 spells: client build
  `1.60.1.70425`'s own data, mined from an MIT-licensed open-source engine, with two entries
  independently cross-checked against a second site (exact match both times).
- `data/warlock-forever-2026-10-23-rev2.json` - rev1 plus three pet/talent-mechanic entries
  (`firebolt`, `lash_of_pain`, and `demonic_brand_hit` under `pet_and_talent_mechanics`) mined
  from the same repository's `prepare/export.rs`. This is the one the engine actually loads
  (`spelldata.rs`'s `include_str!`).

`tools/mine_warlock_data.py` reproduces rev1's extraction from a fresh `spells.jsonl`; the
rev2 additions were a one-off manual extraction (see `research-notes/sources.md`) not yet
folded into that script.

A third pack holds the gear system's items, enchants, race base stats and rating-conversion
constants - see "Gear system" above. Also versioned, same discipline:
`data/warlock-gear-2026-10-23-rev1.json` (the original `destruction_reference` build),
`data/warlock-gear-2026-10-24-rev2.json` (adds the three `{spec}_starter_undead` builds used
for the mythicsim.com comparison, plus the two items they needed that rev1 didn't have), and
`data/warlock-gear-2026-10-24-rev3.json` (same items/builds as rev2, `unimplemented_but_parsed`
corrected after the fifth-pass completeness audit - see "Gear system" above). rev3 is the one
the engine loads (`gear.rs`'s `include_str!`).

Mage has its own two packs, same discipline. `data/mage-forever-2026-10-25-rev1.json`
(Fireball, Scorch, Fire Blast, Evocation - the first pass's four spells), rev2 (adds Pyroblast
and Combustion for Fire's second pass), rev3 (adds Arcane Blast, Arcane Missiles/
`arcane_missiles_barrage`, Frostbolt and Ice Lance for Arcane and Frost Mage), and
`data/mage-forever-2026-10-25-rev4.json` (the one the engine loads - corrects
`arcane_missiles_barrage`'s own tick count, 10 ticks over 5000ms down to 5 ticks over 2500ms,
found during a rigorous per-ability root-cause pass - see `research-notes/mage-arcane-
comparison.md`). `data/mage-gear-2026-10-25-rev1.json` held just
the `fire_mage_starter_undead` build; `data/mage-gear-2026-10-25-rev2.json` (the one the engine
loads) adds `arcane_mage_starter_undead` and `frost_mage_starter_undead` plus the 9 new items
they needed - all three builds still share 9 of their 15 slots byte-for-byte. `GearPack`'s
fields dropped their "warlock_"-prefixed names when the gear pack was added, since the
crit-per-Intellect rate and race base stats are genuinely per-class (confirmed directly from
`go-tables.json`) - a Warlock's gear pack and a Mage's are the same *shape*, never the same
*values*.

Priest has its own two packs, same discipline, both still on their first revision:
`data/priest-forever-2026-10-25-rev1.json` (Mind Blast, Shadow Word: Pain, Devouring Plague,
Shadow Word: Death, Mind Flay, Smite, Smite rank 2, Holy Fire, Penance and Inner Focus - every
spell both specs cast) and `data/priest-gear-2026-10-25-rev1.json` (both `shadow_priest_
starter_undead` and `smite_priest_starter_undead` builds, sharing most items/enchants with
Mage's own gear pack where the same real items happen to appear in both classes' builds).

Unlike the sibling `wowsimforever` project, this project doesn't yet have a placeholder/overrides
tier (every spell it implements has a sourced row) - if that changes, adopt the same discipline
that project settled on: placeholder data only in a clearly-separate, never-auto-loaded file,
and any result using one has to say so.

## Layout

```
Cargo.toml                 workspace (independent of the repo's root Cargo.toml)
crates/engine/              the simulation engine + per-class spell/talent/rotation logic
  src/sim.rs                 the class-agnostic fight engine (damage resolution, auras, dots)
  src/casting.rs             class-agnostic cast helpers (mana/cast-time/cooldown), shared by all classes
  src/gear.rs                the gear/item system (race, items, enchants -> CasterStats), per class
  src/warlock/                Warlock: affliction/demonology/destruction - the mature part
  src/mage/                   Mage: fire, arcane, frost - begun 2026-10-25
  src/priest/                 Priest: shadow, smite - begun 2026-10-25
crates/cli/                  a small runner binary (--class warlock|mage|priest, --spec within it)
data/                        the sourced data packs (spells, gear), one pair per class
research-notes/              how the rotations and numbers were obtained (read this first)
tools/mine_warlock_data.py   script that produced data/warlock-forever-*rev1.json from mined client data
```
