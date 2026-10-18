# Client-side web frontend

Implements Plan Section 5's "the core simulation engine compiles to run client-side (e.g. WASM, mirroring `wowsims/classic`'s own approach)" and the confidence-tag UI treatment ("results views must visibly color-code or badge confidence level... and surface an explicit warning when a result depends on a `community-disputed` data point").

## What's real here

- **The WASM module is the actual simulation engine**, not a reimplementation. `crates/wasm` wraps `wowforever-engine` and `wowforever-burst` directly; every number this page shows comes from the same Rust code the CLI runs, compiled to `wasm32-unknown-unknown`. Verified, not assumed: a Node smoke test and a real headless-Chrome test (via Puppeteer, pointed at the system's `google-chrome` binary) both confirm the browser-run numbers match the native CLI's output exactly for the same build/seed/data-pack (Warlock DPS, Mage DPS, and the burst calculator all checked).
- **Confidence-tag UI treatment is real**, not a mockup: every spell badge is color-coded from the data pack's own `confidence` field (`beta-datamine` = blue, `community-reported` = amber, `community-disputed`/`placeholder` = red, etc.), and a disputed-mechanic result shows an explicit warning banner with the computed rule-A-vs-rule-B delta (Principle 13), not just a flag.
- **"Accounts/identity" (Plan Section 5) is deliberately client-side-only**: a locally-generated anonymous id (`crypto.randomUUID()`) persisted in `localStorage`, nothing server-side. This isn't a stub standing in for a real accounts system -- it's the actual design choice, made because there is no server in this architecture to retain anything, which sidesteps the data-retention/privacy-obligation complexity a server-side accounts system would introduce (Plan Section 5's "privacy obligations" note). "Export my data" and "Delete my data" are both real, working buttons.

## Known limitation, stated rather than hidden

Only two demo builds exist (Warlock, Mage), matching the two classes this project has sourced real data for so far (`crates/ingest` output in `data-packs/`). This is a bring-up frontend for a Phase 1/2 engine, not a build-editor UI -- there's no form for constructing an arbitrary rotation yet.

## Running it locally

From the repo root:

```
./start.sh
```

This builds the real engine to `wasm32-unknown-unknown` (installing the target and a matching `wasm-bindgen-cli` if needed), regenerates `web/pkg/`, frees port 8731 if something else is already bound to it, and serves the repo root (not `web/`, so `/data-packs/...` resolves) via `python3 -m http.server`. Then open `http://localhost:8731/web/index.html`.

The page needs to be served over HTTP (not opened as a `file://` URL -- ES module imports and `fetch()` both require it). `web/pkg/` (the compiled wasm + generated JS glue) is a build artifact, not checked in -- `start.sh` regenerates it every run, so it always reflects the current `crates/wasm`/`crates/engine`/`crates/burst`/`crates/data` code.

Manual equivalent, if you need it (e.g. to build without serving):

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match the wasm-bindgen crate version in Cargo.lock exactly
cargo build -p wowforever-wasm --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/release/wowforever_wasm.wasm --out-dir web/pkg --target web --out-name wowforever
python3 -m http.server 8731   # from the repo root
```
