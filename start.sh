#!/bin/bash
# WoW Forever DPS & Burst Simulator - Web Frontend Launcher
# Builds the real simulation engine to WASM (if needed) and serves web/
# Ensures port 8731 is free, then starts a Python HTTP server.

set -e

PORT=8731
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_DIR"

echo "==========================================="
echo "  WoW Forever DPS & Burst Simulator"
echo "==========================================="
echo ""

# Make sure cargo/rustup are on PATH even in a fresh shell.
if ! command -v cargo &> /dev/null && [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

if ! command -v cargo &> /dev/null; then
    echo "Error: cargo is not installed or not in PATH. Install Rust from https://rustup.rs first."
    exit 1
fi

# Check if port $PORT is already in use and free it.
echo "Checking if port $PORT is available..."
PORT_PID=$(lsof -ti:$PORT 2>/dev/null || true)
if [ -n "$PORT_PID" ]; then
    echo "Port $PORT is in use by process $PORT_PID -- killing it..."
    kill -9 $PORT_PID 2>/dev/null || true
    sleep 1
    echo "Process killed."
else
    echo "Port $PORT is available."
fi
echo ""

# Ensure the wasm32 target is installed (idempotent, no-op if already present).
if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
    echo "Installing rustup target wasm32-unknown-unknown..."
    rustup target add wasm32-unknown-unknown
fi

# Ensure wasm-bindgen-cli is installed and matches the wasm-bindgen crate
# version pinned in Cargo.lock exactly -- a mismatch here is a hard error
# at bindgen time, not a warning, so check and fix it proactively.
LOCKED_VERSION=$(grep -A2 '^name = "wasm-bindgen"$' Cargo.lock | grep '^version' | head -1 | sed -E 's/version = "(.*)"/\1/')
INSTALLED_VERSION=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ -z "$LOCKED_VERSION" ]; then
    echo "Error: could not find wasm-bindgen's locked version in Cargo.lock."
    exit 1
fi
if [ "$INSTALLED_VERSION" != "$LOCKED_VERSION" ]; then
    echo "Installing wasm-bindgen-cli $LOCKED_VERSION (found: ${INSTALLED_VERSION:-none}, need: $LOCKED_VERSION)..."
    cargo install wasm-bindgen-cli --version "$LOCKED_VERSION"
fi

echo ""
echo "Building the real simulation engine to WASM (crates/wasm, release)..."
cargo build -p wowforever-wasm --target wasm32-unknown-unknown --release

echo ""
echo "Generating JS bindings into web/pkg/..."
wasm-bindgen target/wasm32-unknown-unknown/release/wowforever_wasm.wasm \
    --out-dir web/pkg --target web --out-name wowforever

echo ""
echo "==========================================="
echo "  Serving at http://localhost:$PORT/"
echo "==========================================="
echo ""
echo "Press Ctrl+C to stop the server"
echo ""

# Serve from the repo root (not web/) so /data-packs/... resolves correctly.
exec python3 -m http.server "$PORT"
