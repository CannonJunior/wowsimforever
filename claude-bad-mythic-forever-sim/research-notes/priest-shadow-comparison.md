# Comparing Shadow Priest against the real engine's own trace (2026-10-25)

Same methodology as the Mage comparisons: build `mythicsim-forever-engine`'s own Rust CLI
locally (already cloned in a prior session), run it with `--rotation-stats` against the real
`validation/2026-10-04-builder-starters/shadow-priest--undead.request.json` fixture, extract
the per-ability `damage` field from `raidMetrics.parties[].players[].actions`, cross-check the
sum against `raidMetrics.dps.avg`, then run this engine's own CLI with matching flags and build
a side-by-side table - plus, new this pass, the `forever-engine prepare` subcommand, which
resolves every talent to its own final numbers (`early_demise_crit: 30.0`, `shadow_weaving.
proc_chance: 1.0`, exact stat totals) without needing to decode this fixture's own 17-slot
Shadow-tree talent-string segment by hand.

## Sourcing method used

1. `forever-engine sim --request shadow-priest--undead.request.json --rotation-stats --outfile
   result.json` for the real per-ability trace and aggregate DPS (667.5).
2. `forever-engine prepare --request shadow-priest--undead.request.json --outfile
   prepared.json` for every talent's own resolved numbers and this build's own exact caster
   stats (`stat_auras`: SpellDamage 378, SpellCritPercent 14.9656, MP5 82, Spirit 225/239) -
   confirmed to match this project's own gear-pack-computed totals exactly (spell_power 378 +
   bonus_shadow_power 160 = 538, matching the real debug log's own `SP: 538.0` on every Shadow
   cast).
3. Every spell ID resolved directly against `data/spells.jsonl` (Mind Blast 10947, Shadow Word:
   Pain 10894, Devouring Plague 19280, Shadow Word: Death 1309636, Mind Flay 18807).
4. The real priority list read directly from the request JSON's own declarative condition tree
   (not a prose summary), dumped in full rather than truncated.

## The real rotation, read from source

1. Shadow Word: Pain, kept up (skipped once under 6s of fight time remain).
2. Devouring Plague, kept up (skipped under 12s remaining).
3. Inner Focus immediately followed by Mind Blast, once Shadow Weaving is at 5 stacks and Inner
   Focus is off its own 180s cooldown.
4. Mind Blast otherwise (unconditional, whenever off cooldown).
5. Shadow Word: Death (unconditional, whenever off cooldown - no extra condition at all on this
   priority item, confirmed from the full, untruncated JSON).
6. Mind Flay, the channel filler - with a real mid-channel clip (interrupts after tick 2 of 3
   once Mind Blast comes off cooldown, confirmed directly from two consecutive real log samples
   each showing only 2 ticks fired, not 3) this engine doesn't model (see "Known simplifications"
   below).

## What's modeled

- **Shadowform** (talent, 1/1): cast once *before* the pull (confirmed by the real debug log's
  own negative timestamp, `[-3.00] Spent 533.500 mana`), so it's applied as a one-time deduction
  from the starting mana pool at setup, not through the normal cast machinery. +10% Shadow
  damage and -50% Shadow mana cost (both blanket), +100% crit-damage bonus on exactly the five
  Shadow spells this rotation casts.
- **Shadow Weaving** (talent, 5/5): a landed Shadow-school hit has a 100%-at-this-rank chance
  (confirmed via `prepare`'s own `proc_chance: 1.0`, not decoded from the 20%/40%/60%/80%/100%
  curve) to add a stack (cap 5, 15s, refreshing) raising this priest's own Shadow damage 2%/stack.
- **Improved Mind Flay** (talent, 2/2): each successive tick within one channel deals 20% more
  than the last, compounding - confirmed from two real log samples, tick 2 / tick 1 = 1.200
  exactly, twice. This engine resolves Mind Flay's whole channel as one atomic application, so
  the ramp is folded into one flat average bonus across the 3 ticks (`(1+1.2+1.2^2)/3 - 1 ≈
  +21.3%`) rather than three separately-scaled ticks.
- **Inner Focus** (talent, 1/1, Discipline tree): a 180s-cooldown buff making the next costed
  cast free with +25% crit chance (this build's own real effect, `prepare`'s own `crit_percent:
  25.0`) - spent only on Mind Blast, matching the real priority list exactly.
- **Early Demise**: +30% crit chance on Shadow Word: Death (this build's own real `prepare`
  output gives the resolved number directly, `early_demise_crit: 30.0`, not decoded from a rank
  curve) - **gated on the real engine's own 20%-execute-phase condition**
  (`src/classes/priest/spells/direct.rs::apply_shadow_word_death`: "Early Demise adds its crit
  for this roll inside the 20% execute phase"). A first version of this module applied the
  bonus unconditionally - found wrong while chasing Smite Priest's own per-hit damage gap and
  re-reading this same source file for an unrelated reason. Fixed even though doing so *lowers*
  this spec's own aggregate number slightly (Shadow Word: Death's own crit rate was
  overstated) - this engine doesn't track a target health pool by default (no `--target-health`
  flag passed), so the bonus now correctly never procs rather than guessing a health state
  that was never modeled, consistent with this project's own standing rule to keep a
  verified-correct fix even when it moves the chased number the wrong way.
- **Two real, one-time mana items** (found via the "diff every real `Gained X mana` log line"
  technique that found Mage's own): a mana potion (`ItemID: 13444`, +1800, identical to Mage's
  own) and a conjured item (`ItemID: 12662`, "Demonic Rune", +1200 - a different item from
  Mage's own `ItemID: 8008`). Modeled with the same generic `Fight::maybe_use_one_time_mana_item`
  primitive Mage's own fix added, reusing Mage's own sourced `mana<=80%` condition for the
  conjured item since this fixture's own priority list doesn't carry an explicit condition for
  it (picked up via the generic `autocastOtherCooldowns{}` instead).
- **Judgement of Wisdom**: already generic (`CasterStats.standing_judgement_of_wisdom`), applies
  automatically via the existing `BuilderStarterBuffs::Priest` wiring.

## Known simplifications (not chased further this pass)

- **Mind Flay's own mid-channel clip isn't modeled.** The real rotation interrupts a Mind Flay
  channel after 2 of its 3 ticks whenever Mind Blast comes off cooldown mid-channel (confirmed
  directly in the real log). This engine always resolves the full 3-tick channel. This is the
  same category of simplification Arcane Mage's own Arcane Missiles channel already accepted.
- **Dark Inspiration** (a temporary +Spirit buff after Mind Blast/a full Mind Flay channel - a
  minor mana-regen effect, not a damage one) and **Vampiric Embrace** (a healing mechanic) are
  deliberately unmodeled, same DPS-only scope cut every other class's own comparison already
  makes.
- **Shadow Word: Death's own backfire-on-the-caster drawback** isn't modeled - this project
  tracks no player health pool for any class.

## Twin Disciplines, found by comparing two abilities' own attacker-mod ratios on the same character (2026-10-26)

Asked explicitly, again, to go ability-by-ability and find the cause rather than accept the
uniform 0.68x band as unexplained. Checking Mind Blast's own real debug line precisely:
`AfterAttackerMods / BaseDamage` reads *exactly* 1.21 - matching this project's own formula
exactly (`1.10` Shadowform `* 1.10` Shadow Weaving at 5 stacks, the two multiplicative
modifiers this project already applies). Checking Shadow Word: Pain's own line on the *same*
character at the *same* moments: the identical ratio reads 1.2707 instead - `1.2707 / 1.21 =
1.0502`, a clean, constant extra +5.02% this project had no explanation for yet.

The difference between the two abilities pointed straight at the answer: **Twin Disciplines**
(Discipline tree, `src/classes/priest/prepare/talents/discipline.rs::apply_twin_disciplines`)
gives "+1% damage and healing per point on *instant* spells" - its own class mask names Shadow
Word: Pain, Devouring Plague, Shadow Word: Death and Mind Flay explicitly, but *not* Mind
Blast, which has a 1.5s cast time and so doesn't qualify as "instant." At 5/5 (the only rank
that produces exactly +5.02%, matching `1.21 * 1.05`), this explains the gap precisely - the
one ability excluded from the talent's own mask (Mind Blast) is the one ability that matched
the pre-existing formula exactly, and the four abilities the mask *does* cover are the four
that showed the extra bonus. Fixed as a flat `direct_pct`/`dot_pct` bonus on those four
ability keys (an earlier investigation pass had wrongly concluded Shadow Word: Pain's own
real debug line "confirmed no extra bonus needed at all" - that check used the *Smite*
character's own copy of Shadow Word: Pain, which has zero Shadow-tree talents at all and so
was never a valid comparison for the Shadow Priest build's own talent allocation; this pass
corrected that by comparing two abilities on the *same* character instead).

## Per-ability trace

3000-iteration runs, `--buffs builder-starter`, matched talent defaults.

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Mind Flay | 255.8 | 187.8 | 0.73x |
| Mind Blast | 139.2 | 95.5 | 0.69x |
| Shadow Word: Pain | 128.6 | 88.1 | 0.69x |
| Devouring Plague | 77.0 | 54.2 | 0.70x |
| Shadow Word: Death | 60.7 | 45.6 | 0.75x |
| Touch of the Grave (racial, unmodeled) | 5.8 | 0 | - |
| **Total** | **667.5** | **471.2** | **0.71x** |

0.68x → 0.71x from Twin Disciplines alone. Every ability still sits in a tight, uniform
0.69-0.75x band, confirming the per-hit damage formula is now solid across the board (Mind
Blast's own ratio, unaffected by this fix, is identical to before) and the remaining gap is a
shared cast-frequency issue, not a per-ability damage one.

**The leading suspect, tried twice and reverted twice**: the real priority list's own Mind
Flay mid-channel clip (interrupts the channel once Mind Blast is ready, checked only at a tick
boundary - worked out as three distinct cases depending on exactly when Mind Blast's cooldown
finishes: ready within 1000ms of channel start → 1 tick delivered; 1000-2000ms → 2 ticks;
later → the full 3). A first attempt modeled only the simplest case ("always clip to 2
whenever ≤2000ms remain") and made the total *worse* (457.1 → 428.5 DPS pre-Twin-Disciplines).
A second attempt implemented the full three-case version precisely and made it *worse again*
(down to 411.6) - both reverted. Whatever the real engine's own net effect from clipping
actually is, modeling it (correctly or not) consistently moves this project's own total
*further* from the real number, meaning the cast-frequency gap isn't explained by "Mind Flay
runs shorter on average" at all - something else about why real casts everything more often
remains genuinely unexplained, flagged honestly as an open question rather than shipped as a
plausible-sounding guess.

This is a first-pass implementation (real spells, real talents, real gear, real priority-list
order) refined twice since (one real bug fixed - the missing one-time mana items - the Early
Demise execute-phase gate, and now Twin Disciplines); the residual ~0.71x gap is honestly
reported, not papered over.

## A third, structurally different attempt at Mind Flay's clip (2026-10-26, same day) - reverted a third time

Asked again for a comprehensive, web-researched pass since 0.71x was still "woefully
inaccurate." A fresh single-iteration real debug trace (`debugFirstIteration` + `iterations:
1`), analyzed tick-by-tick rather than cast-by-cast this time, found the real mechanism more
precisely than either prior attempt had: Mind Flay channels split cleanly into three outcomes
(0 ticks, 2 ticks, or the full 3 - never 1), and the 0-tick case is **not a clip at all** - it's
the channel's own single hit roll simply missing (confirmed directly: `[31.60] ... {SpellID:
18807} Miss`, followed immediately by the next decision 1.5s later, the GCD only). Only the
2-tick case is a genuine mid-channel interrupt, and it always preceded a Mind Blast cast with
essentially zero gap - never Shadow Word: Death, never a Shadow Word: Pain/Devouring Plague
refresh, even when a full 3-tick channel was immediately followed by one of those. This matches
the real priority list's own literal condition exactly: `interruptIf: spellTimeToReady
(mind_blast)<=0s AND dotTimeToNextTick<=0.05s AND gcdIsReady` - Mind Blast specifically, nothing
else.

Built a real per-tick channel primitive (`Fight::cast_channeled_dot` in `sim.rs`, since
removed) so the channel could structurally stop early and hand control back mid-channel - the
two prior attempts never had this capability at all (`cast_dot_only`'s single `advance()` call
is one fixed-size jump with no way to return control partway through), so no amount of
precomputing the right case could have worked even in principle. This also let Improved Mind
Flay's real 20%-per-tick ramp apply exactly instead of the flat average below. Two real bugs
were found and fixed while building this that the prior attempts never got the chance to hit:
(1) a first version of the interrupt check tested only "is Mind Blast off cooldown," which could
fire the interrupt and then have Mind Blast fail anyway for lack of mana, falling through all
the way back to Mind Flay for a double loss (the clipped ticks *and* no replacement cast) -
fixed by also checking affordability (`can_afford`); (2) a first version of the interrupt
condition also fired on Shadow Word: Death and the Shadow Word: Pain/Devouring Plague refreshes
("anything higher-priority"), which actively over-interrupted relative to the real list's own
narrower condition.

**Even after fixing both bugs and narrowing to the literal real condition (Mind Blast only),
the total still came out lower than the unclipped baseline: 471.2 → 467.3.** Packing in a few
more Mind Blast casts measurably starves Shadow Word: Pain/Devouring Plague of mana later in
the fight (their own cast counts dropped from 5.6/3.8 to 5.0-5.2/3.0 at 20000 iterations, a
real, repeatable shift, not RNG noise) - trading two high-value-per-cast dots for several cheap
Mind Blast casts the real engine's own reported per-ability *mix* doesn't actually show: this
project's pre-existing, un-clipped baseline's own per-ability shares (SWP 18.7%, DP 11.5%, MB
20.3%, SWD 9.7%, MF 39.8%) were already close to the real engine's own shares (19.3%, 11.5%,
20.9%, 9.1%, 38.3% respectively) - within 1-2 points on every ability - and clipping moved every
one of them slightly *further* from its own real share, not closer, even though the clip
mechanic itself is now modeled exactly as the real condition states it. Reverted a third time.

**Three independently-motivated attempts, the last one built correctly against the literal real
condition and free of both bugs the first two attempts never got a chance to have, all moving
the wrong direction, is strong evidence this project's own mana economy - not the clip
mechanic's own correctness - is what actually differs from the real one here.** The real engine
evidently affords more total Mind Blast casts *without* this tradeoff; this project's own mana
regen formula was independently re-verified during this same pass (it already matches the real
engine's own reported `MP5: 82` exactly, and deliberately omits the 5-second-rule throttle in
the *generous* direction - continuous regen even while casting - so under-modeled regen isn't
the explanation either). The next lead, if this is chased again, is a regen-rate or cost-basis
gap elsewhere in the mana economy, not a fourth attempt at the clip mechanic itself - see
[[mythic-forever-sim-gap-closing-methodology]]'s lesson on two failed attempts being strong
evidence, now extended to three.

**The same pass's web research and comprehensive per-ability re-check paid off for Smite
Priest instead** - see `priest-smite-comparison.md`'s own "Two more real mechanics found"
section for a major, verified fix (Judgement of the Crusader, previously misdiagnosed as a
crit-chance debuff and left unmodeled across three prior passes, turned out to be a flat
Holy-school damage-taken bonus explaining Smite's own dominant 23% `BaseDamage` shortfall)
found the same day by applying this same rigor to the other spec instead of a fourth Mind Flay
attempt. Shadow Priest's own final number this pass: unchanged at **471.3 DPS (0.71x of
667.5)** - the Mind Flay investigation was thorough and the two bugs it found are real, but the
net result was a confirmed revert, not a fix.

## The mana economy, addressed directly: Dark Sacrifice found, and the clip retried a fourth time (2026-10-26, later the same day)

Asked explicitly to address the mana economy behind the cast-frequency gap, the Smite
mana-pacing condition, and whether any already-fixed Mage/Warlock mana mechanic generalizes
here. Took the same "diff every real `Gained X mana` line" technique the Mage passes used
(`research-notes/mage-fire-comparison.md`'s own eighth pass) and applied it fully to Priest's
own real debug trace for the first time, grouping every gain by its own source tag rather than
spot-checking the ones already known about.

**Found: Dark Sacrifice, the Undead racial (not a Priest talent at all - `src/classes/priest/
spells/dark_sacrifice.rs`'s own doc comment: "the Undead priest's racial, registered for
Undead only"), a massive and previously entirely unmodeled mana source.** A real log line reads
`Major cooldown used: {SpellID: 1277328}`, followed by five `Gained 367.8 mana from {SpellID:
1277328}` lines 3s apart (a 15s duration, matching the real spell row's own `DurationMs: 15000`,
`PeriodMs: 3000`). The real engine's own activation rule (`should_activate`:
`max_mana - mana >= amount * ticks`) only fires once the *entire* projected gain would fit
without overflow - and the per-tick amount (`src/classes/priest/prepare/export.rs`: "each tick
pays the client base plus a fifth of Spirit") resolves to a clean, exact formula: total gained
over one full use = `5 * (320 + Spirit/5) = 1600 + Spirit`. Confirmed to the tenth of a mana
point on two independent builds: Shadow Priest (`Spirit: 239` → `1839.0` gained, matching the
trace exactly) and Smite Priest (`Spirit: 237` → `1837.0`, matching exactly). At ~1600-1850
mana on pools of 6600-6900, this is roughly **27% of the entire mana pool**, from a mechanic
this project had never looked for because it isn't a talent, a buff package item, or a gear
stat - it's a racial, the same category this project had only ever modeled passively (race
base stats, Intellect-to-crit rate) rather than as an active cooldown.

**Checked whether this generalizes to Mage or Warlock (both races also Undead in this
project's own reference builds) - it does not, confirmed two ways.** First, by source
structure: `dark_sacrifice`/`DarkSacrifice` only appears inside `src/classes/priest/`
anywhere in the real engine's own codebase - no `classes/mage` or `classes/warlock` file
registers it at all. Second, empirically: a fresh real debug trace against Destruction
Warlock's own "builder starter" fixture (also Undead) shows zero occurrences of `1277328`,
`Dark Sacrifice`, or `Major cooldown used` anywhere in the full log. Whatever the real reason
(a genuine class restriction on the racial, or simply an engine-coverage gap that happens to
match the real game either way), the real engine's own ground-truth behavior is unambiguous:
Warlock and Mage don't get this, so adding it there would be a regression, not a fix. Every
other Mage/Warlock mana mechanic already modeled (Judgement of Wisdom, Master of Elements,
the Arcane Blast `CostPct`-is-a-percent-of-base-mana fix, the two shared one-time items) was
re-checked against this same Priest trace and found to already generalize correctly where it
should (Judgement of Wisdom: present and correctly modeled for Priest too; the two one-time
items: present and correctly modeled) and correctly *not* applied where it shouldn't
(Master of Elements and the Arcane Blast fix are Mage-specific talents, absent from both
Priest fixtures' own `prepare` output, correctly never referenced in `priest/*.rs`).

Implemented as a new generic primitive, `Fight::maybe_use_headroom_gated_mana_cooldown` (in
`sim.rs`, alongside `maybe_use_one_time_mana_item`), modeling the real engine's own point-in-
time "does the whole 5-tick amount fit" gate as a single flat grant at that same decision
instant rather than genuinely spreading it over 15s - defensible because the real activation
check already commits to "room for the *whole* output" up front, so collapsing delivery to one
instant changes *when* within a fight the mana lands, not *whether* a given fight can use it.
Wired into both specs' own `step()` as `dark_sacrifice_1277328` (shared cooldown key naming the
real spell ID, same convention the other one-time items use).

**Result: Shadow 471.3 → 511.8 DPS (0.71x → 0.77x). Smite 379.3 → 395.1 DPS (0.74x → 0.77x).**
Both specs' biggest single jump from one fix since the Judgement of the Crusader find.

**The Smite mana-pacing rank-switch condition itself was re-verified against the real
priority list's own raw JSON (not the prose paraphrase) and found exactly correct**: the real
condition reads `currentManaPercent >= remainingTimePercent * 0.9` literally (a `cmp`/`math`
node tree in the request's own `priorityList`), matching `smite_mana_pacing_ok`'s own
implementation term-for-term. Smite's own ability-level ratio (0.52x before this pass) is not
caused by a mana-pacing bug - full-rank and downranked Smite both have similar cast times
(2500ms/2000ms), so which rank fires barely changes total *throughput*; the remaining gap there
is the same project-wide cast-frequency shortfall every other ability shows, now partly (not
fully) closed by Dark Sacrifice.

**Mind Flay's clip was retried a fourth time, under what looked like a corrected premise, and
still failed - this time conclusively.** The three prior attempts' own standing conclusion
("the mana economy, not the mechanic, differs") predicted that fixing the mana economy should
let the clip succeed. It was retried with the exact same (already-correct, bug-free) attempt-3
implementation, now running on top of the Dark-Sacrifice-corrected mana economy: **511.8 →
507.1, still worse, by almost exactly the same ~1% margin as attempt 3's failure (471.2 → 467.3)
had, despite meaningfully more mana being available.** A margin that doesn't shrink when the
resource it was blamed on becomes more abundant is strong evidence the resource was never the
actual constraint: clipping trades Mind Flay ticks for Mind Blast casts at a fixed, deterministic
GCD-slot exchange rate, and that exchange is a loss in this project's own damage model
regardless of whether mana can sustain it. **This closes the Mind Flay clip investigation for
good - four independent attempts, the last two free of every implementation bug the first two
had and the last specifically testing (and refuting) the mana-economy hypothesis the third
attempt itself proposed. Do not attempt a fifth time.** The remaining ~0.77x cast-frequency gap
on both specs has a real cause that isn't the channel-clip mechanic and isn't (at least not
primarily) a missing mana source - genuinely still open, named precisely rather than guessed at
further.
