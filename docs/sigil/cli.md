# `cant sigil`

Render a Cant program — or a `cant.graph` document — as a sigil.

```bash
cant sigil program.cant                          # program.sigil.svg, veiled
cant sigil program.cant --mode revealed          # with labels drawn
cant sigil program.cant --format png --width 2400
cant sigil program.cant --format html            # self-contained interactive page
cant sigil -e '[1,2,3] -> * -> $ * 2 -> []'      # an expression, no file
cant graph program.cant --format json | cant sigil --graph -
```

`cant sigil --help` is the authoritative flag list; this page is the map of
which flags belong together and the decisions behind the defaults.

## Input

One of three sources, checked in this order:

- a positional `FILE` (or `-` for stdin) of Cant source,
- `-e EXPRESSION`, quoted, for a program typed at the shell,
- `--graph PATH` (or `-`) for a `cant.graph` JSON document — the same thing
  `cant graph --format json` emits, so the two commands compose.

## Output

`--output`/`-o` names the file (`-` for stdout); without it the input's name
plus `.sigil.<ext>`. `--format` picks the artifact:

| Format | What you get |
|---|---|
| `svg` | the artifact, deterministic bytes (default) |
| `png` | rasterised via resvg; `--width` sets pixels, `--scale` scales the default |
| `html` | one self-contained interactive page: pan, zoom, hover, and a Codex |
| `scene-json` | the `rite.sigil.scene` document, for tooling |

HTML always carries labels for its Codex unless `--metadata none` — disclosure
still governs what the canvas draws, so `--format html --mode veiled` is a
veiled picture with a decodable legend beside it. `--embed-scene` additionally
embeds the scene JSON, and requires `--metadata full` because a scene carries
labels.

## The look

- `--theme neon-ritual|void|parchment` — see [themes.md](themes.md).
- `--tracery flowing|concentric|circuit` — how traces are drawn; changes every
  edge's shape and no mark's position.
- `--ornament none|sparse|ritual|maximal` — non-semantic geometry only;
  removing it never moves anything.
- `--background theme|transparent|#rrggbb`.
- `--simplify` draws skeleton marks only, for a graph too dense for full
  variation.
- `--weights PATH` — a `cant.trace` document from `cant run --trace-out`. The
  render becomes a picture of what the program *did*: every trace scales with
  how many emissions left its source node, hot paths bright, never-ran branches
  faint. Positions do not move (ADR 0004) and the weights join the graph, so a
  weighted render has its own fingerprint.
- `--diff OLD.cant` — the review picture: the old program's semantic geometry
  ghosted beneath the new render, anonymous and faint. Layout determinism is
  what makes it honest — everything unchanged sits exactly under its ghost —
  so `--canonical` is required (two seeded rotations would read as everything
  moving) and the output is SVG, fingerprinted `format=svg-diff`.

## Disclosure and metadata

Two independent axes, deliberately (ADR 0007):

- `--mode veiled|inscribed|revealed` — what the *picture* shows.
- `--metadata full|safe|minimal|none` — what the *file* embeds (titles,
  identifiers, the fingerprint, snippets).

`--mode revealed --metadata none` is meaningful — labels drawn, nothing
embedded — and the CLI warns when a pairing looks like a privacy mistake
rather than silently resolving it.

## Determinism

- `--seed graph|canonical|INTEGER` — the default seed *is* the graph
  fingerprint, so the same program produces the same picture on any machine.
- `--canonical` — the documented fixed orientation and seed; what golden tests
  and reproducible-output workflows use.
- `--check` prints the render fingerprint —
  `sigil/<version> graph=… theme=…@… tracery=… seed=… mode=… metadata=… format=…`
  — which two parties can compare instead of shipping bytes.

## Limits

`--max-nodes N` refuses a graph larger than N before laying anything out. The
built-in ceilings live in `crates/rite-sigil/src/limits.rs`; the browser build
uses stricter ones than the CLI, because a tab should refuse rather than hang.
