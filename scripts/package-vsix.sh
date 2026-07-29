#!/usr/bin/env bash
# Compile and package the VS Code extension into a .vsix (CI + local gate).
#
# Usage:
#   bash scripts/package-vsix.sh [output.vsix]
#
# Performs a clean-ish install (npm ci or npm install) so lockfile breakage is caught.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXT="$ROOT/editors/vscode"
OUT="${1:-$EXT/rite.vsix}"
# Absolute out path (vsce may be run from EXT)
if [[ "$OUT" != /* ]]; then
  OUT="$ROOT/$OUT"
fi
mkdir -p "$(dirname "$OUT")"

need() { command -v "$1" >/dev/null 2>&1 || { echo "error: need $1" >&2; exit 1; }; }
need node
need npm

cd "$EXT"

# Prefer a self-contained npm lockfile (not pnpm-workspace linked).
if [[ -f package-lock.json ]]; then
  # Detect broken lockfiles that point into monorepo pnpm store
  if grep -q 'node_modules/\.pnpm' package-lock.json 2>/dev/null \
     || grep -q '"\.\./\.\./node_modules' package-lock.json 2>/dev/null; then
    echo "error: package-lock.json looks pnpm-linked; regenerate with npm in editors/vscode" >&2
    exit 1
  fi
  npm ci
else
  npm install
fi

npm run compile

# Ensure tsc produced the entrypoint
[[ -f out/extension.js ]] || { echo "error: out/extension.js missing after compile" >&2; exit 1; }

# vsce from npx (don't require global install)
npx --yes @vscode/vsce@3 package \
  --no-dependencies \
  --allow-missing-repository \
  -o "$OUT"

[[ -f "$OUT" ]] || { echo "error: vsix not written to $OUT" >&2; exit 1; }
# Basic size sanity (empty/corrupt packages are tiny)
SIZE=$(wc -c < "$OUT" | tr -d ' ')
if [[ "$SIZE" -lt 1000 ]]; then
  echo "error: vsix too small ($SIZE bytes)" >&2
  exit 1
fi

echo "==> VSIX ready: $OUT ($SIZE bytes)"
ls -la "$OUT"
