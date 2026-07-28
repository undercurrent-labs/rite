#!/usr/bin/env bash
# Build the unified Rite product site (home + docs + studio) into apps/rite-web/dist
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> WASM"
bash "$ROOT/scripts/build-wasm.sh"

echo "==> Copy WASM into rite-web public/"
mkdir -p "$ROOT/apps/rite-web/public/wasm"
cp -a "$ROOT/apps/rite-studio/public/wasm/." "$ROOT/apps/rite-web/public/wasm/"

echo "==> Installer endpoints (/install, /install.sh)"
mkdir -p "$ROOT/apps/rite-web/public"
install -m 644 "$ROOT/scripts/install.sh" "$ROOT/apps/rite-web/public/install.sh"
# curl …/install | sh  (no extension; must be a real asset, not SPA HTML)
install -m 644 "$ROOT/scripts/install.sh" "$ROOT/apps/rite-web/public/install"

echo "==> Install JS deps (if needed)"
if [[ ! -d "$ROOT/apps/rite-web/node_modules" ]]; then
  pnpm install
fi

echo "==> Vite build (rite-web)"
pnpm --dir apps/rite-web build

# Ensure wasm landed in dist (Vite copies public/)
if [[ ! -f "$ROOT/apps/rite-web/dist/wasm/rite_wasm_bg.wasm" ]]; then
  echo "error: wasm missing from dist after build" >&2
  mkdir -p "$ROOT/apps/rite-web/dist/wasm"
  cp -a "$ROOT/apps/rite-web/public/wasm/." "$ROOT/apps/rite-web/dist/wasm/"
fi

echo "==> Site ready at apps/rite-web/dist"
ls -la "$ROOT/apps/rite-web/dist" | head -20
