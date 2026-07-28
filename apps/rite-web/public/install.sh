#!/usr/bin/env bash
# Install Rite CLI (and optionally rite-lsp) from GitHub Releases — no clone required.
#
# Usage:
#   curl -fsSL https://rite.undrc.dev/install | sh
#   curl -fsSL https://rite.undrc.dev/install.sh | sh
#
#   RITE_VERSION=v0.1.0 sh install.sh
#   RITE_INSTALL_DIR=$HOME/bin INSTALL_LSP=0 sh install.sh
#
# Environment:
#   RITE_VERSION       Tag to install (default: latest release). Example: v0.1.0
#   RITE_REPO          GitHub repo (default: undercurrent-labs/rite)
#   RITE_INSTALL_DIR   Install directory (default: $HOME/.local/bin)
#   INSTALL_LSP        Install rite-lsp too (default: 1)
#   RITE_BASE_URL      Override asset base (default: GitHub releases download URL)
#   RITE_DRY_RUN       If 1, print actions only
set -euo pipefail

REPO="${RITE_REPO:-undercurrent-labs/rite}"
INSTALL_DIR="${RITE_INSTALL_DIR:-${HOME}/.local/bin}"
INSTALL_LSP="${INSTALL_LSP:-1}"
DRY_RUN="${RITE_DRY_RUN:-0}"
VERSION="${RITE_VERSION:-}"

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
CYAN=$'\033[0;36m'
DIM=$'\033[2m'
RESET=$'\033[0m'

# Status lines MUST go to stderr — resolve_version/detect_target are captured via $()
info()  { printf '%s==>%s %s\n' "$CYAN" "$RESET" "$*" >&2; }
ok()    { printf '%sOK%s  %s\n' "$GREEN" "$RESET" "$*" >&2; }
die()   { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }
need()  { command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"; }

need curl
need tar
need mktemp

# uname → rustc target triple (release asset suffix)
detect_target() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    linux)  os_part="unknown-linux-gnu" ;;
    darwin) os_part="apple-darwin" ;;
    msys*|mingw*|cygwin*)
      die "this installer is for Unix shells; on Windows download a .zip from https://github.com/${REPO}/releases"
      ;;
    *)
      die "unsupported OS: $(uname -s)"
      ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    aarch64|arm64) arch_part="aarch64" ;;
    armv7l) die "armv7 is not published yet; build from source or open an issue" ;;
    *) die "unsupported architecture: $arch" ;;
  esac

  printf '%s-%s\n' "$arch_part" "$os_part"
}

resolve_version() {
  if [[ -n "$VERSION" ]]; then
    # Accept 0.1.0 or v0.1.0
    case "$VERSION" in
      v*) printf '%s\n' "$VERSION" ;;
      *)  printf 'v%s\n' "$VERSION" ;;
    esac
    return
  fi

  info "resolving latest release for ${REPO}…"
  local json tag
  json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")" || \
    die "could not query GitHub API (network, rate limit, or no releases yet)"

  # Prefer jq when present; otherwise take the first top-level tag_name only.
  if command -v jq >/dev/null 2>&1; then
    tag="$(printf '%s' "$json" | jq -r '.tag_name // empty')"
  else
    tag="$(printf '%s' "$json" | tr ',' '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
  fi
  tag="$(printf '%s' "$tag" | tr -d '\r\n' | head -c 64)"
  [[ -n "$tag" && "$tag" != "null" ]] || die "no GitHub Releases found for ${REPO}. Tag a release (e.g. v0.1.0) or set RITE_VERSION=…"
  # Only the tag on stdout (captured by caller)
  printf '%s\n' "$tag"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "need sha256sum or shasum to verify downloads"
  fi
}

main() {
  local target version asset base url sums_url tmp dir expected actual bin
  target="$(detect_target)"
  version="$(resolve_version)"
  # Guard against accidental whitespace/newlines in captured values
  target="$(printf '%s' "$target" | tr -d '[:space:]')"
  version="$(printf '%s' "$version" | tr -d '[:space:]')"
  asset="rite-${target}.tar.gz"
  base="${RITE_BASE_URL:-https://github.com/${REPO}/releases/download/${version}}"
  url="${base}/${asset}"
  sums_url="${base}/SHA256SUMS"

  info "Rite install"
  printf '    repo:     %s\n' "$REPO" >&2
  printf '    version:  %s\n' "$version" >&2
  printf '    target:   %s\n' "$target" >&2
  printf '    asset:    %s\n' "$asset" >&2
  printf '    dest:     %s\n' "$INSTALL_DIR" >&2
  printf '    lsp:      %s\n' "$INSTALL_LSP" >&2
  printf '    url:      %s\n' "$url" >&2

  if [[ "$DRY_RUN" == "1" ]]; then
    info "dry run — would download: $url"
    exit 0
  fi

  # Not `local`: EXIT trap must still see this path under `set -u`
  RITE_INSTALL_TMP="$(mktemp -d "${TMPDIR:-/tmp}/rite-install.XXXXXX")"
  tmp="$RITE_INSTALL_TMP"
  trap 'rm -rf "${RITE_INSTALL_TMP:-}"' EXIT

  info "downloading ${asset}…"
  if ! curl -fsSL "$url" -o "${tmp}/${asset}"; then
    die "download failed: ${url}
Is there a release asset for this OS/arch? See https://github.com/${REPO}/releases"
  fi

  info "downloading SHA256SUMS…"
  if ! curl -fsSL "$sums_url" -o "${tmp}/SHA256SUMS"; then
    die "checksum file missing: ${sums_url}
Refuse to install without checksums."
  fi

  expected="$(awk -v f="$asset" '$2 == f { print $1; exit }' "${tmp}/SHA256SUMS")"
  [[ -n "$expected" ]] || die "no checksum entry for ${asset} in SHA256SUMS"
  actual="$(sha256_file "${tmp}/${asset}")"
  if [[ "$expected" != "$actual" ]]; then
    die "checksum mismatch for ${asset}
  expected: ${expected}
  actual:   ${actual}"
  fi
  ok "checksum verified"

  info "extracting…"
  tar -xzf "${tmp}/${asset}" -C "$tmp"
  dir="${tmp}/rite-${target}"
  if [[ ! -d "$dir" ]]; then
    # allow flat tarball
    if [[ -f "${tmp}/rite" ]]; then
      dir="$tmp"
    else
      die "unexpected archive layout (missing rite binary)"
    fi
  fi
  [[ -f "${dir}/rite" ]] || die "archive missing 'rite' binary"

  mkdir -p "$INSTALL_DIR"
  install -m 755 "${dir}/rite" "${INSTALL_DIR}/rite"
  ok "installed ${INSTALL_DIR}/rite"

  if [[ "$INSTALL_LSP" == "1" && -f "${dir}/rite-lsp" ]]; then
    install -m 755 "${dir}/rite-lsp" "${INSTALL_DIR}/rite-lsp"
    ok "installed ${INSTALL_DIR}/rite-lsp"
  fi

  # PATH hint
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      printf '\n%sNote:%s %s is not on your PATH.\n' "$DIM" "$RESET" "$INSTALL_DIR"
      printf 'Add to your shell rc, for example:\n\n'
      printf '  export PATH="%s:$PATH"\n\n' "$INSTALL_DIR"
      ;;
  esac

  if command -v "${INSTALL_DIR}/rite" >/dev/null 2>&1 || [[ -x "${INSTALL_DIR}/rite" ]]; then
    info "version check:"
    "${INSTALL_DIR}/rite" version 2>/dev/null || "${INSTALL_DIR}/rite" --version 2>/dev/null || true
  fi

  printf '\n%sRite is installed.%s Docs: https://rite.undrc.dev/docs\n' "$GREEN" "$RESET"
  printf 'Studio (no install): https://rite.undrc.dev/studio\n'
}

main "$@"
