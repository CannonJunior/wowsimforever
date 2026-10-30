# Comparing Fire Mage against the real engine's own trace (2026-10-25)

The same methodology `research-notes/mythicsim-comparison.md` already used for all three
Warlock specs, applied to Fire Mage for the first time: build `mythicsim-forever-engine`'s own
Rust CLI locally (MIT-licensed, the same repo this whole project already treats as ground
truth) and run it with `--rotation-stats` against the real `fire-mage--undead.request.json`
fixture, to get a genuine ability-by-ability trace from the real engine - not an estimate, and
not just a single aggregate DPS number to chase blindly.

## A validation failure that caught a real bug before any damage numbers were even compared

Running the fixture's own talent string through the local build immediately failed:

```
mage talents "05012050030000-13550000130135031-004": Elemental Precision has 3 ranks, the string gives 4
```

Both this project's own decoded value (`FireTalents::elemental_precision: 4`, from the first
Fire Mage pass) and the *validation corpus's own logged talent string* for this exact scenario
carry a 4 in that slot - so this isn't a one-off malformed request, it's a real discrepancy
between whatever game-data version generated this validation corpus and the current HEAD of
the engine repo (the same kind of "repo drift" already documented for a couple of Warlock's own
comparison runs, just surfacing as a hard rejection instead of a quiet few-percent number
drift). The UI talent-tree metadata this project used to decode the string
(`ui/sim/talents/trees/mage.json`'s `maxPoints: 4` for Elemental Precision) was simply wrong for
this one talent, confirmed directly against the engine's own validator rather than trusted at
face value. Fixed this project's own `elemental_precision` default from 4 to 3 (and the talent
string used to re-run the comparison, below, to match) - a real, if small, bug this comparison
caught before it ever got to compare a single damage number.

## Two more real bugs this comparison caught

**`--buffs builder-starter` was never passed for the first Fire Mage pass at all.** The same
oversight the very first Warlock comparison made (see that doc's own history) - the request
JSON carries the identical shape of raid-buff + consumable package Warlock's own fixtures do
(confirmed by checking the real engine's own debug log for this exact run: the same raid buff
spell IDs - Arcane Brilliance 23028, Prayer of Spirit 27681, Gift of the Wild 21850, Greater
Blessing of Kings 25898, Greater Blessing of Wisdom 25918, Moonkin Aura 24907 - are all active).
But the *consumables* aren't identical to Warlock's package: this fixture's own `schoolElixirId`
is 6373 (Elixir of Fire Power, +10 Fire-specific spell damage) rather than Warlock's 9264
(Elixir of Shadow Power, +40), and it carries a weapon imbue Warlock's own package has none of
(`mhImbueId`: 20749, Brilliant Wizard Oil, +36 universal spell damage/+1% crit). Modeled as a
new `BuilderStarterBuffs::Mage` variant alongside the existing `::Warlock` one in `gear.rs`,
reusing the identical raid-buff math and only substituting the class-specific consumable
numbers - not a new buff system, the same one generalized to a second class's real values.
Raised this build from 328.8 to 465.5 DPS by itself.

**Pyroblast's own dot was never applied.** The exact same bug this engine's own Fireball had in
the very first Fire Mage pass (caught and fixed before Pyroblast even existed in this engine) -
`cast_direct_with_cost_pct` resolves the direct hit only; Pyroblast's real periodic effect (53
per tick, 4 ticks over 12s) needs the same `apply_dot_unconditional` call Fireball's own cast
already makes, which Pyroblast's newer code simply never got. Found because Pyroblast's DPS
ratio against the real trace (0.51) was far worse than its cast-count ratio (0.93) suggested it
should be - a mismatch between "we're casting it about as often as the real engine" and "each
cast does about half the damage" that had no explanation *except* a missing damage component.
Fixed; raised this build's Pyroblast share from 70.5 to 100.4 DPS and the total to 495.6.

## The per-ability trace, after all three fixes

Scenario: level 60 Undead vs. a level 63 target, 120s, 3000 iterations, seed 42, the
`fire_mage_starter_undead` gear build, `--buffs builder-starter`. Real engine total: 704.9 DPS
(this specific local run's own number - see "Methodology" below for why this, not the archived
validation corpus's 589.76, is the correct baseline to compare against here). This engine: 495.6
DPS. **Gap: 1.42x** - down from the first pass's 2.3x-2.7x range, but not closed to Warlock's
own ~1x the way that class's comparison eventually got.

| Ability | This engine DPS (casts/fight) | Real engine DPS (casts/fight) | Ratio |
|---|---|---|---|
| Fireball | 229.7 (23.4) | 295.3 (22.9) | 0.78 |
| Pyroblast | 100.4 (6.6) | 139.3 (7.1) | 0.72 |
| Fire Blast | 61.7 (9.3) | 101.1 (11.5) | 0.61 |
| Ignite | 60.4 (-) | 88.3 (-) | 0.68 |
| Scorch | 43.4 (11.5) | 45.7 (9.7) | 0.95 |
| Arcane Missiles | *not modeled* | 29.9 (9.4) | - |
| Touch of the Grave (Undead racial) | *not modeled* | 3.6 | - |

Cast counts are close for every ability (Scorch even runs slightly *more* often here than in
the real engine) - this gap is overwhelmingly a **per-hit damage** shortfall, not a rotation or
cast-frequency one, unlike most of what Warlock's own comparison found. Scorch alone comes in
at 95% - the one ability that's nearly exact.

## A found, quantified, but not-yet-resolved discrepancy: effective spell power

The real engine's own `[DEBUG]` log lines for Fire Blast casts show `SP: 493.0` - this engine's
own computed effective Fire spell power for the identical build (`spell_power: 329.0 +
bonus_fire_power: 93.0 = 422.0`, confirmed via a direct `CasterStats` printout) is **71 points
(17%) lower**. This is very likely the dominant explanation for the uniform-ish shortfall across
Fireball/Pyroblast/Fire Blast/Ignite (all of which scale off effective spell power directly) -
Scorch's own near-exact 0.95 ratio despite the same formula is the one data point that doesn't
fit a pure spell-power-scaling story as *the whole* explanation, so this likely isn't the only
factor either. Not resolved in this pass:
- Brilliant Wizard Oil's own +36 universal spell power was modeled as inert (the same
  `ExclusivePerStat` rule that makes Greater Arcane Elixir's +35 lose to the Flask's +150 for
  Warlock) - if a weapon imbue is actually a *different* category from an elixir and doesn't
  compete with the flask the same way, that would recover 36 of the missing 71 points, but
  this wasn't independently confirmed against `consumable_stats.go`'s own exclusivity
  categories in the time available.
- Even with that 36 recovered, 35 points of effective spell power remain unaccounted for.
- The same debug line's `AfterAttackerMods` step shows roughly a +15% multiplier on top of the
  base roll that doesn't cleanly decompose into this build's own known talent percentages
  (Fire Power's 3% alone doesn't explain it) - not chased further; could be the same missing
  spell-power points cascading through a percentage step, could be something else entirely.

This is the concrete next step if Fire Mage's gap needs closing further: find exactly where the
71-point effective spell power gap comes from (Wizard Oil's real exclusivity category is the
most promising specific lead), the same way Warlock's own per-ability comparison eventually
traced its Demonology residual to a specific, named cause rather than stopping at "it's close
enough."

## Resolved (2026-10-25, third investigation, prompted by Arcane/Frost Mage's own comparisons)

Both open items above turned out to be real, and both are now fixed - see
`mage-arcane-comparison.md`'s own "A rigorous, formulaic root-cause pass" section for the full
methodology and source citations (found while comparing Arcane and Frost Mage, then confirmed
to apply here too, since all three specs share the same gear/buff code):

- **The 71-point gap was exactly Brilliant Wizard Oil's +36 and Greater Arcane Elixir's +35,
  both wrongly modeled as inert.** The guessed lead above ("Wizard Oil's real exclusivity
  category") was right in spirit but wrong in scope - a direct read of the real engine's own
  `consumes.rs::register_consumes` shows *neither* consumable is exclusive with the flask at
  all; both are added unconditionally. Fixed in `gear.rs`, not per-spec (`BUFF_SPELL_POWER_
  ELIXIR` for every buffed build, `MAGE_BUFF_WIZARD_OIL_SPELL_POWER` for Mage's own two
  variants) - this also quietly raises Warlock's own three specs' DPS by the same 35 points,
  since their fixtures carry the identical `spellPowerElixirId`.
- **The "roughly +15% that didn't decompose cleanly" was this project's crit-multiplier
  formula itself being wrong**, not a missing percentage. This project used to compute a
  crit's multiplier as `1.5 + bonus_pct` (e.g. Incineration 3/3's +6% giving 1.56x); the real
  engine's own source (`src/mechanics/damage.rs::crit_damage_multiplier`) computes
  `1.0 + (1.5 - 1.0) * (1.0 + bonus_pct)` instead (1.53x for the same +6%) - a small difference
  for Fire's own modest Incineration bonus, but the same formula bug that produced a much
  larger, cleanly-isolated 2.5x-vs-2.0x error for Arcane Mind/Ice Shards' own +100% bonuses is
  what the "doesn't decompose cleanly" observation above was actually seeing, just at a scale
  too small here to pin down on its own. Fixed once, project-wide, as `Fight::combined_crit_
  multiplier`.

**Updated per-ability trace** (same scenario/commands): total DPS rose from 495.6 to 579.0
(0.70x → 0.82x of the real engine's 704.9).

| Ability | This engine's DPS (new) | Real DPS | Ratio |
|---|---|---|---|
| Fireball | 269.3 | 295.3 | 0.91x |
| Pyroblast | 118.0 | 139.3 | 0.85x |
| Fire Blast | 70.6 | 101.1 | 0.70x |
| Ignite | 69.9 | 88.3 | 0.79x |
| Scorch | 51.2 | 45.7 | 1.12x |
| Arcane Missiles (unmodeled) | 0 | 29.9 | - |
| Touch of the Grave (unmodeled) | 0 | 3.6 | - |

Every ability moved toward parity except Scorch, which was already near-exact (0.95x) and is
now mildly *over* - consistent with Scorch's own smaller Fire Power/Incineration shares meaning
the SP and crit-multiplier fixes overshot it slightly; not chased further, a small and
directionally-expected side effect of two large, well-sourced, uniform fixes rather than a new
named gap on its own.

## Methodology note: which "real" number is the baseline here

Running this comparison's own request JSON through `--go-rules` (the Go-reference ruleset
Warlock's own comparisons preferred, since it tends to match the archived validation corpus's
logged number more closely) **fails with the same Elemental Precision rejection** - this isn't
a quirk of the default "Forever" ruleset, both reject the string identically. After fixing the
talent string to the corrected 3-point value, the *default* Forever ruleset run was used (704.9
DPS) rather than chasing `--go-rules` further, since both tested rulesets agreed on the
rejection and there was no reason to expect one to be meaningfully more "correct" than the other
once the underlying string was fixed. This real local run (704.9) is a different number from
the archived validation corpus's own logged one (589.76) for the same nominal scenario - a
larger drift than Warlock's own comparisons ever showed (those were within 1-3%). Likely
explanation: Mage is newer, more actively-developed territory in the upstream engine than the
already-stable Warlock classes this project started with, so more real balance/mechanic changes
have landed between whenever the validation corpus was generated and this local HEAD checkout.
Not independently confirmed (would need the corpus's own generation commit, which isn't
recorded in the corpus file itself) - named as a real limitation of this comparison rather than
quietly treated as settled.

## A third investigation (2026-10-25, prompted by "still woefully inaccurate")

See `mage-arcane-comparison.md`'s own matching section for the full writeup. One of the two
new fixes found there applies directly to Fire too, confirmed independently from this build's
own debug log: a real, consistent ~4.76-4.78% bonus on every Scorch cast at the
`AfterResistances` stage, previously unexplained, now identified as Arcane Subtlety's own
missing Spell Piercing effect - this build has 2/2 as well (this project's own decode of its
arcane-tree talent-string segment is corrupted there, with several `maxPoints` violations
elsewhere in the same segment never chased down, since Fire's talents are all CLI-flag-driven
rather than programmatically decoded - the real debug log's own evidence settled the value, not
the broken decode). Added as `FireTalents::arcane_subtlety`, default 2, same generic
`Fight::spell_vulnerability_bonus` mechanism Arcane/Frost Mage now use.

**Updated total: 579.0 → 606.5 DPS (0.86x of the real engine's 704.9, up from 0.82x).**

## A fourth investigation (2026-10-25): two real condition bugs, found but not yet closing the gap further

Asked explicitly to address Fire's own remaining per-ability gaps. Checking real cast counts
per ability (from a fresh 3000-iteration `--rotation-stats` run) against this engine's own
found Fire Blast under-cast (9.3/120s here vs. 11.47 real) and Scorch over-cast (11.5 vs.
9.67) - everything else (Fireball, Pyroblast, Ignite) tracked closely. Re-reading the real
priority list's own exact condition tree (not this project's own prose summary of it, which
turned out to have one real error) found two things:

1. **Combustion's own condition was wrong.** The real condition is `NOT auraIsKnown
   (FireVulnerability) OR auraNumStacks == 5` - "known" means the aura is registered at all
   (true all fight long once Improved Scorch is talented), not "currently at 0 stacks". This
   project had read it as `stacks == 0 || stacks == 5`, which let Combustion fire at the very
   start of the fight (0 stacks, before Fire Vulnerability has ever landed) for a build with
   Improved Scorch talented. Confirmed wrong directly: the real debug log's own first
   Combustion cast is at t=9.00, the exact moment Fire Vulnerability first reaches 5 stacks -
   never at t=0. Fixed in `mage/fire.rs`.
2. **Fire Blast had no mana/Clearcasting gate at all.** The real condition on that priority
   item is `(NOT Clearcasting known OR NOT Clearcasting active) AND currentManaPercent >= 30%`
   - this engine cast Fire Blast unconditionally. Added the same gate (fall through to
   Fireball when it fails, matching the real list's own next item).

Both are real, verified-against-source fixes - but **neither moved the aggregate total**
(606.5 → 605.5, within noise) **and the Fire Blast/Scorch cast-count skew did not close** (Fire
Blast actually fell further, to 7.7/120s, after adding the new mana gate). This engine's own
`max_mana` for this build (6477) matches the real engine's own debug-log value (6468) almost
exactly, ruling out another Arcane-Blast-style base-cost bug as the explanation. The real
cause of the cast-count skew is still open - flagged honestly rather than reverting a
verified-correct fix just because it didn't improve the number it was chasing.

## A fifth investigation (2026-10-25): per-cast logging added, the real cause found

Asked explicitly to close this with per-cast logging, since the prior pass had run out of
leads using only aggregate per-ability totals. Added `Fight::debug_log` (a human-readable,
timestamped line per rotation decision/event, opt-in via `enable_debug_log`, zero cost for a
normal multi-iteration run) and wired it through `mage/fire.rs::run_with_debug_log` and a new
`--debug-log` CLI flag - a single-iteration trace this engine never had before, diffable
directly against the real engine's own.

**Two real timing bugs found and fixed immediately**, both from comparing the two logs'
opening few seconds line by line:

1. **Combustion's own condition was still slightly wrong.** The real condition is `NOT
   auraIsKnown(FireVulnerability) OR auraNumStacks == 5` - this project had read "known" as
   "currently at 0 stacks", letting Combustion fire at t=0 (before Fire Vulnerability ever
   landed) for a build with Improved Scorch talented. The real log's own first Combustion cast
   lands at t=9.00, the exact instant Fire Vulnerability first reaches 5 stacks - confirmed
   directly, not inferred.
2. **Combustion is genuinely off-GCD (`GCD = 0s`, confirmed in the real log) but this
   project's own data pack had `gcd_ms: null` for it**, which `casting.rs`'s own
   `pay_and_advance_with_cost_pct` read as "missing, use the implicit 1500ms default" rather
   than "explicitly zero" - and then floored *that* to a 1000ms minimum via a generic
   "GCD never drops below 1s" rule meant for hasted *normal* GCDs, not genuinely off-GCD
   abilities. Fixed in two places: the data pack's own `gcd_ms` corrected `null` → `0`
   (`mage-forever-...-rev6.json`), and `casting.rs`'s own floor logic corrected to only apply
   when `gcd_ms` is missing or a real nonzero value, never when it's explicitly `0`. Also found
   a second-order bug this exposed: `fire.rs`'s own `step()` function returned immediately
   after a successful Combustion cast, and the outer `run()` loop's "nothing advanced, force a
   1000ms safety-advance" fallback then wrongly punished a *correct* zero-time action as if the
   rotation were stuck - fixed by having Combustion's own branch recurse into `step()`
   immediately instead of returning, so the very next decision happens at the same timestamp,
   matching the real log exactly (Fire Blast landing at the identical moment as Combustion, not
   1.5+ seconds later).

Neither of these (now three, counting the original Combustion-condition fix) actually moved
the *aggregate* skew much on their own (Combustion fires only once every ~180s). The real
discovery came from using the new log to directly inspect a long, unexplained silent gap in
this engine's own trace (t=60 to t=69.5, nothing logged at all) - which turned out to be an
**8-second Evocation cast this engine's own log simply hadn't been logging** (a logging gap,
not a game-logic one - fixed by adding the missing log lines). Checking the *real* engine's own
trace for the same spell (12051) found its first and only Evocation cast at **t=124.50** -
essentially after the 120-second fight is already over. This engine's own Fire Mage needed a
full, DPS-dead Evocation at roughly the fight's halfway point; the real one barely ever needs
one at all.

**Root cause of the mana gap**: diffing every `Gained X mana from {...}` line in the real log
against what this engine models found two entire mana-return mechanics missing, both
confirmed by solving the real log's own numbers exactly, not guessed at:

- **Judgement of Wisdom** (`raid.debuffs.judgementOfWisdom: true` - present in *every* real
  fixture checked so far, Warlock's included, not just Mage's): a landed **direct** hit has a
  flat 50% chance to return 59 mana to its caster, confirmed directly against the real engine's
  own `JUDGEMENT_PROC_CHANCE = 0.5` constant and 29 identical `Gained 59.000 mana` lines.
  Modeled generically in `sim.rs` (`CasterStats.standing_judgement_of_wisdom`,
  `Fight::maybe_proc_judgement_of_wisdom`, called from every *player* direct-hit resolution
  path, deliberately not the pet-attack one - a pet's own landed hit doesn't return mana to
  the player) - a cross-class mechanic, not Mage-specific, so this also quietly helps
  Warlock's and every other spec's own mana economy.
- **Master of Elements** (Fire Mage talent, 3/3 in this build despite this project's own
  separately-already-documented-as-corrupted fire-tree decode reading 0 points): a Fire or
  Frost spell crit refunds 10%/point of its own *base* mana cost - confirmed by four real
  `Gained X mana from {SpellID: 29074}` lines (123/45/132/102) solving exactly for 30% of
  Fireball/Scorch/Pyroblast/Fire Blast's own real costs (410/150/440/340). Modeled in
  `mage/fire.rs` as `FireTalents::master_of_elements`, no internal cooldown applied (the real
  source says there is one, but this build's own observed proc rate was consistent with none
  mattering at its crit frequency - not independently confirmed).

**Updated total: 605.5 → 614.7 DPS (0.87x of the real engine's 704.9, up from 0.86x)** -
Evocation's own cast moved from t=60 to t=79.5, later but still real, within this 120s fight.

## What's left, and why it's not a bug

Tallying every real mana-gain source this pass's own log-diffing surfaced: natural regen
(~3838 over the fight), Judgement of Wisdom (~1711, now modeled), Master of Elements (~1704,
now modeled) - and then three **one-time, mid-fight item/consumable uses** this project has
*always* deliberately excluded, in the same bucket as Mana Ruby and every other
consumable-usage case already named in this project's own module docs: Robe of the Archmage's
own "Replenish Mana" on-use trinket effect (593.785, once), a mana potion (1800, once), and a
second mana item (1100, once) - roughly **3493 mana**, more than half this build's own max
mana pool, over the course of one fight. This project's own README already states plainly
that it doesn't simulate consumable *usage* mid-fight, only standing buffs - the real engine's
own near-total avoidance of Evocation is, in real and significant part, downstream of exactly
the kind of mid-fight item use this project scoped out from the start, not a new finding. The
Fire Blast/Scorch cast-count skew is accordingly **substantially explained, not 100% closed**:
real, generalizable mechanics were missing and are now fixed; what measurably remains traces
to an existing, intentional scope boundary, not an undiscovered bug - and is left there rather
than widening this project's own stated scope to chase the last few points of parity.

## Two more generic fixes (2026-10-25, see `mage-arcane-comparison.md`'s own "sixth
investigation" for the full writeup): a systemic cooldown-timing bug affecting every
cooldown-bearing ability project-wide (`Fight::set_cooldown_from` - found via this spec's own
Fire Blast cast-cadence, in fact) and Mana Spring Totem's own missing +25 mp5 (`gear.rs::
BUFF_MP5`). Fire doesn't talent Missile Barrage or Arcane Blast, so neither that investigation's
third fix nor the later "seventh investigation" Arcane-Blast-buff-sharing fix apply here -
confirmed by scanning this spec's own real debug log for any per-cast `SP:` fluctuation and
finding none (a single flat 493.0 for the whole fight, every Fire-school cast). **Updated
total: 614.7 → 619.5 DPS (0.88x of the real engine's 704.9)**.

## Two more real bugs found by a direct per-ability investigation (2026-10-25, prompted by "0.88x is unacceptably low")

Asked to go ability-by-ability again rather than accept 0.88x. Pulled the real engine's own
`raidMetrics...actions` breakdown (not just the aggregate) for this exact fixture and resolved
every spell ID directly against `data/spells.jsonl` (25306 Fireball, 10199 Fire Blast, 10207
Scorch, 18809 Pyroblast, 412538 Ignite's own `RefIDs` target, **25346 Arcane Missiles' own
tick**). That last one was the first real finding: **Arcane Missiles has a genuine, non-trivial
real-engine contribution for Fire Mage (29.9 DPS, ~4% of the total)** that this project's own
first-pass module doc had dismissed with a flatly wrong claim - "aura ID 44404 doesn't resolve
to any real spell this project could find." It does: it's Missile Barrage's own buff, already
resolved and modeled in `mage/arcane.rs` by the time Arcane Mage was added, just never checked
against Fire's own claim. Pulling this fixture's own real priority list directly (not a prose
summary of it) confirms item #7, between Fire Blast and the Fireball fallback: cast Arcane
Missiles whenever Missile Barrage (44404) is known and active - and Missile Barrage's own real
trigger list (`src/classes/mage/prepare/talents/arcane.rs::register_missile_barrage`) includes
Fireball landing at half the base rate, which this build clearly talents (its own real Arcane
Missiles cast cadence, ~1.9/120s, matches `Fireball-landed-rate(~21/120s) * 10%` - rank 1/2,
not the un-decodable fire-tree segment's own corrupted reading). **Fixed**: added `FireTalents::
missile_barrage`, the same `Fight::register_tick_proc` self-sustain chain Arcane/Frost already
use, a `maybe_proc_missile_barrage` roll on landed Fireball casts, and the priority-list slot
itself (gated on the buff, `cast_dot_only` into `arcane_missiles_barrage`, mirroring `mage/
arcane.rs::step` exactly). **619.5 → 646.6 DPS (0.92x)**.

**Second finding, from diffing every real `Gained X mana` log line against this engine's own**
(same technique that found Judgement of Wisdom/Master of Elements): three "Major cooldown
used" one-time mana items fire exactly once each in the real log - `ItemID: 8008` (+1100,
real priority item #0's own explicit condition: `currentManaPercent <= 80%`), `ItemID: 13444`
(+1800, a mana potion, fired via the engine's own automatic potion-use logic rather than an
explicit priority-list condition), and `ItemID: 14152` (a worn trinket, ~550-600 mana,
inconsistent across every build checked - see below). The first two are **real, exactly-
sourced, and reliably present in every Mage "builder starter" fixture checked so far** (Fire,
Arcane, *and* Frost all show the identical `ItemID: 8008`/`+1100` and `ItemID: 13444`/`+1800`
lines) - this project's own "no mid-fight consumable-use modeling" scope note was written for
*player-discretionary* item use in general, but these two are neither discretionary (one has an
explicit, sourced trigger condition; the other fires automatically and consistently) nor
unsourced (the amounts are exact and identical across three independent builds). Modeled
generically in `sim.rs` (`Fight::maybe_use_one_time_mana_item`, a flat mana grant gated on a
mana-percent ceiling and usable once per fight), called from all three Mage specs' own `step`,
not duplicated per-spec logic. **The third item (14152) was deliberately not added**: its
observed amount differs across every build checked (593.8 here, 547.2 for Arcane, 599.2 for
Frost) with no clean flat or percentage pattern across them, consistent with a randomized
"restore X to Y mana" effect this project doesn't have enough independent samples to source a
real range for - left open rather than averaged into a guess, per this project's own standing
rule against filling an unknown value with one.

**Updated total: 646.6 → 669.8 DPS (0.95x of the real engine's 704.9)** - the closest Fire Mage
comparison yet. Per-ability ratios after both fixes: Fireball 1.01x (essentially exact), Scorch
0.97x, Ignite 0.91x, Pyroblast 0.89x, Arcane Missiles 1.64x (over - likely this engine's own
Missile Barrage rank-1 approximation running slightly hot, or Fireball's own landed-cast rate
differing enough from the real one to shift the expected trigger count; not chased further
given its small absolute DPS share), **Fire Blast 0.74x - still the one real laggard**. Traced
via a fresh debug-log trace to the same root cause the "what's left" section below already
named: even with both new mana items, this engine's own mana still crashes into the sub-30%
Fire-Blast-skip zone and a full Evocation cast late in at least some seeds, while the real
engine's own trace never needs Evocation inside the 120-second window at all. The ~550-600
mana from the still-unmodeled trinket (14152) would very plausibly close most of this
specific remainder, but per the above, its amount isn't confidently sourced - flagged honestly
as the next concrete lead rather than guessed at.
