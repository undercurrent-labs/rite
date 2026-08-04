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

echo "Sigil site built at apps/sigil-web/dist"
