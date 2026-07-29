#!/usr/bin/env bash
# Package skills/rite into dist archives for releases and the product site.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:-$ROOT/dist/skill}"
mkdir -p "$OUT"

# Ensure agent bundle is fresh when rite is available
if command -v rite >/dev/null 2>&1; then
  rite docs agent --output skills/rite 2>/dev/null || true
elif [[ -x "$ROOT/target/release/rite" ]]; then
  "$ROOT/target/release/rite" docs agent --output skills/rite 2>/dev/null || true
elif [[ -x "$ROOT/target/debug/rite" ]]; then
  "$ROOT/target/debug/rite" docs agent --output skills/rite 2>/dev/null || true
fi

[[ -f skills/rite/SKILL.md ]] || { echo "error: skills/rite/SKILL.md missing" >&2; exit 1; }

STAGE="$OUT/stage"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -a skills/rite "$STAGE/rite"

VER="$(grep -E '^version\s*=' "$ROOT/Cargo.toml" | head -1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ -n "$VER" ]]; then
  mkdir -p "$STAGE/rite/machine"
  cat > "$STAGE/rite/machine/version.json" <<EOF
{
  "version": "$VER",
  "tag": "v$VER",
  "skill": "rite",
  "tool_version": "$VER",
  "language_version": "1",
  "formatter_version": "1"
}
EOF
fi

tar -C "$STAGE" -czf "$OUT/rite-agent-skill.tar.gz" rite

if command -v zip >/dev/null 2>&1; then
  (cd "$STAGE" && rm -f "$OUT/rite-agent-skill.zip" && zip -qr "$OUT/rite-agent-skill.zip" rite)
else
  python3 - "$STAGE" "$OUT/rite-agent-skill.zip" <<'PY'
import sys, zipfile
from pathlib import Path
stage, out = Path(sys.argv[1]), Path(sys.argv[2])
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for p in stage.rglob("*"):
        if p.is_file():
            z.write(p, p.relative_to(stage).as_posix())
print("wrote", out)
PY
fi

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$OUT" && sha256sum rite-agent-skill.tar.gz rite-agent-skill.zip > SHA256SUMS)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$OUT" && shasum -a 256 rite-agent-skill.tar.gz rite-agent-skill.zip > SHA256SUMS)
fi

rm -rf "$STAGE"
echo "==> Skill packages in $OUT"
ls -la "$OUT"
