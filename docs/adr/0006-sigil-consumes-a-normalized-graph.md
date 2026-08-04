# ADR 0006 — Sigil consumes a normalized adapter graph

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0003 — Sigil is a semantic renderer, not a runtime](0003-sigil-is-a-renderer-not-a-runtime.md) ·
  [ADR 0001 — Cant is a sibling front end](0001-cant-sibling-frontend.md)

## Context

`cant_sem::CantProgram` is a good graph. It was built for this: nodes, directed
edges, numbered ports, branch ordinals, subgraph ownership, source spans, orbit
policy, and an effectful flag per leaf, with identifiers assigned by a
deterministic depth-first walk so the JSON is snapshot-testable. Its module doc
says lowering reads the graph rather than the AST specifically so that a future
Sigil consumes what executes.

The obvious move is for `rite-sigil` to take a `&CantProgram` and draw it. Three
things make that wrong.

**It is Cant's internal type.** `rite-sigil` would depend on `cant-sem`, which
depends on `cant-syntax`, `rite-sem` and `rite-fmt`. The specification requires
that `rite-sigil` not depend on Cant parsing internals, and the practical reason
is the WASM boundary: a renderer that types its input as Cant's AST-adjacent
graph drags Cant's whole front end into every build that wants to draw a picture
from a JSON file.

**The shapes genuinely differ.** Sigil's visual grammar needs node kinds Cant's
graph does not have. `Effect` is an outer-boundary invocation; in Cant it is a
`bool` on a leaf plus capability names recoverable from leaf text. `Output` is
the closing seal; in Cant it is `CantProgram::exit`, a node identifier pointing
at whatever kind happens to be last. `Unknown` must exist so that a graph written
by a newer `cant` renders instead of panicking; Cant's `NodeKind` is a closed
enum with no such variant, and should stay closed.

**Something has to be untrusted.** Graph JSON pasted into a web page is attacker
input. Validation, limits, ID sanitization and label bounding have to happen at a
boundary that exists. If the renderer's input type is Cant's type, the boundary
is wherever someone remembered to put it.

The alternative to an adapter is worse than it looks: Sigil would recover
capability families by reading `@fs.read` out of leaf text — inferring critical
semantics from labels, which §7 of the specification forbids by name, and which
breaks the moment a leaf says `@fs.read` inside a string.

## Decision

**`rite-sigil` defines its own normalized input model, `SigilGraph`, and Cant is
adapted into it. The renderer never sees a Cant type.**

Binding:

1. `rite-sigil`'s dependencies are `rite-core` (spans, diagnostics), `serde`,
   `serde_json`, and deterministic utility crates. **Not** `cant-syntax`,
   `cant-sem`, `cant`, `rite-sem`, `rite-syntax`, `rite-runtime`, or `rite-caps`.
   A test reads the manifest and fails on any of them.
2. The adapter — Cant's `CantProgram` (or its JSON) into `SigilGraph` — lives on
   the Cant side of the boundary, where knowing both shapes is allowed by ADR
   0001's `cant-* -> rite-*` edge.
3. `SigilGraph` is versioned as `rite.sigil.graph` independently of
   `cant.graph`. The two schemas move on their own schedules, and the normalized
   graph records which source schema produced it.
4. **Semantics come from graph fields, never from label text.** Where the
   information Sigil needs is absent from the Cant graph, the Cant graph contract
   is extended and versioned — that is the correct fix, and it is what §6.2
   requires. Recovering meaning by pattern-matching a label is prohibited; the
   only thing a label may drive is a label.
5. `SigilGraph` carries no coordinates (ADR 0004), a canonical serialization with
   sorted maps and stable number formatting, and a fingerprint derived from it
   that is the default render seed.
6. Unknown node kinds inside a supported schema version adapt to
   `SigilNodeKind::Unknown(String)`, render through a generic fallback mark, and
   raise a warning. They do not abort a render and they never panic.
7. Validation and limits are the adapter's job and run on every path, including
   graphs Sigil itself produced. A graph is untrusted regardless of provenance —
   the same rule `cant_sem::validate_deserialized` already applies.

## Consequences

**Good.** `rite-sigil` is testable and fuzzable from JSON alone, with no Cant in
the build. That is what makes the fuzz targets in §26.6 cheap enough to actually
run.

**Good.** A second producer becomes possible without touching the renderer. The
deferred `rite sigil` — a Rite semantic-graph projection — is an adapter, not a
fork.

**Good.** The WASM package stays small, because the renderer half of it has no
parser in its dependency graph.

**Cost.** Two graph models and a translation between them, kept in step by
round-trip and golden tests. This is duplication, and it is the price of the
dependency direction; the mitigation is that the adapter is a single file with a
fixture per node kind rather than a layer spread across the renderer.

**Cost.** Cant's graph contract has to grow to carry what Sigil needs — per-node
capability metadata is the concrete case, because deriving it textually is
exactly the label-inference this ADR forbids. That is a versioned change to a
published schema and it belongs to Cant, not to Sigil.

**Risk accepted.** `SigilGraph` will initially look like `CantProgram` with extra
fields, which invites the question of why both exist. The divergence is real and
already visible in `Effect`, `Output` and `Unknown`, and it widens the first time
a second producer or a second Cant schema version appears.

## Alternatives rejected

**`rite-sigil` takes `&CantProgram` directly.** Rejected on all three counts
above: it inverts the dependency the specification requires, it drags Cant's
front end into the browser build, and it leaves the untrusted-input boundary
undefined.

**Sigil reads Cant graph JSON with its own serde types that mirror Cant's
exactly.** Rejected: same shape mismatch, and it converts a compile-time
dependency into an undeclared structural one — Cant could change its JSON and
nothing would fail until a picture came out wrong.

**Put `SigilGraph` in `rite-core` so both sides can name it.** Rejected: it makes
every Rite crate carry the renderer's input model, and ADR 0001's rule that Rite
crates never depend on Cant has a sibling here — Rite core does not depend on
Sigil either.

**Infer capability family from leaf text in the renderer.** Rejected explicitly.
`cant_sem::graph::capabilities_in` already does this textually and carefully on
the *Cant* side, where it is a producer's summary of its own source; it is not a
model for a renderer deciding what a node means. Sigil reads a field or draws the
unknown mark.
