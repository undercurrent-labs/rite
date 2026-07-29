#!/usr/bin/env bash
# Local/CI gate: build skill archives + VS Code VSIX the way Release does.
# Failures here should fail before a slow GitHub Actions matrix.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "======== package-skill (relative OUT — CI path) ========"
# Relative path is the bug class that broke Release; must stay relative here.
rm -rf dist/skill-check
bash scripts/package-skill.sh dist/skill-check
test -f dist/skill-check/rite-agent-skill.tar.gz
test -f dist/skill-check/rite-agent-skill.zip
test -s dist/skill-check/SHA256SUMS

echo "======== package-vsix ========"
bash scripts/package-vsix.sh dist/vscode/rite.vsix

echo "======== packaging checks OK ========"
ls -la dist/skill-check/ dist/vscode/
