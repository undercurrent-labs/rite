# ADR 0004 — Sigil layout is non-semantic

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0003 — Sigil is a semantic renderer, not a runtime](0003-sigil-is-a-renderer-not-a-runtime.md) ·
  [ADR 0008 — Graphviz stays the technical view](0008-graphviz-stays-the-technical-view.md)

## Context

`cant_sem::graph::LayoutHint` already exists, and its doc comment already says
the thing this ADR is about: *never semantic*. It was reserved in Phase 3 for a
renderer that did not exist yet. That renderer is now being built, and the
reservation is about to be tested by the first thing that actually wants to write
coordinates.

The pressure is real and it comes from the product, not from carelessness. Sigil
is a *radial* renderer: it will assign every node an angle, a radius, a ring, and
a sector. Those are rich, meaningful-looking quantities. Once a fork branch owns
a wedge of the circle, "which wedge" is one small step from "which wedge matters",
and a language in which the position of a stage changes what a program does is a
language that has smuggled in spatial semantics without ever deciding to have
them.

The same pressure exists in the other direction. `apps/sigil-web` is an
interactive canvas. Dragging a node is the most natural gesture a canvas affords,
and if a drag wrote back to the graph, geometry would have become source.

## Decision

**Coordinates, radii, rotation, curvature, ornament, and manual adjustments carry
no execution meaning. The graph is authoritative; the scene is a deterministic
projection of it; the artifact is a presentation of the scene.**

Binding:

1. Node coordinates, ring radius, global rotation, arc curvature, sector angle,
   and ornament are **not** semantic. Nothing in Cant's validation, lowering,
   expansion, or execution reads any of them, and nothing may start to.
2. `cant_sem::graph::LayoutHint` stays reserved and stays unread by the language.
   Sigil **does not read it either** in v0 — the layout engine computes its own
   geometry from topology, so a hostile or stale hint cannot move semantic marks.
   Round-tripping it through the graph JSON continues to work, and that is all.
3. The visual artifact is not a program representation that can be edited back.
   `apps/sigil-web` has no node-drag-to-edit gesture in v0, and the export
   formats (SVG, PNG, HTML, scene JSON) are not accepted as renderer *input*
   for reconstructing a graph.
4. Deleting every geometric value from a scene must leave the graph's meaning
   intact, because the graph never contained them.
5. Ornament is a separate scene layer with separate CSS classes, carries no graph
   node IDs, and receives no hit regions. Toggling ornament must not move a
   single semantic coordinate — asserted by a property test, not by inspection.
6. Future spatial semantics, if they are ever wanted, arrive as a **language**
   feature with its own ADR, its own graph fields, and its own validation. They
   do not arrive as a renderer that quietly started to matter.

## Consequences

**Good.** Layout is free to change. Ring allocation, sector weighting, and
collision resolution can be tuned — and will be, repeatedly, because this is an
aesthetic product — without any of it being a breaking change to Cant. Only the
golden scene snapshots move, which is exactly the signal a layout change should
produce.

**Good.** The determinism requirement becomes tractable. Because geometry is a
pure function of topology plus seed, "same graph and options produce the same
scene" is a property test rather than a hope, and a layout regression is
localized to the scene layer instead of being indistinguishable from a semantic
one.

**Good.** A user can rotate, re-seed, or re-theme a render and know with
certainty that they are looking at the same program.

**Cost.** Sigil cannot honour a hand-tuned layout in v0, even though the graph
has a field for one. Someone who wants a specific arrangement has one lever —
`--seed` — and it is coarse. This is a deliberate deferral: reading `LayoutHint`
means deciding what happens when a hint collides with a semantic band, and every
answer to that question is a rule about geometry that the language would then
have to keep.

**Cost.** Two things that look alike are governed by different rules. A semantic
edge and an ornamental filament are both strokes on a circle. Keeping them
distinguishable is a continuing design obligation on the visual grammar, not a
property the architecture gives for free — §4.5 of the specification lists the
constraints and the ornament layer separation is how they are enforced.

**Risk accepted.** "Non-semantic" is a claim that decays if nothing checks it.
The check is: `rite-sigil` does not depend on anything that executes (ADR 0003),
and the ornament-invariance and determinism property tests fail if geometry ever
starts to matter differentially. Neither catches a *future* reader of geometry
inside Cant; that is what this ADR is for.

## Alternatives rejected

**Read `LayoutHint` when present, compute otherwise.** Rejected for v0. It sounds
free, and it is not: a hint that lands outside its semantic band, overlaps a
ring, or names a node that no longer exists needs a documented resolution rule,
and that rule is a spatial semantics in everything but name. Worse, the hint is
untrusted input from a graph JSON file, so the rule would have to be safe as well
as defined. Deferred until there is a real editing story to justify it.

**Make orbit ring radius encode `:max`.** Rejected, and it is the tempting one:
it would be genuinely informative. But it makes radius load-bearing, so a layout
change becomes a semantic change, and it fails the monochrome/shape test in §4.6
— two orbits differing only in radius are not distinguishable without a ruler.
`:max` is drawn as a bounded tick group, which is a *shape* difference.

**Let the web app write positions back into the graph.** Rejected: it makes the
picture a source of truth, which contradicts the first sentence of the decision.
It is also the fastest possible route to the deferred "spatial programming" item
becoming true by accident.
