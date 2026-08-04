# The Sigil scene, version 1

`rite.sigil.scene` — geometry, layered, with every element traceable back to the
graph. What `cant sigil --format scene-json` will emit.

**Stability: experimental**, and the specification says so. It moves
independently of both graph schemas.

## Why a scene exists at all

So that a layout regression and a serialization regression are different test
failures.

A renderer that went from graph nodes to SVG strings would have one place to look
when a picture came out wrong, and every change to the SVG writer would move the
goldens whether or not any node moved. Scene JSON is the fast loop: it is
diffable, it has no styling in it, and it can be asserted on structurally.

The golden scene snapshots landed **before** there was an SVG writer, which is
the only time that separation is free.

## The pipeline

```text
SigilGraph  →  analyze()  →  Topology  →  build_scene()  →  SigilScene
```

`analyze` answers questions about the graph; `build_scene` decides geometry.
They are separate because depth is used by both fork sector width and ring
radius, and two implementations that disagreed by one would put a branch's nodes
in a band its own boundary did not cover.

## Composition

```text
viewBox      0 0 1600 1600
center       800 800
safe radius  700
```

Radius is allocated in bands by what a node *is*, never by what it is called:

| Band | Fraction of safe radius | What lives there |
|---|---|---|
| Core | 0.00–0.15 | the entry |
| Flow | 0.15–0.65 | flow, and the regions nested in it |
| Seal | 0.65–0.85 | joins, seals, region exits, detached nodes |
| Boundary | 0.85–1.00 | invocations |

The canonical axis is north (`-π/2`); angles increase clockwise from it, which is
the direction branch 0 begins in.

### Placement

One decision, made once, in this priority order:

1. **Detached** — unreachable from the entry.
2. **Invocation** — the node performs an effect. Outranks everything else,
   because reaching the host world is the strongest thing a node does; an
   effectful node inside an orbit body still belongs on the boundary.
3. **Core** — the entry.
4. **Seal** — an exit, or an `output`.
5. **Ring** — a member of an orbit's body.
6. **Flow** — everything else.

### The four structures

**The spine spirals.** Angle advances with position along the top-level chain,
radius with depth, sweeping 0.82 of a turn — less than a full one so the end does
not land on its own beginning and read as a join. This is what makes an unlabelled
render still show where a program starts and which way it goes.

Depth is **longest path** from the entry, not shortest. A shortcut edge would
otherwise let a node sit at the same radius as its own predecessor, and two nodes
at the same radius on the same spoke overlap.

**A fork fans.** Branches take angular sectors clockwise by ordinal, widths
weighted by content — a branch holding an orbit needs room for a ring, one
holding a single stage does not. Widths are floored at a minimum and then
renormalized, because a floor that overflows its own budget is how branches end
up overlapping.

**An orbit rings.** Body members go on a closed circle centred on the orbit node,
starting at the entry notch and running clockwise, on a circumference sized to
hold them at the minimum separation.

Rings are laid out **after** collision resolution, from the orbit node's settled
position. This ordering is the design, not a convenience: the collision pass
nudges in polar coordinates about the *canvas* centre, and applying that to a
ring member walks it straight off the circle it is supposed to sit on. A ring
settles as a unit. What its members can still do is overlap something outside the
ring, and that is reported rather than left silent.

**An invocation is pulled outward.** It keeps the angle it would have had in the
flow and moves to the boundary band, so the spoke back to its calling position
shows which stage reaches the world.

### Collision resolution

Deterministic, bounded, and in §11.7's priority order: band ownership is absolute,
angle is adjusted first, radius second and only within the node's own band, and
an unresolved overlap is a warning rather than a silent stack.

A sorted single pass over a total order — placement, then radius, then angle,
then identifier — rather than a relaxation loop, because a loop's result depends
on how many iterations it happened to run.

## Layers

Paint order, back to front:

```text
background · guide-geometry · ornament-back · semantic-regions ·
semantic-edges · semantic-nodes · inscriptions · ornament-front · interaction
```

`is_semantic()` and `is_ornament()` are the single answer to "may this be
dropped?", so the invariance test, the hit-region builder and the SVG writer
cannot disagree. `background` is neither: it is a fill, not a decoration and not
a meaning.

**Ornament carries no graph reference.** An ornament element that could be
selected would put a meaningless entry in the Codex, so `graph_ref` is `None`
exactly when an element is ornament, and a test asserts it.

Dropping the ornament layers must leave every remaining coordinate bit-for-bit
what it was. That is a property test, not a convention.

## Elements

```json
{
  "id": "node/n2",
  "layer": "semantic-nodes",
  "semantic": { "node": "effect" },
  "graph_ref": { "kind": "node", "id": "n2" },
  "geometry": "mark",
  "center": { "x": 1123.4, "y": 402.1 },
  "size": 30.0,
  "rotation": 0.41,
  "path": [],
  "title": "fs invocation",
  "legend_key": "node/n2",
  "bounds": { "x": 1093.4, "y": 372.1, "width": 60.0, "height": 60.0 }
}
```

Element IDs are derived from the graph reference and a role, **never from draw
order** — an ID that changed whenever something moved would be useless for the
three things that need it: the Codex mapping a click back to a node, the
accessible summary, and a stable SVG element ID that survives a re-render.

Identifiers are sanitized: anything outside `[A-Za-z0-9._-]` becomes `_`. The raw
value stays in `graph_ref`, so the Codex still shows what the author wrote.

Geometry is one of `circle`, `arc`, `polygon`, `path`, `mark`, `text`.
Deliberately few — a scene made of a hundred primitive types would be a drawing
format. Phase 3's generated marks arrive as path data inside `mark`, not as new
variants.

### Titles never carry a label

`title` is the node's *kind* — `"orbit"`, `"fs invocation"` — never its label. A
title carrying source text would put it in a Veiled render's accessibility tree,
which is exactly the leak the disclosure/metadata split exists to prevent.

The Codex may carry the label. That is what a Codex is for, and it is present
only because the graph carried one at all.

## Hit regions and the Codex

Hit regions are separate from drawn elements, because the drawn thing may be a
two-pixel stroke and a keyboard target has to be reachable — never smaller than
22 units whatever the mark's size.

`tab_index` follows **graph order**, so keyboard traversal follows the program
rather than the accident of where things landed.

Each node gets a legend entry: kind, safe summary, label when present, span,
capabilities, region, branch ordinal, attributes, and warnings.

## The accessible summary

Generated from the census, in the order of the visual grammar centre outward, so
the sentence reads in the order someone would trace the picture:

```text
This sigil contains one source, seven stages, one ward, two invocations,
and one output seal.
```

## Comparison goes through text

**`serde_json` does not round-trip every `f64` exactly.** Writing
`927.9171087042969` and reading it back yields `927.9171087042968` — one unit in
the last place low. The writer is correct; the parser mis-rounds.

That is load-bearing for the native/WASM parity requirement. "Native scene JSON
equals browser scene JSON" cannot be checked by deserializing both and comparing
structures, because deserialization is the step that loses information.
Comparisons compare **canonical text**, produced from the live `f64` values on
each side and never parsed back.

`float_round_trip_is_not_exact_which_is_why_text_is_canonical` pins this. If
`serde_json` ever fixes it, that test fails and the note can go — the
text-comparison discipline should stay regardless.

## Fixtures

`fixtures/sigil/scenes/*.scene.json`, one per example in `examples/sigil/`,
written under the canonical orientation — a golden written against a seeded
rotation would be a golden of the seed, not of the layout.

```bash
cargo test -p cant-sem --test sigil_fixtures
SIGIL_BLESS=1 cargo test -p cant-sem --test sigil_fixtures   # regenerate
```

**The snapshot is not the assertion.** Every fixture is also checked
structurally: node kinds present, band membership, branch order, determinism,
every graph node and edge represented, no source text present, nothing escaped
into an element ID. The snapshot's job is to make a *diff* readable when one of
those starts failing.

A missing fixture is a failure, not a skip.

## Measured performance

Release build, per scene including JSON serialization:

| Nodes | Scene + JSON | §24 target (scene + SVG) |
|---|---|---|
| 25 | < 1 ms | 25 ms |
| 100 | 4 ms | 100 ms |
| 500 | 46 ms | 1000 ms |

Scene JSON is roughly 1.1 KiB per node. The collision pass is O(n²) by
construction; `crates/rite-sigil/tests/performance.rs` measures rather than
enforces, with assertions an order of magnitude loose so CI hardware variance
does not produce failures nobody investigates.
