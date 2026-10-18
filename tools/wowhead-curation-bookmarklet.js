/**
 * WoW Forever Spell Curation Bookmarklet
 *
 * Why this exists: automated fetching of Wowhead pages (direct HTTP, or a
 * fetch-and-summarize tool) is unreliable for this project -- confirmed
 * during the 2026-10-17 data-sourcing investigation (see docs/PLAN.md).
 * Direct requests get blocked (403), and even successful fetches were
 * found to silently collapse real min/max damage ranges into a single
 * value, and in one case (Immolate) returned a figure wrong by roughly an
 * order of magnitude. A human looking at the real, rendered page in their
 * own browser does not have either problem -- so this tool assists manual
 * curation instead of attempting automated extraction.
 *
 * What it does: when run on a Wowhead spell page (e.g.
 * https://www.wowhead.com/forever/spell=17923/searing-pain), it best-effort
 * scans the page's own rendered text for damage/cooldown/cost patterns,
 * opens an editable form pre-filled with its guesses, and lets you correct
 * anything before generating a JSON object in exactly the shape this
 * project's `SpellDef` type expects (see crates/data/src/lib.rs). Nothing
 * is auto-submitted anywhere -- you copy or download the JSON yourself and
 * paste it into a data pack after review. Confidence defaults to
 * `community-reported` (per CLAUDE.md's "Data sourcing" section: Wowhead
 * is a third-party site regardless of who's reading it, never
 * `beta-datamine`, which is reserved for genuine client-data extraction).
 *
 * Install: see tools/README.md for how to turn this file into a bookmark.
 * Usage: open a Wowhead spell page, click the bookmarklet. Click it again
 * (or click Close) to dismiss the panel.
 */
(function () {
  'use strict';

  var EXISTING = document.getElementById('wfcur-overlay');
  if (EXISTING) {
    EXISTING.remove();
    return;
  }

  // innerText reflects rendered/visible text (what a human actually sees)
  // and is what real browsers populate correctly; textContent is a
  // fallback for any environment where innerText comes back empty.
  var pageText = document.body.innerText || document.body.textContent || '';
  var SCHOOLS = ['Shadow', 'Fire', 'Physical', 'Frost', 'Arcane', 'Nature', 'Holy'];
  var schoolAlt = SCHOOLS.join('|');

  function toNum(str) {
    return parseFloat(String(str).replace(/,/g, ''));
  }

  // --- Best-effort extraction from the page's own rendered text ---

  var guess = {
    school: '',
    baseMin: '',
    baseMax: '',
    coefficient: '',
    castTimeMs: 0,
    cooldownMs: 0,
    resourceCost: 0,
    hasDot: false,
    tickIntervalMs: 3000,
    numTicks: 0,
    tickBase: '',
    tickCoefficient: '',
    rangeFound: false
  };

  // Collects every match of `re` in `text`, not just the first -- a spell
  // page showing a full rank table (Rank 1..max) can contain this pattern
  // more than once, and silently taking the first match risks grabbing a
  // low rank's numbers instead of the max-rank/level-60 ones, with no
  // visible sign anything went wrong. See `guess.rangeMatchCount` below.
  function findAll(re, text) {
    var g = new RegExp(re.source, re.flags.indexOf('g') === -1 ? re.flags + 'g' : re.flags);
    var out = [];
    var mm;
    while ((mm = g.exec(text)) !== null) {
      out.push(mm);
      if (mm.index === g.lastIndex) g.lastIndex++;
    }
    return out;
  }

  // Narrative range, e.g. "blasts the target for 65 to 74 Shadow damage"
  var rangeMatches = findAll(new RegExp('for\\s+([\\d,]+)\\s+to\\s+([\\d,]+)\\s+(' + schoolAlt + ')\\s+damage', 'i'), pageText);
  guess.rangeMatchCount = rangeMatches.length;
  if (rangeMatches.length > 0) {
    guess.baseMin = toNum(rangeMatches[0][1]);
    guess.baseMax = toNum(rangeMatches[0][2]);
    guess.school = rangeMatches[0][3].toLowerCase();
    guess.rangeFound = true;
  }

  // Structured single value, e.g. "School Damage (Fire) Value: 115 (SP mod: 0.429)"
  var schoolValMatches = findAll(new RegExp('School Damage \\((' + schoolAlt + ')\\)\\s*Value:\\s*([\\d,]+)', 'i'), pageText);
  guess.schoolValMatchCount = schoolValMatches.length;
  if (schoolValMatches.length > 0) {
    if (!guess.school) guess.school = schoolValMatches[0][1].toLowerCase();
    if (!guess.rangeFound) {
      guess.baseMin = guess.baseMax = toNum(schoolValMatches[0][2]);
    }
  }

  var mCoeff = pageText.match(/SP mod:\s*([\d.]+)/i);
  if (mCoeff) guess.coefficient = parseFloat(mCoeff[1]);

  var mCooldown = pageText.match(/Cooldown:\s*([\d.]+)\s*sec/i);
  if (mCooldown) guess.cooldownMs = Math.round(parseFloat(mCooldown[1]) * 1000);

  if (/Cast [Tt]ime:\s*Instant/.test(pageText)) {
    guess.castTimeMs = 0;
  } else {
    var mCast = pageText.match(/Cast [Tt]ime:\s*([\d.]+)\s*sec/i);
    if (mCast) guess.castTimeMs = Math.round(parseFloat(mCast[1]) * 1000);
  }

  var mMana = pageText.match(/Mana Cost:\s*([\d,]+)/i);
  if (mMana) guess.resourceCost = toNum(mMana[1]);

  // DoT guess, e.g. "an additional 275 Fire damage over 15 sec"
  var mDot = pageText.match(new RegExp('additional\\s+([\\d,]+)\\s+(?:' + schoolAlt + ')\\s+damage over\\s+([\\d.]+)\\s*sec', 'i'));
  if (mDot) {
    guess.hasDot = true;
    var totalDot = toNum(mDot[1]);
    var durationSec = parseFloat(mDot[2]);
    guess.numTicks = Math.max(1, Math.round(durationSec / 3));
    guess.tickBase = Math.round((totalDot / guess.numTicks) * 100) / 100;
  }

  // Id / name guesses from the URL and page title.
  var urlMatch = location.href.match(/spell=(\d+)\/([a-z0-9-]+)/i);
  var slugGuess = urlMatch ? urlMatch[2].replace(/-/g, '_') : '';
  var titleGuess = (document.title || '').replace(/\s*-\s*Spell\s*-\s*Forever.*$/i, '').trim();

  // --- Build the overlay UI ---

  var overlay = document.createElement('div');
  overlay.id = 'wfcur-overlay';
  overlay.style.cssText =
    'position:fixed;top:0;right:0;width:380px;height:100%;background:#1b1b22;color:#e8e8ef;' +
    'z-index:2147483647;overflow-y:auto;font-family:-apple-system,Segoe UI,Roboto,sans-serif;' +
    'font-size:12px;line-height:1.4;box-shadow:-6px 0 24px rgba(0,0,0,.6);padding:14px;box-sizing:border-box;';

  function fieldHtml(id, label, value, type) {
    type = type || 'text';
    return (
      '<label style="display:block;margin-top:8px;font-size:11px;color:#9a9aa8;">' + label + '</label>' +
      '<input id="' + id + '" type="' + type + '" value="' + String(value).replace(/"/g, '&quot;') + '" ' +
      'style="width:100%;box-sizing:border-box;background:#2a2a33;color:#fff;border:1px solid #44444f;' +
      'border-radius:4px;padding:5px 6px;font-size:12px;">'
    );
  }

  function selectHtml(id, label, options, selected) {
    var opts = options
      .map(function (o) {
        return '<option value="' + o + '"' + (o === selected ? ' selected' : '') + '>' + o + '</option>';
      })
      .join('');
    return (
      '<label style="display:block;margin-top:8px;font-size:11px;color:#9a9aa8;">' + label + '</label>' +
      '<select id="' + id + '" style="width:100%;box-sizing:border-box;background:#2a2a33;color:#fff;' +
      'border:1px solid #44444f;border-radius:4px;padding:5px 6px;font-size:12px;">' + opts + '</select>'
    );
  }

  var warningHtml = guess.rangeFound
    ? ''
    : '<div style="background:#5a2a2a;border:1px solid #c0392b;padding:7px;border-radius:4px;margin-top:10px;font-size:11px;">' +
      '⚠ No explicit "X to Y" range was found in the page text -- base_min and base_max were both set to the one ' +
      'value found. Verify the real range on the page and correct this before trusting it (this is exactly the failure ' +
      'mode that corrupted this project\'s earlier automated extraction).</div>';

  var maxMatchCount = Math.max(guess.rangeMatchCount, guess.schoolValMatchCount);
  var ambiguityHtml =
    maxMatchCount > 1
      ? '<div style="background:#5a4a1a;border:1px solid #c0902b;padding:7px;border-radius:4px;margin-top:10px;font-size:11px;">' +
        '⚠ Found ' + maxMatchCount + ' possible damage values on this page -- it may show multiple ranks (e.g. a rank ' +
        'comparison table). The FIRST one found was used, which may not be the max rank / level 60 value. Scroll the ' +
        'page and confirm this is the right rank before trusting it; this failure mode is silent otherwise -- the ' +
        'fields would look populated and correct either way.</div>'
      : '';

  overlay.innerHTML =
    '<div style="display:flex;justify-content:space-between;align-items:center;">' +
    '<strong style="font-size:14px;">WoW Forever Spell Curation</strong>' +
    '<button id="wfcur-close" style="background:none;border:none;color:#9a9aa8;font-size:18px;cursor:pointer;">×</button>' +
    '</div>' +
    '<div style="font-size:10px;color:#777;margin-top:2px;">Best-effort pre-fill from this page. Review every field.</div>' +
    warningHtml +
    ambiguityHtml +
    fieldHtml('wfcur-id', 'id (snake_case)', slugGuess) +
    fieldHtml('wfcur-name', 'name', titleGuess) +
    selectHtml('wfcur-school', 'school', ['shadow', 'fire', 'physical'], guess.school || 'shadow') +
    fieldHtml('wfcur-cast', 'cast_time_ms', guess.castTimeMs, 'number') +
    fieldHtml('wfcur-cd', 'cooldown_ms', guess.cooldownMs, 'number') +
    fieldHtml('wfcur-cost', 'resource_cost (mana)', guess.resourceCost, 'number') +
    fieldHtml('wfcur-min', 'base_min', guess.baseMin, 'number') +
    fieldHtml('wfcur-max', 'base_max', guess.baseMax, 'number') +
    fieldHtml('wfcur-coeff', 'coefficient', guess.coefficient, 'number') +
    '<label style="display:block;margin-top:10px;"><input id="wfcur-hasdot" type="checkbox" ' +
    (guess.hasDot ? 'checked' : '') +
    '> Has a DoT component</label>' +
    '<div id="wfcur-dot-fields" style="display:' + (guess.hasDot ? 'block' : 'none') + ';padding-left:10px;border-left:2px solid #44444f;">' +
    fieldHtml('wfcur-tick-interval', 'tick_interval_ms', guess.tickIntervalMs, 'number') +
    fieldHtml('wfcur-num-ticks', 'num_ticks', guess.numTicks, 'number') +
    fieldHtml('wfcur-tick-base', 'tick_base', guess.tickBase, 'number') +
    fieldHtml('wfcur-tick-coeff', 'tick_coefficient', guess.tickCoefficient, 'number') +
    '<label style="display:block;margin-top:6px;"><input id="wfcur-cancrit" type="checkbox"> can_crit</label>' +
    '</div>' +
    selectHtml('wfcur-confidence', 'confidence', ['community-reported', 'beta-datamine', 'dev-blue-post', 'live-confirmed', 'placeholder'], 'community-reported') +
    fieldHtml('wfcur-source-name', 'source.name', 'Wowhead: ' + (titleGuess || document.title)) +
    fieldHtml('wfcur-source-url', 'source.url', location.href) +
    '<div style="display:flex;gap:6px;margin-top:12px;flex-wrap:wrap;">' +
    '<button id="wfcur-generate" style="flex:1;padding:7px;background:#4a7fd6;color:#fff;border:none;border-radius:4px;cursor:pointer;">Generate JSON</button>' +
    '</div>' +
    '<div id="wfcur-output" style="display:none;margin-top:10px;">' +
    '<textarea id="wfcur-json" readonly style="width:100%;height:220px;box-sizing:border-box;background:#111;color:#8f8;' +
    'font-family:monospace;font-size:11px;border:1px solid #44444f;border-radius:4px;padding:6px;"></textarea>' +
    '<div style="display:flex;gap:6px;margin-top:6px;">' +
    '<button id="wfcur-copy" style="flex:1;padding:6px;background:#3a3a44;color:#fff;border:1px solid #555;border-radius:4px;cursor:pointer;">Copy</button>' +
    '<button id="wfcur-download" style="flex:1;padding:6px;background:#3a3a44;color:#fff;border:1px solid #555;border-radius:4px;cursor:pointer;">Download .json</button>' +
    '</div></div>';

  document.body.appendChild(overlay);

  document.getElementById('wfcur-close').addEventListener('click', function () {
    overlay.remove();
  });

  document.getElementById('wfcur-hasdot').addEventListener('change', function (e) {
    document.getElementById('wfcur-dot-fields').style.display = e.target.checked ? 'block' : 'none';
  });

  function val(id) {
    return document.getElementById(id).value;
  }
  function numVal(id) {
    var v = parseFloat(document.getElementById(id).value);
    return isNaN(v) ? 0 : v;
  }

  document.getElementById('wfcur-generate').addEventListener('click', function () {
    var hasDot = document.getElementById('wfcur-hasdot').checked;
    var spell = {
      id: val('wfcur-id'),
      name: val('wfcur-name'),
      school: val('wfcur-school'),
      cast_time_ms: numVal('wfcur-cast'),
      cooldown_ms: numVal('wfcur-cd'),
      resource_cost: numVal('wfcur-cost'),
      base_min: numVal('wfcur-min'),
      base_max: numVal('wfcur-max'),
      coefficient: numVal('wfcur-coeff'),
      dot: hasDot
        ? {
            tick_interval_ms: numVal('wfcur-tick-interval'),
            num_ticks: numVal('wfcur-num-ticks'),
            tick_base: numVal('wfcur-tick-base'),
            tick_coefficient: numVal('wfcur-tick-coeff'),
            can_crit: document.getElementById('wfcur-cancrit').checked
          }
        : null,
      confidence: val('wfcur-confidence'),
      source: { name: val('wfcur-source-name'), url: val('wfcur-source-url') }
    };
    document.getElementById('wfcur-json').value = JSON.stringify(spell, null, 2);
    document.getElementById('wfcur-output').style.display = 'block';
  });

  document.getElementById('wfcur-copy').addEventListener('click', function () {
    var ta = document.getElementById('wfcur-json');
    ta.select();
    try {
      navigator.clipboard.writeText(ta.value);
    } catch (e) {
      document.execCommand('copy');
    }
  });

  document.getElementById('wfcur-download').addEventListener('click', function () {
    var idVal = val('wfcur-id') || 'spell';
    var blob = new Blob([document.getElementById('wfcur-json').value], { type: 'application/json' });
    var a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = idVal + '.json';
    document.body.appendChild(a);
    a.click();
    a.remove();
  });
})();
