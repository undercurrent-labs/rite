# ADR 0008 — Graphviz stays the technical view; Sigil owns its own layout

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0004 — Sigil layout is non-semantic](0004-sigil-layout-is-non-semantic.md) ·
  [ADR 0005 — One renderer, in Rust](0005-one-renderer-in-rust.md)

## Context

`cant graph --format dot` already exists. `crates/cant-sem/src/dot.rs` emits DOT
with clusters for subgraphs, dashed enter/join edges, a bold pink orbit feedback
edge drawn `constraint=false`, and colours matching `grammar/palette.json`. CI
installs Graphviz and `cant-sem/tests/dot_renders.rs` pipes the output through
`dot`, requiring exit 0 *and* empty stderr. The pictures in
`docs/cant/graphs/` come from it.

So there is a working layout engine in the repository, and Sigil needs a layout
engine. Reusing it would mean: shell out to `dot`, parse the coordinates back,
and draw ritual marks at the positions Graphviz chose.

It does not work, for reasons that are structural rather than aesthetic.

`dot` is a hierarchical layered layout. It produces a top-to-bottom DAG with
ranks. Sigil's composition is radial and square: a central core, concentric
semantic bands at documented radial fractions, fork branches in ordered clockwise
sectors, orbits as closed rings, effects on an outer invocation boundary. These
are not two settings of one algorithm; the second is not a layout `dot` can be
asked for. `neato` and `circo` are closer in spirit and still wrong — neither
allocates angular sectors by branch ordinal, and neither reserves radial bands by
semantic class.

Then there are the operational problems. Graphviz is a C library and a
subprocess: it cannot be compiled into the WASM package, which breaks ADR 0005's
single-implementation requirement immediately. It would make `cant sigil` depend
on a binary that most machines running a released `cant` do not have. And DOT
generation means writing user labels into a text format that gets parsed by a C
program — a new injection surface, in a language where those are expensive.

Determinism is the last one. Graphviz output is deterministic enough for a
regression test that only asks whether `dot` accepted the file. It is not a
documented, versioned, byte-stable contract across Graphviz releases, which is
what ADR 0004's determinism requirement needs from a layout engine.

## Decision

**Graphviz remains the technical topology view. Sigil implements its own radial
layout in Rust and does not consult Graphviz at render time.**

Binding:

1. `cant graph --format dot` keeps its current meaning, output, and test. It is
   the accurate, boring, technically-legible view, and it is what documentation
   should reach for when the question is "what is the topology".
2. `cant sigil` never invokes `dot`, never emits DOT, and adds no Graphviz
   dependency to any crate or to the release archive.
3. Sigil's layout is the pipeline in §11.1 of the specification, implemented in
   `rite-sigil`, deterministic under (graph, options, seed), and shared with the
   browser per ADR 0005.
4. Graphviz stays permitted — and useful — as a *development* aid: comparing
   Sigil's edge routing against a known-good topology, generating fixture
   pictures for `docs/cant/`, and cross-checking that Sigil has not silently
   dropped or duplicated an edge. It informs no rendered coordinate.
5. "Take a Graphviz drawing and restyle it" is not an acceptable implementation
   of any acceptance criterion in this project, including as a temporary
   scaffold. A stylized `dot` render is explicitly named in the quality bar as
   something that does not count.

## Consequences

**Good.** The WASM constraint is satisfied by construction, and `cant sigil`
works on a machine that has only the release archive.

**Good.** The two views stay honestly different. Someone debugging topology gets
ranks and boxes; someone making an artifact gets the circle. Neither is a
degraded version of the other, and nobody has to wonder which one is authoritative
— the graph is (ADR 0004).

**Good.** No new injection surface. Labels go into SVG through one escaping path,
already required to be fuzzed.

**Cost.** A radial layout engine has to be written, including sector allocation,
ring assignment, edge routing, and deterministic collision resolution. This is
the single largest piece of engineering in the MVP and there is no shortcut to
it. Buying it was never actually on offer, but the cost is real and is being
chosen rather than discovered.

**Cost.** Crossing minimization will not be as good as `dot`'s for a while.
Graphviz has decades in it. Sigil's mitigation is structural rather than
algorithmic — semantic bands and ordered sectors mean most crossings are
prevented by placement rather than removed by search — and the honest position is
that a large graph is better viewed with `cant graph`, which is what §11.8 says.

**Risk accepted.** Two layouts of one graph can disagree in ways that look like
a bug in whichever the reader trusts less. The mitigation is that neither is
semantic: both are projections of `CantProgram`, and the graph is the answer to
any question about what a program means.

## Alternatives rejected

**Shell out to `dot`, parse `plain` output, restyle.** Rejected: no WASM, a
runtime binary dependency, a DOT injection surface, an undocumented determinism
contract, and — decisively — a layered layout where the product requires a radial
one.

**Vendor a Rust port of a Graphviz algorithm.** Rejected for v0. `circo`-style
circular layout is the closest and still does not allocate sectors by branch
ordinal or bands by semantic class, so the port would be modified until it was a
different algorithm. Writing the intended one directly is less work than
converging on it from something else.

**Make Graphviz the fallback for graphs over the node cap.** Rejected as a
*Sigil* behaviour: silently producing a technical diagram when someone asked for
an artifact is a surprising substitution. §11.8's answer stands — fail above the
hard cap with a diagnostic that recommends `cant graph` or `--simplify`, and let
the user choose.
