#!/usr/bin/env bash
# Build rite-wasm for the browser and copy into Studio public/
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

rustup target add wasm32-unknown-unknown 2>/dev/null || true

# Prefer wasm-pack when available
if command -v wasm-pack >/dev/null 2>&1; then
  wasm-pack build crates/rite-wasm \
    --target web \
    --out-dir "$ROOT/apps/rite-studio/public/wasm" \
    --out-name rite_wasm \
    -- --no-default-features --features wasm
  # also copy for Studio standalone dist + unified product site
  mkdir -p "$ROOT/apps/rite-studio/dist/wasm" 2>/dev/null || true
  cp -a "$ROOT/apps/rite-studio/public/wasm/." "$ROOT/apps/rite-studio/dist/wasm/" 2>/dev/null || true
  mkdir -p "$ROOT/apps/rite-web/public/wasm" 2>/dev/null || true
  cp -a "$ROOT/apps/rite-studio/public/wasm/." "$ROOT/apps/rite-web/public/wasm/" 2>/dev/null || true
  echo "WASM package written to apps/rite-studio/public/wasm (+ apps/rite-web/public/wasm)"
else
  echo "wasm-pack not found; install: cargo install wasm-pack" >&2
  exit 1
fi
