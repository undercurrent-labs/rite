# ADR 0012 — Parallel fork is a modifier on the fork

- **Status:** Accepted
- **Date:** 2026-08-05
- **Supersedes:** nothing
- **Related:** [ADR 0002 — Generated canonical Rite is the v0 execution boundary](0002-cant-lowers-through-rite.md) ·
  [ADR 0010 — Error routing is a rescue stage](0010-error-routing-is-a-rescue-stage.md) ·
  [ADR 0011 — Named flows are spliced](0011-named-flows-are-spliced.md)

## Context

`|{ a ; b }` runs its branches in source order, one after another, and
`docs/cant/language.md` said so twice: under Fork, and under Determinism, where
"no parallelism, no nondeterminism" was a property of the whole language. Parallel
fork has been the last item in Cant's deferred design space since v0.

The reason to want it is not arithmetic. A fork whose branches compute are best
left sequential; a fork whose branches *wait* — two files, two HTTP requests —
spends its time doing nothing, and a Cant program has no other way to say
"these do not depend on each other".

Four facts about the implementation decide the design.

- **Rite already has the concurrency.** `parallel(xs, f)` in
  `rite-runtime/src/eval/hof.rs` runs branches that make progress together,
  returns results in **input order**, and caps the window at 16 in flight. Cant
  needs no runtime of its own, which ADR 0002 §4 would have required an ADR to
  add.
- **Effectful calls must stay visible to Rite's effect analysis.** This is
  `expand.rs`'s standing rule: a closure handed to a helper hides its host calls,
  and a lowering that did it would let a Cant program read files with no marker
  and no grant. But the analysis *does* track a **named** function passed as an
  argument — the `each(shout)` case in `rite-sem/src/resolve.rs` — and demands a
  `!` on the call that passes it.
- **A modifier's value was mandatory.** The parser rejected `:name` with nothing
  after it (`CANT-P010`), so a valueless modifier was not representable.
- **The graph schema is frozen**, in lockstep with `docs/cant/graph-schema.md`
  and `crates/cant-sem/tests/schema_freeze.rs`. Any key added is a version bump,
  and a bump re-fingerprints every stored Sigil scene.

## Decision

**A fork takes the modifier `:par`. Its branches then run concurrently, and their
emissions still join in branch order.**

```cant
5 -> |{ $ + 1 ; $ * 2 ; $ * $ } :par -> []          // [6, 10, 25]
"config.json" -> |{ !@fs.read($)? ; !@log.write($) } :par -> []
```

### Syntax

`:par` is a modifier, not a new operator, because that is what it is: a policy on
a form, like `:by` and `:max` on an orbit. A fork with `:par` has the same
branches, the same separators and the same braces as one without, and everything
already written about `|{ }` continues to apply.

This required making a modifier's value **optional**. `:par true` would be a
value carrying no information, and the alternative — inventing a second opener,
`||{` — would have meant a sixth block token, a glyph nobody has a reason for, a
second entry in the manifest, the formatter and the TextMate grammar, and two
spellings of a construct the graph represents as one node either way.

The parser stays as position-only as it was. It records `:name` and `:name value`
alike and knows no modifier names; **which** names take a value is
`cant-sem/src/validate.rs`'s question, beside "which names exist at all". There
is no ambiguity to resolve: after a block's `}`, a bare leaf cannot follow, so
`:par upper` can only be a modifier with a value, and validation says `:par`
takes none.

The glyph dialect needs nothing. A modifier's spelling is the same in both
(`grammar/cant/operators.toml`), so `⫴⟦ a ; b ⟧:par` is the glyph form.

### Semantics

**The value is deterministic. The effect order is not.**

Branches run concurrently and their emissions are concatenated in branch order,
whatever order they finish in, because `parallel` answers in input order. A
`:par` fork therefore emits exactly what the same fork without `:par` emits, and
`cant run` on it twice prints the same thing. That is the property worth keeping,
and it is the one this ADR does not trade away.

What it does trade away is the ordering of *effects between branches*. Two
branches writing files, calling a host or mutating `@store` reach the world in
whatever order they get there. `docs/cant/language.md`'s Determinism section is
amended to say so: value-deterministic always, effect-order-deterministic except
inside a `:par` fork.

Console output is deliberately **not** part of that trade. `RuntimeContext::fork`
gives each branch its own output buffer and `parallel` splices them back in
branch order, so two branches printing do not interleave. This was measured, not
assumed. Promising it is safe because it is the runtime's existing guarantee
rather than something Cant arranges, and it is worth having: `@console.println`
is how anyone debugs a fork, and a debugging aid that scrambles itself under the
thing being debugged is worse than none.

**A branch failure fails the fork, after the others settle.** `parallel` joins a
whole window before it reports, and reports the first failure in *input* order
rather than the first in time — so which error a two-branch fork raises is
deterministic too. A fork with more branches than the window (16) fails at the
end of the window containing the failure. There is no cancellation: a failing
branch does not stop the others, it waits for them. Cancellation needs a
vocabulary Cant does not have, and is out of scope below.

**Effect discipline is unchanged.** A parallel branch is an ordinary branch. An
unmarked host call inside one is still `CANT-S001`, and a marked one still needs
its grant — `conformance/cant/execution/parallel-denied` exits 5 without it.

### Lowering

The load-bearing part. A parallel fork becomes a call to `parallel` over one item
per branch, plus a **named dispatcher** that calls the right branch chain:

```rite
def! cant_x_n1(__in) [[
  __out <~ []
  for __e in __in [[
    __r <- ! parallel([ << branch: 0, value: __e >>, << branch: 1, value: __e >> ], cant_x_n1_par)
    for __b in __r [[
      __out := concat(__out, __b)
    ]]
  ]]
  ^ __out
]]

def! cant_x_n1_par(__p) [[
  if (__p.branch = 0) [[ ^ cant_x_s0([ __p.value ]) ]]
  if (__p.branch = 1) [[ ^ ! cant_x_s1([ __p.value ]) ]]
  ^ []
]]
```

The branch chains are the ones `fork_node` already called; nothing about them
changes. The dispatcher is the whole of the new machinery, and it exists for one
reason: **it is named.** Rite's resolver sees `cant_x_n1_par` passed to
`parallel`, looks it up, finds `def!`, and requires the `!` on the call —
which in turn makes `cant_x_n1` effectful and propagates outward to `main`.
Removing that `!` by hand is rejected with `E021`, which is the check working.

A closure would have compiled and been wrong. That is the failure mode this
lowering is shaped around, and `an_effectful_parallel_branch_keeps_its_marker_and_its_def`
in `expand.rs` asserts no closure appears.

Each item is a record rather than a pair because `__p.branch` and `__p.value`
read as what they are in the generated Rite someone audits with `cant expand`.

Two properties of `parallel` are relied on and both are stated in its own
documentation: results in input order, and 16 in flight with each window settling
before the next begins.

### The graph, at version 3

`cant.graph` goes to **3**, adding `parallel: bool` to the `fork` node kind.
Always serialized, in both states.

The alternative was to encode parallelism only in lowering and leave no trace in
the graph. That is cheaper — no bump, no re-fingerprinting — and it is wrong:
`cant graph`, `cant explain` and Sigil would then describe a program whose most
consequential property they could not see, and the graph would stop being the
thing that executes. The schema document already listed parallel fork as the
change that "would need an ordering or concurrency field on the fork node".

It is a field on `fork` rather than a second node kind because a parallel fork
*is* a fork: same branches, same ordinals, same two-port shape. Only the
scheduling differs.

### Sigil

The fork's mark, unchanged. The node projects to `SigilNodeKind::Fork` and its
branches to `Branch` regions exactly as before; only the label says "parallel
fork". A distinct visual treatment would need a vocabulary for concurrency, and
inventing one here would commit the renderer to whatever this ADR happened to
pick — the same reasoning ADR 0010 applied to the rescue. Future work.

The bump re-fingerprints all 16 stored scenes and SVGs. The diff is one
`graph_fingerprint` line and one `source_schema` line per fixture, with no
geometry moved, and is worth reading rather than waving through.

### Tracing

**`cant run --trace` is correct over a parallel fork, and does not force it
sequential.** This was investigated rather than assumed, because the counters are
`@store` reads and writes and a read-modify-write across concurrent branches is
the obvious hazard.

Three facts make it safe:

- `RuntimeContext::fork` clones `capabilities` as an `Arc`, so every branch
  increments the same `@store` namespace. Counts are not lost.
- The increment is one generated statement, and nothing in it suspends: `@store`
  never returns a pending future, and Rite's evaluator has no cooperative yield.
  A branch cannot interleave between the read and the write.
- Addition does not care which order the branches arrive in.

A test pins it: `a_traced_parallel_fork_counts_what_the_sequential_one_does`
compares a `:par` program's trace to the identical sequential one and requires
them equal. If `@store` ever becomes genuinely async, that test fails, which is
the point of writing it as a comparison rather than as an assertion about one
number.

### Diagnostics

Three codes, all in the graph group, all read from the **AST** for the same
reason modifier validation already is.

| Code | Meaning |
|---|---|
| `CANT-G022` | A modifier that takes a value, written without one. |
| `CANT-G023` | A value after a modifier that takes none, such as `:par true`. |
| `CANT-G024` | `:par` on a fork with one branch. **A warning.** |

`CANT-G024` is a warning rather than an error. It changes nothing about what the
program means or emits — unlike `CANT-G017`, where a `?` makes a rescue handler
silently unreachable — and a fork is a thing people edit branches out of while
working. Saying so and continuing is the right weight.

**`CANT-P010` is retired.** "A modifier needs a value" is no longer a parse
question: it cannot be asked without knowing which modifiers take one, and that
vocabulary lives in validation next to `CANT-G010` ("a ward does not take
`:max`"), which is the same shape of mistake. `~{ f } :max` now reports
`CANT-G022` and exits 4 rather than 3. The number is not reused.

### Out of scope

- **Cancellation.** A failing branch does not stop its siblings. Stopping them
  needs a way to interrupt a running Rite call, which the runtime does not have,
  and a rule for what a half-run effectful branch leaves behind.
- **Retries and backoff.** Deferred with cancellation since ADR 0010, and still
  the first place Cant would invent semantics Rite has no opinion about.
- **A concurrency limit per fork.** `parallel`'s window of 16 applies. A `:max`
  on a fork is representable — it is another modifier — and nothing needs it yet;
  a fork with more than 16 branches is not a shape anyone has written.
- **Parallel scatter or parallel orbit.** Scatter's emissions feed one downstream
  chain in list order, and an orbit's worklist is inherently sequential: its next
  candidate depends on what the last one emitted.
- **A distinct Sigil mark**, above.

## Consequences

**Good.** The one thing Cant could not express — "these branches do not depend on
each other, so wait for them together" — is now one modifier, and the program's
value does not change when you add it. That last part is what makes it safe to
try: `:par` is not a rewrite, it is an annotation you can add and remove while
the tests keep passing.

**Good.** No new runtime, no intrinsic, no change to Rite. `parallel` was already
there, already ordered, already windowed.

**Good.** Modifiers can now be valueless, which is a shape the language will want
again and which cost nothing to admit: the parser already knew no modifier names,
so the rule it enforces got simpler rather than more complex.

**Cost.** "No parallelism, no nondeterminism" was a clean thing to be able to say
about the language, and it is no longer true of effect order. The docs now need a
paragraph where they had a bullet list. The value determinism — the part anyone
actually depends on — is intact, and stating the distinction precisely is better
than either losing it or refusing the feature to keep the sentence.

**Cost.** A graph schema bump, and with it every stored Sigil scene
re-fingerprinted for a change that moves no geometry.

**Cost.** A retired diagnostic code and one program's exit code changing from 3
to 4.

## Alternatives rejected

**A new opener, `||{ a ; b }`.** Rejected: a sixth block token for a construct
the graph represents as one node with a flag. It needs a glyph, and there is no
obvious one — `⫴⟦` is already fork and doubling it says nothing about time. It
also puts the difference in the lexer, so `cant fmt`, `cant convert`, the AST,
the TextMate grammar and the manifest each grow a second fork, and a reader has
to learn that `|{` and `||{` are the same construct. The modifier says "this
fork, run this way", which is what is meant.

**`:par true`, keeping values mandatory.** Rejected: a boolean that is always
`true`, since `:par false` would be a fork. It would also have set the precedent
that every future valueless modifier carries a filler word.

**`:parallel` rather than `:par`.** Rejected on balance, beside `:by` and `:max`,
which are both short. It is the sort of thing worth revisiting if `:par` reads as
"parameter" to anyone in practice.

**Encode parallelism in lowering only, with no graph field.** Rejected: it avoids
the bump and the re-fingerprinting, at the price of `cant graph`, `cant explain`
and Sigil being unable to show the one thing about the fork that changes how a
program behaves in the world. The graph is the thing that executes; a property it
cannot represent is a property those tools would describe wrongly.

**Force `--trace` to run parallel forks sequentially.** Rejected once the
counters were shown to be safe. It would have meant a traced run and an untraced
run being different programs, which is exactly what tracing must not be — and it
would have hidden the failure mode it was protecting against rather than testing
for it.

**Make a branch failure cancel the others.** Rejected: `parallel` settles the
window either way, so "cancel" would have meant discarding output from branches
that had already run, which loses what they printed before the failure. Waiting
is also what makes *which* failure is reported deterministic.

**A modifier on the branch rather than the fork** (`|{ a :par ; b }`). Rejected:
concurrency is a property of the set, not of one member. There is nothing for one
branch to be parallel *with* on its own, and the modifier grammar attaches to a
form's closing brace, which a branch does not have.
