// Shared between web/talents.html and web/index.html: the single source of
// truth for (a) which classes can be run through the real engine at all,
// (b) which talents (if any) actually drive a BuildConfig field, and (c)
// the localStorage-backed saved-build store, so a build saved on the
// talent calculator is loadable from the simulator's build selector and
// vice versa -- one shared module instead of two copies that could drift.

export const STORAGE_KEY = 'wowforever_talent_builds';

// The single place both pages get the production data-pack path from --
// previously each page hardcoded its own copy of this string, and it went
// stale (still pointing at an older revision) twice across this project's
// history once a new revision shipped. Bump this one line when
// DEFAULT_DATA_PACK changes in crates/cli/src/main.rs.
export const DATA_PACK_PATH = '/data-packs/forever-beta-2026-10-17-rev14.json';

export const CLASS_LABELS = {
  warlock: 'Warlock', mage: 'Mage', priest: 'Priest', druid: 'Druid', shaman: 'Shaman',
  paladin: 'Paladin', hunter: 'Hunter', rogue: 'Rogue', warrior: 'Warrior',
};
export const CLASS_ICONS = {
  warlock: 'class_warlock', mage: 'class_mage', priest: 'class_priest', druid: 'class_druid',
  shaman: 'class_shaman', paladin: 'class_paladin', hunter: 'class_hunter', rogue: 'class_rogue',
  warrior: 'class_warrior',
};

// `mode`: 'dps' | 'heal' | null (null = no rotation exists with enough
// sourced spell data to run at all -- stated honestly in the UI, not
// hidden). `talentEffects` maps a talent id to the BuildConfig field it
// drives; a class with no entries has zero talents wired to the engine
// yet, which both pages say explicitly rather than implying otherwise.
// `hasBurst`: whether a burst-calculator build can also be derived (only
// Warlock has a sourced burst rotation so far).
// `melee`: present only for attack-power/weapon-damage classes (Hunter/
// Rogue/Warrior). These three deal zero spell-power-scaling damage in
// Classic-era itemization, so `rotation` is empty -- no special ability
// has a sourced attack-power coefficient yet -- and the only real damage
// modeled is auto-attack, on its own independent weapon-speed timer (see
// `wowforever_engine::BuildConfig::weapon_speed_ms`'s doc comment). The
// attack_power/weapon stats are hand-entered build-configuration inputs,
// exactly like `spell_power: 350.0` already is for every caster class --
// illustrative, not sourced game facts.
export const CLASS_SIM = {
  // REV14: added Shadowburn to the main DPS rotation (not just the burst
  // calculator, where it already lived) -- it was already a real, sourced
  // beta-datamine spell in the pack (15s cooldown, flat 266 + 0.429
  // coefficient Shadow nuke) sitting unused outside bursts. Real Shadowburn
  // is a Classic-era execute (bonus/guaranteed-crit only below ~20% target
  // health), which this engine can't gate on since it has no target-health
  // tracking (same stated gap as Warrior's Execute) -- modeled here as a
  // plain cooldown nuke instead, reusing the exact simplification the
  // burst calculator already made rather than inventing a new one.
  warlock: {
    mode: 'dps', rotation: ['shadowburn', 'corruption', 'immolate', 'shadow_bolt'], hasBurst: true,
    talentEffects: { demonic_sacrifice: 'demonic_sacrifice_active', demonic_pact: 'has_demonic_pact' },
  },
  // REV11: a real Arcane Mage kit -- Arcane Power (burst cooldown) >
  // Arcane Blast (stacking buff, build-and-spend identity) > Arcane
  // Missiles (channeled "spend," no own direct hit -- see its dot-only
  // SpellDef) > Arcane Wrath (mana-scaling nuke) > Frostbolt (real,
  // sourced filler -- not Fireball, which real Arcane Mages don't cast).
  // Missile Barrage, Clearcasting, and Presence of Mind are deliberately
  // not modeled -- they're true RNG-driven procs/conversions that would
  // need this engine's deterministic build_schedule to become
  // probabilistic, a materially bigger investment (see docs/PLAN.md's
  // Arcane Mage update for the full reasoning).
  // Recalibrated 2026-10-18 (same pass as `crates/cli`'s `mage_demo_build` --
  // see that function's doc comment for the full root-cause writeup): fixed
  // two real engine bugs (exponential instead of additive per-stack mana
  // cost, and no "stop rebuilding a maxed buff" condition), reordered
  // Arcane Wrath above the cooldown-free Arcane Missiles so a 6s cooldown
  // doesn't get starved out, and raised maxMana/manaRegenPerSecond from
  // 6000/25 to a less mana-starved, still-genuinely-constrained pair.
  mage: {
    mode: 'dps', rotation: ['arcane_power', 'arcane_blast', 'arcane_wrath', 'arcane_missiles', 'frostbolt'],
    hasBurst: false, talentEffects: {}, maxMana: 10000.0, manaRegenPerSecond: 40.0,
    note: 'Arcane Power, Arcane Blast (stacking), Arcane Wrath, Arcane Missiles, and Frostbolt, with mana a real enforced resource. Missile Barrage, Clearcasting, and Presence of Mind are not modeled -- they are true random procs that would require this engine’s deterministic scheduling to become probabilistic, a bigger investment than this pass.',
  },
  // REV14: added Greater Heal -- it was already a real, sourced
  // beta-datamine spell in the pack since REV5, just never wired into a
  // CLASS_SIM rotation. Ordered after Renew (HoT uptime always comes first
  // in this engine's greedy priority model) and before Flash Heal: Greater
  // Heal is the bigger, slower heal-for-more spell real Priests lean on
  // as their default cast, with Flash Heal as the fast-cast filler/top-up.
  priest: { mode: 'heal', rotation: ['renew', 'greater_heal', 'flash_heal'], hasBurst: false, talentEffects: {} },
  druid: { mode: 'dps', rotation: ['moonfire', 'wrath'], hasBurst: false, talentEffects: {} },
  shaman: { mode: 'dps', rotation: ['lightning_bolt'], hasBurst: false, talentEffects: {} },
  paladin: { mode: 'heal', rotation: ['holy_light'], hasBurst: false, talentEffects: {} },
  // Recalibrated 2026-10-18 after the original numbers (AP 400-500, modest
  // weapon damage) produced DPS in the low 40s-60s -- technically correct
  // for those inputs, but an unrealistically *low* illustrative baseline:
  // a raid-buffed Classic-era level 60 character routinely reaches
  // ~1000-2000+ attack power (web-research cross-check: AQ40-geared
  // warriors averaged ~499 *total* DPS including abilities; ~2000 AP
  // buffed is cited as a realistic well-geared breakpoint). These three
  // classes' *auto-attack-only* DPS should still read as honestly lower
  // than the caster demo builds (real melee/ranged output leans heavily
  // on special abilities this engine hasn't modeled yet -- that's a real
  // game-design fact, not a bug to hide), just not implausibly so.
  // REV14: Hunter's first two real special attacks, sourced directly from
  // thewowdb.com (genuine beta-datamine, see each spell's own source field
  // in the data pack): Raptor Strike (melee weapon-damage + flat 30, 6s
  // cooldown, mana-costed) and Arcane Shot (flat 65 Arcane nuke, 6s
  // cooldown). Both modeled as normal GCD-priority rotation entries, not
  // through the engine's off-GCD heroic_strike_spell_id mechanism, even
  // though Raptor Strike is a real off-GCD ability in-game -- that
  // mechanism's affordability check only gates on Rage, and would let a
  // mana-costed ability fire for free on this non-rage build, a real
  // accuracy bug rather than a simplification worth taking. Still auto-
  // attack-plus-two-specials, not a full Hunter kit (no Multi-Shot,
  // Serpent Sting, Aimed Shot, etc. sourced yet) -- a real partial
  // picture, not the full rotation.
  hunter: {
    mode: 'dps', rotation: ['raptor_strike', 'arcane_shot'], hasBurst: false, talentEffects: {},
    melee: { attackPower: 850.0, weaponMin: 50.0, weaponMax: 90.0, weaponSpeedMs: 2800 },
    note: 'Auto-attack (Auto Shot), Raptor Strike, and Arcane Shot -- Raptor Strike (weapon damage + 30, mana-costed) and Arcane Shot (flat 65 Arcane, no attack-power scaling found in its client data) are both real, sourced 6s-cooldown specials, not the rest of the Hunter kit. Real Hunters lean on more shots than these two for total DPS, so this is a real, honest partial picture, not the full picture.',
  },
  rogue: {
    mode: 'dps', rotation: [], hasBurst: false, talentEffects: {},
    melee: { attackPower: 900.0, weaponMin: 35.0, weaponMax: 65.0, weaponSpeedMs: 1700 },
    note: 'Auto-attack only -- no Rogue special ability has a sourced attack-power coefficient yet. Real Rogues lean heavily on combo-point finishers for total DPS, so this is a real, honest floor, not the full picture.',
  },
  // Warrior REV10: a real rage-gated rotation -- Rend (bleed uptime) and
  // Bloodthirst (6s cooldown) in GCD priority, Whirlwind as a GCD
  // filler, plus Heroic Strike firing independently of the GCD via
  // `rageEnabled`/`isTwoHanded`/`heroicStrikeSpellId` (see
  // `wowforever_engine::build_schedule`'s doc comment for the rage
  // system and the swing-queue simplification). Execute is deliberately
  // not modeled -- it needs target-health-over-time tracking this engine
  // doesn't have yet (see docs/PLAN.md's twenty-seventh update).
  warrior: {
    mode: 'dps', rotation: ['rend', 'bloodthirst', 'whirlwind'], hasBurst: false, talentEffects: {},
    melee: {
      attackPower: 1000.0, weaponMin: 80.0, weaponMax: 140.0, weaponSpeedMs: 3300,
      rageEnabled: true, isTwoHanded: true, heroicStrikeSpellId: 'heroic_strike',
    },
    note: 'Auto-attack, Bloodthirst, Whirlwind, Heroic Strike, and Rend, with a real rage resource gating them (WoW Forever reworked rage generation to scale with weapon speed, not damage dealt). Execute is not modeled -- it needs target-health tracking this engine doesn’t have. Heroic Strike is simplified: rage-gated and GCD-exempt, but not literally tied to replacing one specific future swing the way the real ability is.',
  },
};

export function getSavedBuilds() {
  return JSON.parse(localStorage.getItem(STORAGE_KEY) || '[]');
}
export function setSavedBuilds(list) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
}

/** Demo (no-talent, or showcase-talent for Warlock) allocation used by the
 * simulator's own "<Class> -- demo build" option -- not a saved build. */
export const DEMO_ALLOCATION = {
  warlock: { demonic_sacrifice: 1, demonic_pact: 1 }, // matches this project's long-standing Warlock demo
  mage: {}, priest: {}, druid: {}, shaman: {}, paladin: {}, hunter: {}, rogue: {}, warrior: {},
};

/** Builds a DPS/heal BuildConfig for the wasm engine from a class id and a
 * talent allocation (talent id -> rank). Returns null if this class has no
 * sim at all (see CLASS_SIM[classId].reason for why). */
export function buildSimConfig(classId, allocation) {
  const sim = CLASS_SIM[classId];
  if (!sim || !sim.mode) return null;
  const build = sim.mode === 'heal'
    ? { spell_power: 350.0, crit_chance: 0.2, fight_duration_ms: 60000, rotation: sim.rotation }
    : {
        spell_power: sim.melee ? 0.0 : 350.0, crit_chance: 0.2, fight_duration_ms: 60000, rotation: sim.rotation, pet_type: 'imp',
        demonic_sacrifice_active: false, has_demonic_pact: false,
        resummon_event_ms: null, resummon_is_same_pet: false, dispute_rule_override: null,
        // A class opts into a real, enforced mana pool by setting
        // `maxMana` on its CLASS_SIM entry (e.g. Mage) -- `null`/`0.0`
        // (every other caster class so far) preserves the original
        // unconstrained behavior exactly.
        starting_mana: sim.maxMana ? sim.maxMana : null,
        mana_regen_per_second: sim.manaRegenPerSecond || 0.0,
        max_mana: sim.maxMana || 0.0,
        attack_power: sim.melee ? sim.melee.attackPower : 0.0,
        weapon_min: sim.melee ? sim.melee.weaponMin : 0.0,
        weapon_max: sim.melee ? sim.melee.weaponMax : 0.0,
        weapon_speed_ms: sim.melee ? sim.melee.weaponSpeedMs : 0,
        rage_enabled: sim.melee ? !!sim.melee.rageEnabled : false,
        is_two_handed: sim.melee ? !!sim.melee.isTwoHanded : false,
        heroic_strike_spell_id: sim.melee ? (sim.melee.heroicStrikeSpellId || null) : null,
      };
  for (const [talentId, field] of Object.entries(sim.talentEffects)) {
    build[field] = (allocation[talentId] || 0) > 0;
  }
  // Warlock-only: exercise the resummon/dispute mechanic demo whenever Demonic Sacrifice is active,
  // matching this project's long-standing CLI/golden-test demo build shape rather than leaving it inert.
  if (classId === 'warlock' && build.demonic_sacrifice_active) {
    build.resummon_event_ms = 30000;
    build.resummon_is_same_pet = true;
  }
  return build;
}

/** Builds a burst-calculator BurstBuildConfig, or null if this class has no
 * sourced burst rotation (only Warlock does so far). */
export function buildBurstConfig(classId, allocation) {
  if (classId !== 'warlock') return null;
  const dps = buildSimConfig(classId, allocation);
  return {
    spell_power: 350.0, crit_chance: 0.2, window_ms: 15000,
    available_spell_ids: ['shadowburn', 'immolate', 'corruption', 'shadow_bolt'], pet_type: 'imp',
    demonic_sacrifice_active: dps.demonic_sacrifice_active,
    opponent: {
      name: 'Generic PvP target, no resistance gear', shadow_resist_pct: 0, fire_resist_pct: 0,
      physical_resist_pct: 0, frost_resist_pct: 0, arcane_resist_pct: 0, holy_resist_pct: 0, nature_resist_pct: 0,
    },
    opener: { starting_mana: 2500.0, opened_from_stealth_or_cc: false },
    search_beam_width: 500,
  };
}

// --- Result-display helpers, shared by index.html and talents.html so ---
// --- their Overview/Resources panels can never drift out of sync the ---
// --- way DATA_PACK_PATH once did between the two pages. ---

/** The Monte Carlo sample count every DPS/HPS run in this project uses
 * (hardcoded at both call sites already, see runDps()/runSimBtn's click
 * handler) -- centralized here so marginOfError() derives a real standard
 * error from it rather than a second hardcoded copy. */
export const MC_ITERATIONS = 10_000;

/** A 95%-confidence half-width on the *mean* DPS/HPS estimate itself
 * (standard error of the mean, not the full run-to-run spread that
 * `stdev`/`p5`/`p95` already describe) -- distinct information: with
 * 10,000 iterations this is normally small even when `stdev` (real
 * fight-to-fight variance from crit RNG) is not, and that's the correct,
 * expected relationship, not a contradiction. */
export function marginOfError(result) {
  const stderr = result.stdev / Math.sqrt(MC_ITERATIONS);
  const margin = 1.96 * stderr;
  const pct = result.mean > 0 ? (margin / result.mean) * 100 : 0;
  return { margin, pct };
}

/** Formats a short human-readable number the way mythicsim's stat row
 * does (e.g. 1.12k instead of 1120) -- only abbreviates at 1000+, keeps
 * full precision below that since this project's numbers rarely reach
 * four digits and false-precision at small scale is worse than a few
 * extra characters. */
export function formatCompact(n) {
  if (Math.abs(n) >= 1000) return (n / 1000).toFixed(2).replace(/\.00$/, '') + 'k';
  return n.toFixed(1);
}

/** One line of real, data-derived (never hardcoded-per-class) rotation
 * documentation: why a given rotation entry sits where it does, inferred
 * from the spell's own sourced fields rather than asserted as a game
 * fact this project doesn't actually know. */
export function priorityHint(spell) {
  if (!spell) return 'unknown spell';
  if (spell.timed_buff) return `burst cooldown -- activates a ${(spell.timed_buff.duration_ms / 1000)}s, +${(spell.timed_buff.damage_pct * 100).toFixed(0)}% damage buff on cast (+${(spell.timed_buff.cost_pct * 100).toFixed(0)}% cost while active), ${(spell.cooldown_ms / 1000)}s cooldown`;
  if (spell.stacking_buff) return `build stacks -- each cast adds a stack (up to ${spell.stacking_buff.max_stacks}), boosting other spells' damage by ${(spell.stacking_buff.other_damage_pct_per_stack * 100).toFixed(0)}%/stack for ${(spell.stacking_buff.duration_ms / 1000)}s; its own cost rises per stack`;
  if (spell.dot) return `maintain uptime -- recast once its ${(spell.dot.tick_interval_ms * spell.dot.num_ticks / 1000)}s effect expires`;
  if (spell.cooldown_ms > 0) return `cast on cooldown (${(spell.cooldown_ms / 1000)}s)`;
  return 'filler -- cast whenever nothing higher in this list is ready';
}

/** Describes a class's rotation as an ordered, documented priority list
 * plus any off-GCD ability (Heroic Strike today) -- the plain-English
 * answer to "what does this bot actually do," which `CLASS_SIM`'s
 * `rotation` array alone doesn't convey. Returns `null` for a
 * non-runnable class. */
export function describeRotation(sim, dataPack) {
  if (!sim || !sim.mode) return null;
  const bySpell = {};
  for (const s of dataPack.spells) bySpell[s.id] = s;
  const nameOf = (id) => bySpell[id]?.name || id.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
  const priority = sim.rotation.map((id, i) => ({ rank: i + 1, id, name: nameOf(id), hint: priorityHint(bySpell[id]) }));
  const offGcd = sim.melee && sim.melee.heroicStrikeSpellId
    ? { id: sim.melee.heroicStrikeSpellId, name: nameOf(sim.melee.heroicStrikeSpellId) }
    : null;
  return { priority, offGcd, hasMelee: !!sim.melee };
}

/** Resource (mana/rage) spent per ability over one representative run --
 * computed entirely from data the frontend already has (`detail.abilities`'
 * cast counts, each spell's own `resource_cost`/`resource_type`), no
 * engine change needed. Deliberately scoped to *spent*, not a full
 * gained-vs-spent economy (that needs the rage-regen formula exposed from
 * `crates/engine`, which isn't done this pass -- see `renderResources`'
 * caller for the honest caveat shown alongside this). Auto-attack and DoT
 * ticks never have their own resource cost (only the initiating cast
 * does, already counted once as a `direct` event), so only `kind ===
 * 'direct'` abilities are considered. Returns a map of resource type ->
 * `{ total, byAbility: [{id, name, amount}] }`, sorted by spend
 * descending within each resource type. */
export function resourceSpentBreakdown(dataPack, abilities) {
  const bySpell = {};
  for (const s of dataPack.spells) bySpell[s.id] = s;
  const totals = {};
  for (const ab of abilities) {
    if (ab.kind !== 'direct') continue;
    const spell = bySpell[ab.source_id];
    if (!spell || !spell.resource_cost) continue;
    const type = spell.resource_type || 'mana';
    const amount = spell.resource_cost * ab.casts;
    if (!totals[type]) totals[type] = { total: 0, byAbility: [] };
    totals[type].total += amount;
    totals[type].byAbility.push({ id: ab.source_id, name: spell.name, amount });
  }
  for (const t of Object.values(totals)) t.byAbility.sort((a, b) => b.amount - a.amount);
  return totals;
}
