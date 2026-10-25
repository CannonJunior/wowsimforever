# mythic-forever-sim

See `README.md` for the project's actual status, scope, and how to build/run, and
`research-notes/` for how every rotation and number in it was sourced.

## Hard rule: every new class/spec gets a real per-ability comparison against the engine's own trace before it's considered done

Decided 2026-10-25, after Fire Mage's second implementation pass (modeling Ignite, Heating
Up/Pyroblast, Combustion and Clearcasting) shipped without ever actually tracing its output
against the real engine ability-by-ability - only Warlock's three specs had been checked that
closely. Asked explicitly to do that check for Fire Mage, it immediately found three real bugs
a plain "does the total DPS look plausible" pass had missed entirely:

- A talent point this project's own decode had wrong (Elemental Precision: this project assumed
  4 ranks; the real engine's own talent-string validator says 3 - the UI metadata this project
  decoded the talent string against was simply wrong for this one talent).
- `--buffs builder-starter` was never passed for the Fire Mage comparison at all - the same
  mistake the very first Warlock comparison made, repeated because nobody had checked.
- Pyroblast's own dot was never applied - the exact same class of bug Fireball had in Fire
  Mage's *first* pass, re-introduced when Pyroblast was added in the second, because nothing
  compared its per-ability output against a ground truth that would have caught it.

None of these three were visible from the aggregate DPS number alone - each needed an
ability-by-ability trace from the real engine to surface. This is exactly the pattern Warlock's
own history already established (see `research-notes/mythicsim-comparison.md`'s own five
passes): a plausible-looking total hides real, specific, fixable bugs that only a per-ability
comparison exposes.

**Going forward, for every class and every spec this project implements or substantially
changes:**

1. Build `mythicsim-forever-engine`'s own Rust CLI locally (MIT-licensed; check
   `/tmp/claude-1000/*/scratchpad/` first in case a prior session's clone is still there before
   re-cloning and re-building from scratch) and run it with `--rotation-stats` against the real
   `validation/2026-10-04-builder-starters/<scenario>--<race>.request.json` fixture for that
   spec - not an estimate, not "the aggregate DPS is within Nx of the archived corpus number so
   it's probably fine."
2. Extract the real engine's own per-ability damage breakdown from the result JSON's
   `raidMetrics.parties[].players[].actions` (and `.pets[].actions` if the spec has one).
   **Use the `damage` field alone** - it is already the combined total (ticks/hits plus
   crits), not a sum to build from `damage + tickDamage + critDamage + critTickDamage`
   (a real mistake this project made once already on the Warlock side and caught by the
   per-ability totals not summing back up to the engine's own reported aggregate - always
   re-check that the per-ability rows you extract sum back to `raidMetrics.dps.avg`, every
   time, before trusting any single row).
3. Resolve every spell/aura ID in the breakdown via `foreverdb.net/spell/<id>` (robots.txt
   already checked and cleared for ClaudeBot) so the comparison is readable, not a table of
   bare numeric IDs.
4. Run this engine's own CLI with the *matching* flags (gear, race, talents, `--buffs
   builder-starter` or the right class-specific preset, any pet/sacrifice/etc. flags that
   spec needs) and build a side-by-side per-ability table, the same shape as
   `research-notes/mythicsim-comparison.md`'s own tables.
5. **Every real, named discrepancy the table turns up gets fixed if it's a bug (a missing dot,
   a wrong talent cap, a missing buff), or explicitly documented as a found-but-open gap if
   it's a deeper, not-yet-understood difference (e.g. `mage-fire-comparison.md`'s own
   71-point effective-spell-power gap).** Never stop at "the total looks close enough" without
   having actually looked at the per-ability rows at least once for that spec.
6. Write the result up as `research-notes/<class>-<spec>-comparison.md` (or fold it into an
   existing class-wide comparison doc, matching Warlock's own `mythicsim-comparison.md`
   covering all three specs in one file) so the next session doesn't have to re-derive what was
   already found.

This is not a one-time Warlock-specific or Fire-Mage-specific step - it is the acceptance bar
for every future class/spec this project adds (Arcane/Frost/Frostfire Mage next, then whichever
of Warrior/Rogue/Druid/Shaman/Hunter/Paladin/Priest comes after). A spec that "runs without
hanging and produces a plausible-looking DPS number" is not done; a spec that's been compared
ability-by-ability against the real engine's own trace, with every resulting discrepancy either
fixed or named, is.
