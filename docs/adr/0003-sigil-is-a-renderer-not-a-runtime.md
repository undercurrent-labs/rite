# ADR 0003 — Sigil is a semantic renderer, not a runtime

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0004 — Layout is non-semantic](0004-sigil-layout-is-non-semantic.md) ·
  [ADR 0006 — Sigil consumes a normalized adapter graph](0006-sigil-consumes-a-normalized-graph.md) ·
  [ADR 0002 — Cant lowers through canonical Rite](0002-cant-lowers-through-rite.md)

## Context

Sigil turns a Cant program's semantic topology into a ritual diagram. Every
interesting thing it wants to draw is a fact about *execution*: which stages
touch the host world, how many values a scatter emits, which orbit candidates
survive deduplication, which fork branch actually produces the result.

The tempting way to get those facts is to run the program. A renderer that
evaluated a ward predicate could draw the accepted and rejected paths honestly.
One that ran an orbit could size the ring to the real iteration count. One that
resolved `@fs.read` could label the invocation with the file it opens.

Every one of those is a capability invocation performed by a *drawing tool*, on
input the user has not agreed to execute, in a browser tab on `sigil.rite.foo`
where the permission model that makes Rite trustworthy does not exist. Rite's
whole security posture is that effects are declared, propagated to a fixed point
by `rite-sem/src/resolve.rs`, and gated by `rite-caps/src/permissions.rs` against
grants the user wrote on a command line. "Paste a program into a web page and it
opens files to make the picture prettier" is not a feature with a bad default; it
is the inversion of the property the language exists to have.

There is a second reason, independent of security. A render that depends on
runtime values is not a function of the program. Two runs produce two pictures.
Determinism (§4.4 of the specification, and every golden test that follows from
it) would be unachievable, and the artifact would stop being a portrait of the
program and become a portrait of one execution of it.

## Decision

**Sigil consumes an already-validated semantic graph and never executes
anything.** Binding on all later work:

1. `rite-sigil` does not depend on `rite-runtime`, `rite-caps`, `rite-compiler`,
   or `cant`'s `run`/`build` surface. The manifest is the enforcement point and a
   test asserts it.
2. Sigil does not interpret Cant stages, evaluate ward predicates, resolve
   imports, invoke capabilities, perform effects, or infer runtime values.
3. Sigil does not mutate the graph it is given. Adaptation produces a new
   normalized value; the input is read-only.
4. Effect and capability metadata is **read from the graph**, never derived by
   executing or resolving anything. Where the graph does not carry it, the graph
   contract is extended (ADR 0006), not worked around.
5. No server-side render endpoint exists. The Cloudflare Worker at
   `sigil.rite.foo` serves static assets and three read-only informational
   endpoints; it never receives a program.
6. Depiction is not measurement. A scatter is drawn as a divergence of fixed
   arity, not as *n* rays where *n* is a collection length nobody has computed. An
   orbit's `:max` is drawn as a bounded tick group, not as an iteration count.

`cant sigil` parses and lowers to a graph — the same work `cant graph` does — and
stops there. Parsing is not execution: it performs no effect and requires no
grant, which is exactly why `cant check` needs no permissions today.

## Consequences

**Good.** The renderer is a pure function of (graph, options, seed). That is what
makes scene and SVG golden tests meaningful, what makes native/WASM parity a
testable claim rather than an aspiration, and what makes it safe to paste a
stranger's program into a web page.

**Good.** The browser build stays small and honest. Nothing in the WASM
dependency graph can open a socket, because nothing that opens sockets is in it.

**Cost.** Some things a viewer might want are not drawable. The picture cannot
show which fork branch produced the answer, how many items an orbit visited, or
what a `@fs.read` actually read. Sigil shows the *shape of the possibilities*, and
the Codex says so rather than implying a trace.

**Cost.** Capability metadata has to be good enough in the graph, because there is
no fallback of "resolve it properly at render time". This is a real constraint on
Cant's graph contract and the reason ADR 0006 extends it rather than letting the
renderer scrape leaf text.

**Risk accepted.** Execution-trace animation is genuinely desirable and is listed
in the specification's deferred design space. When it arrives it will be a
separate pipeline that feeds a *trace* into the renderer as additional input —
never a renderer that acquired the ability to run things.

## Alternatives rejected

**Evaluate pure leaves only, and refuse effectful ones.** Rejected. "Pure" is a
property Rite establishes with a resolver and a fixed-point effect closure over
the whole call graph; Cant leaf text is Rite expression text whose names Cant has
not resolved. A renderer that decided purity for itself would be a second,
weaker implementation of the check `rite-sem` already owns — and the failure mode
of getting it wrong is executing an effect from a drawing tool.

**Render server-side on the Worker.** Rejected. It would make the source leave the
browser, which ADR 0007 forbids, and it would put an unbounded-input renderer on
a public endpoint. The renderer compiles to WASM; there is no capability the
Worker has that the tab does not.

**Let the renderer call back into Cant to re-derive missing metadata.** Rejected:
it makes `rite-sigil` depend on the Cant parser, which the specification forbids
and ADR 0006 replaces with an adapter. Re-deriving is also how label-inference
gets in through the back door.
