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
export const DATA_PACK_PATH = '/data-packs/forever-beta-2026-10-17-rev9.json';

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
  warlock: {
    mode: 'dps', rotation: ['corruption', 'immolate', 'shadow_bolt'], hasBurst: true,
    talentEffects: { demonic_sacrifice: 'demonic_sacrifice_active', demonic_pact: 'has_demonic_pact' },
  },
  mage: { mode: 'dps', rotation: ['fireball', 'frostbolt'], hasBurst: false, talentEffects: {} },
  priest: { mode: 'heal', rotation: ['renew', 'flash_heal'], hasBurst: false, talentEffects: {} },
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
  hunter: {
    mode: 'dps', rotation: [], hasBurst: false, talentEffects: {},
    melee: { attackPower: 850.0, weaponMin: 50.0, weaponMax: 90.0, weaponSpeedMs: 2800 },
    note: 'Auto-attack (Auto Shot) only -- no Hunter special ability has a sourced attack-power coefficient yet. Real Hunters lean heavily on special shots for total DPS, so this is a real, honest floor, not the full picture.',
  },
  rogue: {
    mode: 'dps', rotation: [], hasBurst: false, talentEffects: {},
    melee: { attackPower: 900.0, weaponMin: 35.0, weaponMax: 65.0, weaponSpeedMs: 1700 },
    note: 'Auto-attack only -- no Rogue special ability has a sourced attack-power coefficient yet. Real Rogues lean heavily on combo-point finishers for total DPS, so this is a real, honest floor, not the full picture.',
  },
  // Warrior gained a real special ability (Bloodthirst, REV9: 45% Attack
  // Power + 48 flat, instant, 6s cooldown -- see data-packs/
  // forever-beta-2026-10-17-rev9.json's source note) on top of
  // auto-attack. Rage is recorded in the data but not enforced -- this
  // project models mana/resource constraints for casters only so far,
  // not a rage system.
  warrior: {
    mode: 'dps', rotation: ['bloodthirst'], hasBurst: false, talentEffects: {},
    melee: { attackPower: 1000.0, weaponMin: 80.0, weaponMax: 140.0, weaponSpeedMs: 3300 },
    note: 'Auto-attack + Bloodthirst only -- no other Warrior special ability has a sourced attack-power coefficient yet. Real Warriors lean on more rage-spending abilities than just this one for total DPS, so this is a real, honest partial picture, not the full rotation.',
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
        starting_mana: null, mana_regen_per_second: 0.0,
        attack_power: sim.melee ? sim.melee.attackPower : 0.0,
        weapon_min: sim.melee ? sim.melee.weaponMin : 0.0,
        weapon_max: sim.melee ? sim.melee.weaponMax : 0.0,
        weapon_speed_ms: sim.melee ? sim.melee.weaponSpeedMs : 0,
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
