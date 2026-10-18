# Wowhead Spell Curation Bookmarklet

**Before using this, check `https://thewowdb.com/wow-forever/spell/<id>` first.** Found 2026-10-17 (see `docs/PLAN.md`'s data-sourcing investigation): it's directly fetchable with a plain HTTP request (no Cloudflare block like Wowhead), its `robots.txt` explicitly welcomes AI/bot crawling of its content pages, and each spell page cites its own client-data extraction directly ("Generated from the SpellEffect, SpellMisc, SpellCooldowns and SpellRange records..."). For anything it covers, that's a better source than this bookmarklet — no browser, no manual review of a lossy guess, just `curl` and parse the HTML yourself. This bookmarklet remains useful for sites that do block automated access, or lack a page that structured, which is still true of Wowhead.

Why: automated fetching of Wowhead pages is unreliable for this project (see `docs/PLAN.md`'s data-sourcing investigation) — direct requests get blocked, and even a successful page-fetch-and-summarize pass was found to silently collapse real damage ranges into single values, in one case (Immolate) producing a figure wrong by roughly an order of magnitude. A human looking at the real, rendered page in their own browser doesn't have either problem, so this bookmarklet assists *manual* curation instead of attempting more automation.

It does not auto-submit anything anywhere. It scans the current page's own rendered text for likely damage/cooldown/cost values, shows them to you in an editable form for correction, and generates a JSON object in exactly the shape this project's `SpellDef` type expects (`crates/data/src/lib.rs`) for you to copy or download and paste into a data pack by hand, after review.

## Install

1. Show your browser's bookmarks bar if it's hidden (Ctrl+Shift+B / Cmd+Shift+B in Chrome and Firefox).
2. Right-click the bookmarks bar → "Add page" / "New bookmark."
3. Name it anything (e.g. "WF Spell Curation").
4. For the URL/location field, paste the **entire contents** of `tools/wowhead-curation-bookmarklet.url.txt` (one long `javascript:...` line). Most browsers will refuse to show this as a normal link but will happily store and run it as a bookmark target.
5. Save.

(`tools/wowhead-curation-bookmarklet.url.txt` is generated from `tools/wowhead-curation-bookmarklet.js` — see "Regenerating" below if you edit the source.)

## Use

1. Open a Wowhead spell page, e.g. `https://www.wowhead.com/forever/spell=17923/searing-pain`.
2. Click the bookmarklet. A panel slides in from the right with its best-effort guesses already filled in.
3. **Read the warning banner if it appears.** It means no explicit "X to Y" range was found in the page text, so `base_min`/`base_max` were both set to whatever single number *was* found — exactly the failure mode that produced bad data earlier in this project. Go find the real range on the page (or in the tooltip's "Additional Info" if present) and correct it yourself.
4. Fix any other field that's wrong, empty, or a bad guess — the extraction is best-effort text pattern matching, not a guarantee. In particular: the DoT tick interval defaults to a guessed 3000ms and the per-tick coefficient is never auto-filled (there's no reliable text pattern for it), so always double-check those.
5. Click "Generate JSON," review the output, then "Copy" or "Download .json."
6. Paste the result into the relevant data pack (or, if you're unsure, into a dev-only overrides file first — see `CLAUDE.md`'s placeholder-data policy, since `confidence: "placeholder"` is in the dropdown for exactly this purpose). Double check `confidence` reflects where the data actually came from — it defaults to `community-reported` (Wowhead is a third-party site regardless of who's reading it; `beta-datamine` is reserved for genuine client-data extraction per `docs/PLAN.md`'s Phase C).
7. Click the bookmarklet again (or the × in the panel) to dismiss it.

## Regenerating the bookmarklet after editing the source

```
python3 -c "
import urllib.parse
with open('tools/wowhead-curation-bookmarklet.js') as f:
    src = f.read()
code = src[src.find('*/')+2:].strip()
uri = 'javascript:' + urllib.parse.quote(code, safe='') + 'void%200%3B'
with open('tools/wowhead-curation-bookmarklet.url.txt', 'w') as f:
    f.write(uri + '\n')
"
```

Then re-test it (there's no automated test runner wired into `cargo test` for this — it's plain browser JS, not Rust). The approach used during development: install `jsdom` (`npm install jsdom`) and eval the un-minified source against a synthetic page body in Node, asserting on the extracted field values and the generated JSON — this is what actually caught a real bug (`document.body.innerText` returns empty in jsdom, silently zeroing every extraction) before this tool shipped. Any change to the extraction regexes should be re-verified the same way, not just eyeballed in a real browser, since a wrong default can look identical to a successful extraction at a glance.
