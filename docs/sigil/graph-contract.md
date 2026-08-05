# The normalized Sigil graph, version 1

What the renderer draws. `rite.sigil.graph` — the input model `rite-sigil`
defines for itself, and that Cant is adapted into.

**Stability: experimental.** A change to the shape bumps `version` and appears in
the changelog.

## Why this is not the Cant graph

`cant.graph` is a good graph and Sigil does not consume it directly. Three
reasons, argued in full in
[ADR 0006](../adr/0006-sigil-consumes-a-normalized-graph.md):

- **It is Cant's type.** Depending on it drags a parser into every build that
  wants to draw a picture from a JSON file.
- **The shapes differ.** Sigil needs `Effect`, `Output` and `Unknown`, and Cant
  has none of them — the first is a `!` plus capability metadata, the second is
  an identifier pointing at whatever was last, and the third would mean opening
  a closed enum.
- **Something has to be untrusted.** Graph JSON pasted into a web page is
  attacker input, and the boundary has to exist somewhere specific.

## What it deliberately does not have

**No coordinates.** Not as a field, not as a hint, not as an option. Geometry is
computed from topology and lives in [the scene](scene.md). Cant's `LayoutHint`
round-trips through Cant's own JSON and does not arrive here — a hostile or stale
hint cannot move a semantic mark ([ADR 0004](../adr/0004-sigil-layout-is-non-semantic.md)).

A test asserts it against the *type*, not just an instance: a serialized graph is
checked for `x`, `y`, `width`, `height`, `radius`, `angle`, `rotation` and
`layout`, and `graph.rs` is scanned for public fields with those names.

## Shape

```json
{
  "schema": "rite.sigil.graph",
  "version": 1,
  "source_language": "cant",
  "source_schema": { "name": "cant.graph", "version": "2" },
  "entry": "n0",
  "exits": ["n4"],
  "nodes": [ … ],
  "edges": [ … ],
  "regions": [ … ],
  "metadata": { … }
}
```

Identifiers are strings, not integers. Sigil takes graphs from more than one
producer, and requiring each to number its nodes densely from zero is a
constraint the renderer has no reason to impose. They are sanitized into
SVG-safe element IDs at serialization; the raw value is preserved so a diagnostic
names what the author wrote.

### Nodes

```json
{
  "id": "n2",
  "kind": "effect",
  "region": "r0",
  "effect": {
    "performs": true,
    "capabilities": [{ "family": "fs" }]
  },
  "source": { "span": { "start": 22, "end": 35 } }
}
```

`kind` is one of `source`, `stage`, `ward`, `scatter`, `collect`, `fork`,
`orbit`, `effect`, `output`, `literal`, or `unknown` with the producer's own name
carried alongside.

**An unknown kind renders.** It gets the fallback mark, a Codex warning, and
keeps its connectivity. It does not abort a render and it never panics — §6.3's
requirement, and a property test over arbitrary kind strings.

#### `effect`

The field the whole version-1 `cant.graph` change existed to feed.

- `performs` — the node invokes the capability. Cant's `!`. This is what earns a
  place on the outer boundary.
- `capabilities[].family` — **always present**. One of `fs`, `net`, `db`,
  `console`, `clock`, `random`, `env`, `process`, `mcp`, or a producer's own
  string. It decides which invocation mark the node gets, so layout cannot work
  without it.
- `capabilities[].name` — `@fs.read`. **Absent unless labels were requested.**

The split is the point. A family is a classification this renderer invented; a
name is text the user wrote. Carrying the name unconditionally would put the
user's source in the Codex of every Veiled render, so the privacy decision is
made once at the adapter rather than filtered out at each place that might
display it ([ADR 0007](../adr/0007-veil-and-source-privacy.md)).

**`capabilities` is not `performs`.** A node can name a capability without
invoking it, and only an invocation reaches the boundary.

### Edges

```json
{ "id": "e3", "from": {"node": "n1", "port": 2}, "to": {"node": "n5"},
  "ordinal": 1, "kind": "enter", "region": "r0" }
```

`kind` is `flow`, `enter`, `join`, or `feedback` — the only cycle.

**Branch order lives in `ordinal`, not in array position.** `edges_from` sorts by
it before returning, so a consumer that reordered the edge list still gets the
same sectors. There is a test that shuffles the region array and asserts every
branch lands in the same place.

### Regions

```json
{ "id": "r0", "kind": "orbit", "owner": "n3", "ordinal": 0,
  "members": ["n4", "n5"], "entry": "n4", "exits": ["n5"] }
```

`kind` is `branch` (gets an angular sector), `orbit` (gets a closed ring), or
`group` (neither). `parent` nests them. Parenthood must be a forest — a cycle is
an error, because concentric rings need an outermost one.

### Metadata

`source_name`, `source_length`, `producer`, `producer_version`, and `extra`.

**None of it is fingerprinted.** Renaming a file must not change the artifact,
and a `cant` release that changed no graph must not invalidate every cached
render.

## Validation

Every graph is validated, including one Sigil produced itself — the adapter and a
hostile JSON file arrive at the same function, so a check that only runs on one
path does not run.

Errors mean the graph cannot be drawn. Warnings mean it can, but something is
worth saying.

| Code | Severity | Meaning |
|---|---|---|
| `SIGIL-G001` | error | No entry, or the entry names a node that is not here |
| `SIGIL-G002` | error | An edge, region, entry or exit names an unknown node |
| `SIGIL-G003` | error | Duplicate identifier |
| `SIGIL-G004` | error | Unknown region reference |
| `SIGIL-G005` | error | Region parenthood cycle |
| `SIGIL-G006` | warning | Node unreachable from the entry; drawn detached |
| `SIGIL-G007` | warning* | Unknown node kind; drawn with the fallback mark |
| `SIGIL-G008` | warning | No exit; the composition has no closing seal |
| `SIGIL-G009` | error | No nodes |
| `SIGIL-G010` | error | A node claimed by two regions |
| `SIGIL-S001` | error | Over the node cap |
| `SIGIL-S002` | error | Over the edge cap |
| `SIGIL-S003` | error | Region nesting too deep |
| `SIGIL-S004` | warning | Label truncated |
| `SIGIL-S005` | error | Input too large |
| `SIGIL-S007` | warning | Past the size where a sigil stays legible |
| `SIGIL-S008` | error/warning | Span ends before it starts / past the source |
| `SIGIL-V001` | error | Not a `rite.sigil.graph` |
| `SIGIL-V002` | error | Unsupported schema version |

\* an error under `strict_unknown_kinds`, which conformance tooling sets.

`SIGIL-S006` (non-finite number) is reserved and does not fire during graph
validation. It cannot: `serde_json` rejects `1e400` at parse time, `NaN` and
`Infinity` are not JSON, and `Number::from_f64` refuses both — so a check there
would be unreachable code wearing the appearance of a safety net. It belongs to
the scene bounds pass, where `f64` arithmetic can genuinely produce one. A test
pins the `serde_json` behaviour so the omission becomes a real gap the moment
that stops being true.

### Limits

```text
soft warning     250 nodes
node cap       2,000
edge cap       8,000
region depth     128
label            4 KiB   (truncated, not rejected)
input           16 MiB native / 2 MiB browser
```

All configurable natively. Only the input ceiling differs in the browser — the
node and edge caps are about whether a picture is legible, which is not a
platform question.

## Fingerprint

SHA-256 over the canonical serialization, truncated to 128 bits, rendered as 32
lowercase hex characters. The first 64 bits are the default render seed, so a
user who copies a fingerprint out of a render's metadata can reproduce it.

Canonical form sorts object keys at every depth, omits absent and default fields,
writes integral floats as integers, and strips the non-semantic metadata listed
above along with source snippets. Spans are kept: moving a stage moves the
program.

`DefaultHasher` was rejected — `SipHash`'s output is not stable across Rust
releases, so every toolchain bump would have re-seeded every sigil in existence.

## Adapting from Cant

`cant_sem::to_sigil_graph(&program, options)`. Infallible: everything that could
fail is `cant_sem::validate`'s job, and Sigil validates again on its own terms.

Three decisions it makes that have no Cant counterpart:

- **An effectful node becomes an `Effect`**, whatever stage it was written as.
  Placement on the boundary is what an effect *is* in the visual grammar.
- **The exit becomes an `Output`** — unless it is already a `collect`, which is
  its own seal. Promoting it would erase the difference between "the values came
  together" and "the program ended".
- **`Unknown` never arises from a live `CantProgram`**, whose `NodeKind` is
  closed. It exists for graphs from a newer producer, read through Sigil's own
  JSON reader.

`AdaptOptions::default()` carries no labels and no snippets. `with_labels()` is
what `--mode revealed` uses.
