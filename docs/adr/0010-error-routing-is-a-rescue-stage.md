# ADR 0010 — Error routing is a rescue stage

- **Status:** Accepted
- **Date:** 2026-08-05
- **Supersedes:** nothing
- **Related:** [ADR 0002 — Generated canonical Rite is the v0 execution boundary](0002-cant-lowers-through-rite.md) ·
  [ADR 0006 — Sigil consumes a normalized graph](0006-sigil-consumes-a-normalized-graph.md)

## Context

Cant v0 has three postures toward a failure, all of them Rite's own vocabulary:
postfix `?`, a ward over `is_ok`, and `unwrap_or`. None of them is a *route*. A
failed emission either becomes a different value in the same flow or stops being
an emission; there is nowhere else for it to go, so "read these files, and log
the ones that failed" cannot be written without turning the whole flow into a
result-handling flow.

The graph was built to admit the missing construct. `PortKind` and `PortRef`
exist because "fork branches, error routing and multi-output nodes are all in the
deferred design space" (`crates/cant-sem/src/lib.rs`), `EdgeRole` labels what an
edge is *for* rather than leaving it to be inferred from shape, and
`docs/cant/graph-schema.md` names error routing as the change that "would add a
role and a second out-port convention".

Three facts about the current implementation bear on the design:

- **A capability call answers a result.** `!@fs.read($)` emits `ok(text)` or
  `err(record)` as an ordinary value. That value is already flowing; nothing has
  to be caught to see it.
- **A `panic` is not a value.** Scatter's type check and an orbit's `:max` both
  `panic`, and Rite has no expression that observes one. Catching those would
  need a runtime intrinsic, which ADR 0002 §4 forbids without its own ADR.
- **`?` inside a stage used to drop the emission it fails on.** A generated
  stage is a `for` loop, `for` lowers to `each` with a closure, and `?`
  returned from the closure — so `["a", "b"] -> * -> !@fs.read($)? -> []`
  answered the files that could be read and continued past the ones that could
  not, while `docs/cant/language.md` said `?` "ends the run". Resolved after
  this ADR: the expansion now strips a leaf's trailing `?` and checks the
  result itself, failing the run with `CANT-R004` and the error in hand
  (`conformance/cant/execution/error-try-fails-the-run` pins it). The triad
  stands: bare call lets the `err` flow as a value, `?` fails fast, a rescue
  routes.

## Decision

**A rescue is a stage, `!{ handler }` (glyph `↯⟦ handler ⟧`), that splits the
emissions reaching it: an `err` goes into the handler flow, an `ok` continues
unwrapped, and anything else passes through.**

### Syntax

`!{` joins `?{`, `|{` and `~{` as a block opener, and closes with the same `}`.
It is unambiguous in the lexer for the same reason they are: the brace must
immediately follow, and `!` prefixes a *call* in Rite, so no leaf can contain a
`!` adjacent to a `{`. `! { … }` with a space stays leaf text, as `? cond { … }`
already does.

```cant
["a.txt", "b.txt"] -> * -> !@fs.read($) -> !{ "failed: " + $.message } -> []
```

The handler is a flow, not one expression — a failure usually wants reporting as
well as replacing:

```cant
["a.txt"] -> * -> !@fs.read($) -> !{ $.message -> "failed: " + $ } -> []
```

`$` in the handler is the **error payload**, the record inside the `err`, not the
`err` itself. Unwrapping is what every handler would do first, and a handler that
had to unwrap would be one `?` away from the failure mode below.

A rescue takes no modifiers.

### Semantics

For each emission arriving at a rescue:

| Arriving | Emits |
|---|---|
| `ok(v)` | `v`, on the main flow |
| `err(e)` | whatever the handler emits, given `e` |
| anything else | itself, unchanged |

The handler's emissions rejoin the main flow in place, so a handler that emits
nothing drops the failure and one that emits several fans out — the ordinary
emission rules, applied to a second entry point.

A non-result passing through unchanged is the deliberate choice. Being strict, as
Rite's `?` is, would mean a fork with one fallible branch and one infallible one
could not be rescued in a single place.

The handler may perform effects, and they are permission-gated like any other.
Nothing about ordering changes: a handler runs immediately, in the middle of the
emission that failed.

**What is catchable is exactly "an `err` value arriving at the rescue".** Not a
`panic`, not a Rite runtime error, not a budget exhaustion — those end the run,
as they did before. And not a failure that a `?` upstream has already removed:
`!@fs.read($)? -> !{ h }` unwraps the `ok` and drops the `err` before the rescue
sees anything, so the handler can never run. That is a silent no-op, and
`CANT-G017` refuses it rather than letting it look like error handling.

### The graph, at version 2

`cant.graph` goes to **version 2**. Two additions, both predicted by the version
1 document:

- **`NodeKind::Rescue { handler }`** — a subgraph id, exactly as an orbit carries
  its body. Out port 0 is the continuation and out port 1 enters the handler; in
  port 0 is the value and in port 1 is where the handler rejoins. That is the
  second out-port convention the schema said this would need.
- **`EdgeRole::Rescue`** — the edge from out port 1 into the handler's entry. The
  handler's return uses the existing `Join`, because it is a concatenation point
  and not a re-entry, exactly as a fork branch's is.

One new role rather than reusing `Enter`: a consumer has to be able to tell the
failure path from a branch without inspecting the node on either end, which is
the whole reason roles are recorded. It carries control forward, so the
"every cycle is orbit-owned" search counts it alongside `Flow` and `Enter`.

### Sigil

The rescue node projects to `SigilNodeKind::Fork` and its edge to
`EdgeKind::Enter`; the handler is a `Branch` region. A rescue *is* a split into a
region that rejoins, which is what those already draw. A mark of its own would
need a visual vocabulary for failure, and choosing one here would commit the
renderer to whatever this ADR happened to pick.

### Out of scope

- **Cancellation and retries.** Both need a notion of re-running a stage that
  Cant does not have, and retries need a policy vocabulary (counts, backoff) that
  would be the first place Cant invented semantics Rite has no opinion about.
- **Catching `panic`.** Needs a runtime intrinsic; see ADR 0002 §4.
- **Routing to a named handler elsewhere in the program.** That is named anchors,
  which is a separate deferred item, and it would end "orbit is the only cycle".
- **Changing what `?` does.** Recorded above, unchanged here.

## Consequences

**Good.** The failure path is in the graph, so `cant graph` and Sigil can draw
where a program's errors go, and `cant explain` can say it. It was previously
invisible: `unwrap_or` is a stage like any other.

**Good.** No new runtime, no intrinsic, no change to Rite. A rescue lowers to a
`match` over the emission inside the loop the stage already generates, calling
the handler's chain in its `err` arm — so the handler's effects sit in a
generated function body with their call sites, which is the shape ADR 0002 §3
requires.

**Cost.** A fourth thing `!` can be doing on a line. `!@fs.read` marks an effect,
`!=` is a comparison, and now `!{` opens a rescue. The lexer separates all three
without lookahead, and the overlap is not accidental: the failures a rescue
routes are overwhelmingly the ones effectful calls produce.

**Cost.** A stored version 1 graph is refused rather than upgraded, as a version 0
one already is. The schema is experimental and says so.

**Cost.** Sigil's fingerprint covers the source schema version, so every stored
scene and SVG in `fixtures/sigil/` is re-fingerprinted by the bump even though no
geometry moves. The diff is one line per fixture and worth reading rather than
waving through: a change that moved a mark would look the same at a glance.

## Alternatives rejected

**An edge modifier, `-!>` or `->!`.** Rejected: an edge is not a place to put a
flow. The handler has to live somewhere, and giving every stage a second outgoing
edge means every stage grows a port it almost never uses, for a construct that is
a stage in every other respect. The `!{ }` form keeps the extra ports on the one
node that needs them.

**Make the rescue see the whole `err(e)` rather than its payload.** Rejected: the
handler's first act would be to open it, and the natural spelling of that is `?`
— which, given the drop-on-failure behaviour above, would silently discard the
failure inside the handler that exists to handle it.

**A modifier on the fallible stage, `!@fs.read($) :else ""`.** Rejected: a
modifier configures a form, and this is a second flow. `:else` would take one
expression, and "log it and substitute" would be back to needing a stage.

**Reuse `EdgeRole::Enter` and let the node kind carry the meaning.** Rejected: a
consumer walking edges would have to look up both endpoints to know whether an
edge is a branch or a failure path, which is the analysis `role` exists to make
unnecessary.
