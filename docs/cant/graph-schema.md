# The Cant graph, version 3

The serialized form of a Cant program: what `cant graph --format json` emits, and
what a renderer (Sigil) consumes.

**Stability: experimental.** This page is the contract, and a change to the shape
bumps `version` and appears in the changelog, but a single-digit version means it
can still change. Treat a stored graph as readable only by the tool version that
wrote it.

`cant version` reports the schema a binary speaks:

```bash
$ cant version
cant_graph_schema_version: 3
```

Check it and refuse rather than guess. Cant's own reader does:

```text
graph schema version `99`, expected `3`
```

and, before it looks at the number at all:

```text
graph schema `rite.sigil.graph`, expected `cant.graph`
```

Name first, because a version is only meaningful once it is known whose it is.

## What changed in version 3

Whether a fork's branches run concurrently.

- **`parallel` on a `fork` node** — `true` for `|{ a ; b }:par`, `false`
  otherwise. Always present, in both states: a consumer deciding whether two
  branches' effects have an order between them has to read an answer rather than
  infer one from a missing key.

Nothing else moves. A parallel fork has the same branches, the same ordinals and
the same two-port shape as a sequential one, because it *is* one — the flag says
how the branches are scheduled, not what they are. Sigil draws it with the fork's
existing mark and says "parallel" in the label; a distinct visual treatment for
concurrency is future work.

See `docs/adr/0012-parallel-fork-is-a-modifier.md`. Version 2 graphs are refused
rather than defaulted to `false`, for the same reason version 1 graphs were: a
reader that guessed would be claiming an ordering the producer never stated.

## What changed in version 2

The failure path is in the graph. Previously an error was handled inside a stage
— `unwrap_or($, "")` is a stage like any other — so where a program's failures
went could not be read off the topology at all.

- **`rescue`** — a node kind, with a `handler` subgraph id. Its out port 1 enters
  the handler and its in port 1 is where the handler rejoins, the same two-port
  shape an orbit uses for its body.
- **`rescue`** — an edge role, on the edge from that out port into the handler.
  Its own role rather than `enter` so that "which edge is the failure path" is a
  field rather than an inference from the nodes at each end. The handler returns
  along a `join`, being a concatenation point like a fork branch's.

See `docs/adr/0010-error-routing-is-a-rescue-stage.md`. Version 1 graphs are
refused, not upgraded — a version 1 consumer walking a version 2 document
would read a rescue's continuation and its failure path as the same kind of edge.

## What changed in version 1

Three additions, all for the same reason: a renderer must not have to infer
meaning from a label.

- **`schema`** — the constant `"cant.graph"`. A consumer that reads more than one
  graph format needs something to dispatch on before it trusts a version number.
- **`producer`** — `{ "name", "version" }`, so a stored graph says what wrote it.
  Diagnostic metadata; it is not part of the graph's meaning and must not be
  hashed into anything a consumer caches on.
- **`capabilities` on a node** — the host capabilities that node's leaf names,
  each with the family it belongs to. Previously a consumer wanting to know
  whether a node touched the filesystem or the network had to re-scan the leaf
  text for `@fs.`; now it reads a field. See
  `docs/adr/0006-sigil-consumes-a-normalized-graph.md`.

Version 0 graphs are refused, not upgraded.

## The contract

**A consumer never has to parse Cant source.** Everything a renderer needs
is in the JSON: structure, order, spans, effects, orbit policy and labels. Nothing
requires the `.cant` file except showing the user their own text, and the spans
are there to make even that possible.

Here is a program with every construct in it, and its graph:

```cant run
4 -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []
```

![The flow graph for a fork containing a ward and an orbit](graphs/nested.svg)

Clusters are subgraphs: a fork branch or an orbit body. Dashed edges enter and
rejoin them; the bold pink edge is an orbit's feedback. Everything in that
picture comes from the JSON below.

## Shape

```json
{
  "schema": "cant.graph",
  "version": "3",
  "language_version": "0",
  "producer": { "name": "cant", "version": "0.3.0" },
  "entry": 0,
  "exit": 4,
  "nodes": [ … ],
  "edges": [ … ],
  "subgraphs": [ … ],
  "source": { "name": "program.cant", "length": 47 }
}
```

`entry` and `exit` are node identifiers: the first and last node of the top-level
flow. `source.length` is in bytes, so a consumer can tell whether a span it holds
is still in range for the text it has.

`producer.version` is Cant's own number, not Rite's; the two version separately
(ADR 0001, Amendment 2). It is there to make a bug report legible and for nothing
else: a consumer that mixed it into a cache key or a fingerprint would invalidate
every stored artifact on a release that changed no graph.

A program's **named flows are not here**, and there is no key for them. A
definition is spliced into the flow that used it before the graph is built, so
what a consumer reads is the program that runs, and a definition used twice is
two copies of its nodes. See
`docs/adr/0011-named-flows-are-spliced.md`; recording the names and the splice
points would be another version, and nothing needs them yet.

`uses` is the program's leading `use` imports, as module names in source order.
Additive and absent when empty, so graphs predating the field serialize
byte-for-byte as they always did. The names are emitted verbatim into the
expansion; Cant never resolves them, so the graph records what was *asked for*,
not what exists.

### Nodes

```json
{
  "id": 2,
  "kind": "ward",
  "predicate": { "text": "$ > 1", "span": { "start": 17, "end": 22 },
                 "effectful": false, "placeholder": true },
  "span": { "start": 15, "end": 24 },
  "subgraph": 0
}
```

`kind` is one of `source`, `stage`, `scatter`, `collect`, `ward`, `fork`,
`orbit`, `rescue`, and its payload is flattened alongside it:

| `kind` | payload |
|---|---|
| `source`, `stage` | `expr`: a leaf |
| `scatter`, `collect` | — |
| `ward` | `predicate`: a leaf |
| `fork` | `branches`: subgraph ids, **in branch order** · `parallel`: whether they run concurrently |
| `orbit` | `body`: a subgraph id · `identity`: an optional leaf (`:by`) · `max_items`: the `:max` |
| `rescue` | `handler`: a subgraph id |

A **leaf** is Rite expression text with what Cant knows about it on its own:
`text`, `span`, `effectful` (it carries a `!`), and `placeholder` (it names `$`).
Whether the names in it resolve is Rite's question, not the graph's.

`subgraph` is absent for a node in the top-level flow.

`label` and `layout` are absent unless something put them there; see below.

### Node capabilities

```json
{
  "id": 2,
  "kind": "stage",
  "expr": { "text": "! @fs.read($)", "span": { "start": 22, "end": 35 },
            "effectful": true, "placeholder": true },
  "span": { "start": 22, "end": 35 },
  "capabilities": [ { "name": "@fs.read", "family": "fs" } ]
}
```

Every host capability the node's leaf names, deduplicated, in source order.
**Absent when empty**, which is the common case.

- `name` is the full spelling including the `@`.
- `family` is the namespace before the first dot. This is the field a renderer
  groups by, and decides which invocation mark a capability gets, so it is
  stored rather than left for every reader to re-split, each of whom would have
  to agree independently about what `@fs` with no dot means.

The scan is textual, over leaf text the lexer already separated from strings and
comments, so `@fs.read` inside a string literal is not reported. It runs once,
during lowering. `CantProgram::capabilities()` and `capability_families()` read
these fields rather than re-scanning, so a program-wide summary and a consumer
walking the nodes cannot disagree.

**`capabilities` is not `effectful`.** A node can name a capability without
performing an effect, and only a `!` makes it an effect. Read `capabilities` for
*which world it touches* and the leaf's `effectful` for *whether it touches it*.
A renderer placing invocation marks on an outer boundary wants both.

### Edges

```json
{ "from": { "node": 1, "kind": "out", "index": 2 },
  "to":   { "node": 5, "kind": "in",  "index": 0 },
  "ordinal": 1,
  "role": "enter" }
```

Ports are explicit and numbered:

- **out port 0** is the continuation: the value leaving along the main flow.
- **a fork's out port *n+1*** enters branch *n*; **an orbit's out port 1** enters
  its body; **a rescue's out port 1** enters its handler.
- **in port 0** is the incoming value; **in port 1** is the join a branch, an
  orbit body or a rescue handler returns to.

`role` is one of:

| `role` | meaning |
|---|---|
| `flow` | one stage to the next |
| `enter` | a fork or orbit into its branch or body |
| `join` | a fork branch or rescue handler returning its emissions |
| `orbit_feedback` | an orbit body returning candidates to the worklist; **the only cycle** |
| `rescue` | a rescue into its handler: the failure path |

**Branch order lives in `ordinal`, not in the array order.** A consumer that
sorts or reorders the edge list must still read branch order correctly, and one
that relies on array position will be wrong the first time anything reorders.

The orbit feedback edge is a real edge rather than something implied by the node,
so that "every cycle must belong to an orbit" is a question a validator can
actually ask. Expect exactly one cycle per orbit, and draw it distinctly:
`cant graph --format dot` draws it bold, pink and `constraint=false`.

### Subgraphs

```json
{ "id": 0, "owner": 1, "entry": 2, "exit": 4, "nodes": [2, 3, 4] }
```

A fork branch, an orbit body or a rescue handler. `owner` is the node that
contains it; `nodes` is its members in flow order. `entry` and `exit` are absent for an empty branch,
which is a validation error but still has to be representable.

Every node appears once in the flat top-level `nodes` array, whatever subgraph it
belongs to. A renderer walking `nodes` sees all of them; `subgraphs` gives the
grouping. Nothing is nested, so nothing needs recursive traversal to enumerate.

## Layout is reserved, and never semantic

```json
{ "id": 3, "kind": "scatter", "span": { … },
  "label": "fan out the paths",
  "layout": { "x": 220.0, "y": 96.0, "width": 80.0 } }
```

`label` and `layout` exist for an editor to write into. **Nothing in compilation,
validation or execution reads either.** A graph with every one of them stripped
behaves identically; geometry stays out of the language. Both survive a JSON
round trip, so a renderer's work is not lost by a tool that reads and rewrites
the graph.

## Identifiers are stable, not unique

Identifiers are assigned by a depth-first walk in source order, so the same source
and tool version always produce the same numbers, so a diff of two graphs reads as
a diff of the program.

They are **not** globally unique. Two graphs from different sources both start at
`0`, so a consumer storing several must key them by graph.

## Deserialized graphs are untrusted

Cant validates a graph read from JSON on the same terms as one it built. The
checks that matter to a consumer:

- dangling edges, and ports a node does not have;
- duplicate identifiers;
- a branch that does not rejoin the fork that opened it;
- **any cycle that is not orbit feedback**, since relabelling one as
  `orbit_feedback` does not launder it.

Do not assume a graph you did not produce is valid. Run it through
`cant_sem::validate_deserialized`, or reimplement those checks.

## Reading it from Rust

```rust
use cant_sem::{validate_deserialized, CantProgram};

let analysis = validate_deserialized(&json, file_id)?;
if analysis.diagnostics.has_errors() {
    // report and stop; the graph is not safe to walk
}
for node in &analysis.graph.nodes {
    // `node.kind` is the tagged enum above
}
```

`CantProgram` also answers the three questions a renderer or an explainer asks
without walking anything: `capabilities()`, `effectful_nodes()`, and
`max_orbit_items()`.

## What could change

Things this version does not settle, so a consumer knows where the edges are:

- **Named anchors and explicit feedback edges** would add edge roles and would end
  "exactly one cycle per orbit".
- **Multi-output nodes** are representable, which is why ports are numbered
  rather than anonymous, and a rescue is the first thing to use the second one.
- **Retries** would put a policy on a rescue, and are the one place a `:max`-like
  modifier could reappear.
