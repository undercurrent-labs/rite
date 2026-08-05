# Cant internals

How Cant is built, what it borrows from Rite, and where the two meet. This is the
implementation log the spec's Phase 0 asks for; the architecture decisions behind
it are [ADR 0001](../adr/0001-cant-sibling-frontend.md) and
[ADR 0002](../adr/0002-cant-lowers-through-rite.md).

Status: **Phase 7 — v0 complete.** Every published command works, Cant ships in
the release archives, and the graph schema is frozen as experimental. See
[the checklist](checklist.md) for the per-criterion record and
[the graph schema](graph-schema.md) for the consumer contract.

## Pipeline

```text
.cant source
  -> cant-syntax   lex (ASCII/glyph normalized through grammar/cant/operators.toml)
  -> cant-syntax   parse -> Cant AST (+ the spans read as operators)
  -> cant-syntax   fmt / convert                                                    [Phase 2]
  -> cant-sem      lower  -> Cant graph (CantProgram: nodes, edges, ports, spans)
  -> cant-sem      validate + JSON / DOT export
  -> cant-sem      expand -> canonical ASCII Rite + span map
  -> cant-sem      expand -> canonical ASCII Rite + source map                       [Phase 4]
  -> rite-syntax / rite-sem   parse, load modules, resolve, desugar -> ProgramIr
  -> rite-runtime (cant run) | rite-compiler (cant build)
```

The Rite half of that is untouched. Cant's output is ordinary Rite text and
enters through `rite_sem::compile_to_ir` / `compile_path` like any other script.

## Crate dependency graph

```text
cant-cli ──────► cant ──────► cant-sem ──────► cant-syntax ──────► rite-core
                  │              │                  │
                  │              ├──► rite-syntax   └──► (rite-syntax, Phase 2+:
                  │              ├──► rite-sem            leaf-expression validation)
                  │              └──► rite-fmt
                  ├──► rite (RiteEngine facade)
                  ├──► rite-runtime
                  ├──► rite-caps
                  └──► rite-compiler
```

The edge direction is one-way and enforced by a test
(`crates/cant-cli/tests/boundaries.rs`): no `rite-*` crate's manifest may name a
`cant-*` crate, and no `rite-*` source file may `use cant_*`.

- **`cant-syntax`** — operator manifest, lexer, tokens, parser, AST, Cant
  diagnostics. Depends only on `rite-core` (spans, source files, labels,
  rendering). Deliberately *not* on `rite-syntax` yet: leaf expressions are
  carried as text plus a span in Phase 1, and are only handed to Rite's parser
  when leaf-level validation lands.
- **`cant-sem`** — the Cant graph (`graph.rs`), lowering the AST into it
  (`lower.rs`), validation (`validate.rs`) and DOT export (`dot.rs`). Lowering to
  Rite and source maps arrive in Phase 4.
- **`cant`** — the public facade (`parse`, `analyze`, `graph`, `expand`, `run`,
  `build`, `format`, `convert`, `explain`), wrapping Rite types rather than
  duplicating them.
- **`cant-cli`** — the `cant` executable.

## Reusable Rite APIs

Found during the Phase 0 audit and confirmed against the current branch, not the
docs.

| What | Where | Used for |
|---|---|---|
| `Span`, `BytePos`, `SourceSpan`, `FileId` | `rite_core::span` | Cant spans are Rite spans. No parallel span type. |
| `SourceFile`, `SourceMap` | `rite_core::source` | `line_col` counts *characters*, `line_utf16_col` gives LSP positions. Cant reuses both. |
| `Label`, `Severity`, `Diagnostics` | `rite_core::diagnostic` | Cant diagnostics carry `rite_core::Label`s so the renderer is shared. |
| `render_snippet` | `rite_core::diagnostic` | **Extracted in Phase 1** (see below). |
| `lex`, `parse_expression(FileId, &[Token])` | `rite_syntax` | Already public. The seam for validating a Cant leaf as a Rite expression. |
| `parse_source`, `parse_file` | `rite_syntax` | Checking that generated Rite parses. |
| `compile_to_ir`, `compile_to_ir_with_roots`, `compile_path`, `compile_source` | `rite_sem` | The front end generated Rite enters through. `compile_path` is the one that resolves `use` relative to the file. |
| `run_file(&SourceFile, &mut RuntimeContext)`, `check_source` | `rite_runtime` | `cant run` / `cant check`. |
| `ExecutionBudget` + `with_timeout`, `with_max_steps`, and public `max_call_depth` / `max_collection_size` / `max_string_size` | `rite_runtime::budget` | All four budget knobs the Cant CLI must expose already exist. |
| `Permission::parse`, `PermissionSet::{default_secure, allow_all, grant, deny}`, `install_defaults` | `rite_caps` | Cant adds no permission grammar of its own. |
| `RiteEngine` + `RiteEngineBuilder` | `rite` | Embedding path; `with_output` gives Cant a sink for differential stdout capture. |
| `build_script(&Path, release, emit_rust, output, &PermissionSet)` | `rite_compiler` | `cant build`, via a generated `.rite` file on disk. |
| `run_interpreted`, `run_ir_mode` | `rite_compiler` | Both execution modes, for Cant's differential tests. |
| `format_with_dialect`, `convert_source`, `Dialect`, `LineSourceMap`, `build_line_source_map`, `map_cursor` | `rite_fmt` | Canonicalizing generated Rite; `LineSourceMap` is a model for Cant's own maps (it is line-aligned and approximate — Cant's expansion map is span-to-span and exact). |
| `HostCapabilities::all_descriptors()` -> `NativeFunctionDescriptor { effectful, permission, .. }` | `rite_caps` | `cant explain` reports required capabilities from the same table `rite capabilities` reads. |

### Extraction made in Phase 7

`rite_render::svg_to_png(svg, scale)`.

`render_png` rasterises *highlighted Rite source*; the font handling underneath
it — a `usvg` database seeded with the system fonts and the face this crate
embeds, plus a real monospace fallback because `ui-monospace` is a CSS keyword
nothing has installed — was never about Rite. It is now a public function that
takes any SVG, which is what a caller with a hand-authored one needs.

Useful without Cant, and it earns its keep immediately: this repository has no
image tooling installed, and `cargo run -p xtask -- cant-og` builds a 1200×630
social card with the rasteriser already in the tree rather than one somebody has
to `apt install`.

The font handling is the reason it is worth being a real API rather than a
snippet each caller copies. The first PNG this crate ever produced had every
shape and no text at all, and every SVG assertion passed while it did.

### Extraction made in Phase 5

`rite::options::RuntimeOptions` and `rite::options::parse_duration`.

`rite-cli`'s `Commands::Run` built its `PermissionSet` and `ExecutionBudget`
inline, with `parse_duration` private beside it. `cant run` needs exactly the
same behaviour, and two copies of "what does `--allow fs:read=./data` mean" stay
in sync right up until they do not. The *meaning* of the strings is now shared;
the `clap` declarations stay each tool's own, so `rite` gains no argument-parser
dependency and each `--help` reads like its own tool's.

`rite run` was switched onto it in the same change, so the behaviour is
preserved by construction rather than by assertion. Two latent bugs surfaced
while doing it:

- **A bad `--deny` was discarded silently.** `if let Ok(p) = Permission::parse(d)`
  meant a typo in a *revocation* left the permission in place — the failure mode
  where you think you locked something down and did not. It is an error now, as
  a bad `--allow` always was.
- **`rite run` exposed two of the five budget knobs.** `max_call_depth`,
  `max_collection_size` and `max_string_size` were on `ExecutionBudget` and
  reachable from nothing. `cant` exposes all five; adding the flags to `rite run`
  is now a two-line change rather than a design question.

### Extraction made in Phase 1

`rite_core::render_snippet(header, labels, notes, help, sources) -> String`.

`Diagnostic::render` was a single 75-line function that formatted its own header
from `ErrorCode` and then did the real work — resolving each label's file,
computing the caret's display width with `unicode-width`, and printing the
excerpt. Everything after the header line is independent of what kind of code the
diagnostic carries. It is now a free function; `Diagnostic::render` builds its
header (`"{severity}[{code}]: {title}"`) and calls it. Output is unchanged
character for character, which
`crates/rite-core/src/diagnostic.rs`'s tests pin.

This is useful without Cant — anything holding spans and labels can render a Rite
style excerpt — and it is what lets Cant show `error[CANT-P004]: …` over `.cant`
source with the same carets as `rite check`.

### Missing seams (not extracted yet)

Recorded so a later phase does not rediscover them:

- **Permission and budget CLI flags** are declared inline in
  `rite-cli/src/main.rs`'s `Commands::Run` variant, and `parse_duration` is a
  private function there. `cant run` needs the same flags. Spec §2.1 explicitly
  permits moving them to a reusable library module; that extraction is scheduled
  for Phase 5 rather than Phase 1, so the flags are designed once against a real
  second consumer instead of guessed at.
- **`rite run` exposes only `--timeout` and `--max-steps`**, though
  `ExecutionBudget` also carries `max_call_depth`, `max_collection_size` and
  `max_string_size`. The Cant CLI is specified to expose all five. Setting the
  extra three on `ExecutionBudget` directly works today (public fields); adding
  the matching flags to `rite run` would be an independently sensible Rite
  improvement and is noted, not done.
- **`rite_compiler::build_script` takes a path**, reads it, and calls
  `compile_path` itself. `cant build` therefore writes its generated Rite to a
  file (under `.rite/cant/<hash>/`) before calling it. A source-plus-roots entry
  point would avoid the temporary, but the path form is also what makes the
  generated Rite inspectable, so this is not obviously a defect.

## Operator manifest

`grammar/cant/operators.toml` is the single source of truth for the ASCII
spelling, the glyph spelling, and the token kind of every structural Cant
operator. `cant-syntax` embeds it with `include_str!` and parses it at first use;
nothing hard-codes a spelling anywhere else, and
`crates/cant-syntax/tests/manifest_sync.rs` fails if the Rust `CantTokenKind`
enum and the manifest disagree in either direction.

The file is parsed by a ~90-line reader for the restricted subset it uses
(`key = "value"` and `[[operator]]` tables). That is deliberate: the workspace has
no `toml` dependency, and this repository already hand-reads `grammar/keywords.toml`
in `crates/rite-cli/tests/editor_grammar_sync.rs`. The file remains valid TOML and
is readable by any TOML tool; the reader rejects anything outside the subset with
a clear error rather than guessing.

`grammar/cant/tokens.json` mirrors `grammar/tokens.json`: version metadata only.
The token *list* lives in the operator manifest, so there is one place to change.

## Lexing and the two ambiguities

Cant's lexer is context-free and preserves everything, including trivia. Two
ASCII spellings are genuinely ambiguous, and both are resolved by the parser from
position rather than by the lexer from lookahead:

**`*` — scatter or multiply.** `[1,2,3] -> *` is scatter; `$ * 2` is
multiplication inside a leaf expression. The lexer emits one `Star` token for
both `*` and the glyph `⋇`, and records which spelling was used. The parser calls
it scatter only when a stage consists of nothing but that token. A `⋇` used
anywhere else is a parse error naming the confusion, because the glyph has no
other meaning.

**`:name` — modifier or Rite atom.** `:max 4096` configures an orbit; `:error` is
Rite's ASCII spelling of the atom `#error` (`grammar/aliases.json`), and appears
inside leaf expressions like `?{ $.level = :error }`. The parser accepts `:name`
as a modifier only immediately after a structural block's closing `}` / `⟧`.
Everywhere else it is leaf text and is handed to Rite unchanged.

A modifier's **value is optional**, and the parser does not decide which names
take one. `:par` is the whole modifier and `:max 4` is a name and a value, and
after a `}` there is no third reading to confuse them with: a bare leaf cannot
follow a block, so a run of tokens there is a value or there is nothing. Which
names may have none is `validate_modifiers`', beside which names exist —
`CANT-G022` for a `:max` written without its number, `CANT-G023` for a `:par`
written with one. `CANT-P010` was the parser's version of the first and is
retired.

A third case is handled by requiring adjacency: `?{`, `|{` and `~{` are single
tokens only when the brace immediately follows the glyph. Rite’s parser accepts
`{` as a block opener, so `? cond { … }` with a space is a Rite conditional and
stays leaf text.

A third: **`:{` — a definition or a Rite record field.** `clean:{ trim }` names a
flow; `<< f:{ |x| x } >>` is a record whose field holds a block, and is ordinary
leaf text. The lexer emits one `DefineOpen` for both, and the parser calls it a
definition only after an identifier in the preamble. Unlike `?{`, `|{`, `~{` and
`!{`, it is deliberately **not** a block opener for the purposes of "a block
opener can only start a stage": that rule breaks a leaf run, so applying it here
would truncate the record. It counts leaf depth instead, so the `}` still
matches and the leaf comes out whole.

Block nesting is tracked by the parser over `(`/`)`, `[`/`]` and `{`/`}`: a `}`
seen at leaf-depth zero closes the enclosing Cant block, and one seen deeper
belongs to a Rite closure such as `keep { |n| n % 2 = 0 }`.

## Parallel forks

`|{ a ; b }:par` lowers to Rite's `parallel(xs, f)` over one item per branch,
plus a generated **named** dispatcher that calls the right branch chain. See
[ADR 0012](../adr/0012-parallel-fork-is-a-modifier.md).

The dispatcher is the entire reason this is expressible. Rite's effect analysis
cannot see through a function *value*, so handing it a closure would have hidden
every host call inside the branches — but it does track a named function passed
as an argument (`resolve.rs`, the `each(shout)` case), so a `def!` dispatcher
forces the `!` on `parallel` and propagates effect-ness outward exactly as a
direct call would. The branch chains themselves are the ones a sequential fork
already calls; nothing about them changes.

Two properties of `parallel` are load-bearing and both are its own documented
guarantees: results come back in **input order**, which is what keeps the joined
value in branch order and therefore deterministic; and at most 16 branches are in
flight, each window settling before the next, which is why a failing branch is
reported only after its siblings finish.

**Tracing is safe over one, and was checked rather than assumed.** Counts are
`@store` reads and writes; `RuntimeContext::fork` shares the capability host
through an `Arc` so every branch increments the same namespace, the increment is
one statement with no suspension point inside it (`@store` never returns a
pending future, and the evaluator has no cooperative yield), and addition does
not care about arrival order. `a_traced_parallel_fork_counts_what_the_sequential_one_does`
in `crates/cant-cli/tests/cli.rs` compares the two traces and requires them
equal, so this stops being true loudly.

**Console output does not interleave.** Each branch buffers its own and
`parallel` splices them back in branch order. So a `:par` program prints the same
thing every run; what has no order is effects that reach the world directly —
files, hosts, `@store`.

## Named flows

A definition is spliced into the flow that used it during lowering, so nothing
downstream of `lower` knows definitions exist: not the graph, not the schema,
not `expand`, not Sigil. See
[ADR 0011](../adr/0011-named-flows-are-spliced.md).

Three consequences worth knowing before changing any of it.

**Lowering has to terminate on a program validation will refuse.** Lowering
rejects nothing, and it runs first, so a definition that names itself would
recurse until the stack ran out — with no diagnostic, because a stack overflow
is not one. `Builder::splicing` holds the names currently being spliced, and a
name already on it is left as an ordinary leaf. That terminates, and it leaves a
node for `CANT-G020` to point at.

**The four definition checks read the AST, not the graph**, for the same reason
modifier validation does: by the time there is a graph, the definition and its
name are gone. `validate_definitions` runs before `validate_modifiers` so that a
recursion is the first error reported, since the graph after that is not worth
describing.

**Effect-ness is per splice, and gets it for free.** Each use produces fresh
nodes with fresh hygienic names, so `expand` computes effects over the generated
call graph exactly as it always did and each splice site gets its own `def!` and
its own `!`. `conformance/cant/execution/definition-effectful` pins it: one
definition holding `!@fs.read`, used in two fork branches, two `def!` functions,
and a permission denial without the grant.

`cant-sem` gained a dependency on `rite-sem` for this, for
`resolve::BUILTIN_NAMES` alone: a definition may not shadow a name Rite binds in
every scope, and the alternative was a second copy of that list.

## Formatting and conversion

Both consume `ParseResult::structural` — the list of tokens the parser consumed
*as Cant operators*, recorded by the code that made the judgement. That is the
whole design. The lexer cannot know whether a `}` closes a Cant block or a Rite
closure, whether a `*` is scatter or multiplication, or whether a `[]` is collect
or an empty list; the answer is positional and only the parser has the position.
Recording it once means neither the formatter nor the converter re-derives it,
and neither can disagree with the parser about what the program is.

`convert` is a **splice**: it copies the source and replaces exactly those spans.
Everything else — whitespace, line breaks, comments, strings, leaf text — is
byte-identical by construction rather than by care. `convert_offset_map` is
therefore exact rather than interpolated, since only the operator spans change
length.

`format` reprints from the AST and re-attaches comments by span. It refuses two
things on purpose: a source with syntax errors (the AST is a recovery, and
reprinting a guess is how a formatter loses code) and its own output if the
comment multiset changed (a backstop, matching `rite-fmt`'s).

## The graph

Three decisions in it are worth stating, because each one had an easier
alternative that would have been wrong.

**The orbit's loop is a real edge.** It would be simpler to leave it out and let
the `Orbit` node imply it — but then "every cycle must be orbit-owned", the one
structural rule v0 enforces, would be checking a graph in which no cycle can
appear. The edge is present so validation is a genuine question rather than a
tautology, and `EdgeRole` labels what each edge is *for* so the check does not
have to re-derive it from shape.

**A fork's enter and join edges are not a cycle.** This one was found by a test
rather than by thinking: cycle detection over all edges reported every program
containing a fork as an illegal cycle, because a branch's `Join` edge points back
at the fork that opened it. Only `Flow` and `Enter` carry control forward, so
only they can form a loop that runs twice. The join is a concatenation point.

**Lowering never rejects anything.** A `:max` of `"eight"` and a ward that reads
a file both produce a graph, and validation refuses them. The graph is what a
diagnostic points *at*, so it has to exist first — and `cant graph` on a broken
program is more useful than a refusal, because seeing the shape is usually how
someone works out what went wrong.

### Validation runs on untrusted input

The structural checks — dangling edges, port bounds, duplicate identifiers,
unowned cycles — cannot fail on a graph that came from `lower`, because the
builder assigns all of it. They exist for graphs that arrive as **JSON**, which
the specification requires be unable to smuggle in an unvalidated cycle. On a
freshly lowered graph they cost one pass and find nothing, which is the right
price for not having to trust the input. The cycle search uses an explicit stack
rather than recursion for the same reason: deserialized input can be arbitrarily
deep, and trading one unbounded construct for another would defeat the point.

### Where each check lives, and why it is not all in one place

Modifier validation reads the **AST**, not the graph. Lowering consumes `:by` and
`:max` into the orbit's policy and drops everything else, so by the time there is
a graph an unknown `:depth` has already vanished. Reporting it needs the thing
that was written.

## Expansion

Generated Rite is one function per node, chained. A stage becomes a loop over the
incoming emissions; a ward a conditional emission; a fork a call to each branch's
chain, concatenated; an orbit a FIFO worklist, a seen-set and a bounded `while`;
a rescue a `match` over each emission whose `err` arm calls the handler's chain.
The program boundary normalizes zero, one and many.

### Three ways to apply a leaf, and why

Established by experiment against the real `rite` binary, not assumed:

| Leaf | Lowering | Because |
|---|---|---|
| contains `$` | substitute `$` with the emission variable | Rite rejects `$` outside a call: `5 -> ($ > 2)` is a runtime error and `3 -> $ + 1` is `E015`. A ward predicate like `$ % 2 = 0` has no pipeline form. |
| is entirely a capability call | insert the receiver: `!@fs.read` → `!@fs.read(__e)` | Rite's pipeline does **not** insert into `@cap.fn` — `"[1]" -> @json.decode` fails with "expects string" because nothing was passed — and an effect marker cannot appear inside a pipeline stage at all (`x -> ! @fs.read` does not parse). Both force the direct call, which is also what ADR 0002 wants. |
| anything else | a Rite pipeline, `__e -> leaf` | Rite's own first-argument insertion, so Cant cannot drift from it. |

Substitution re-lexes the leaf with the Cant lexer rather than scanning text, so
a `$` inside a string stays a `$` inside a string.

### Effects

Effect-ness is computed over the *generated* call graph and iterated to a fixed
point: a fork or orbit is effectful exactly when something inside it is. Every
generated function that performs a host call holds that call in its own body,
marked, and is declared `def!`; call sites carry `!`. Rite's resolver re-derives
all of it independently and rejects the expansion if we got it wrong — `E021`,
which is what `every_expansion_passes_rite_check` is really testing.

### Diagnostics

Rite reports an unmarked host call **three times**: at the call, at the generated
function containing it, and at the generated `main`. Only the first names
something a user wrote. `collapse_cascades` drops any diagnostic whose text
mentions a generated identifier — the test is the *text*, not the span, because
the second one maps to a perfectly good Cant span (the user's leaf is inside that
function). When every diagnostic names a generated identifier they are all kept:
that means Cant generated something wrong, and hiding it would turn a bug here
into a mystery.

## Leaf expressions

A leaf is Rite expression text. Phase 1 stores it as `(source text, span)` plus
two flags the Cant lexer can determine on its own: whether it contains a Cant
effect marker, and whether it contains an explicit `$` placeholder. That is
enough for the v0 rules that Cant owns — a ward predicate and an orbit `:by`
function must not be effectful — while leaving every question about *names* to
Rite's resolver, which is the only thing that can answer them.

Leaf text reaches generated Rite verbatim, so `!@fs.read` in Cant is `!@fs.read`
in Rite. Rite's lexer accepts `!` as `TokenKind::Effect` and `@` as
`TokenKind::Host` in ASCII source (they are the glyph spellings of `do` and
`host.`, and the lexer is dialect-agnostic), so the compact Cant spelling is
already valid Rite.

## Conflicts between the specification and the current repository

Recorded rather than resolved by weakening the spec, per the brief.

1. **~~Cant v0 has no way to define a function, but the required examples call
   user-defined ones.~~ Resolved in Phase 4.** The answer is builtins, Rite
   closures inside a leaf, and `use` of a Rite module for anything larger — Cant
   does not learn to parse Rite declarations. The examples were rewritten to need
   only builtins and closures, which is what made them runnable; `use` is
   designed but not yet implemented. Original note follows.

   **Was:** Spec §15.1 and §15.3 use `square`, and §15.5 uses
   `dependencies`, `canonical`, and `resolve`. None is a Rite builtin
   (`crates/rite-runtime/src/builtins.rs` has `map`, `keep`, `sum`, `join`,
   `length`, … but no `square`), and Cant defines no `def`. As written, those
   examples cannot run. Three ways out — allow Rite item declarations as a
   preamble in a `.cant` file, support `use` of a Rite module, or restrict the
   examples to builtins — and the choice belongs to Phase 4/5 when lowering makes
   the cost of each visible. Flagged as **open**; Phase 1 parses `square` as a
   leaf without complaint, and Rite's resolver will report `E020` remapped to
   Cant until this is settled.

2. **`grammar/cant/tokens.json` is listed as a token table.** Rite's
   `grammar/tokens.json` is four version keys, not a token list. Cant mirrors the
   Rite file's actual role; the operator manifest holds the tokens. No
   duplication.

3. **Modifier glyph is "same" as ASCII (spec §6).** True, but the ASCII spelling
   collides with Rite's ASCII atom spelling. Resolved positionally, above. Worth
   noting because it means a modifier is *only* legal after a block close — the
   grammar is stricter than "`:name value` configures the preceding form" reads.

4. **Exit codes (spec §10.2) say "existing corresponding Rite exit category"
   without a mapping.** Rite's contract is 0 success, 1 runtime, 2 usage, 3
   parse, 4 resolve, 5 permission, 6 compile, 7 test, 8 budget
   (`Diagnostics::rejection_exit_code` picks 3 vs 4 by whether any error code is
   below 20). Cant's mapping is fixed here: lexical and parse diagnostics
   (`CANT-L`, `CANT-P`) exit **3**; graph, semantic and expansion diagnostics
   (`CANT-G`, `CANT-S`, `CANT-X`) exit **4**; orbit budget exhaustion
   (`CANT-O`) exits **8**, matching Rite's budget category; invalid CLI usage
   exits **2**. Anything raised by Rite after expansion keeps the exit code Rite
   gives it.

5. **`cant -e` as a top-level flag** (spec §4.2) coexists with subcommands.
   `rite`'s CLI already rewrites a leading non-subcommand positional into
   `run` (`rewrite_argv_for_implicit_run`). Cant's `-e` is unambiguous by
   comparison — it takes a value and implies `run` — but `run` does not exist
   until Phase 5, so Phase 1 accepts `-e` only on the commands it has
   implemented. No ambiguity is being designed in; it is sequencing.

## Running and building

`cant run` expands, then hands the generated Rite to `rite_runtime::run_file`.
`cant build` writes the expansion to `.rite/cant/` **beside the source** and
hands the path to `rite_compiler::build_script`. Beside rather than in a
temporary directory for two reasons: `build_script` resolves modules relative to
the file it is given, and a compiled Cant program should still be auditable
afterwards — that file is the audit.

### Two decisions the differential harness forced

**Exit codes are Rite's, always.** An orbit reaching its `:max` was briefly
reclassified as a budget exhaustion (8) because that reads better. The harness
immediately caught `cant run` reporting 8 while `rite run <cant expand>` reported
the `panic` it really is as 1 — the two paths disagreeing about one execution,
which is precisely the claim ADR 0002 exists to protect. The reclassification was
reverted; the stable identifier is the *code*, `CANT-O002`, which is what the
specification actually asks for. Rite's own budgets are a genuine exhaustion and
keep exit 8 as `CANT-O001`.

**Generated code tags its own failures.** The scatter type check and the orbit
limit `panic` with a message that starts `CANT-R003: ` / `CANT-O002: `, so
`cant run` classifies them without pattern-matching prose. The tag is left in the
generated Rite deliberately: someone running the expansion directly with
`rite run` gets a code they can look up rather than an anonymous panic.

### What the user is not shown

Rite appends a stack traceback per frame, and every frame of a Cant program is
generated scaffolding — three tracebacks naming `cant_1f4a9c2b_n2` for a
two-stage program. §2.4 forbids putting that in front of anyone, and a traceback
through scaffolding is not something anyone can act on, so it is stripped.
`cant expand` is how to see it. A test over every execution fixture asserts no
diagnostic ever contains `cant_` or `<generated>`.

## Release and removability

`cant` ships in the same archive as `rite`. A release that carried one and not
the other would make the pair harder to keep in step than keeping them together
ever was, and the archive is where anyone looking for either will look.

The removability claim is checked rather than asserted, by
`crates/cant-cli/tests/removable.rs`. `boundaries.rs` checks the dependency
direction, which is necessary and not sufficient: a Rite test could assert
something about a Cant fixture, a Rite script could shell out to `cant`, a shared
file could grow a Cant-only branch. None of those is a dependency edge and all of
them would break the removal.

The result is stronger than expected. **No Rite source file mentions Cant at
all** — two that did were reworded in Phase 7, a comment in `rite-cli` and an
example header string in a `rite-core` test, both of which read better without
the coupling. Rite's grammar, conformance fixtures, examples, book and skill
bundle have never mentioned it. What remains is thirteen shared files, each
listed with what removing Cant does to it: four workspace members, three pnpm
scripts, two ignore lines, a CI job, some release lines, an installer block, a
host key. Every one is a deletion.

## Conflicts found in Phase 4

6. **Generated Rite is not piped through `rite fmt`, contrary to spec §8.1.**
   Rite's formatter expands the statement sugars — `for`, `while`, `say`,
   `unless` — because `items.rs` rewrites them before the AST exists
   (`IMPLEMENTATION.md` gap 8 says so, and it is why `rite fmt --check` is not a
   CI gate). Running the expansion through it turns

   ```rite
   for __e in __in [[ __out := concat(__out, [ __e -> f ]) ]]
   ```

   into

   ```rite
   __in -> each([[ |__e| __out := concat(__out, [__e -> f]) ]])
   ```

   which is the same program and materially harder to read. `cant expand` exists
   so a program can be *audited*; formatting it into a shape the book does not
   teach defeats that. The generator emits already-canonical, deterministic ASCII
   Rite instead, and the invariant that is actually tested is §8.1's first
   bullet — Rite accepts it — rather than "rite-fmt would not change it".

7. **A leaf can be valid Cant and invalid Rite.** `[[1, 2], [3]]` parses as a
   Cant leaf (Cant's `[` is just a bracket) and is not a valid Rite list, because
   Rite lexes `[[` as its block opener. This was a real bug in
   `examples/cant/04-scatter-collect`. It is caught by `cant check`, which runs
   the expansion through Rite's front end and remaps the parse error onto the
   leaf as `CANT-S004` — a *semantic* failure of the program, exiting 4, not a
   syntax error in the Cant, which parsed fine.

8. **Spec §5.3 says `path -> !@fs.read` means `! @fs.read(path)`.** True as a
   description of Cant, but it cannot be lowered as a Rite pipeline: Rite does
   not insert a pipeline value into a capability call, and does not accept an
   effect marker inside a stage. Cant does the insertion itself. See the leaf
   table above.

## Testing

Phase 1 tests live with their crates:

- `crates/cant-syntax/tests/manifest_sync.rs` — the manifest and the token enum
  agree, both directions.
- `crates/cant-syntax/tests/lexer.rs` — spelling normalization, strings and
  comments containing operator characters, trivia preservation, round-tripping
  the token stream back to the exact source.
- `crates/cant-syntax/tests/parser.rs` — every construct, ASCII and glyph
  producing equal ASTs, and structured diagnostics for malformed input.
- `crates/cant-syntax/tests/no_panic.rs` — a deterministic corpus of malformed
  and truncated sources, every prefix of every fixture, plus a `proptest`
  generator over operator characters; parsing must always terminate with
  diagnostics and never panic.
- `crates/cant-sem/tests/graph.rs` — the §7.1 validation list, one test that
  trips each check and one that does not, plus hand-edited JSON for the
  untrusted-input cases.
- `crates/cant-sem/tests/dot_renders.rs` — shells out to Graphviz and requires
  exit 0 *and* empty stderr, because `dot` warns rather than fails on most
  malformed input. Prints a note and returns when `dot` is absent, so a machine
  without it reports the hole instead of a false green.
- `crates/cant-syntax/tests/fmt.rs` — layout rules, and the §11.2 properties
  (idempotence, dialect round trip, program preservation, comment preservation,
  source-map bounds) over the whole fixture corpus plus a generator.
- `crates/cant-cli/tests/cli.rs` — the real binary: exit codes, which stream a
  diagnostic lands on, the three source forms, and that `--help` advertises only
  what is implemented.
- `crates/cant-cli/tests/boundaries.rs` — the dependency direction from ADR 0001,
  plus that Rite's grammar files and `rite_fmt::Dialect` never mention Cant.

Fixtures are under `conformance/cant/` and `examples/cant/`. The conformance
*runner* arrives with execution in Phase 5; until then the fixtures are consumed
by the parser tests, and a test asserts every fixture directory is reachable so
none is silently orphaned.
