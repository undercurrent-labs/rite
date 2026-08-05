# Internals

The renderer from the inside: what runs, in what order, and which properties
are enforced rather than hoped for. The companion pages own the details —
[graph-contract.md](graph-contract.md) for the input model,
[scene.md](scene.md) for the output model — this page is the spine between
them.

## The pipeline

```text
Cant source ──(cant-syntax, cant-sem)──▶ cant.graph v2
    ──(adapter, cant-sem::sigil)──▶ rite.sigil.graph v1
    ──(normalize: validate + limits + fingerprint)──▶ NormalizedGraph
    ──(analyze: topology, placement, region weights)──▶ Topology
    ──(build_scene: layout + routing + marks + ornament)──▶ rite.sigil.scene v1
    ──(render_svg / render_png / render_html)──▶ the artifact
```

Everything above `normalize` is untrusted; everything below it trusts and
never re-checks. `rite-sigil` parses no language, executes nothing, and opens
no file — `tests/boundaries.rs` reads the manifest and the sources to keep it
that way (ADR 0003).

## Determinism, mechanically

Same graph, same options → same bytes. The ways that can silently break, and
what blocks each:

- **No `HashMap` iteration** anywhere order matters — `BTreeMap` and sorted
  passes only.
- **Randomness is one seeded PRNG**, and per-node streams derive from node
  *identity*, not visit order — a mark is the same whatever order emitted it.
- **Collision resolution is a sorted single bounded pass**, not a relaxation
  loop whose result depends on iteration count.
- **Comparison goes through canonical text.** `serde_json` does not round-trip
  every `f64`; parity and goldens compare serialized bytes, never deserialized
  structures.

## Layout, in one paragraph

Radius is allocated in bands by what a node is to the composition (core /
flow / seal / boundary); the spine spirals outward-clockwise with angle from
sequence and radius from progress; forks fan into weighted clockwise sectors,
recursively — a nested fork subdivides its parent branch's sector, never the
circle; orbits ring around their own settled position, sized so members never
touch; invocations keep their flow angle and move to the boundary band. Every
constant in `layout.rs` carries a comment saying what it is for, and several
say what went wrong before.

## Routing

Traces are routed after placement, one tracery at a time
([scene.md § Traceries](scene.md)). Two properties hold across all three:

- **Marks are hard obstacles.** A candidate that would pass within a mark's
  clearance is rejected while any candidate clears; the least-bad one is the
  last resort, mirroring the collision pass's posture.
- **Earlier traces are soft obstacles.** Edges route in graph order, and among
  mark-clearing candidates the fewest crossings of already-routed traces wins
  — deterministic crossing reduction without moving a node. Traces sharing an
  endpoint are exempt: they meet at a mark, which is a junction.

## Limits

`limits.rs` bounds nodes, edges, input bytes and label lengths before any
layout runs, with stricter ceilings in the browser build — a tab should refuse
rather than hang. Refusal is a diagnostic with a stable code, not a panic.

## The parity gates

Three, from cheapest to most literal:

1. `cant-sigil-wasm/tests/parity.rs` — the CLI pipeline and the browser
   binding, called natively, agree on scene and SVG across programs × options.
2. `tests/browser_fixture.rs` — pins the native render of the ceremony example
   as a fixture.
3. `scripts/check-sigil-wasm-parity.mjs` — executes the *built wasm32 bundle*
   in Node against that fixture, byte for byte, inside `build-sigil-site.sh`,
   so CI and every release run it.

## Versions in every artifact

The render fingerprint names them all:
`sigil/<renderer> graph=<fingerprint> theme=<name>@<version> tracery=<name>
seed=<n> mode=<disclosure> metadata=<mode> format=<format>` — enough to
reproduce a picture from its file, and nothing that leaks a label.
