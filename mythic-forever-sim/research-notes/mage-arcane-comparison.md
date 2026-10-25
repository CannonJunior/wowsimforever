# Comparing Arcane Mage against the real engine's own trace (2026-10-25)

Same methodology as `mage-fire-comparison.md`: build `mythicsim-forever-engine`'s own Rust CLI
locally and run it with `--rotation-stats` against the real
`arcane-mage--undead.request.json` fixture, extract the per-ability `damage` field from
`raidMetrics.parties[].players[].actions`, cross-check the sum against `raidMetrics.dps.avg`,
then run this engine's own CLI with matching flags and build a side-by-side table.

## A talent-string decode bug, caught the same way Fire Mage's own was

Decoding this fixture's talent string against `ui/sim/talents/trees/mage.json` (sort each
tree's talents by `(rowIdx, colIdx)`, zip against the string's digits) gave `elementalPrecision:
5` - immediately suspicious, since the Fire Mage comparison had already established the real
cap is 3. Running the request through the real engine confirmed it:

```
mage talents "0551205003100301531-230500001-005": Elemental Precision has 3 ranks, the string gives 5
```

Re-checking every tree's decode for both this fixture and the Frost Mage one (below) against
each talent's own `maxPoints` found exactly one more violation (Frost's own Shatter-adjacent
slot, see `mage-frost-comparison.md`) and one structural problem: Frost Mage's own talent
string segment was 19 characters for an 18-talent tree - a genuine excess character, not a
decode-order bug (the first 17 characters all landed on a talent whose own `maxPoints` wasn't
exceeded; only position 2, Elemental Precision, and position 17, Ice Barrier, read a stale
pre-nerf value). Both fixtures' violations are capped to the talent's real current max (5→3 for
Elemental Precision here; Frost's own two violations documented in that file) rather than
reinterpreted some other way - the simplest, most defensible fix once the real validator had
confirmed exactly which single values were wrong, consistent with how Fire Mage's own
`elemental_precision: 4→3` fix was made. This is very likely the same root cause across all
three Mage fixtures: the validation corpus was generated against an older build where a handful
of talents (so far: Elemental Precision, Ice Barrier) had different ranks than the current
engine's own talent-tree data, not a bug in the decode method itself (confirmed: every other
talent slot in both fixtures' Arcane and Fire segments decoded clean, with no violations at
all).

## The real rotation and its mechanics, read from source

`arcane-mage--undead.request.json`'s own priority list: Evocation below 15% mana (20+ sec
left); Arcane Missiles whenever Missile Barrage (aura 44404) is active; Frostbolt once the
Arcane Blast buff (400573) has 3+ stacks or mana drops below 40%; Arcane Blast otherwise. Every
mechanic below is read from `src/classes/mage/spells/arcane_blast.rs` and
`talents/missile_barrage.rs`, not guessed at:

- **Arcane Blast** (1/1): each landed cast adds a stack (max 4, 15 sec from the *first* stack -
  later stacks don't refresh that timer) giving 15% of the caster's Intellect as spell damage
  and raising Arcane Blast's own mana cost by 175% per stack. Modeled with two new generic
  engine primitives: `Fight::add_aura_stack_no_refresh` (the non-refreshing timer) and
  `Fight::resolve_direct_with_bonus_power`/`casting::cast_direct_with_bonus_power` (a flat
  spell-power bonus on top of `effective_spell_power`, since 15%-of-Intellect isn't a
  percentage this engine's existing `extra_pct` mechanism fits). This is also why `CasterStats`
  gained a new `intellect` field - no prior mechanic in this project needed raw Intellect
  itself rather than something already derived from it.
- **Missile Barrage** (1/2 in this build): a landed Arcane Blast has a 20%/40%-at-1/2-points
  chance (Frostbolt/Fireball/Frost Blast/Arcane Wrath: half that; an Arcane Missiles tick: a
  quarter) to grant a buff making the next Arcane Missiles free and tick twice as fast -
  modeled as a second data-pack entry (`arcane_missiles_barrage`, 500ms/10-tick vs the normal
  1000ms/5-tick `arcane_missiles`) rather than a live field override.
- **Arcane Impact** (3/3), **Arcane Mind** (5/5) and **Arcane Instability** (3/3): static
  crit/damage/Intellect modifiers, curves read from `spell-extras.json` (11242: `[2,4,6]`;
  11232: `[2,4,6,8,10]` Intellect% and `[20,40,60,80,100]` crit-damage% fraction; 15058: three
  `[1,2,3]` curves for damage%/crit% both read as `rank/100`).

Out of scope, same bucket Fire Mage's own module already excludes: Presence of Mind and Arcane
Power (both `autocastOtherCooldowns`-driven in the real priority list) and the Undead racial
Touch of the Grave (still unmodeled project-wide).

## The per-ability trace

Scenario: level 60 Undead vs. a level 63 target, 120s, 3000 iterations, seed 42, this engine's
own `--buffs builder-starter` (the exact raid-buff + consumable package the fixture itself
carries). Real engine command: `forever-engine sim --request arcane-mage--undead.request.json
--rotation-stats`. This engine: `mythic-forever-sim --class mage --spec arcane --gear
arcane_mage_starter_undead --buffs builder-starter --duration 120 --iterations 3000`.

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Frostbolt | 255.4 | 98.6 | 0.39x |
| Arcane Missiles | 126.5 | 99.2 | 0.78x |
| Arcane Blast | 111.2 | 46.7 | 0.42x |
| Touch of the Grave (racial, unmodeled) | 4.0 | 0 | - |
| **Total** | **497.8** | **244.9** | **0.49x** |

(Real per-ability rows sum to 497.0, within rounding of the engine's own reported `dps.avg`
497.83 - the standard double-check this project's own `CLAUDE.md` requires before trusting any
row.)

## Analysis: a found, not fully closed, gap - likely a mana-pacing difference, not a damage-formula bug

Frostbolt's own damage formula is shared byte-for-byte with Frost Mage's implementation, where
the same spell lands at 0.81x (see `mage-frost-comparison.md`) - a real but much smaller gap.
That strongly suggests Frostbolt's *per-hit* damage here is close to correct, and Arcane Mage's
much worse 0.39x ratio on the same spell is a *cast-frequency* problem, not a per-hit one.

Checking cast cadence confirms it: the real engine casts Arcane Blast roughly every 6.4s and
Frostbolt every 4.7s (computed from the real trace's own `casts` fields over the full 3000 x
120s simulated time). This engine casts Arcane Blast every ~11.5s and Frostbolt every ~8s -
roughly half the real cadence for both, while Arcane Missiles' own cast rate (~3.1 per 120s,
matching the real trace's implied Missile Barrage proc rate closely) lines up fine. Totaling up
this engine's own cast durations (10.4 Arcane Blast x 1.5s + 15.0 Frostbolt x 3.0s + 3.1 Arcane
Missiles x ~2.5s average) accounts for only about 68s of the 120s fight - roughly 43% of the
fight is spent not casting anything at all.

The likely cause: Arcane Blast's own cost ramps *faster* than the rotation's 40%-mana escape
hatch can reliably catch. With this build's ~6560 max mana, the first Arcane Blast cast (0
stacks) costs 984 mana (15%); the second (1 stack, +175%) costs 2706 (41%) - after just two
casts, 3690 mana is gone (56%), leaving 43.75% remaining: *just above* the 40% threshold that
would otherwise divert to the cheaper Frostbolt. The rotation then attempts a third Arcane
Blast at 2-stacks' cost (4428, 67.5% of the pool) with only 2870 left, fails outright
(insufficient mana), and - since Arcane Blast is the priority list's own last, unconditional
fallback with nothing after it - this engine simply advances the clock and retries, repeatedly,
until mp5 trickle-regens enough mana to either afford that cast or (more likely) push the
*percentage* check above 40% the opposite way. This produces exactly the kind of long idle
stretch the cast-cadence numbers above show.

This may be a real mechanic (a priority list with no further fallback *should* produce idle
GCDs when its own last resort can't be afforded, and the real engine might do the same under
the right mana curve) or it may mean the real engine's own mana-cost ramp, threshold timing, or
Intellect-as-spell-damage feedback loop (which raises *future* Arcane Blast casts' own damage,
not modeled as affecting pacing here) resolves this knife-edge differently. Not chased further
in this pass - flagged here, honestly, as a found-but-not-fully-understood gap, the same
treatment Fire Mage's own 71-point effective-spell-power gap got rather than a guessed fix.
Arcane Missiles' own 0.78x ratio (the one ability whose cast rate *does* track the real
engine's) is consistent with this theory: once it does fire, it's reasonably close, which is
what you'd expect if the dominant problem is pacing, not per-hit damage.
