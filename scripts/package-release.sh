#!/usr/bin/env bash
# Build release tarball(s) for the current host (or $TARGET) into dist/release/
# Used by CI and for local smoke packaging.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TARGET="${TARGET:-}"
if [[ -z "$TARGET" ]]; then
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "unknown arch: $arch" >&2; exit 1 ;;
  esac
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  case "$os" in
    linux) TARGET="${arch}-unknown-linux-gnu" ;;
    darwin) TARGET="${arch}-apple-darwin" ;;
    *) echo "unknown os: $os" >&2; exit 1 ;;
  esac
fi

OUT="${OUT:-$ROOT/dist/release}"
STAGE="${OUT}/rite-${TARGET}"
mkdir -p "$STAGE"

echo "==> building rite + rite-lsp for ${TARGET}"
if [[ "$(rustc -vV | sed -n 's/^host: //p')" == "$TARGET" ]]; then
  cargo build -p rite-cli -p rite-lsp --release
  BIN_DIR="$ROOT/target/release"
else
  rustup target add "$TARGET" 2>/dev/null || true
  cargo build -p rite-cli -p rite-lsp --release --target "$TARGET"
  BIN_DIR="$ROOT/target/${TARGET}/release"
fi

# Windows uses .exe — this script is Unix-oriented; CI handles windows zip.
cp "$BIN_DIR/rite" "$STAGE/rite"
cp "$BIN_DIR/rite-lsp" "$STAGE/rite-lsp"
chmod +x "$STAGE/rite" "$STAGE/rite-lsp"

# Optional strip (ignore failures on exotic linkers)
strip "$STAGE/rite" 2>/dev/null || true
strip "$STAGE/rite-lsp" 2>/dev/null || true

ARCHIVE="${OUT}/rite-${TARGET}.tar.gz"
tar -C "$OUT" -czf "$ARCHIVE" "rite-${TARGET}"
echo "==> wrote ${ARCHIVE}"

# Append / write checksum line
SUMS="${OUT}/SHA256SUMS"
(
  cd "$OUT"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "rite-${TARGET}.tar.gz"
  else
    shasum -a 256 "rite-${TARGET}.tar.gz"
  fi
) >> "$SUMS.tmp" 2>/dev/null || true

# De-dupe checksum file by asset name
if [[ -f "$SUMS.tmp" ]]; then
  if [[ -f "$SUMS" ]]; then
    cat "$SUMS" "$SUMS.tmp" | awk '!seen[$2]++' > "$SUMS.new"
    mv "$SUMS.new" "$SUMS"
  else
    mv "$SUMS.tmp" "$SUMS"
  fi
  rm -f "$SUMS.tmp"
fi
echo "==> checksums in ${SUMS}"
ls -la "$ARCHIVE"
