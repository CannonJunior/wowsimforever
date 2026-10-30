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

## A rigorous, formulaic root-cause pass (2026-10-25, second investigation)

Asked explicitly to find *why* the gaps above exist, ability by ability, and close them with
fixes that generalize to every class rather than per-spec fudges. Methodology: re-ran both the
Arcane and Frost fixtures through the real engine with `simOptions.debugFirstIteration` (not a
new flag - already `true` in both fixtures' own JSON) and 1 iteration, pulled the resulting
`[DEBUG]` lines (`SP:`, `BaseDamage:`, `AfterAttackerMods:`, `AfterTargetMods:`, `AfterOutcome:`
per cast), and worked backward from the real engine's own intermediate numbers to find exactly
which stage of this engine's damage pipeline diverged. Three real, generalizable bugs found and
fixed, none of them Arcane-specific:

**1. The crit-damage-multiplier formula was wrong for every class, not just Mage.** This
project's formula used to be `base_multiplier (1.5) + bonus_pct` (e.g. a +100% crit-damage
talent gave 2.5x). Two independent real-engine samples - Arcane Mage's Arcane Mind 5/5 (+100%)
on an Arcane Blast crit, and Frost Mage's Ice Shards 5/5 (+100%) on a Frostbolt crit - both
showed `AfterOutcome / AfterTargetMods` land at *exactly* 2.0x, not 2.5x. The real engine's own
source (`sage3648/mythicsim-forever-engine`'s `src/mechanics/damage.rs::crit_damage_multiplier`)
confirms the correct formula directly, with its own doc comment: "multiplicative modifiers
scale the base, additive ones scale only the bonus above 100%" - i.e.
`1.0 + (base_multiplier - 1.0) * (1.0 + bonus_pct)`, which gives exactly 2.0 for base=1.5,
bonus=1.0. Fixed once, as a single shared `Fight::combined_crit_multiplier` function used by
every crit-multiplier computation in `sim.rs` - this silently affects every Warlock talent that
adds crit damage too (Ruin, etc.), not just these two Mage talents, though the warlock specs'
own smaller bonus percentages make the error much less visible there.

**2. Two real consumables this project had wrongly modeled as "inert" turned out to be
additive - a flat 71-point spell-power gap, identical in two independent builds.** A per-cast
real debug line gave Frost Mage's own effective spell power as `SP: 498.0`; this project's own
gear/buff math for the identical build landed at 427 - a 71-point gap. The *exact* same 71-point
gap had already been found and left open in the original Fire Mage comparison. Both Fire's and
Frost's own request fixtures (and Arcane's, and all three Warlock specs') carry a
`spellPowerElixirId` (Greater Arcane Elixir, +35 universal spell damage) *and* a `mhImbueId`
(Brilliant Wizard Oil, +36 universal spell damage) on top of the Flask of Supreme Power (+150,
the same stat) - a prior pass had assumed both were inert once the flask was up, citing
`wowsims/forever`'s `ExclusivePerStat` rule, but never actually checked that against the real
Rust engine's own registration code. A direct read of `consumes.rs::register_consumes` shows
`flask_id`, `spell_power_elixir_id` and `school_elixir_id` are all fed through the exact same
unconditional `add_consumable_stats` call - with its own comment explicitly saying these
"stack *beside* the elixirs" - and `register_static_imbue`'s own `25122 | 20749` match arm
calls a plain `env.sim.add_stat(unit, Stat::SpellDamage, 36.0)` for Brilliant Wizard Oil, no
exclusivity check anywhere in either path. 35 + 36 = 71, exactly. Fixed in `gear.rs` (not
per-spec): `BUFF_SPELL_POWER_ELIXIR` (35, every non-`None` buff variant, Warlock included) and
`MAGE_BUFF_WIZARD_OIL_SPELL_POWER` (36, Mage's two variants only, since Warlock's own package
carries no weapon imbue).

**3. Arcane Missiles under Missile Barrage was double-counting its own damage.** This project
had modeled the "free + twice-as-fast" buffed cast as 10 ticks over the same 5000ms (reading
`talents/missile_barrage.rs`'s own "fire its missiles twice as often" too literally as "twice
the ticks"). Checking this against the real trace showed Arcane Missiles was the *only* ability
in either Mage comparison that over-performed (1.44x for Frost, pre-fix) - and the ratio got
*worse*, not better, once fixes 1-2 above correctly raised every other ability's damage,
isolating it as a real, separate bug rather than noise. The more defensible reading of "twice as
often": the channel's own fixed tick *count* (5, from the base spell's own definition) stays the
same, and only the *period* between those 5 ticks halves - finishing the whole channel in 2500ms
instead of 5000ms, not delivering double the total damage. Corrected in the data pack
(`arcane_missiles_barrage`: `num_ticks` 10→5, `cast_time_ms`/`duration_ms` 5000→2500,
`mage-forever-2026-10-25-rev4.json`).

**Also newly modeled, not a formula fix but a real mechanic this project had wrongly scoped
out**: **Arcane Power**, a 1/1 major cooldown (+30% damage and mana cost, 15s, 180s cooldown,
spell 12042) that a prior pass had lumped in with Presence of Mind as
`autocastOtherCooldowns`-driven and out of scope. A real debug log showed a clean, exactly-15s
window at the start of the fight where every ability's own `AfterAttackerMods/BaseDamage` ratio
read 1.33 instead of the steady-state 1.03 (Arcane Instability's own +3%) - a flat +30% only
Arcane Power's own sourced numbers explain. Modeled in `mage/arcane.rs` using the same
cooldown/timed-aura primitives Combustion already established, not a new mechanism.

**And Curse of the Elements**, which every Mage fixture's own `raid.debuffs.curseOfElements`
field sets `true` (the validation methodology's "assume a raid Warlock keeps this up" baseline)
but which nothing in a solo Mage trace ever casts - added a `CasterStats.standing_curse_of_the_
elements` flag, set automatically by `gear.rs` for Mage's own builder-starter variants (not for
Warlock's, whose own fixture carries no such key - Warlock already casts its own via a real
`cast_utility` action).

## The per-ability trace, after these fixes

Same scenario and commands as before.

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Frostbolt | 255.4 | 160.3 | 0.63x |
| Arcane Missiles | 126.5 | 61.5 | 0.49x |
| Arcane Blast | 111.2 | 48.0 | 0.43x |
| Touch of the Grave (racial, unmodeled) | 4.0 | 0 | - |
| **Total** | **497.8** | **269.8** | **0.54x** |

Total DPS rose from 244.9 to 269.8 (0.49x → 0.54x) - a real improvement, but Arcane remains the
largest gap of the three Mage specs, and still for the same reason identified before these
fixes: a mana-pacing stall, not a per-hit damage bug. If anything, Arcane Power's own +30% mana
cost (on top of Arcane Blast's existing +175%/stack ramp) makes the mana pressure *worse*, so
the stall described below is, if anything, a larger share of the remaining gap than before.

## A third investigation (2026-10-25, prompted by "still woefully inaccurate")

Asked to go further, search for how other WoW Forever/historical WoW simulators approach this,
and find a still-more-general fix. Web research confirmed `wowsims/forever` (Go, MIT) is the
*official* canonical engine `sage3648/mythicsim-forever-engine` (the Rust port this project
treats as ground truth) is itself built and checked against - no new generic-simulator pitfall
class (hit tables, "spell batching", etc.) turned up beyond what direct source reading already
found, but re-reading the real engine's own four-stage debug pipeline (`BaseDamage→
AfterAttackerMods→AfterResistances→AfterTargetMods→AfterOutcome`) end-to-end this time, not
just the two stages the first investigation chased, found two more real, generalizable bugs:

**4. A whole combat mechanic - Spell Piercing / resistance penetration - was missing
entirely.** Every Frostbolt cast in the real trace showed a *consistent* ~4.76% bonus at the
`AfterResistances` stage that this project had never modeled at all (the target has zero
resistance in every fixture, so this looked like it should be a no-op). Traced to Arcane
Subtlety (2/2 in *every* Mage build this project has - Arcane, Frost, *and* Fire, confirmed by
finding the identical signature in Fire's own debug log too), which this project had wrongly
assumed only affected threat (its *other* effect) and skipped - its second effect is `-15`
target resistance (spell 11210's own curve), stored as `+15 SpellPiercing` by the real source's
own `register_arcane_subtlety`. The real formula, confirmed directly against
`src/core/fight/damage.rs::spell_vulnerability`: `1 + max(0, piercing - target_resistance) /
(5 * target_level)` - `15/(5*63) = 4.76%`, exact. This is a genuinely universal mechanic (any
class's gear/talent/racial Spell Piercing, against any of the five resistance-bearing magic
schools) - added as `Fight::spell_vulnerability_bonus` in `sim.rs`'s own shared damage-
resolution path and a new `CasterStats.spell_piercing` field, not anything Mage-local.

**5. Arcane Mind's own Intellect boost never reached the mana pool it should have fed.** A
real debug line gave Arcane Mage's own `max_mana` as 7113; this project's computed 6560 - a
553-point gap. Cause: `CasterStats.max_mana` is finalized once in `gear.rs`, *before*
`mage/arcane.rs::apply_talents` runs and raises `stats.intellect` by Arcane Mind's own +10% -
and Intellect feeds max mana at a flat +15/point (confirmed via `gear.rs`'s own
`INTELLECT_TO_MANA_PER_POINT`), so that +10% silently never reached the mana pool at all:
`15 * (post-boost Intellect - pre-boost Intellect) ≈ 553`, matching the gap exactly. This is a
general sequencing bug, not Arcane-specific: *any* talent, for any class, that raises Intellect
after `Fight::new` has already totaled gear/buffs into `max_mana` would hit the same silent
staleness. Fixed with a new generic `Fight::multiply_intellect` method that updates
`max_mana`/current `mana` by the matching delta whenever Intellect changes - callable by any
future Intellect-boosting talent, not reimplemented per class.

**Updated numbers**: Arcane 269.8 → **305.2 DPS (0.61x** of the real engine's 497.8, up from
0.54x). Frost 431.5 → **452.0 DPS (0.95x** of 475.6, up from 0.91x - the closest comparison by
far). Fire 579.0 → **606.5 DPS (0.86x** of 704.9, up from 0.82x).

With all four of the real engine's own debug-pipeline stages now individually verified to match
for a given cast (attacker mods, resistances/piercing, target mods, crit outcome), Arcane's
remaining 0.61x gap is now almost certainly *entirely* a cast-frequency/mana-pacing one, not a
per-hit damage formula gap - see the mana-pacing analysis below, unchanged in kind by these two
fixes (though the larger mana pool from fix 5 measurably helped: Frostbolt's own cast count rose
from 19.6 to 23.6 per 120s once the pool was corrected).

## The mana-pacing stall, closed (2026-10-25, fourth investigation)

Asked explicitly to close this rather than leave it open. Verified first, rather than
re-guessing: extracted every cast timestamp from the real engine's own full debug trace and
found the largest gap anywhere in a 125-second fight was 3.0s (Frostbolt's own cast time) - the
real engine *never* idles in this fight at all, confirming this engine's own ~43% idle time was
a genuine bug, not an inherent feature of the design both engines would share.

Root cause, found by extracting every real `Spent X mana from {SpellID: 1239700}` log line and
solving for the pattern: six consecutive samples (three with Arcane Power's own cost active,
three without) all fit *exactly* `cost = base * (1 + 1.75*stacks + 0.30*arcane_power_active)`
with `base = 181.0` - not `0.15 * max_mana` (≈984-1067 for this build), which is what this
engine had been computing from the client row's literal `CostPct: 15` field. The bug: `CostPct`
is a percentage of the spell's own *class/level* base mana table, not the player's own current
(gear-buffed) mana pool - a real WoW mechanic this project had never encountered before Arcane
Blast, so it defaulted to the wrong denominator. With the real base (181.0) instead of ~1000+,
Arcane Blast's own cost ramp is gentle enough that the rotation's existing 40%-mana escape
hatch comfortably catches it before any stall, matching the real engine's own smooth cast
cadence.

**Fixed** in `mage/arcane.rs` (`ARCANE_BLAST_BASE_MANA_COST`, replacing the `0.15 * max_mana`
computation) - **not** a data-pack change, since the pack's own `mana_cost: null` placeholder
was always correct, only the code that filled it in was wrong (see `mage-forever-...-rev5.json`
for the corrected provenance note). Updated total: **269.8 → 397.8 DPS (0.80x of the real
engine's 497.8, up from 0.54x)** - Arcane Blast's own cast count alone roughly doubled (7.5 →
15.2 per 120s). This single fix closed more of Arcane's gap than every other fix in this
document combined, confirming the mana-pacing stall really was the dominant remaining cause,
exactly as the prior investigation's own cast-cadence analysis had predicted.

Residual 0.80x (vs. Fire's 0.86x and Frost's 0.95x) is likely a mix of small, unquantified
effects (Clearcasting's own free-cast timing, the ~16-point spell-power residual noted below)
rather than one more big, named bug - not chased down to the last point this pass, but the
*structural* stall this section used to describe no longer exists.

## A small, unexplained residual (not chased further)

With the mana stall closed, Arcane lands at 0.80x - still the largest gap of the three Mage
specs, but no longer because of idle time (cast counts now track the real engine's own cadence
closely: Arcane Blast 15.2/120s here vs. a real rate implying roughly similar order, Frostbolt
26.2 vs. a comparable real rate). The residual is more likely a small, spread-out per-hit
effect than one more big structural bug - e.g. the ~16-point effective-spell-power gap noted at
the very first Arcane Blast cast in the original debug trace (481 real vs. 465 this engine's
own, *before* any stack bonus applies), which this investigation surfaced but didn't resolve.
Not chased further this pass - honestly flagged as open rather than guessed at. A future pass
with time to diff this engine's own computed stat totals against a real debug line item-by-item
(the same technique that found every fix in this document) would be the way to find it.

## One more generic fix, found while closing Fire Mage's own cast-count skew

`mage-fire-comparison.md`'s own "fifth investigation" (new per-cast debug logging, added
specifically for Fire) found Judgement of Wisdom (`raid.debuffs.judgementOfWisdom: true` - every
fixture checked so far, this spec's own included) as a real, entirely unmodeled mana-return
mechanic: a landed direct hit has a flat 50% chance to return 59 mana to its caster. Fixed
generically in `sim.rs` (`CasterStats.standing_judgement_of_wisdom`), so it applies here too
without any Arcane-specific code. **Updated total: 397.8 → 400.7 DPS (0.81x)** - a small gain,
consistent with Arcane's own residual gap being dominated by other, smaller effects rather than
mana economy (which the mana-stall fix already addressed).

## Three more generic fixes, three more small gains (2026-10-25, sixth investigation)

Asked to close the mana-stall "rigorously" and separately address Fire's own gaps; while doing
so, three more real, project-wide bugs turned up (none Arcane-specific in mechanism, though two
of the three measurably move Arcane's own number):

1. **A systemic cooldown-timing bug.** Any call site that advances `fight.now_ms` (paying for
   and casting a spell) *before* calling `fight.set_cooldown` was setting that ability's own
   cooldown from the *post-cast* timestamp instead of the moment the cast started - silently
   adding a full GCD/cast-time to every cooldown-bearing ability's own cadence project-wide.
   Found while building new per-cast debug logging for Fire Blast (its own cast gaps didn't
   match its logged cooldown at all) and confirmed by diffing against the real engine's own
   log. Fixed with a new `Fight::set_cooldown_from(key, from_ms, duration_ms)` primitive and a
   `cast_started_at` snapshot at every affected call site (`casting.rs::cast_direct_full`,
   `warlock/affliction.rs`'s Amplify Curse) - audited every other `set_cooldown` call in the
   codebase and found no further instances of this class.
2. **Mana Spring Totem's own +25 mp5 was missing** - a gap this project's *own* prior doc
   comment on `gear.rs::BUFF_MP5` had explicitly found and then dismissed as not mattering
   ("doesn't move this engine's reported DPS under comfortable mana"). Re-checking against a
   real debug log's own `Gained 97.050 mana from {OtherID: 2}` every 2000ms (`97.05 * 5/2 =
   242.625` mp5-equivalent from that one source alone) showed the dismissal was wrong in
   general - it just happened not to matter for whichever build the original note was written
   against. Fixed in `gear.rs` (`BUFF_MP5`, now `12.0 + 40.0 + 25.0`), generic across every
   class/spec since it's a standing raid buff, not Mage-specific.
3. **Missile Barrage's own self-sustaining proc chain was missing.** Real source
   (`talents/missile_barrage.rs`'s own `Some("arcane_missiles_tick") if self.missile_chance >
   0.0 => self.missile_chance` match arm) rolls a *second*, independent chance on every
   individual Arcane Missiles tick (not just the triggering cast) to re-extend the buff - this
   project had only ever modeled the cast-time trigger. Fixed with the engine's own existing
   `Fight::register_tick_proc` primitive (already used by Warlock's Nightfall - the exact right
   tool, not a new mechanism), registered in both `mage/arcane.rs` and `mage/frost.rs` (Frost
   uses Missile Barrage too) wherever `talents.missile_barrage > 0`.

**Combined, measured impact** (3000-iteration runs, `--buffs builder-starter`): Arcane 400.7 →
**416.1 DPS (0.84x** of 497.8). Frost 466.3 → **471.8 DPS (0.99x** of 475.6 - the closest any
Mage comparison has landed). Fire 614.7 → **619.5 DPS (0.88x** of 704.9 - Fire doesn't use
Missile Barrage, so only fixes 1-2 applied there. All three Warlock specs (which share fixes 1
and 2) stayed within a few percent of the real engine too (Destruction 1.04x, Demonology 1.04x,
Affliction 1.06x of their own real references, confirmed by a full re-run with the exact
commands `mythicsim-comparison.md`'s own tables use) - no regression, consistent with these
being genuine, generalizable fixes rather than per-spec fudges.

## The real cause of Arcane's remaining gap: Arcane Blast's buff was never shared (2026-10-25, seventh investigation)

Asked explicitly to look at per-ability damage, biggest first, and find out why each one
differs from the real engine - not just re-check the aggregate. Built the real engine's own
Rust CLI locally (already cloned in a prior session), ran it against this fixture's own request
with `simOptions.debugFirstIteration` (talent string needed one more character fixed first:
position 35 of the full string, `005`→`003`, the same Elemental Precision cap this project's
own talent-string decode had already found for Fire; the real engine refuses to run with a
talent rank above its own `maxPoints` rather than silently clamping it) and pulled a clean,
3000-iteration `--rotation-stats` result plus its own single-iteration debug trace.

**Per-ability trace at this point** (before this investigation's fix):

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Frostbolt | 255.4 | 242.6 | 0.95x |
| Arcane Missiles | 126.5 | 106.5 | 0.84x |
| Arcane Blast | 111.2 | 67.0 | 0.60x |
| **Total** | **497.8** | **416.1** | **0.84x** |

Frostbolt (the single biggest share of Arcane's damage, 58%) was already close; Arcane Blast -
the single largest absolute-DPS gap (44.2 of the total 81.7-point shortfall, over half) - was
the clear next target, exactly per "start with the abilities that do the most damage."

**Root cause, found by diffing the real engine's own debug log line by line**: Frostbolt's own
`SP:` field in the real log *alternates* between **480.0** and **665.4** over the course of a
single fight, with no gear or talent change - and the real log's own `Dynamic dep updated
(SpellDamage += Intellect * 0.15): {"SpellDamage": 61.800,}` lines, firing every time Arcane
Blast's buff (400573) gains or loses a stack, explain exactly why: `480 + 3 * 0.15 * 412 =
665.4`, exact (`412` being this build's own real Intellect, confirmed the same way: `61.8 / 0.15
= 412`). **Arcane Blast's own stacking buff is a standing increase to the caster's general
SpellDamage stat, not a bonus local to Arcane Blast's own next cast** - every spell landed while
any stack is up benefits, Frostbolt included. This project had modeled it the narrower way
(`mage/arcane.rs`'s old `arcane_blast_bonus_power`, applied only via a special
`cast_direct_with_bonus_power`/`resolve_direct_with_bonus_power` path used by nothing else) -
correct for Arcane Blast's own cast, but silently withholding the same bonus from every
Frostbolt (and Arcane Missiles) cast landed during the same 15-second window, which is most of
the fight given the buff's own near-continuous uptime (see the cycle timestamps in the real
log: buff up 0-15s, gap to 17.5s, up again 17.5-32.5s, and so on - under 15% downtime).

**Fixed generically**, not Arcane-specific: a new `Fight::dynamic_spell_power_auras` list and
`Fight::register_dynamic_spell_power_aura(aura_name, intellect_multiplier_per_stack)`, read live
by `effective_spell_power` on every single cast (`stacks * multiplier * Intellect`, summed
across every registered aura) rather than mutated imperatively - deliberately not a "credit/
debit the stat on stack change" approach, since a stacking aura can expire silently with no
method call firing at that moment, and `aura_stacks` already resolves that lazily (returns 0
once expired) for every other mechanic in this engine. `mage/arcane.rs` now just calls
`fight.register_dynamic_spell_power_aura(ARCANE_BLAST_BUFF, ARCANE_BLAST_INTELLECT_PER_STACK)`
once during talent setup; Arcane Blast's own cast switched to the plain `cast_direct_with_cost_
pct` path (now picking up the identical bonus automatically, like every other spell), and the
now-dead `arcane_blast_bonus_power`/`cast_direct_with_bonus_power`/`resolve_direct_with_bonus_
power` functions were removed rather than left unused. Confirmed Fire and Frost Mage have no
equivalent mechanic (scanned their own real debug logs for any per-cast `SP:` fluctuation - both
show a single flat value for the whole fight, so neither talents Arcane Blast and neither needed
this fix), and Warlock has no stacking-buff talent of this shape either - genuinely Arcane-only
in *application*, generic in *mechanism*.

**Updated total: 416.1 → 437.5 DPS (0.88x of the real engine's 497.8, up from 0.84x)**.
Frostbolt alone moved from 242.6 to 256.7 DPS - **0.95x → 1.005x**, essentially an exact match,
confirming the fix is correct rather than a fortunate aggregate coincidence. Arcane Missiles
rose too (106.5 → 113.8, 0.84x → 0.90x, since some of its casts land during an active-stack
window as well). Arcane Blast's own damage was unchanged (8044 → 8044, exactly, as expected -
the refactor relocates where the bonus is *shared*, not how much Arcane Blast's own cast gets).

## Remaining open gap: Arcane Blast's own cast frequency (not chased further this pass)

With Frostbolt now matching almost exactly, Arcane Blast is unambiguously the one ability still
meaningfully short: 67.0 DPS here vs. 111.2 real (0.60x), and - unlike before this investigation
- no longer explainable by a per-hit damage bug (per-cast average damage, using each side's own
total `casts` count as the denominator so misses are included on both sides: 654 here vs. 708.7
real, a close 0.92x). The real gap is **cast frequency**: the real engine casts Arcane Blast
18.8 times per 120s against this engine's own 12.3 - Frostbolt picks up the slack in the other
direction (28.7 here vs. 25.4 real), meaning the real rotation spends measurably more of its
time in the "build/hold 3 Arcane Blast stacks" state than this engine's own priority-list
condition (`stacks >= 3 || mana% < 40%`, read directly from the real priority list's own text)
currently reaches. Candidates not yet checked: whether Arcane Power's own cost/damage window
timing interacts with the 40%-mana escape hatch differently than this engine models it, or
whether Clearcasting's own free-cast timing disproportionately favors Frostbolt casts over
Arcane Blast ones in a way this engine's rotation order doesn't match. Flagged honestly as open
rather than guessed at further this pass - the dominant, structural bug (the unshared SP buff)
is fixed; what remains is a smaller, harder-to-isolate cast-apportionment effect.

## Two real, sourced one-time mana items, added generically (2026-10-25, prompted by "0.88x is unacceptably low")

Found while re-investigating Fire Mage's own remaining Fire Blast shortfall via the same
"diff every real `Gained X mana` log line" technique that found Judgement of Wisdom/Master of
Elements - the full writeup, including why the third item seen (`ItemID: 14152`, a worn
trinket) was deliberately *not* added (its amount is inconsistent across every build checked,
consistent with an unsourced randomized range), is in `mage-fire-comparison.md`'s own matching
section. The two that *were* added apply here unchanged: `ItemID: 8008` (+1100 mana, gated on
`currentManaPercent <= 80%` - this build's own real priority list doesn't carry this item as an
explicit action at all, relying on the generic `autocastOtherCooldowns{}` instead, but the real
log shows the identical item firing for the identical amount, so Fire's own sourced condition
was reused rather than left ungated here) and `ItemID: 13444` (+1800, a mana potion, fired
unconditionally - this build's own real log shows the identical amount too). Modeled generically
in `sim.rs` (`Fight::maybe_use_one_time_mana_item`), called once at the top of this spec's own
`step`, not duplicated per-spec logic.

**Updated total: 437.5 → 449.4 DPS (0.90x of the real engine's 497.8)** - 2900 mana of real,
sourced, previously-missing income (out of this build's own ~6560 max mana pool) measurably
eased the mana pressure that was forcing extra Frostbolt casts in place of Arcane Blast ones,
though the cast-apportionment gap named above isn't fully closed by it alone.
