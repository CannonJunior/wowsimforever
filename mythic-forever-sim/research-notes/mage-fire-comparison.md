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
