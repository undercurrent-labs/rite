#!/usr/bin/env bash
# Build cant-sigil-wasm for the browser and copy it into the Sigil site's public/.
#
# A third script beside the Rite and Cant ones, and separate for the same reason
# they are separate from each other: this builds a different crate into a
# different site, and folding them together would mean neither site could be
# built without the others.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="$ROOT/apps/sigil-web/public/wasm"

rustup target add wasm32-unknown-unknown 2>/dev/null || true

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack not found; install: cargo install wasm-pack" >&2
  exit 1
fi

# `--no-default-features` drops `native`, which exists only so the API can be
# tested with `cargo test`. `--features wasm` adds the bindings.
wasm-pack build crates/cant-sigil-wasm \
  --target web \
  --out-dir "$OUT" \
  --out-name cant_sigil_wasm \
  -- --no-default-features --features wasm

# The app loads these by URL at runtime rather than importing them, so a rename
# or a missing file is a blank canvas at runtime rather than a build error.
# Check here instead.
for asset in cant_sigil_wasm.js cant_sigil_wasm_bg.wasm; do
  if [[ ! -f "$OUT/$asset" ]]; then
    echo "error: $asset missing from $OUT — the app would load nothing" >&2
    exit 1
  fi
done

echo "WASM package written to $OUT"
ls -la "$OUT"
