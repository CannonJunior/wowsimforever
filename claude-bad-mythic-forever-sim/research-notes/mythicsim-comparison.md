# Comparing against mythicsim.com (2026-10-24)

The task was to run this engine for all three Warlock specs and compare against mythicsim.com's
current results. The literal site couldn't be touched for this the same way it couldn't for the
original reverse-engineering task: `mythicsim.com/robots.txt` disallows `/sims/` and `/api/` for
`ClaudeBot`/`Claude-User`/`Claude-SearchBot` by name, and that's where both a specific sim result
and the Quick Sim tool's live computation would live.

## What was used instead

`sage3648/mythicsim-forever-engine`'s own validation corpus,
`validation/2026-10-04-builder-starters.json`: a 166-case cross-check of MythicSim's production
Go engine against its newer Rust port, run on "the production application's builder starters -
all 29 starters users begin Build a character from, each in every race it offers... 3000
iterations and seed 42... run by Go and Rust." All 166 matched exactly (`go_dps == rust_dps` in
every case) - this is MythicSim's own actual engine output for real, reproducible inputs, not a
live-site number that could drift or that I'd have to guess at reproducing. Arguably a *better*
ground truth than scraping the live site for this purpose: versioned, exact, and the full
request (talents, gear, encounter) needed to replicate it is checked in alongside it.

15 of the 166 cases are Warlock (5 races × 3 specs). The Undead case of each spec was used, to
match this project's existing `destruction_reference` example build's race:

| Spec | mythicsim go_dps/rust_dps | First run (bug) | After hit-chance fix | After cross-tree talents | After Affliction pet | After buffs + rotation-bug fixes | After per-ability breakdown audit | After Succubus melee (final) |
|---|---|---|---|---|---|---|---|---|
| Affliction | 541.44 | 135.4 | 240.2 | 300.3 (1.80x) | 316.8 (1.71x) | 514.8 (1.05x) | 514.8 (1.05x) | 589.4 (**0.92x**) |
| Demonology | 608.76 | 91.8 | 203.4 | 277.7 (2.19x) | 277.7 (2.19x) | 459.0 (1.33x) | 451.3 (1.35x) | 618.2 (**0.99x**) |
| Destruction | 510.85 | 110.9 | 276.0 | 306.2 (1.67x) | 306.2 (1.67x) | 487.2 (1.05x) | 516.8 (0.99x) | 516.8 (**0.99x**, unaffected - no pet) |

The gap closed from ~4-6x to ~1.7-2.2x across two follow-up passes (2026-10-24): one fixing a
real bug the comparison caught, one implementing the specific missing talents the comparison's
own build-vs-struct diff turned up. A third pass gave Affliction the pet simulation it was
missing entirely. A fourth pass, "analyze why the gap is still large and address it" (same day),
found that the *actual* dominant cause was never pet melee auto-attack at all - it was raid
buffs and consumables this engine never applied, plus two real rotation bugs the earlier passes'
talent-by-talent audit hadn't caught. A fifth pass, "produce a per-ability breakdown in both
sims and analyze the differences" (same day again), built and ran `mythicsim-forever-engine`'s
own Rust CLI locally against the exact same request files to get a genuine ability-by-ability
trace from the real engine - not just its total DPS - and found a reporting bug (dot ticks were
being counted as "casts"), a real damage-calculation bug (dot-only spells never rolled their own
hit chance), a wrong CLI default (Affliction's real build has 0 points in Siphon Life, not the
engine's assumed default), and a whole missing cross-tree talent (Suppression, +1%/point spell
hit, present in all three builds but modeled in none) plus one more missing field (Destruction's
own Improved Corruption). See "Per-ability breakdown: this engine vs. the real one" below for
the full trace and "What the per-ability breakdown pass found" for all five fixes. Destruction
now lands almost exactly on mythicsim's own number; Affliction is close; Demonology's own
residual is now measured and attributed directly to her Succubus's melee auto-attack and a
proportionally under-triggered Demonic Brand, not estimated - see that section.

Scenario: level 60 character vs. a level 63 target, 120s fight duration, 3000 iterations,
`RaceUndead`. Gear and talents came from the matching
`validation/2026-10-04-builder-starters/{spec}-warlock--undead.request.json` (equipment lists
added to `data/warlock-gear-2026-10-24-rev3.json` as `{spec}_starter_undead`; talent strings
decoded below).

### Reproducing these exact numbers

Re-running this comparison (2026-10-24) cost three wrong attempts before matching the
previously-documented figures again, because the exact CLI invocation for each spec was never
actually written down here - only the decoded talents prose above, which doesn't map 1:1 onto
CLI flag defaults. Recorded now so that doesn't happen again:

```sh
cargo run -p mythic-forever-cli -- --spec affliction  --gear affliction_starter_undead  --race RaceUndead --pet succubus --buffs builder-starter --duration 120 --iterations 3000 --seed 42
cargo run -p mythic-forever-cli -- --spec demonology  --gear demonology_starter_undead  --race RaceUndead --pet succubus --sacrifice imp --buffs builder-starter --duration 120 --iterations 3000 --seed 42
cargo run -p mythic-forever-cli -- --spec destruction --gear destruction_starter_undead --race RaceUndead --sacrifice imp --incinerate false --improved-life-tap 2 --buffs builder-starter --duration 120 --iterations 3000 --seed 42
```

`--buffs builder-starter` (added in the gap-analysis pass) applies the raid-buff + consumable
package below - every one of the "builder starter" request JSON files uses this exact package
(confirmed identical across all three specs' own files, 2026-10-24), so it's always on for this
comparison from here on.

Two flags have no spec-specific default and so must always be passed explicitly when they
differ from the CLI's generic default (which matches no particular real build - it's just a
reasonable started value): `--pet` (every spec defaults to no reasonable guess of its own: `Imp`
for Demonology, `Succubus` for Affliction, N/A for Destruction - this build needs Succubus for
Demonology and Affliction, nothing for Destruction) and `--sacrifice` (defaults to no
sacrifice; this build sacrifices the Imp for both Demonology and Destruction, not Affliction).
`--incinerate` defaults to `true` (a reasonable guess for a generic Destruction run) but this
specific decoded build is one point short of the real Incinerate talent's 31-point
prerequisite, so it needs `false` here. `--improved-life-tap 2` is this decoded Destruction
build's real Affliction-tree dip, which the CLI's generic default (0) doesn't anticipate.
Affliction and Demonology's other talent flags already default to this comparison's exact
decoded values (set that way in the pass that added them), which is why they don't need to be
repeated above.

## Decoding the talent strings

`wowsims/forever`'s `ui/sim/talents/trees/warlock.json` gives each talent's exact
`rowIdx`/`colIdx` - sorting each tree's talents by `(row, col)` and zipping against that tree's
segment of the `-`-separated talent string (after right-padding with `0`s - classic WoW talent
strings truncate trailing zeros per tree, which is what made the naive "`len(digits) ==
len(talents)`" first attempt fail) gives the exact point allocation. All three builds spend
51 points total (the real level-60 cap), confirming `CHARACTER_LEVEL = 60` independently again.

Decoded (nonzero only):
- **Affliction build**: full Affliction tree (suppression 5, improvedCorruption 5, grimReach 2,
  improvedDrains 3, improvedBaneOfAgony 2, amplifyCurse 1, pandemic 3, malevolence 5,
  nightfall 2, shadowMastery 5 = 33pts) + Destruction dip (improvedShadowBolt 5, bane 5,
  ruin 5, agonizingFlames 3 = 18pts). Pet: Succubus, summoned (not sacrificed).
- **Demonology build**: Affliction dip (suppression 2) + full Demonology tree through its
  capstone (demonicEmbrace 2, unholyPower 5, felVitality 3, demonicEnergies 2,
  improvedSayaad 3, demonicSacrifice 1, decimation 2, demonicBrand 3, soulLink 1,
  demonicKnowledge 3, masterDemonologist 5, demonicPact 1) + the *same* Destruction dip as the
  Affliction build. Pet: Succubus active, **with an Imp sacrificed first** (Demonic Pact lets
  the sacrificed demon's buff persist under a different active pet - not modeled here, see
  below).
- **Destruction build**: Affliction dip (improvedLifeTap 2, suppression 5, improvedCorruption 2,
  grimReach 2) + Demonology dip (demonicEmbrace 5, demonicAegis 2, felVitality 3,
  demonicSacrifice 1) + full Destruction tree short of its capstone (destructiveReach 1,
  improvedShadowBolt 5, bane 5, cataclysm 3, ruin 5, shadowburn 1, agonizingFlames 3,
  conflagrate 1, shadowAndFlame 5 = 29pts, one short of Incinerate's 31). Pet: Imp, summoned
  then **immediately sacrificed** (no active pet - matches this engine's own no-pet
  Destruction model exactly).

These are the game's own "builder starter" defaults (the census file says so explicitly), not
hand-tuned BiS specs - the identical Destruction-tree dip in both the Affliction and
Demonology builds is the generator reusing one filler template, not a coincidence worth reading
into.

## A real bug this comparison caught and fixed

The first run came out 4-6x below mythicsim's numbers - far more than any talent gap could
explain. The per-ability breakdown showed why immediately: hit rates near 5%, not the 80-90%
range they should be. `gear::equipment_to_stats` was feeding `CasterStats.spell_hit` *only* the
gear-granted hit bonus (4-5% for a starter build), silently dropping the base spell hit chance
every character has before any gear at all. Confirmed the real number directly in
`wowsims/forever`'s own `sim/core/attack_table_test.go`, whose base-chance table has an exact
row for level 63: `spellMiss: 0.17` - 83% base hit, not 0%. Fixed (`gear.rs` now adds this base
before the gear bonus) and added two regression tests (`gear_sanity.rs`). This moved the
comparison from a 4-6x gap to the ~2-2.5x one in the table above, and is a genuine, general
correctness fix for every `--gear`-driven run, not something specific to this comparison.

## What the "address the DPS gap" follow-up pass closed (2026-10-24, same day)

Every item identified above as a missing, named field was implemented:

- **Ruin, Improved Shadow Bolt, Bane, Agonizing Flames** are now fields on `AfflictionTalents`
  and `DemonologyTalents` too, reusing the exact same mechanics `destruction.rs` already had
  (the live timed debuff for Improved Shadow Bolt, the flat cast-time cut for Bane), applied to
  whichever of each spec's own spells those talents' real class-masks actually reach (Shadow
  Bolt for both; Immolate too for Affliction when `keep_immolate` is set).
- **Demonic Brand** no longer requires `PetChoice::Imp` - `maybe_brand_hit` takes the pet's own
  school (Fire for Imp, Shadow for Succubus) and either pet's landed hit can now spend a charge.
- **Master Demonologist** no longer requires the Imp either - both demons' matching-school
  halves are the same +2%/point, so the gate was simply wrong.
- **Demonic Sacrifice** is a new `Option<SacrificedPet>` field (shared between
  `DestructionTalents`/`DemonologyTalents`/`AfflictionTalents` via `warlock::SacrificedPet`),
  applying the real +15% - confirmed directly from the client rows for spells 18789 ("Burning
  Shadow", the Imp's sacrifice) and 18791 ("Touch of Fire", the Succubus's): Forever really does
  reverse Classic's pairing, Imp's sacrifice buffs *Shadow*, Succubus's buffs *Fire*. Modeled as
  independent of which pet is actively simulated, which happens to cover Demonic Pact's real
  effect (a sacrifice persisting under a different active pet) for free, without needing to
  model the mechanic's own talent-gating explicitly.
- **Unholy Power** (+2%/point to *any* active demon's damage, confirmed via the source's own
  `for pet in &self.pets.base` - not Imp-specific) and **Soul Link** (+3% damage dealt for the
  warlock and the active demon, confirmed via spell 25228's own `BasePoints: 3`) were found
  while re-checking the Demonology build's full talent list for anything else still missing,
  and added the same way.
- Caught and fixed one more real bug in the same pass: `destruction.rs`'s Agonizing Flames was
  missing Immolate's direct hit entirely from its damage bonus (the real class-mask excludes
  only Immolate's *dot*, not its direct hit) and never gave Immolate's dot its own
  `DotDamageDonePct` line at all. Both fixed; `AfflictionTalents`' Agonizing Flames picked up
  the corrected version from the start since it was written after this fix.

Two new regression tests (`data_pack_sanity.rs`) pin the cross-tree talents actually raising
Affliction's Shadow Bolt damage and prove Demonic Sacrifice's school reversal (sacrificing the
Imp buffs Shadow, sacrificing the Succubus does *not*).

## What the 2026-10-24 "simulation differences + gear" follow-up pass closed

- **Affliction now simulates its Succubus.** `PetChoice`/`pet_cast` (the Firebolt/Lash of
  Pain/Demonic Brand mechanics) moved out of `demonology.rs` into `warlock/mod.rs` as shared
  code, and `AfflictionTalents` got a `pet: Option<PetChoice>` field plus the same
  player/pet-interleaving loop `demonology::run` already had. Defaults to
  `Some(PetChoice::Succubus)` on the CLI (matching the real "builder starter" build, which keeps
  a Succubus summoned, not sacrificed) unless `--pet none|imp|succubus` overrides it; library
  callers still get `None` (no pet) via `AfflictionTalents::default()` unless they opt in, so
  this is additive, not a behaviour change for existing callers. Raised the
  `affliction_starter_undead` comparison from 300.3 to 316.8 DPS (1.80x -> 1.71x gap) - a real
  but modest gain, since Lash of Pain was only ever ~1.4% of this build's total damage (9.6
  casts/120s at a 429-coefficient scaled to 10% of spell power - a much smaller contributor
  than a Demonology pet, which is most of that spec's damage).
- **Gear audited against the real engine's item mechanics for completeness.** Re-fetched
  `sage3648/mythicsim-forever-engine`'s own `data/items.jsonl` directly (2026-10-24) to check
  `pseudo_stats`, `rand_prop_points`/random suffixes, `item_effects` and item-set bonuses
  against this pack's actual 17 mined items, rather than trusting this project's own prior,
  partly-stale claims about what gear.rs did and didn't apply. Found: `gear.rs`'s module doc
  was stale (ShadowDamage/FireDamage and Intellect-to-mana were already implemented as of
  rev2, just not documented as such); none of the other four concepts are live for any item or
  build in this pack today (checked in detail, see `gear.rs`'s updated module docs and the
  rev3 data pack's `unimplemented_but_parsed` field), so nothing was being silently dropped,
  but `ItemRow` genuinely has no field for any of them if a future item needs one. Published as
  `data/warlock-gear-2026-10-24-rev3.json` per this project's own versioning convention (never
  overwrite a pack already used for a test or a result).

## What the gap-analysis pass actually found (2026-10-24)

The task was to analyze *why* the ~1.7-2.2x gap was still so large, not just list scope cuts -
the previous section's "most likely the Succubus's melee auto-attack" was a guess, never
checked against the numbers. It was checked this time, and turned out to be wrong as the
*dominant* cause (it's real, see below, but small). Re-fetching the actual "builder starter"
request JSON files directly (not just their talent strings, which earlier passes had already
decoded) showed why: every one of them runs with a full raid-buff and consumable package this
engine never applied at all, and two of the three rotations had real translation bugs
independent of all of that.

**Raid buffs and consumables - the dominant cause.** All three request JSON files carry an
identical `raid.buffs`/`raid.debuffs`/party `buffs`/player `buffs`/`consumables` block (checked
byte-for-byte across all three, 2026-10-24) - a standard pre-pull buff loadout, not something
this comparison's gear/talent decoding had ever looked at. None of it touches a flask/elixir
GCD mid-fight (only Flask/Elixir/food/potion-as-standing-buff, no potion *usage* - see below),
so it's exactly the kind of static, pre-fight stat delta `gear.rs` already computes from items -
just from a different source. Implemented as `gear::equipment_to_stats`'s new
`builder_starter_buffs: bool` parameter (`--buffs builder-starter` on the CLI, default off so
every existing caller/test is unaffected) applying real, individually-sourced amounts (see
`gear.rs`'s `BUFF_*` constants for the full per-item/per-spell citations, each checked directly
against `foreverdb.net`, robots.txt-cleared the same way every other source here was):
- Arcane Brilliance (+31 Int), Gift of the Wild (+16 all attributes), Well Fed from Runn Tum
  Tuber Surprise (+15 Int), Zanza's potion/Cerebral Cortex Compound (+25 Int), Prayer of Spirit
  (+40 Spirit) - flat Intellect/Spirit, feeding the existing Int-to-crit/Int-to-mana/
  Spirit-to-regen formulas.
- Greater Blessing of Kings: +10% to the *total* Intellect/Spirit from every source above
  (applied last, multiplicatively).
- Flask of Supreme Power: +150 universal spell damage. The package's own `spellPowerElixirId`
  (Greater Arcane Elixir, +35, same universal stat) is real but inert once the flask is up -
  confirmed via `wowsims/forever`'s own `ExclusivePerStat` rule
  (`sim/common/shared/consumable_stats.go`): only the single highest buff of the *same* stat
  applies, so the elixir doesn't also add on top.
- Elixir of Shadow Power: +40 *Shadow-specific* spell damage - a different stat from the flask's
  universal one, so it does stack on top (confirmed via the same `ExclusivePerStat` rule: it
  only dedups same-stat buffs).
- Moonkin Aura: +3% spell crit - not Classic/TBC's 5%, confirmed directly from this client's own
  tooltip.
- Mageblood Elixir (+12 mp5) and Greater Blessing of Wisdom (+40 mp5) - sustain only, doesn't
  move DPS in these particular runs (none of them ever actually Life Tap - see each ability
  table's absence of a `life_tap` row).

This one fix alone raised Affliction 316.8 -> 468.0, Demonology 277.7 -> 401.5, Destruction
306.2 -> 442.9 - by far the largest single jump across every pass of this whole comparison
effort, confirming it as the dominant cause, not pet melee.

**A second real bug: Curse of the Elements did nothing.** All three rotations already cast and
tracked Curse of the Elements (`aura_active("curse_of_the_elements")` existed purely for refresh
timing), but nothing ever multiplied damage by its actual effect - a warlock's own curse debuff
was mechanically a no-op on their own numbers. The spell's real effect (confirmed via its data
pack entry, spell 1311680, cross-checked against `foreverdb.net`'s own tooltip text): "+10%
Magic damage taken." Added as `Fight::curse_of_the_elements_bonus()` (`sim.rs`), checked inside
`resolve_direct`, `resolve_direct_raw` and `apply_dot` - the same aura-gated pattern as every
other live buff this engine already tracks, not a new mechanism.

**A third real bug: `demonology.rs`'s rotation was translated incompletely.** Dumping
Demonology's real priority list in full (not just the Curse of the Elements line already found)
showed two of its real actions were missing from this project's translation of it, both
present since the very first mining pass, not introduced by anything since:
- Curse of the Elements itself (spell 1311680, action 7 of 12) - `demonology.rs` never cast it
  at all, unlike `affliction.rs`/`destruction.rs`.
- Immolate (spell 25309 "Immolate Rank 8", action 10 of 12) - entirely absent; Demonology never
  dealt any Immolate damage before this pass. Added the same dot-refresh cast
  `affliction.rs`/`destruction.rs` already use, and extended Ruin/Bane/Agonizing Flames (which
  already applied to Shadow Bolt) to also cover Immolate the same way `affliction.rs`'s
  `keep_immolate` case does - these three cross-tree Destruction talents' real class-masks reach
  Immolate regardless of which spec casts it.

Both fixes are in `demonology.rs`'s `step`/`apply_talents`; the other two actions in its real
priority list that aren't modeled (`1259821` "Eureka!", an Engineering-profession racial
ability, and `OtherActionPotion`/item `12662`, a combat-use mana item) are legitimate,
already-documented scope cuts (no racial/profession system, no mid-fight consumable usage), not
further bugs.

## Per-ability breakdown: this engine vs. the real one (2026-10-24)

**A historical snapshot** - the tables below are this engine's output *before* the Succubus
melee combat system ("Fixing Demonology's gap" further down closed that specific gap); kept
as-is since they're what the fixes in "What the per-ability breakdown pass found and fixed"
were actually diagnosed from, not retroactively updated.

Produced by building `mythicsim-forever-engine`'s own Rust CLI locally (MIT-licensed, the same
repository this whole comparison already treats as ground truth - `cargo build --release` from
a shallow clone, not touching mythicsim.com itself) and running it with
`sim --request <same request JSON> --rotation-stats --go-rules` against the identical three
request files this comparison already uses. `--go-rules` matched the validation corpus's own
total DPS closely (Demonology 609.49 vs. 608.76 logged, Destruction 503.93 vs. 510.85,
Affliction 554.22 vs. 541.44 - the small remaining gaps are most likely this HEAD checkout
having drifted slightly from whatever commit the validation corpus was generated at, not a
methodology error); this is a real trace from the real engine, not an estimate. Average
per-fight values below (raw per-iteration totals ÷ 3000 iterations ÷ 120s), spell IDs resolved
against `foreverdb.net`:

**A mistake caught and fixed while building these tables**: the real engine's own JSON reports
each ability's `damage` field as the *already-combined* total (normal hits/ticks plus crits),
with `critDamage`/`tickDamage`/`critTickDamage` as informational sub-breakdowns of that same
total, not separate components to add on top. A first pass through this data summed `damage +
tickDamage + critDamage + critTickDamage`, roughly doubling every dot's and crit-heavy
ability's number - caught by summing the per-ability table back up and finding it didn't match
the engine's own reported total `dps.avg` (off by as much as 60%). Recomputed using only the
`damage` field below; every row now sums to that engine's own reported total, player actions
plus pet actions, within rounding.

**Affliction** (this engine: 514.8 DPS; real engine, same request, same seed: 554.2 DPS, 1.08x):

| Ability | This engine DPS (casts/fight) | Real engine DPS (casts/fight) |
|---|---|---|
| Shadow Bolt | 275.3 (28.2) | 245.0 (28.9) |
| Bane of Agony | 90.6 (5.5) | 92.2 (5.5) |
| Corruption | 82.2 (6.5) | 82.5 (6.9) |
| Immolate (direct+dot) | 60.8 (7.7) | 56.0 (7.6) |
| Lash of Pain (Succubus) | 5.9 (9.3) | 6.1 (10.5) |
| Succubus melee auto-attack | *not modeled* | 62.2 (60.4) |
| Touch of the Grave (Undead racial) | *not modeled* | 6.1 |
| Searing Pain | *not in this rotation* | 3.6 |

**Destruction** (this engine: 516.8 DPS; real engine: 503.9 DPS, **0.98x** - this engine is
now slightly *ahead* of the real one, not behind):

| Ability | This engine DPS (casts/fight) | Real engine DPS (casts/fight) |
|---|---|---|
| Shadow Bolt | 205.9 (22.0) | 194.6 (21.6) |
| Immolate (direct+dot) | 79.9 (8.4) | 63.0 (7.8) |
| Corruption | 74.2 (6.5) | 77.4 (6.9) |
| Bane of Agony | 70.4 (5.4) | 74.8 (5.4) |
| Shadowburn | 42.9 (6.6) | 49.3 (7.3) |
| Conflagrate | 43.5 (6.3) | 33.2 (6.8) |
| Touch of the Grave (Undead racial) | *not modeled* | 6.8 |
| Searing Pain | *not in this rotation* | 4.1 |

**Demonology** (this engine: 451.3 DPS; real engine: 609.5 DPS, **1.35x** - the one real
residual):

| Ability | This engine DPS (casts/fight) | Real engine DPS (casts/fight) |
|---|---|---|
| Shadow Bolt | 196.8 (21.1) | 252.2 (25.4) |
| Corruption | 72.4 (7.2) | 81.1 (7.1) |
| Bane of Agony | 70.6 (5.5) | 84.0 (5.7) |
| Immolate (direct+dot) | 62.4 (7.6) | 60.8 (7.8) |
| Searing Pain | 32.5 (11.7) | 17.6 (5.2) |
| Lash of Pain (Succubus) | 9.0 (9.3) | 13.6 (10.5) |
| Demonic Brand hit (Succubus) | 7.4 | 22.6 |
| Succubus melee auto-attack | *not modeled* | 70.8 (60.4) |
| Touch of the Grave (Undead racial) | *not modeled* | 6.1 |

Affliction's and Destruction's totals are now close enough that the two sims are describing
essentially the same fight - the per-ability deltas that remain partly cancel (this engine runs
a bit hot on Shadow Bolt/Immolate/Conflagrate, a bit cold on the dots and Shadowburn, and is
missing the Succubus's melee/Undead racial entirely for Affliction). Demonology is different in
kind, not just degree - see below.

## What the per-ability breakdown pass found and fixed (2026-10-24)

Producing the table above surfaced five real issues, independent of raid buffs/consumables and
the two rotation bugs the previous pass already found:

- **A reporting bug**: `AbilityTotals.casts` was incremented by every dot *tick*, not just
  every cast - Bane of Agony showed ~51.9 "casts" over 120s (its real duration is 24s, ~5
  refreshes) because its 12 ticks per application were silently folded into the same counter.
  Fixed by splitting `casts`/`ticks` into separate fields (`sim.rs`); `process_due_dot_ticks`
  now calls a dedicated `record_tick` that doesn't touch `casts`.
- **A real damage-calculation bug, found while the `casts` count was still obviously wrong**:
  `apply_dot` never rolled the spell's own hit chance at all - every dot-only cast (Corruption,
  Bane of Agony, Siphon Life) landed 100% of the time regardless of `spell_hit` (~87-88% for
  these builds), while every direct-damage spell correctly rolled. Fixed: `apply_dot` now rolls
  hit before applying; a direct+dot spell's dot (Immolate) applies only when the one shared
  roll already made by `resolve_direct` landed, rather than a second independent roll (a real
  cast lands or misses as a whole, not two separately-resolved coin flips) - see
  `apply_dot_unconditional` in `sim.rs`.
- **A wrong CLI default**: `AfflictionTalents.siphon_life` defaulted to `true`, but the real
  "builder starter" talent string (`0552032013520005--0550005003`, decoded directly against
  `wowsims/forever`'s own talent-tree layout) has 0 points in Siphon Life - a leftover guess
  from before this project ever decoded that string. Fixed the default to `false`.
- **A whole missing cross-tree talent**: Suppression (+1%/point spell hit - confirmed from spell
  18174's own tooltip, not Classic's +2%/point) is a tier-1 Affliction talent any build can pick
  up, and all three "builder starter" builds do (Affliction 5, Demonology 2, Destruction 5) -
  none of the three `Talents` structs had a field for it at all. Added to all three, applied in
  each spec's `apply_talents`.
- **One more missing field**: Destruction's own decoded build also spends 2 points in Improved
  Corruption, which `DestructionTalents` had no field for (the other two specs already did).
  Added, reusing `affliction.rs`'s exact formula.

Four regression tests cover these (`curse_of_the_elements_raises_damage_once_cast`,
`demonology_now_casts_immolate`, and `dot_only_casts_can_now_miss_like_every_other_spell` are
from this and the prior pass combined - see `data_pack_sanity.rs`).

## Fixing Demonology's gap: a melee combat system (2026-10-24, same day again)

The previous pass left Demonology's gap (1.35x) measured but unfixed: ~71 DPS of it was the
Succubus's unmodeled melee auto-attack, the single largest remaining piece across the whole
comparison effort. Asked explicitly to fix it, this pass built a real melee system rather than
approximating the aggregate DPS number - reading the actual mechanics from the same
`mythicsim-forever-engine` clone this whole comparison already treats as ground truth
(`src/classes/warlock/prepare/pets.rs`, `src/core/fight/melee.rs`, `src/prepare/attack.rs`,
`src/prepare/character.rs`), then cross-checking every derived constant against that same
engine's own debug combat log for this comparison's exact fixture (`debugFirstIteration: true`
in the request JSON produces one - a genuinely useful source this project hadn't used before):

- **Weapon damage, attack power, swing timing** - all sourced directly, no cross-check needed:
  the Succubus's weapon is 95-131 (`petAutoAttacks`; the Imp has none at all - `Summon::Imp =>
  None`, she never melees), attack power converts to damage at 14 AP per 1 DPS
  (`src/prepare/character.rs`), and her swing lands every 2.0s (confirmed against the log's own
  60.4 swings/120s).
- **Glancing blows** - sourced from `src/prepare/attack.rs`'s own level-indexed table (a pet
  swinging at a target 3 levels up lands in its highest band: 40% chance, 65% average damage,
  ±10% spread) and cross-checked twice against the log (measured glance rate 40.1-40.2%,
  measured average glance damage exactly 65.0% of a normal hit's own average, both builds).
  Melee crit multiplier (2.0x) confirmed the same way (measured crit average / measured normal
  average = 2.001).
- **Two numbers the debug log gave directly but whose own general formula wasn't tracked down
  in the time available** (flagged the same way this project already flags its other
  Go-literal pet constants as a lower sourcing tier than the main data pack): the Succubus's own
  innate attack power from her fixed Strength/Agility (258.0 - the log's own "Pet stats"
  AttackPower minus "Pet inherited stats" AttackPower), and her own base physical crit on top of
  her owner's inherited spell crit (4.25% - same subtraction, this time on crit percent).
- **Demonic Brand now triggers from melee too**, not just Lash of Pain - the log showed 181267
  melee swings and 31435 Lash of Pain casts for the same pet, vastly more trigger opportunities
  than this engine modeled before. Master Demonologist, Unholy Power and Soul Link (already
  "the active pet's own damage," not spell-specific, per their own doc comments) now reach her
  melee too.
- **A real scheduling bug, found because the new melee timeline made it impossible to miss**:
  both the pet's spell-cast timer and the new melee timer rebased their next scheduled time off
  `fight.now_ms` every time they fired, instead of incrementing from their own previous
  schedule. Whenever another actor's turn happened to come first (routine, with three
  interleaved timers), the delayed actor's `advance` call became a no-op and it fired at
  whatever time it was *actually* getting around to - silently losing whatever ground it fell
  behind by, every single cycle. Harmless-looking for Lash of Pain's coarse 12s cadence (a
  measured ~12% shortfall: ~9.2 casts/fight instead of ~10.5), glaring for melee's 2.0s one (a
  measured ~35% shortfall: ~38 swings instead of ~60) - fixed for both by self-incrementing from
  the previous schedule instead, which can never lose ground to execution-order jitter.

Result: Demonology 451.3 -> **618.2 DPS against the real engine's 609.5 (0.99x)** - essentially
matched. Affliction's own (smaller) Succubus share closed the same way: 514.8 -> 589.4 against
554.2 (**0.92x** - now running a few percent *ahead* of the real engine, the mirror image of the
gap it used to have below it). Destruction has no active pet in this comparison's build and is
unaffected (516.8, 0.99x, unchanged).

## Why the remaining gap is real

- **Demonology (0.99x) and Destruction (0.99x) are now within 1% of the real engine - as close
  as this comparison is likely to get without chasing sub-percent noise.** What's left for
  Demonology specifically: the real engine's own trace showed Demonic Brand triggering at 22.6
  DPS; this engine's melee-plus-Lash-of-Pain triggering now *overshoots* that (consistent with
  this engine casting more Searing Pain than the real rotation does - 11.7 vs. 5.2 casts/fight,
  a rotation-tuning difference flagged in the previous pass and not changed here, since fixing
  it needs decoding a second Searing Pain gating condition this project's translation of the
  real priority list simplified away, not a missing mechanic). This overshoot happens to roughly
  cancel against Shadow Bolt running a little cold, landing the total almost exactly on target -
  not chased further at the per-ability level given how close the total already is.
- **Affliction (0.92x) now runs a few percent ahead**, the same melee/crit formula calibrated
  against Demonology's own build generalizing slightly differently to Affliction's (lower spell
  crit, no pet talents at all) - expected variance from calibrating two of the melee constants
  against one build's debug log rather than an independently-derived general formula (see
  `SUCCUBUS_BASE_ATTACK_POWER`/`SUCCUBUS_BASE_PHYSICAL_CRIT` in `warlock/mod.rs`), not
  investigated further given the total is already this close.
- **Potions/consumables *used* mid-fight, racial abilities (Touch of the Grave, ~6 DPS both
  builds) and "autocastOtherCooldowns"** remain out of scope (see "Scope" in the README) - this
  comparison's own per-ability trace shows them concretely now, not just as names in a scope
  list, but actually modeling them is a different kind of system (consumable-usage timing, a
  racial-ability system) than anything this pass added.

The real Searing Pain/Demonic Brand refresh condition - decoding the request JSON's exact
boolean AST rather than this engine's simplified flat-threshold approximation of it - is the
one remaining concrete next step, and a small one relative to everything this and the previous
two passes already closed.

## Two cross-class fixes found while chasing Mage gaps, re-verified here (2026-10-25)

`mage-arcane-comparison.md`'s own "sixth investigation" found two generic bugs via Fire/Arcane
Mage's own debug logs that apply to every class, Warlock included: a systemic cooldown-timing
bug (`Fight::set_cooldown_from`, also fixed directly in this spec's own `amplify_curse` cooldown
block) and Mana Spring Totem's own missing +25 mp5 (`gear.rs::BUFF_MP5`). Re-running all three
Warlock specs with the exact commands this file's own tables use (same gear/talent/buff flags)
confirms no regression - if anything, a small further gain on top of the numbers above:
Destruction 516.8 → 525.9 (now 1.04x of 503.9), Demonology 618.2 → 632.2 (1.04x of 608.76),
Affliction 589.4 → 588.3 (1.06x of 554.2, within noise of the prior 1.08x-inverse/0.92x figure
above). All three specs' own full test suite (`cargo test --workspace`, 42 tests) still passes.
