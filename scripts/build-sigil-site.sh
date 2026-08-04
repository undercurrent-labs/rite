#!/usr/bin/env bash
# Build the Sigil site: WASM, then the Vue app.
#
# Separate from the Rite and Cant site scripts for the reason they are separate
# from each other — this builds a different app, and coupling them would mean
# none of the three could be built alone.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

bash scripts/build-sigil-wasm.sh

# The executed-wasm parity gate (AR4/Q2): the bundle just built, run in Node,
# must draw byte-for-byte what the native renderer drew. See the script.
node scripts/check-sigil-wasm-parity.mjs

pnpm --dir apps/sigil-web typecheck
pnpm --dir apps/sigil-web build

# The app loads the engine by URL at runtime, so a build that dropped it is a
# blank canvas in production rather than a build failure. Check.
for asset in cant_sigil_wasm.js cant_sigil_wasm_bg.wasm; do
  if [[ ! -f "$ROOT/apps/sigil-web/dist/wasm/$asset" ]]; then
    echo "error: $asset missing from dist/wasm — the deployed app would load nothing" >&2
    exit 1
  fi
done

# The Worker's /api/version and /api/schema read this asset; without it they
# answer "build info unavailable" in production while every test passes.
if [[ ! -f "$ROOT/apps/sigil-web/dist/build-info.json" ]]; then
  echo "error: build-info.json missing from dist — the Worker's version endpoints would go dark" >&2
  exit 1
fi

# The Worker must be deployable. A dry run catches a broken `wrangler.jsonc`
# here rather than at the moment someone tries to publish.
if command -v npx >/dev/null 2>&1; then
  (cd "$ROOT/apps/sigil-web" && npx --no-install wrangler deploy --dry-run --outdir /tmp/sigil-dry) \
    || echo "note: wrangler dry run skipped (wrangler not installed)" >&2
fi

echo "Sigil site built at apps/sigil-web/dist"
