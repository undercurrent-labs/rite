# ADR 0001 — Cant is a sibling front end, not a Rite dialect

- **Status:** Accepted, amended 2026-08-03 (see [Amendment 1](#amendment-1--one-pipeline-two-workers))
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0002 — Cant lowers through canonical Rite](0002-cant-lowers-through-rite.md)

## Context

Rite already has two spellings. `grammar/aliases.json` maps every concept to one
ASCII spelling and one glyph (`def`/`◆`, `<-`/`←`, `->`/`→`, `[[`/`⟦`), the lexer
accepts both, and `rite-fmt` converts between them through `Dialect::{Glyph,
Ascii, Mixed, Preserve}`. Both spellings produce the *same* `rite_syntax::Program`
and the same `rite_sem::ProgramIr`. That is what a dialect is here: a lexical
choice with no semantic consequence, which is why
`crates/rite-test/tests/dialect_value_parity.rs` can assert the two forms evaluate
identically.

Cant is not that. Its composition model differs from Rite's at the semantic level:

- A Cant stage emits **zero or more** values; a Rite pipeline stage yields exactly one.
- Ward (`?{ … }`) can emit nothing, which has no Rite expression equivalent.
- Scatter (`*`) and collect (`[]`) change the *arity* of the value stream.
- Fork (`|{ a ; b ; c }`) runs ordered branches from one input and concatenates results.
- Orbit (`~{ … }`) is a bounded breadth-first fixed point — the only cyclic construct
  in either language.

None of these can be expressed as an alternative spelling of an existing Rite
node. Adding them to the Rite AST or to `ExprIr` would mean every Rite consumer —
the tree-walking interpreter in `rite-runtime`, the AOT backend in
`rite-compiler/src/codegen.rs`, `rite-fmt`, `rite-analysis`, `rite-lsp`,
`rite-wasm` — grows arms it can never reach from Rite source. The interpreter and
the compiler are held to strict agreement by
`crates/rite-test/tests/interpreter_ir_parity.rs` and by every conformance case,
which runs both ways; unreachable IR variants are exactly the kind of thing that
rots on one side of that gate.

## Decision

**Cant is a separate language with its own lexer, parser, AST, and semantic graph,
living in the Rite monorepo and reusing Rite's runtime, capabilities, compiler,
diagnostics, and release pipeline through public APIs.**

Concretely, and binding on all later work:

1. Cant operators are **not** added to `grammar/aliases.json`. Cant has its own
   manifest at `grammar/cant/operators.toml`. The two files never reference each
   other.
2. Cant is **not** added to `rite_fmt::Dialect`. `Dialect` continues to mean
   "which spelling of Rite"; Cant has its own `cant::Dialect` with `Ascii` and
   `Glyph`.
3. Cant constructs are **not** added to `rite_syntax::TokenKind`, `Expr`, `Item`,
   or `Stmt`, nor to `rite_sem::ExprIr` / `StmtIr` / `ProgramIr`.
4. Cant does **not** get a `@cant` host capability, or any other public entry in
   Rite's capability namespace.
5. **Rite crates never depend on Cant crates.** The dependency edge is one-way:
   `cant-* -> rite-*`. Deleting `crates/cant-*`, `grammar/cant/`, `docs/cant/`
   and the workspace member entries must leave a Rite that builds and behaves
   identically.
6. Cant's file extension is `.cant`. `rite fmt`, `rite check` and
   `rite-cli/src/util.rs::collect_rite_files` continue to see only `.rite`.

### Permitted changes to Rite crates

Only narrow API extractions that are useful to a caller who has never heard of
Cant, and that preserve existing Rite behaviour byte for byte. Each one is
justified where it lands. The list identified during the Phase 0 audit is in
[`docs/cant/internals.md`](../cant/internals.md#reusable-rite-apis); the first is
lifting the snippet renderer out of `Diagnostic::render` so that any tool with
spans and labels — Cant, a future linter, a test harness — can render a
caret-underlined excerpt without owning a `rite_core::ErrorCode`.

Not permitted, under this ADR: changing Rite's evaluation semantics, its grammar,
its exit codes, or its effect table to make a Cant construct easier to lower.

## Consequences

**Good.** Rite's gates keep their meaning. `dialect_value_parity` stays a
statement about two spellings of one language. `interpreter_ir_parity` stays a
statement about two executions of one IR. Cant can iterate on its own vocabulary
without a Rite release, and can be removed without one.

**Cost.** Cant duplicates a lexer. It cannot reuse `rite_syntax::lex` directly
because Cant's structural operators (`->` at depth 0, `?{`, `|{`, `~{`, `[]`,
`:modifier`) mean different things than the same characters do in Rite, and
because Cant must resolve `*` positionally (scatter vs. multiply) — a decision
Rite's lexer has no reason to make. The duplication is bounded to tokenization:
Cant leaf expressions are Rite expression text and are handed to Rite verbatim.

**Cost.** Two diagnostic code spaces (`E0xx` and `CANT-Xxxx`). ADR 0002 and the
JSON diagnostic contract in the spec require a Cant diagnostic to carry the
underlying Rite code as related metadata, so the mapping stays visible rather
than being flattened away.

**Risk accepted.** A future feature might genuinely want to be in both languages.
The answer is to build it in Rite first and let Cant lower to it, not to widen
Rite's IR speculatively.

## Alternatives rejected

**Add Cant as `Dialect::Cant`.** Rejected: the dialect machinery in `rite-fmt`
assumes both inputs parse to the same `Program`. Cant does not, so every function
taking a `Dialect` would need a "this variant is different" branch, and
`dialect_value_parity` would have to grow an exception. A dialect enum that
contains one member which is not a dialect is worse than two enums.

**Extend `ProgramIr` with `Scatter`/`Ward`/`Orbit` nodes and let Cant target it
directly.** Rejected for v0 in ADR 0002. It also fails this ADR: those variants
would be unreachable from Rite source but still owned by Rite's interpreter and
its compiler backend, which must agree about them forever.

**A separate repository.** Rejected: Cant's value is that it reuses Rite's
runtime, capability enforcement, and native build. A split repo turns every
shared API into a versioned release boundary before either language is stable.

## Amendment 1 — one pipeline, two workers

**Date:** 2026-08-03. Amends the Decision; nothing above is withdrawn.

The original text said Cant reuses Rite's release pipeline, and the surrounding
notes read that as *shares the machinery, ships separately*. Both sites were
therefore described as separate deploys, on separate schedules.

**They ship together.** This is one repository with one version number and one
tag. `cant` is already built, archived and smoke-tested by
`.github/workflows/release.yml` alongside `rite`, and a Cant that could be
released independently would need its own version, its own changelog and its own
compatibility statement against whichever Rite it lowers to — which is exactly
the versioned release boundary the "separate repository" alternative was rejected
for. So `apps/cant-web` deploys in the same job as `apps/rite-web`, on the tag.

The direction of the dependency is what carries the architecture, and it is
unchanged: **`cant-* → rite-*`, never the reverse.** Rite is releasable without
Cant; Cant is not releasable without Rite, and pretending otherwise by giving it
its own cadence would have been a fiction maintained by hand.

Consequences for the requirements above:

- Requirement 5 stands as written, with one addition to what removal deletes: the
  two Cant steps in the `sites` job of `.github/workflows/release.yml`. It remains
  a deletion, not an unpicking — `crates/cant-cli/tests/removable.rs` names the
  file and what comes out of it, and the Rite deploy runs first in that job, so
  Cant cannot fail the Rite site's publish.
- Cant keeps its own worker (`cant-web`) and its own domain (`cant.rite.foo`).
  Shared *timing* is not shared *serving*: the sites remain separately
  addressable, separately built, and separately removable.
- "Cant can iterate without a Rite release" in the Consequences above is now false
  for anything that reaches users, and was always optimistic — `cant` ships inside
  the Rite archive. It stays true where it matters: Cant's vocabulary and graph
  schema can change without touching Rite's grammar, IR or gates.
