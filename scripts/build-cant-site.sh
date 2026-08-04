#!/usr/bin/env bash
# Build the Cant site into apps/cant-web/dist.
#
# Smaller than scripts/build-site.sh: Cant ships no installer and no skill
# bundle. It does ship an engine — Studio runs the real thing in the page — so
# the wasm build is here, with the same asserts the Rite build makes about
# artifacts really landing in dist, since Cloudflare's SPA fallback answers a
# missing file with 200 + index.html rather than a 404.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> Social card"
# Rasterised from a hand-authored SVG through the rasteriser already in this
# tree. Scrapers do not render SVG, and the alternative was installing an image
# toolchain to draw one picture.
if [[ ! -f "$ROOT/apps/cant-web/public/og.png" ]]; then
  cargo run -p xtask -- cant-og
fi

echo "==> Engine (cant-wasm)"
bash "$ROOT/scripts/build-cant-wasm.sh"

echo "==> Install JS deps (if needed)"
if [[ ! -d "$ROOT/apps/cant-web/node_modules" ]]; then
  pnpm install
fi

echo "==> Vite build (cant-web)"
pnpm --dir apps/cant-web build

# The operator table, the version badge and the sibling link are all injected at
# build time from files outside the app. If any of them silently produced nothing
# the page would render an empty vocabulary, which is worse than failing here.
DIST="$ROOT/apps/cant-web/dist"
if ! grep -rq "orbit" "$DIST/assets"/*.js; then
  echo "error: built bundle has no operator vocabulary — check the operators.toml reader in vite.config.ts" >&2
  exit 1
fi
# The card is referenced absolutely by <meta>, so a missing one is a broken link
# preview rather than a build failure — which is exactly the kind of thing that
# ships unnoticed.
for asset in og.png brand/logo.svg wasm/cant_wasm.js wasm/cant_wasm_bg.wasm; do
  if [[ ! -f "$DIST/$asset" ]]; then
    echo "error: $asset missing from dist — link previews, the favicon or Studio would 404" >&2
    exit 1
  fi
done

echo "==> Cant site ready at apps/cant-web/dist"
ls -la "$DIST"
