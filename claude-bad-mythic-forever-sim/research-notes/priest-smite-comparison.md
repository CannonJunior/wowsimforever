# Comparing Smite Priest against the real engine's own trace (2026-10-25)

Same methodology as `priest-shadow-comparison.md` (full writeup there) - `forever-engine sim
--rotation-stats` for the real per-ability trace, `forever-engine prepare` for every talent's
own resolved numbers, every spell ID resolved against `data/spells.jsonl`.

## The real rotation, read from source

1. Inner Focus immediately followed by Holy Fire, once Holy Fire's own dot has under 3s
   remaining and Inner Focus is off cooldown - falls through to a plain Holy Fire refresh if
   Inner Focus isn't available (same combo shape as Shadow Priest's own Inner Focus + Mind
   Blast, just spent on a different spell).
2. Penance, unconditional whenever off cooldown.
3. Shadow Word: Pain, kept up (no remaining-fight-time gate on this item, unlike Shadow
   Priest's own copy of the same spell or this spec's own Devouring Plague below).
4. Devouring Plague, kept up (skipped under 12s remaining).
5. Smite (full rank, `10934`), gated on a real mana-pacing check: `currentManaPercent >=
   remainingTimePercent * 0.9` - only spend the expensive rank while there's still enough mana
   left to sustain it for the *rest* of the fight, within a 10% margin.
6. Smite (rank 2, `591` - a cheap downranked spell), the mana-saving fallback once that check
   fails.

## What's modeled

- **Power in Light** (talent, Discipline tree): +10% damage on this build's own Holy-school kit
  (`multiplier: 1.1` in the real `prepare` output) - applied to Smite, Holy Fire and Penance,
  the exact spell list this rotation already casts.
- **Inner Focus** (talent, shared mechanic with Shadow Priest - see that file's own module
  docs): spent on Holy Fire's own refresh here, not Mind Blast - this spec's real priority list
  never casts Mind Blast at all, despite the spell being registered/available in this build's
  own `prepare` output. Trusted exactly as the real priority list states it.
- **The real mana-pacing throttle on full-rank Smite** (`mana% >= remainingTimePercent * 0.9`) -
  implemented literally, not approximated, since the real condition's own formula is explicit
  in the request JSON.
- **The same two real, one-time mana items** Shadow Priest's own fixture carries (`ItemID:
  13444` potion, +1800; `ItemID: 12662` "Demonic Rune", +1200) - confirmed present in this
  build's own real log too, with the identical amounts.
- **Judgement of Wisdom**: generic, applies automatically.

## Not modeled

- **Holy Nova**: present in this build's own `prepare` output (the spell is registered) but
  never cast by the real priority list at all - an AoE spell irrelevant to a single-target
  fight, not a gap.
- **Searing Light** (a healing-cost-reduction proc - this project's DPS-only scope excludes
  healing, same as every other class) and **Judgement of the Crusader** (a *new* raid debuff
  this fixture's own `raid.debuffs` block carries, `judgementOfTheCrusader: true`, that no
  other class's own fixture has shown yet - found but not chased down this pass; likely a flat
  crit% debuff on the target, would need its own sourcing pass before modeling).

## Per-ability trace

3000-iteration runs, `--buffs builder-starter`, matched talent defaults. Real numbers extracted
directly from `raidMetrics.parties[].players[].actions`, `smite` (10934) and `smite_downrank`
(591) summed under one row since this engine's own report attributes both to the same
`ability_key` (`"smite"`).

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Smite (both ranks) | 188.7 | 67.6 | 0.36x |
| Holy Fire | 113.4 | 83.4 | 0.74x |
| Shadow Word: Pain | 84.6 | 63.4 | 0.75x |
| Penance | 73.5 | 29.0 | 0.40x |
| Devouring Plague | 47.9 | 40.9 | 0.85x |
| Touch of the Grave (racial, unmodeled) | 5.4 | 0 | - |
| **Total** | **513.5** | **284.2** | **0.55x** |

Shadow Word: Pain and Devouring Plague (shared spells with Shadow Priest) sit at roughly the
same ratio Shadow Priest's own comparison found for them, consistent with the same underlying
mana-economy shortfall. **Smite and Penance are a different, larger kind of gap, and not
explained by cast frequency alone**: Smite's own real cast count (full+downrank, 27.75/120s)
is only moderately ahead of this engine's own (19.4/120s, a 0.70x ratio) - the *per-cast*
damage average is the dominant factor (real ≈816/cast vs this engine's own ≈418/cast, a 0.51x
ratio on top of the cast-count gap). A real debug-log line for Smite shows `SP: 509.9,
BaseDamage: 655.6` - this engine's own formula (`170 + 0.714 * 509.9 = 534.1`) is ~23% short of
that `BaseDamage` figure *before* any of the subsequent attacker/target/outcome-stage
multipliers even apply, so this isn't an effective-spell-power gap (509.9 real vs this
project's own 498 computed for the identical build is a much smaller, already-familiar-sized
residual) - something else is adding to Smite's (and very likely Penance's, given its own
similarly large shortfall) *base* damage before the SP-coefficient stage. Checked and ruled
out as the cause: Twin Disciplines (`src/classes/priest/prepare/talents/discipline.rs` - real
source confirms it only affects *instant* spells: Holy Nova/Chastise/Divine Grace/Contingency
Plan/Shadow Word: Pain/Devouring Plague/Shadow Word: Death/Mind Flay/Starshards/Penance - not
in this build per its own `prepare` output anyway, and wouldn't reach Smite/Holy Fire even if
it were), Holy Specialization (crit *chance*, not damage) and Divine Fury (cast time, not
damage). **Flagged as a genuine, found-but-unexplained gap** rather than guessed at further -
the next concrete lead for a future pass is reading the real engine's own `BaseDamage`
computation in `src/classes/priest/spells/direct.rs` directly rather than inferring it from the
log, since every candidate checked by elimination so far has come back negative.

Smite Priest's own total gap (0.55x) is meaningfully larger than Shadow's (0.70x), driven
mostly by this unexplained Smite/Penance base-damage shortfall rather than the mana economy
(which the one-time-mana-item fix already measurably improved here too, from 0.527x to 0.553x
before this per-ability breakdown even ran). This is a first-pass implementation (real spells,
real talents, real gear, real priority-list order including the literal mana-pacing formula);
the Smite/Penance base-damage gap is the clear next thing to chase, not yet attempted beyond
ruling out the three talent candidates above.

## The real cause, found: Healing Power is a universal gear stat this project never tracked (2026-10-26)

Asked explicitly to take a comprehensive approach, re-examine every ability's own real debug
line, and search the web for how other WoW Forever/historical simulators handle this, since
both specs were still "woefully inaccurate" and the request was for a *general* fix, not a
Smite-specific patch.

**The web research paid off directly**: `classicwow.gg`'s own WoW Forever Priest guide states
plainly, in its gearing section, that *"Healing gear now provides one-third as much bonus
damage"* - a real, Forever-specific combat mechanic, not a Classic-era rule this project had
simply assumed away. Re-reading Smite's own real debug line with this in mind resolved the
~23% `BaseDamage` mystery completely: this build's own 15 items (checked item-by-item against
`data/items.jsonl`'s own stat index 4) each carry a `HealingPower` value *exactly equal* to
their own `SpellDamage` value - not a coincidence, standard "hybrid caster" itemization for
this entire item set - summing to the same 277 total. This project's own gear-mining passes
(Warlock's, Mage's, *and* Priest's) had all independently parsed and then silently dropped this
stat, assuming (never verified) that a DPS-only engine had no use for a "healing" number.

**A second, smaller piece of the same mechanic**: `src/prepare/consumes.rs::register_static_
imbue`'s own `25122 | 20749` match arm (Brilliant Wizard Oil, already modeled for its own
`+36 SpellDamage`) *also* grants `+36 HealingPower` unconditionally - this project's own prior
doc comment on that fact had explicitly (and wrongly) noted it as "not tracked by this
DPS-only engine." It is now.

**Found to be Holy-school-scoped, not universal, by checking Shadow Word: Pain on the exact
same (Holy-geared) character**: a first version of this fix added `/ 3.0` of HealingPower
straight into the universal `spell_power` field, reasoning "any school benefits equally." That
overshot badly for every *other* class (Fire Mage jumped to 1.02x of real, Destruction Warlock
to 1.14x) - because Mage/Warlock gear *also* carries HealingPower (confirmed the same way,
item-by-item), and a Shadow-school/Fire-school spell has no business reading it. The fix: this
same character's own Shadow Word: Pain debug line (`SP: 549.9, BaseDamage: 237.0`) matches
this project's pre-existing formula *exactly*, with zero healing-power bonus needed, while the
Holy-school spells on the identical character still needed one. This proves the mechanic is
Holy-school-scoped - the same shape as the existing `bonus_shadow_power`/`bonus_fire_power`
fields, not a `spell_power` one. Corrected to add `healing_power / 3.0` to a new `bonus_holy_
power` field instead (`CasterStats` already had this field, added when Smite Priest's own
Holy-school damage was first modeled) - re-running every other class/spec afterward confirmed
their numbers returned to their own pre-existing baselines exactly, since none of them ever
read `bonus_holy_power` at all.

**Per-ability trace after the Healing-Power fix**:

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Smite (both ranks) | 188.7 | 79.0 | 0.42x |
| Holy Fire | 113.4 | 94.5 | 0.83x |
| Shadow Word: Pain | 84.6 | 63.4 | 0.75x |
| Penance | 73.5 | 32.1 | 0.44x |
| Devouring Plague | 47.9 | 40.9 | 0.85x |
| **Total** | **513.5** | **309.9** | **0.60x** |

A real, substantial improvement (0.55x → 0.60x) - Holy Fire alone moved from 0.74x to 0.83x.
Smite and Penance still showed the *largest* remaining ratios.

## Two more fixes, found by extending Shadow Priest's own Twin Disciplines discovery (2026-10-26)

Shadow Word: Pain's own real debug line on *this* (Smite) character reads `AfterAttackerMods /
BaseDamage = 1.0498` - almost exactly `1.05`, and this build has neither Shadowform nor Shadow
Weaving (both Shadow-tree-exclusive) to explain any part of it, unlike Shadow Priest's own
compound `1.21 * 1.05` reading. This isolates the same **Twin Disciplines** talent (see
`priest-shadow-comparison.md`'s own full citation) at 5/5 here too - Smite Priest's own build
invests heavily in the Discipline tree (already noted project-wide: "the reason the Smite
build goes thirty-one points deep" for Power Infusion), so it's not surprising it independently
picked up this talent as well. The same class mask covers **Penance** (confirmed instant via
this build's own real log, `Cast Time = 0s`) in addition to Shadow Word: Pain and Devouring
Plague - Smite and Holy Fire are excluded (both have a real cast time).

**Penance's own channel fires an "extra tick that leaves the tick count alone" on landing**
(`src/classes/priest/spells/holy.rs::apply_penance`'s own doc comment, "Go `Dot.TickOnce`") -
confirmed directly against a real log showing three bolts (t=0.00, 1.00, 2.00) for one cast,
not the two this project had modeled. Corrected to 3 ticks in the data pack.

**Updated total: 309.9 → 332.6 DPS (0.60x → 0.65x)** - Penance alone jumped from 32.1 to a
higher share (both the extra tick and Twin Disciplines together). Smite itself (0.42x) remains
the single largest gap, untouched by either fix (it has a real cast time, so Twin Disciplines
doesn't reach it, and it isn't a channel, so the tick-count fix doesn't apply) - still likely
explained in part by **Spiritual Guidance** (`src/classes/priest/prepare/talents/holy.rs`:
"damage 1/3/5/6/8% of Spirit, healing 5% per point", converting a share of Spirit directly into
both SpellDamage and HealingPower), not yet modeled since it's a Priest-specific talent rather
than a generic gear mechanic this project's own cross-class fixes already cover. **Mind Flay's
own mid-channel clip** (see `priest-shadow-comparison.md`'s matching section) was tried here by
extension too and reverted for the same reason it was reverted there - modeling it moved this
spec's own total further from the real number, not closer.

## Two more real mechanics found, closing most of Smite's own remaining gap (2026-10-26, same day)

Asked again, explicitly, for a comprehensive ability-by-ability pass plus web research into how
other WoW Forever/historical simulators handle this, since 0.65x was still called "woefully
inaccurate." Pulled a fresh single-iteration real debug trace (`debugFirstIteration` +
`iterations: 1`) and checked Smite's own `BaseDamage` line precisely against this project's
formula again, this time tracing every stage of the real engine's own source rather than
inferring from the log alone.

**Spiritual Guidance, implemented**: this build's own `prepare` output gives `talents.
spiritual_guidance: 3` (3/5 points) - the real curve (`data/spell-extras.json`, spell 14901:
damage% = `[1,3,5,6,8]` at ranks 1-5, not the module doc's rounded "1/3/5/6/8%" prose) adds a
flat 5% of Spirit to the *universal* `SpellDamage` stat (`src/classes/priest/prepare/talents/
holy.rs::apply_spiritual_guidance`: `sdm.add_stat_dependency(Stat::Spirit, Stat::SpellDamage,
damage)` - unlike Healing Power, this is not Holy-school-scoped). This build's own real
`SpellDamage: 509.85` minus `Spirit: 237.0 * 0.05 = 11.85` equals `498.0` *exactly* - this
project's own pre-existing gear-computed total. The previously-"unexplained, still open"
509.9-vs-498 SP residual from the first pass above was this talent, fully resolved, not a
separate mystery. Implemented as `SmiteTalents::spiritual_guidance` + `Fight::stats.spell_power
+= spirit * curve[points]` in `priest/smite.rs::apply_talents`.

**Judgement of the Crusader, implemented - the real cause of the dominant 23% `BaseDamage`
shortfall three prior passes had each independently ruled out candidates for and left open.**
This fixture's own `raid.debuffs.judgementOfTheCrusader: true` flag had been found twice before
(this file's first pass, and again above) and dismissed both times on the assumption it was the
classic-WoW crit-chance debuff shape - never actually read against source. It isn't: the real
engine's own `src/prepare/buffs/paladin.rs::judgement_of_the_crusader_aura` sets `sim.unit_mut
(target).pseudo_stats.school_bonus_spell_damage[holy] += bonus`, a flat **damage-taken** bonus
on the target (`A_MOD_DAMAGE_TAKEN`, not a crit mechanic at all), always resolved at its own max
rank (`JUDGEMENT_OF_THE_CRUSADER_SPELL = 20303`, Rank 6, `BasePoints: 161`, `Misc: 2` = Holy -
read directly from `data/spells.jsonl`). Since this project only ever simulates one attacker
against one target, "the target takes +161 more Holy damage" and "the caster deals +161 more
effective Holy spell power" are the same number - modeled as a flat `bonus_holy_power`
addition (`JUDGEMENT_OF_THE_CRUSADER_HOLY_POWER` in `gear.rs`), the exact same shape every other
school-scoped bonus this project already has. **Confirmed against the real trace precisely**: a
real Smite cast shows `SP: 509.9, BaseDamage: 655.6` (and several more samples clustering
649.0-663.8); this formula gives `170 + 0.714 * (509.9 + 161) = 649.6` - matching to within this
spell's own small `Variance: 0.112` roll, with zero other factor needed. Applied unconditionally
for every Priest build in `gear.rs` (confirmed absent from Shadow Priest's own fixture's
`raid.debuffs` block, but harmless there since Shadow never reads `bonus_holy_power`) rather
than threading a new per-spec flag through `equipment_to_stats`, the same reasoning the Healing
Power fix already used.

**Updated per-ability trace** (20000-iteration average, `--buffs builder-starter`):

| Ability | Real DPS | This engine's DPS | Ratio |
|---|---|---|---|
| Smite (both ranks) | 188.7 | 97.8 | 0.52x |
| Holy Fire | 113.4 | 112.7 | **0.99x** |
| Shadow Word: Pain | 84.6 | 67.2 | 0.79x |
| Penance | 73.5 | 58.5 | 0.80x |
| Devouring Plague | 47.9 | 43.1 | 0.90x |
| **Total** | **513.5** | **379.3** | **0.74x** |

0.65x → **0.74x** - the biggest single jump either Priest spec has seen. Holy Fire moved to an
essentially exact match (0.99x), confirming the `BaseDamage` formula itself (base + coefficient
* (SP + bonus_holy_power), now including both of today's fixes) is correct. **Smite itself
(0.52x) is still the one named laggard** - its own per-cast damage average (607/cast) is now
much closer to real's (816/cast, ratio 0.74x) than before, but its own *cast count* is also
still short (19.3/120s here vs real's combined full+downrank 27.75/120s, ratio 0.70x) in a way
Holy Fire's own cast count isn't - both factors multiply to the observed 0.52x. Unlike Holy
Fire, Smite is the one ability gated by the real priority list's own explicit mana-pacing
throttle (`currentManaPercent >= remainingTimePercent * 0.9`, switching to the cheap downranked
copy once it fails) - the next concrete lead, if chased, is whether this project's own
implementation of that exact condition reproduces the real rank-switch cadence, not a new
per-hit damage mechanic (Holy Fire's own near-exact ratio already rules that class of bug out
for the shared formula).

**Web research for this pass** (explicitly requested): same conclusion as the Mage passes'
own research - `sage3648/mythicsim-forever-engine` (this project's ground truth) is itself a
checked community port of the official `wowsims/forever` Go engine, and no additional
generic-simulator pitfall class beyond what direct source-reading already found (missing raid
debuffs, mis-attributed mechanics, stale "not found" notes never re-checked) turned up this
pass either. The two real fixes above were found by re-reading this project's *own* prior,
already-collected evidence (the fixture's own `raid.debuffs` flag, the real `BaseDamage` log
line) against the real engine's actual source for the first time, not by a new external lead -
a reminder that "found but not yet acted on" items from earlier passes are themselves a
productive place to look before reaching for a new technique.

## Dark Sacrifice, the mana economy's own biggest missing piece (2026-10-26, same day)

See `research-notes/priest-shadow-comparison.md`'s own matching section for the full
derivation, citation, and the Mage/Warlock cross-check - the finding and fix are identical for
both specs (a generic engine primitive, `Fight::maybe_use_headroom_gated_mana_cooldown`, wired
into each spec's own `step()`). Summary for this spec: the Undead racial grants `1600 + Spirit`
mana once per fight the moment the full amount would fit without overflow (confirmed exactly:
`Spirit: 237` → `1837.0` gained in the real trace, matching to a tenth of a mana point) -
previously unmodeled because it's a racial, not a talent, buff-package item, or gear stat, a
category this project had only modeled passively before.

**Result: 379.3 → 395.1 DPS (0.74x → 0.77x of 513.5)** - the real mana-pacing rank-switch
condition itself (`currentManaPercent >= remainingTimePercent * 0.9`, re-verified against the
real priority list's own raw JSON node tree rather than the prose paraphrase, and confirmed
exactly correct) now has more headroom to work with, and Devouring Plague/Shadow Word: Pain's
own cast counts both rose slightly as a direct result. Smite's own ability-level ratio (0.52x)
remains the one named laggard - not a mana-pacing bug (full-rank and downranked Smite have
similar cast times, so which rank fires barely moves total throughput), the same project-wide
cast-frequency shortfall every ability shows to some degree, now partly closed.
