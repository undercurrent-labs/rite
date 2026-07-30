#!/usr/bin/env bash
# Package skills/rite into dist archives for releases and the product site.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Resolve OUT to an absolute path up front — callers often pass relative
# paths (e.g. dist/skill). Zip/tar steps may cd into a staging dir, which
# would otherwise break relative destinations (CI failure mode).
OUT_IN="${1:-$ROOT/dist/skill}"
mkdir -p "$OUT_IN"
OUT="$(cd "$OUT_IN" && pwd)"

# Packages what is committed. It used to regenerate the bundle in place first, with
# whichever `rite` it could find — PATH, then target/release, then target/debug — and
# `2>/dev/null || true` so nothing it did could fail the package.
#
# A stale `target/release/rite` is the normal state of a working tree, and this rewrote
# tracked files with its output: version 0.1.6 and 45 fewer capability lines than the
# committed manifest, silently, on every `cargo test` that exercised packaging. An old
# enough binary predates the guard that stops `docs agent --output skills/rite` rewriting
# the hand-written SKILL.md, so it could rewrite that too.
#
# Regenerating is a deliberate act — `rite docs build` — not a side effect of packaging.
# If the committed bundle drifts from what the current binary emits, that is for a
# freshness check to report, not for a release script to paper over with unknown output.

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

# Prefer pure-Python zip so we never depend on `zip` CLI or cwd-relative paths
python3 - "$STAGE" "$OUT/rite-agent-skill.zip" <<'PY'
import sys, zipfile
from pathlib import Path
stage, out = Path(sys.argv[1]), Path(sys.argv[2])
out.parent.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for p in stage.rglob("*"):
        if p.is_file():
            z.write(p, p.relative_to(stage).as_posix())
print("wrote", out)
PY

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$OUT" && sha256sum rite-agent-skill.tar.gz rite-agent-skill.zip > SHA256SUMS)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$OUT" && shasum -a 256 rite-agent-skill.tar.gz rite-agent-skill.zip > SHA256SUMS)
fi

rm -rf "$STAGE"

# Validate archive contents
python3 - "$OUT/rite-agent-skill.tar.gz" "$OUT/rite-agent-skill.zip" <<'PY'
import sys, tarfile, zipfile
tar_path, zip_path = sys.argv[1], sys.argv[2]
with tarfile.open(tar_path, "r:gz") as t:
    names = t.getnames()
    assert any(n.endswith("SKILL.md") for n in names), f"SKILL.md missing in tar: {names[:20]}"
    assert any(n.startswith("rite/") for n in names), "expected rite/ root in tar"
with zipfile.ZipFile(zip_path) as z:
    names = z.namelist()
    assert any(n.endswith("SKILL.md") for n in names), f"SKILL.md missing in zip: {names[:20]}"
print("skill archives validated")
PY

echo "==> Skill packages in $OUT"
ls -la "$OUT"
