# Reading a sigil

A sigil is readable with every label removed. That is the design constraint the
whole visual language answers to: with nothing but shape, position and line, a
viewer should be able to say where the program begins, which way it flows, what
it touches in the host world, and where it ends. This page is the decoder.

The authoritative shapes live in `crates/rite-sigil/src/marks.rs`; this page is
their reading order.

## The composition

A sigil is radial. Radius means something; angle means sequence; nothing about
position carries execution semantics (ADR 0004) — the picture is an argument
about *structure*, not a trace of a run.

```text
centre          the entry: where the program begins
inner band      the flow: stages, wards, scatters, collects, forks, orbits
outer band      the seals: where results come to rest
the boundary    a drawn circle: everything outside it is the host world
rim             invocations: the points where the program touches that world
```

The main flow spirals outward and clockwise from the centre. A chain of
operations reads as a curve you can follow with a finger; the composition
closes at the bottom, where the seal sits. The gap at the top of the spiral is
deliberate — the end of a program never lands on its own beginning.

## The marks

Every node kind has a fixed skeleton — the part you learn — plus deterministic
per-node variation that only ever *adds* strokes, so two stages are visibly
different individuals without either stopping being a stage. Colour never
carries the distinction: every pair of kinds differs in topology — stroke
count, closed versus open form, symmetry — because one of the three themes is
monochrome on purpose.

| Kind | Skeleton | How to read it |
|---|---|---|
| Source | nested core | the only concentric-closed form; an origin |
| Stage | rune spine | one bar, two terminals — the least mark that is still a mark |
| Ward | gate | a bar *across* the flow with a gap: a conditional passage |
| Scatter | flare | rays diverging from one point |
| Collect | knot | rays converging into a closed ring — Scatter, inverted |
| Fork | trident | discrete ordered tines; the branches are countable |
| Orbit | circular lock | a ring with a break (the exit) and an inward key |
| Invocation | altar | an open bracket facing outward, touching nothing inward |
| Output | seal | a closed polygon with an inner mark: nothing leaves it |
| Literal | dot cluster | no strokes at all — a value, not an operation |
| Unknown | broken hex | a familiar form with a piece missing |

Scatter and Collect are deliberate inverses: a viewer who has learned one gets
the other for free. The seal is the only closed polygon, which is what lets
"where does this end" have one answer at a glance.

## The traces

A line between marks is a data flow. Its *kind* is carried by line quality and
its shape by the render's tracery:

- An ordinary flow trace is solid.
- A **feedback** trace — an orbit going round again — is dashed, and routes the
  long way: bowed outward in `flowing`, on an outer ring in `concentric`.
- Traces never cross a mark they do not end at; when two traces cross each
  other mid-air, that crossing means nothing. Junctions happen at marks only.

The three traceries (`flowing`, `concentric`, `circuit`) change the calligraphy
of every trace and the position of nothing — the same program in a different
hand. See [scene.md](scene.md) for how they are constructed.

## Structures

- A **fork** fans: its branches take angular sectors clockwise by ordinal, so
  the first branch is the first one clockwise. A fork inside a branch
  subdivides its parent's sector, never the whole circle.
- An **orbit** rings: its body sits on a drawn circle around the orbit mark —
  the one shape that says "this may go round again" without a caption.
- An **invocation** sits on the rim with a spoke back to its calling position:
  the picture of "this is where the program touches the world". The boundary
  circle it crosses is semantic, not ornament, and survives `--ornament none`.

## What is not semantic

Ornament — the tick rings, scattered dots and radial hairlines — is generated
from the seed alone, on its own layers, at low opacity. It never moves anything
and can be removed without a relayout. If a stroke is faint and touches
nothing, it is ornament.

## Veils

A **Veiled** render (the default) is everything above and no text: the artifact
carries no source. **Inscribed** adds short annotations beside marks;
**Revealed** draws full labels. The Codex — the legend beside the picture in
the app and the HTML export — can decode a Veiled render's *kinds* regardless,
carrying labels only when metadata allows them; that split is ADR 0007, and
`--metadata none` is the setting that means nothing anywhere.
