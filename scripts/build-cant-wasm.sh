#!/usr/bin/env bash
# Build cant-wasm for the browser and copy it into the Cant site's public/.
#
# The mirror of scripts/build-wasm.sh, and separate from it on purpose: this
# builds a different crate into a different site, and folding the two together
# would mean Rite's site could not be built without Cant's.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="$ROOT/apps/cant-web/public/wasm"

rustup target add wasm32-unknown-unknown 2>/dev/null || true

if ! command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack not found; install: cargo install wasm-pack" >&2
  exit 1
fi

wasm-pack build crates/cant-wasm \
  --target web \
  --out-dir "$OUT" \
  --out-name cant_wasm \
  -- --no-default-features --features wasm

# Studio is the only page that loads these, and it loads them by URL at runtime
# rather than importing them — so a rename or a missing file is a blank panel at
# runtime rather than a build error. Check here instead.
for asset in cant_wasm.js cant_wasm_bg.wasm; do
  if [[ ! -f "$OUT/$asset" ]]; then
    echo "error: $asset missing from $OUT — Studio would load nothing" >&2
    exit 1
  fi
done

echo "WASM package written to apps/cant-web/public/wasm"
ls -la "$OUT"
