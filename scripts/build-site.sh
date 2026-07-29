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
# curl …/install | bash  (no extension; must be a real asset, not SPA HTML)
install -m 644 "$ROOT/scripts/install.sh" "$ROOT/apps/rite-web/public/install"

echo "==> Agent skill packages (/skill/*)"
bash "$ROOT/scripts/package-skill.sh" "$ROOT/apps/rite-web/public/skill"
# keep dist/skill in sync for local packaging
mkdir -p "$ROOT/dist/skill"
cp -a "$ROOT/apps/rite-web/public/skill/." "$ROOT/dist/skill/" 2>/dev/null || true

echo "==> VS Code VSIX placeholder dir (/vscode/)"
mkdir -p "$ROOT/apps/rite-web/public/vscode"
# Prefer a freshly packaged vsix when present (CI release or local)
if [[ -f "$ROOT/editors/vscode/rite.vsix" ]]; then
  cp -f "$ROOT/editors/vscode/rite.vsix" "$ROOT/apps/rite-web/public/vscode/rite.vsix"
elif [[ -f "$ROOT/dist/vscode/rite.vsix" ]]; then
  cp -f "$ROOT/dist/vscode/rite.vsix" "$ROOT/apps/rite-web/public/vscode/rite.vsix"
else
  # Lightweight note so /vscode/ isn't empty in dev; real vsix comes from release CI
  printf 'Packaged VSIX is published on GitHub Releases and copied here by the release pipeline.\n' \
    > "$ROOT/apps/rite-web/public/vscode/README.txt"
fi

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

# Hard fail if skill packages missing from dist (SPA would otherwise serve HTML for /skill/*)
if [[ ! -f "$ROOT/apps/rite-web/dist/skill/rite-agent-skill.tar.gz" ]] \
   || [[ ! -f "$ROOT/apps/rite-web/dist/skill/rite-agent-skill.zip" ]]; then
  echo "error: skill packages missing from dist — /skill/* would 200 HTML on Cloudflare" >&2
  ls -laR "$ROOT/apps/rite-web/public/skill" "$ROOT/apps/rite-web/dist/skill" 2>/dev/null || true
  exit 1
fi
if grep -q '<!DOCTYPE html' "$ROOT/apps/rite-web/dist/skill/rite-agent-skill.zip" 2>/dev/null; then
  echo "error: skill zip looks like HTML" >&2
  exit 1
fi

echo "==> Site ready at apps/rite-web/dist"
ls -la "$ROOT/apps/rite-web/dist" | head -20
ls -la "$ROOT/apps/rite-web/dist/skill/"
