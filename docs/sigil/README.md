# Sigil

Sigil turns a program's semantic topology into a ritual artifact.

A Cant program is a graph: a source, stages, wards, scatters, collects, forks,
orbits, invocations of the host world, and a closing seal. `cant graph` shows you
that graph as ranks and boxes. Sigil shows you the same graph as a radial
diagram — circular, symbolic, and deterministic — that you can strip of every
label and still read, and still hang on a wall.

```bash
cant sigil program.cant                 # program.sigil.svg
cant sigil program.cant --mode revealed # with labels and a codex
cant graph program.cant --format dot    # the technical view, unchanged
```

## What it is not

- Not a runtime. Sigil never executes a program, evaluates a predicate, or
  invokes a capability ([ADR 0003](../adr/0003-sigil-is-a-renderer-not-a-runtime.md)).
- Not a visual programming language. Position carries no execution meaning, and
  moving a mark cannot change a program ([ADR 0004](../adr/0004-sigil-layout-is-non-semantic.md)).
- Not a Graphviz skin. The layout is its own, in Rust
  ([ADR 0008](../adr/0008-graphviz-stays-the-technical-view.md)).
- Not a server. The hosted app at `sigil.rite.foo` renders in your browser and
  never uploads your source ([ADR 0007](../adr/0007-veil-and-source-privacy.md)).

## Vocabulary

A **glyph** is a visual spelling of one language token: `◆`, `←`, `⟦`. A
**sigil** is the artifact a whole program renders to. The two words are not
interchangeable, and the repository was migrated so they never have been
([ADR 0009](../adr/0009-glyph-names-a-token-sigil-names-an-artifact.md)).

| Term | Meaning |
|---|---|
| **Graph** | The technical semantic topology Cant emits |
| **Scene** | The renderer-ready semantic and geometric representation |
| **Codex** | The optional legend that decodes a sigil |
| **Veil** | The visible-information policy for a render |
| **Ornament** | Non-semantic visual geometry |
| **Mark** | A generated semantic symbol inside a sigil |
| **Invocation** | A capability node on the outer boundary |
| **Seal** | A terminal or collection structure |

## The pipeline

```text
Cant source
  -> cant-syntax parse, cant-sem lower       (cant.graph v2)
  -> Sigil graph adapter                     (rite.sigil.graph v1)
  -> topology analysis, radial layout
  -> Sigil scene                             (rite.sigil.scene v1)
  -> SVG / PNG / interactive HTML / scene JSON
```

Three schemas, versioned independently, each with a documented shape. The graph
is authoritative; the scene is a deterministic projection of it; the artifact is
a presentation of the scene.

## Status

**Phase 0–4 of nine.** `cant sigil` renders SVG, PNG and scene JSON in three themes, three traceries, four ornament levels and three disclosure modes. See [the implementation checklist](checklist.md) for every
MVP acceptance criterion against the artifact that satisfies it, and
[the implementation log](implementation-log.md) for what was decided, deviated
from, or discovered along the way.

## Pages

| Page | What it covers |
|---|---|
| [visual-language.md](visual-language.md) | How to read a sigil with every label removed |
| [cli.md](cli.md) | `cant sigil` — inputs, formats, axes, determinism |
| [themes.md](themes.md) | The three palettes and the rules every theme obeys |
| [accessibility.md](accessibility.md) | The summary, titles, keyboard, and motion |
| [internals.md](internals.md) | The pipeline, determinism, routing, and parity gates |
| [checklist.md](checklist.md) | Every acceptance criterion, its artifact, and its proof |
| [implementation-log.md](implementation-log.md) | Deviations, constraints found, test status |
| [graph-contract.md](graph-contract.md) | The normalized `rite.sigil.graph` input model |
| [scene.md](scene.md) | The `rite.sigil.scene` model and its layers |
| [deployment.md](deployment.md) | The Worker, its headers, and what it deliberately cannot do |
