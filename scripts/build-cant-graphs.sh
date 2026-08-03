#!/usr/bin/env bash
# Render the graph pictures the documentation embeds.
#
# One SVG per construct, produced by the real `cant graph --format dot` and
# Graphviz — never drawn by hand. A diagram that was drawn rather than generated
# is a diagram that can be wrong, and the whole claim of `cant graph` is that the
# picture is the program.
#
#   bash scripts/build-cant-graphs.sh
#
# Needs graphviz. The output is tracked, so this only runs when a diagram should
# change; `crates/cant-cli/tests/docs.rs` fails if a tracked SVG no longer matches
# what the current `cant` would produce.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="docs/cant/graphs"
CANT="${CANT:-$ROOT/target/debug/cant}"

if ! command -v dot >/dev/null 2>&1; then
  echo "error: graphviz is not installed (need \`dot\`)" >&2
  exit 1
fi
if [[ ! -x "$CANT" ]]; then
  echo "error: $CANT not found — cargo build -p cant-cli" >&2
  exit 1
fi

mkdir -p "$OUT"

# name<TAB>program. Kept here rather than in a data file so the caption in the
# docs and the program that produced the picture stay next to each other.
render() {
  local name="$1" program="$2"
  "$CANT" graph --format dot -e "$program" > "$OUT/$name.dot"
  dot -Tsvg "$OUT/$name.dot" -o "$OUT/$name.svg"
  # The DOT is an intermediate; only the SVG is embedded.
  rm -f "$OUT/$name.dot"
  printf '  %-14s %s\n' "$name.svg" "$program"
}

echo "==> rendering $OUT"
render flow    '[1, 2, 3] -> * -> ?{ $ % 2 = 0 } -> []'
render fork    '5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []'
render orbit   '[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 64 -> []'
render nested  '4 -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []'
render effects '"data.json" -> !@fs.read? -> @json.decode -> .name'

echo "==> done"
ls -la "$OUT"
