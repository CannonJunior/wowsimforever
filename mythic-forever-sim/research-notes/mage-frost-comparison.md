# Comparing Frost Mage against the real engine's own trace (2026-10-25)

Same methodology as `mage-fire-comparison.md` and `mage-arcane-comparison.md`: build
`mythicsim-forever-engine`'s own Rust CLI locally, run it with `--rotation-stats` against the
real `frost-mage--undead.request.json` fixture, extract per-ability `damage`, cross-check
against `raidMetrics.dps.avg`, then run this engine's own CLI with matching flags.

## The same talent-string decode issue, a second time

This fixture's own Frost segment decoded to 19 characters against an 18-talent tree - a length
mismatch, not just a bad digit. Checking every position against each talent's real `maxPoints`
found the string was otherwise clean except at exactly two positions: Elemental Precision
(digit 5, real max 3 - the same talent Fire's and Arcane's own comparisons already found) and
Ice Barrier (digit 5, real max 1). Dropping the segment's one excess trailing character and
capping those two positions to their real max produced a string the real engine's own validator
accepted outright. Treated the same way as Fire's and Arcane's own `elemental_precision`
fix - the simplest defensible read once the validator had identified exactly which values were
stale, not a reinterpretation of the decode method (every other position in this fixture's
Arcane, Fire and Frost segments decoded clean with no `maxPoints` violation at all, so the
decode-by-`(rowIdx, colIdx)` method itself is not the problem).

## The real rotation and its mechanics, read from source

Priority list: a mana-potion item below 80% mana (out of scope, same bucket as Fire Mage's own
Mana Ruby cut); Evocation below 20% mana (25+ sec left); Ice Lance whenever Fingers of Frost is
up; Arcane Missiles whenever Missile Barrage is up (same mechanism as Arcane Mage's own, this
build also has 1/2 Missile Barrage despite being Frost-specced); Frostbolt otherwise. Read from
`talents/fingers_of_frost.rs` and `talents/winters_chill.rs`:

- **Fingers of Frost** (1/2 in this build): Frostbolt's own *launch* has a flat 15% chance
  (not rank-scaled - the rank only changes the charge count, 1 vs 2, per spell 400647's own
  curve `[[1,2]]`) to grant charges treating the next consuming cast as hitting a "frozen"
  target. Modeled with `Fight`'s existing charge-counter primitives
  (`set_charges`/`consume_charge`/`charges_remaining`) rather than the stacking-aura ones,
  since a charge system (decrement-on-use) is the better fit than a stack count with no
  decrement operation.
- **Shatter** (1/3): +17/33/50% bonus crit (spell 11170's curve `[[17,33,50]]`) on every spell
  while frozen-charges are up - folded into Fingers of Frost in the real source too ("the two
  only ever fire together").
- **Ice Lance's own 200%-vs-frozen bonus** is not talent-gated at all (the base spell's own
  tooltip) - a flat x3.0 multiplier whenever charges are up, independent of Shatter.
- **Ice Shards** (5/5): +20%/point crit-damage bonus on Frost spells (spell 11207's curve
  `[[20,40,60,80,100]]`).
- **Improved Frostbolt** (5/5): -100ms/point cast time (spell 11070's curve
  `[[-100,-200,-300,-400,-500]]` - same shape as Fire Mage's Improved Fireball).
- **Winter's Chill** (2/5 in this build, real points, genuinely inert for this rotation): its
  only damage effect buffs Cone of Cold, which this priority list never casts - confirmed by
  re-reading `talents/winters_chill.rs`'s own `attach`, restricted to `masks::CONE_OF_COLD`.
  Not unmodeled by oversight; deliberately skipped because it would never fire.

## The per-ability trace

Scenario: level 60 Undead vs. a level 63 target, 120s, 3000 iterations, seed 42, `--buffs
builder-starter`. This engine: `mythic-forever-sim --class mage --spec frost --gear
frost_mage_starter_undead --buffs builder-starter --duration 120 --iterations 3000`.

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Frostbolt | 369.5 | 299.3 | 0.81x |
| Ice Lance | 42.1 | 27.9 | 0.66x |
| Arcane Missiles | 59.5 | 85.8 | 1.44x |
| Touch of the Grave (racial, unmodeled) | 4.1 | 0 | - |
| **Total** | **475.6** | **413.4** | **0.87x** |

(Real per-ability rows sum to 475.1, within rounding of the engine's own reported `dps.avg`
475.60.)

## Analysis: the closest Mage comparison so far, with one real ability-level gap worth naming

At 0.87x, this is a smaller gap than either Fire's (0.70x before its own fixes landed at 1.42x
*over*-performing, see that file) or Arcane's (0.49x) comparisons found, and Frostbolt's own
0.81x ratio - the same spell, same formula, same data-pack row Arcane Mage's own 0.39x ratio
used - confirms this spec doesn't share Arcane's severe mana-pacing stall (see
`mage-arcane-comparison.md`): Frost's rotation has no equivalent to Arcane Blast's
exponentially-ramping cost, so there's no equivalent mana knife-edge to fall into.

Two real, smaller gaps remain, not yet closed:

1. **Ice Lance under-performs (0.66x) more than Frostbolt does (0.81x).** Both share the same
   Shatter-crit/frozen-multiplier mechanics in this engine, gated on the same Fingers of Frost
   charge. A plausible cause: this engine's flat 15% Fingers of Frost proc chance (read
   directly from `fingers_of_frost.rs`'s own `proc_chance` field, not rank-scaled) may not be
   the full picture - the real source also separately rolls a *second*, independent trigger for
   "chill" spells (Cone of Cold/Improved Blizzard) landing, `should_proc`, which this rotation
   never exercises (no Cone of Cold cast) and so was left unmodeled as irrelevant. If the real
   engine's own Frostbolt-launch roll uses a different effective chance than the flat 15% this
   pass assumed (e.g. itself rank-scaled, contrary to this pass's reading of the source), Ice
   Lance would fire less often in this engine than in the real one, which a 0.66x-vs-0.81x gap
   is consistent with. Not independently re-verified against a debug log in this pass - flagged
   as the most likely single explanation, not confirmed.
2. **Arcane Missiles over-performs (1.44x) rather than under-performing.** This is the
   opposite direction from every other ability in both Mage comparisons so far, which all
   under-perform. Missile Barrage's own proc rate (driven by Frostbolt's landing, `bolt_chance`
   = half of `arcane_blast_chance` at this build's 1/2 points = 10%) produces a similar *cast
   count* to the real trace's own implied rate, so the gap looks like a per-channel damage
   difference rather than a frequency one - possibly this engine's `arcane_missiles_barrage`
   data-pack entry's free-cost/double-tick-rate modeling (sourced from `missile_barrage.rs`'s
   own plain-English module comment, not a numeric client row - see that entry's own `source`
   note) overstates the buffed cast's value relative to the real engine's own more precise
   `cost_percent_add`/`tick_length_delta_ns` curve lookup, which this pass didn't fully resolve
   (see the data pack's own top-level `provenance_note`).

Both are named rather than guessed at further, per this project's own stated bar for an "open"
gap - a future pass with more time to pull a real debug-log trace for this exact fixture (the
same technique that resolved Fire Mage's effective-spell-power gap down to a specific
unconfirmed suspect) would be the way to actually close either one.
