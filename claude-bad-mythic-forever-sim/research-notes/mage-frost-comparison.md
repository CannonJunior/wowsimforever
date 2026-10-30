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

## A rigorous, formulaic root-cause pass (2026-10-25, second investigation)

Asked explicitly to find *why* these gaps exist, ability by ability, and close them with fixes
that generalize across every class rather than per-spec fudges - full methodology and the three
fixes found (a project-wide crit-multiplier formula bug, two consumables wrongly modeled as
inert that account for a flat 71-point spell-power gap, and an Arcane-Missiles-under-Missile-
Barrage tick-count bug) are written up in `mage-arcane-comparison.md`'s own matching section,
since two of the three were found using this spec's own debug log and apply identically here;
not re-duplicated. The one Frost-specific numeric input that pass's SP investigation used: a
real debug line gave Frostbolt's own effective spell power as `SP: 498.0`; this project's own
gear/buff math for the identical build landed at 427 pre-fix (71 short) and exactly 498
post-fix (`spell_power` 454 + `bonus_frost_power` 44, confirmed arithmetic match, not rounding).

## The per-ability trace, after these fixes

Same scenario and commands as before.

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Frostbolt | 369.5 | 346.1 | 0.94x |
| Arcane Missiles | 59.5 | 56.2 | 0.94x |
| Ice Lance | 42.1 | 29.2 | 0.69x |
| Touch of the Grave (racial, unmodeled) | 4.1 | 0 | - |
| **Total** | **475.6** | **431.5** | **0.91x** |

Total DPS rose from 413.4 to 431.5 (0.87x → 0.91x), and - more importantly than the aggregate
move - Arcane Missiles flipped from the one ability that *over*-performed in either Mage
comparison (1.44x) to tracking Frostbolt's own 0.94x almost exactly, confirming the tick-count
fix (`mage-arcane-comparison.md`'s finding 3) was the right call: Missile Barrage's own proc
rate was never the problem, the per-channel damage was.

## A third investigation (2026-10-25, prompted by "still woefully inaccurate")

Asked to go further and find a still-more-general fix - see `mage-arcane-comparison.md`'s own
matching section for the full writeup (web research into other WoW Forever/historical WoW
simulators, and two more real, generalizable bugs found by reading the real engine's own
four-stage debug pipeline end-to-end). Both fixes apply to Frost too, confirmed independently
from this spec's own debug log: Arcane Subtlety's own missing Spell Piercing mechanic (2/2 in
this build as well, the same ~4.76% unexplained bonus found on Frostbolt here first) and the
Intellect→max-mana sequencing bug (this spec has no Arcane Mind, so it wasn't directly affected,
but the fix landed in shared code regardless).

**Updated total: 431.5 → 452.0 DPS (0.95x of the real engine's 475.6, up from 0.91x)** - the
closest Mage comparison by a wide margin now. With Spell Piercing modeled, Frostbolt's own ratio
rose further (already close before); Ice Lance remains the one named open gap below, unaffected
by either of these two fixes (it has no SP-stage anomaly on its own, since `sp_coefficient` is
already 0.0 for this spell - the gap really is about *cast frequency*, i.e. Fingers of Frost's
own proc rate, consistent with the original theory).

## Remaining, open gap: Ice Lance (now 0.73x; Frostbolt and Arcane Missiles both ~0.98-0.99x)

The one gap these fixes didn't touch, since none of the three were about Ice Lance or Fingers
of Frost specifically. Frostbolt and Ice Lance share the same Shatter-crit/frozen-multiplier
mechanics in this engine, gated on the same Fingers of Frost charge, yet Ice Lance still
under-performs noticeably more (0.73x vs ~0.98x). Leading suspect, unchanged from the prior
pass: this engine's flat 15% Fingers of Frost proc chance (read directly from
`fingers_of_frost.rs`'s own `proc_chance` field, not rank-scaled) may not be the whole picture -
the real source also separately rolls a *second*, independent trigger for "chill" spells (Cone
of Cold/Improved Blizzard) landing, `should_proc`, which this rotation never exercises (no Cone
of Cold cast) and so was left unmodeled as irrelevant. If the real engine's own Frostbolt-launch
roll uses a different effective chance than the flat 15% this pass assumed, Ice Lance would
fire less often here than in the real one - consistent with the gap, not confirmed. Flagged
honestly as still open, not guessed at further; a future pass with time to pull a real debug
log specifically around Fingers of Frost's own charge-grant events (not just damage lines)
would be the way to actually close it.

## One more generic fix, found while closing Fire Mage's own cast-count skew

`mage-fire-comparison.md`'s own "fifth investigation" (new per-cast debug logging, added
specifically for Fire) found Judgement of Wisdom as a real, entirely unmodeled mana-return
mechanic (a landed direct hit has a flat 50% chance to return 59 mana to its caster) - fixed
generically in `sim.rs`, applying here too with no Frost-specific code. **Updated total: 452.0
→ 466.3 DPS (0.98x)** - the closest any Mage comparison has landed yet, Ice Lance's own gap
aside.

## Three more generic fixes (2026-10-25, see `mage-arcane-comparison.md`'s own "sixth
investigation" for the full writeup - all three were found via Fire/Arcane's own debug logs,
not this spec's, but apply here unchanged): a systemic cooldown-timing bug affecting every
cooldown-bearing ability project-wide (`Fight::set_cooldown_from`), Mana Spring Totem's own
missing +25 mp5 (`gear.rs::BUFF_MP5`), and Missile Barrage's own self-sustaining tick-proc chain
(`talents/missile_barrage.rs`'s per-tick re-trigger roll, modeled here the same way as Arcane's
own fix via `Fight::register_tick_proc`, registered in this file's own `apply_talents`). Frost
also uses Missile Barrage, so the third fix applies directly, not just by inheritance.
**Updated total: 466.3 → 471.8 DPS (0.99x of the real engine's 475.6)** - essentially an exact
match. Arcane's own seventh-investigation fix (Arcane Blast's stacking buff turning out to be a
shared SpellDamage increase, not a bonus local to its own cast) does **not** apply here - Frost
has no Arcane Blast talent at all, confirmed by scanning this spec's own real debug log for any
per-cast `SP:` fluctuation and finding none (a single flat value for the whole fight).

## Two real, sourced one-time mana items, added generically (2026-10-25)

Found while re-investigating Fire Mage's own remaining shortfall (full writeup in
`mage-fire-comparison.md`'s own matching section) and confirmed present in this spec's own
real log too, with the identical amounts: `ItemID: 8008` (+1100, gated on `currentManaPercent
<= 80%` - this build's own real priority list carries this item explicitly, item #0, same
condition) and `ItemID: 13444` (+1800, a mana potion, fired unconditionally). Modeled
generically in `sim.rs` (`Fight::maybe_use_one_time_mana_item`), called once at the top of this
spec's own `step` too. A third item (`ItemID: 14152`, a worn trinket) was *not* added - its own
amount here (599.2) differs from both Fire's (593.8) and Arcane's (547.2), consistent with an
unsourced randomized range, not a flat or percentage value - left open rather than guessed at.

**Updated total: 471.8 → 471.9 DPS (0.99x, unchanged)** - Frost's own mana economy was already
comfortable enough (unlike Fire/Arcane) that this extra income doesn't move its number
measurably; included for consistency and because the underlying mechanic is real and
cross-spec, not because this spec needed it.
