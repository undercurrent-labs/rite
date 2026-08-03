# ADR 0002 — Generated canonical Rite is the v0 execution boundary

- **Status:** Accepted
- **Date:** 2026-08-03
- **Related:** [ADR 0001 — Cant is a sibling front end](0001-cant-sibling-frontend.md)

## Context

A Cant program has to execute somewhere. There are three places it could enter
Rite's stack:

1. **Source.** Lower the Cant graph to ASCII Rite text and feed it to
   `rite_sem::compile_to_ir` / `compile_path`, i.e. the ordinary front end.
2. **IR.** Construct a `rite_sem::ProgramIr` directly and hand it to
   `rite_runtime` and `rite_compiler`.
3. **Runtime.** Interpret the Cant graph in a new Cant evaluator over
   `rite_runtime::Value`.

Facts from the Phase 0 audit that bear on the choice:

- `ProgramIr` is the declared boundary between Rite's front end and every
  consumer (`CLAUDE.md`: "the boundary between front end and every consumer:
  interpreter, compiler, analysis, WASM"). But the *invariants* consumers rely on
  are established upstream of it, in `rite-sem/src/resolve.rs`: effect inference
  closed to a fixed point over the call graph, `E021` when a host call is
  unmarked, arity and binding checks, module name mangling
  (`math.square` -> `math__square`) validated in both `desugar` and `resolve`.
  A hand-built `ProgramIr` skips all of it.
- Rite's effect discipline is enforced by the resolver, cross-checked against
  `rite-caps`' `NativeFunctionDescriptor { effectful, .. }` by
  `crates/rite-caps/tests/effect_parity.rs`. There is no independent check at the
  IR level that would catch a Cant lowering that lost a `!`.
- `rite_compiler::build_script` takes a `&Path` and calls `compile_path` itself.
  Native builds therefore *already* require Rite source on disk; a direct-IR path
  would need a second entry point into the compiler.
- Rite's effect analysis is incomplete for effectful functions passed through
  parameters, records, or returned values (spec §8.4, and `IMPLEMENTATION.md`'s
  gap list). Any lowering that routes Cant bodies through a higher-order helper
  can therefore *silently* lose an effect marker.

## Decision

**For v0, Cant lowers to canonical ASCII Rite source and executes by passing that
source through the ordinary Rite pipeline: parser, module loader, resolver,
desugar, `ProgramIr`, then `rite-runtime` or `rite-compiler`.**

Binding consequences:

1. `cant run`, `cant check`, and `cant build` all go through generated Rite.
   There is no second execution path and no Cant evaluator.
2. `cant expand` is a **public, permanent** command, not a debugging aid. It
   prints exactly the text that `cant run` executes. Generated Rite must be
   valid, deterministic for a given (source, tool version), formatted through
   `rite_fmt`, hygienic, and source-mapped.
3. **Effectful calls stay structurally visible.** Ward predicates, fork branches,
   and orbit bodies lower to generated Rite *function bodies containing the call
   sites*, never to closures handed to an opaque helper. This is not a style
   preference: given the gap above, it is the only shape in which Rite's resolver
   can see the effect and demand a `!`. Conformance fixtures assert that a
   missing marker inside an orbit or fork is rejected, and that a capability
   inside one is still permission-gated.
4. **No private runtime intrinsic without its own ADR.** Where generated Rite is
   verbose, it stays verbose. An intrinsic would have to be implemented twice —
   `rite-runtime` and `rite-compiler/src/codegen.rs` — to satisfy the parity
   gate, and would be a Rite language change in everything but name.
5. Diagnostics from generated Rite are remapped: the primary label points at
   `.cant` source whenever a mapping exists, and the Rite code and generated span
   are carried as related metadata. A user must never be shown a generated
   identifier as the location of their mistake.

Direct `ProgramIr` construction is deferred, not forbidden. Revisiting it
requires a superseding ADR that demonstrates the resolver, effect, source-map,
and module invariants above are preserved — with the tests that prove it.

## Consequences

**Good.** Cant gets interpreter/compiler parity, capability enforcement, effect
checking, native builds, and budgets for free and *correctly*, because it is the
same code path Rite uses. `cant expand` gives every Cant program an auditable
plain-Rite reading, which is worth more than the performance it costs.

**Good.** The failure mode is legible. If a Cant program misbehaves, the question
"is this a Cant lowering bug or a Rite bug?" is answered by running
`cant expand | rite run`. The differential tests required by the spec are exactly
this comparison, so the debugging tool and the gate are the same artifact.

**Cost — compile time.** Every `cant run` parses and resolves generated Rite.
Acceptable: it is milliseconds, and it is what `rite run` already pays.

**Cost — generated-code size.** Orbit lowers to a worklist, a seen-set, and a
bounded loop; fork to sequential branch evaluation and concatenation. A dense
one-liner expands to a screenful of Rite. This is a feature of `cant expand` and
a cost everywhere else.

**Cost — hygiene is now a real obligation.** Generated helper names must not
collide with user names or with each other. A reserved prefix alone is not
enough; names combine the prefix, a hash of the Cant source, and a monotonic node
number, so two different Cant programs cannot generate the same helper and a user
identifier cannot collide with one by accident.

**Constraint discovered.** Generated Rite must be *canonical ASCII* Rite, but
Cant's canonical spelling of a capability call is the compact `!@fs.read`. Rite's
lexer accepts `!` (`TokenKind::Effect`) and `@` (`TokenKind::Host`) in ASCII
source — they are the glyph spellings of `do` and `host.`, and the lexer is
dialect-agnostic — so the compact form is directly valid Rite. Whether the
generated text prints `!@fs.read` or `do host.fs.read` is a formatter question
settled in Phase 4, not a semantic one.

## Alternatives rejected

**Direct `ProgramIr`.** Rejected above: it bypasses the resolver, which is where
effect inference, `E021`, arity checking, and module mangling validation live.
Reimplementing those checks over the Cant graph would mean maintaining a second
copy of Rite's semantic rules, and a Cant program could then execute something
`rite check` would have rejected.

**A Cant evaluator over `rite_runtime::Value`.** Rejected: it creates a third
execution path that must agree with the interpreter and the compiler, when the
existing two already need a dedicated parity gate to stay honest. It also
forfeits `cant build` entirely.

**Lower to one higher-order runtime helper per construct** (e.g. a generated
`orbit(seed, body_fn, id_fn, max)`). Rejected: this is the exact shape §8.4 warns
about. An effectful body reaching `orbit` as a function value is invisible to
Rite's effect analysis, so a Cant program could perform `@fs.read` without a
marker and without a grant. Structural generation is more verbose and is correct.
