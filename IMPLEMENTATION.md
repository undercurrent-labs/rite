# Rite Implementation Status

Tracks implementation status for the Rite language and V1 tooling. Detailed design specs are kept private and are not part of this public tree.

## Thorough gap review (post-V1 pass)

### Critical language bugs fixed in this pass

| Issue | Fix |
|---|---|
| `~ status ⟦ arms ⟧` failed to parse | Match/if scrutinee disables trailing-block call sugar |
| Example ladder 04/05 broken | Updated examples; match + nested effect rules clarified |
| LSP diagnostics always at 0:0 | Analysis snapshot now emits `start_line` / UTF-16 columns |

### P0 items executed (this pass)

| Item | Status |
|---|---|
| WASM package + Studio hosted path | **Done** — pure eval in WASM; `scripts/build-wasm.sh` → studio + web `public/wasm` |
| Product site (home · docs · studio) | **Done** — `apps/rite-web`, `pnpm site:build` / `site:deploy`, routes `/` `/docs/*` `/studio` |
| Binary install (no clone) | **Done** — `scripts/install.sh` → `/install`, Release workflow tags `v*`, checksummed assets |
| Doctest runner | **Done** — `rite docs check` + `rite-doc` doctest module/tests |
| Format/convert source maps | **Done** — `LineSourceMap` + Studio/VS Code cursor restore |
| Multi-file analysis | **Done** — `WorkspaceIndex` (imports on disk, workspace symbols, references); LSP wired |
| CI release workflow | **Done** — `.github/workflows/ci.yml` (rust, wasm, studio, vsix, manifest) |
| CI matrix | **Done** — Linux, macOS and Windows; clippy is a hard gate; `deploy` requires the Rust job; a guard fails the build if generation rewrites tracked files |
| Outbound HTTP | **Done** — `@http.get` / `post` / `request`, gated per host by `net` |
| HTTP responses and static files | **Done** — `headers` on any response (content-type override, repeated `set-cookie`), `*rest` catch-all routes matched after specific ones, `@http.file` root-anchored static serving with mime types, 405 + `Allow`, `req.form` |
| Streaming output | **Done** — `RuntimeContext::sink`; `rite run` prints as the script runs |
| Script arguments | **Done** — `@process.args`, also in compiled binaries |
| Script exit codes | **Done** — `@process.exit(code)`, 0–255, no permission; same status from `rite run` and a compiled binary |

### Resolved since (this pass)

| Was | Now |
|---|---|
| A module could not `use` another module — the graph was one level deep | Modules import modules; their imports stay private to them |
| `use math` gave no qualifier, so two modules exporting one name could not both be used | Every import binds a qualifier: `math.square` as well as `square` |
| A typo in a qualified call passed `rite check` and failed at runtime naming `m__squre` | Checked when compiling, reported as `module \`m\` has no public \`squre\`` |
| A module's *qualified* use of its own import (`use ./inner as i` then `i.double`) failed at the entry as `undefined name \`i\`` — only flat cross-module calls worked | The merge returns its modules' qualifiers, scoped to the injected copies; entry code using them is still undefined |
| An unknown `@namespace` passed `rite check` and died at runtime with an uncoded message | `E042` at resolve, with a span and the `use` to add |
| `@` was only the host's sigil | `@m.square` is module access: never shadowed by locals, capability namespaces reserved as qualifiers (`use fs` is E022), aliasable as `use math as m` / `use math -> m` / `⊏ math → m` |
| Colliding exports reported a duplicate at the call site, naming neither module | Named, with the qualified forms offered as the fix |
| `item`, `room`, `world`, `test`, `ok`, `err`, `some` as parameter names bound nothing and read back as `none` | They parse as expressions wherever they can be bound |
| `!` marked only the site of a host call, so a function wrapping one was callable unmarked | Effects propagate: `◆!` declares them, inference checks the body and closes over the call graph, callers mark the call |
| `println("x")` reached the terminal with no marker, making the discipline optional | `print`/`println` take a marker like any host call |

### Remaining gaps (after this pass)

1. **WASM host I/O** — FS, HTTP listen, outbound HTTP, `@db`, `@env`, `@clock`,
   `@stdin`, `@sys`, `@udp`, `@tcp`, `@process` and `@mcp` need the native host,
   and a browser run refuses them by name: "capability @fs is native-only and
   unavailable in the browser runtime".

   **Closed:** the capabilities that need nothing a browser lacks now work there.
   `rite-caps` has its own `native` feature (on by default) gating the host half; a
   wasm32 build takes the crate without it and gets `@json`, `@csv`, `@crypto`
   (`random_bytes` included — `getrandom`'s `js` backend supplies the entropy),
   `@regex`, `@store`, `@random` and `@game`, from the same implementations and the
   same descriptors `rite run` uses. Before this the wasm build installed no host at
   all and `@json.encode` answered "capability `@json` not registered".

   **Also closed:** `RunOptions::files` resolves `use`. The module loader takes
   an in-memory overlay (dotted module names; `lib/helpers.rite` file keys
   normalise to `lib.helpers`), consulted before the filesystem, so a Studio
   run executes multi-file programs — including overlay modules that `use`
   each other. `browser_surface.rs` holds the contract.

   `cargo run -p xtask -- wasm-check` builds both browser crates for wasm32 *and*
   runs `cargo test -p rite-wasm --no-default-features`, which is the browser host
   on the host target — `crates/rite-wasm/tests/browser_capabilities.rs`.

2. **Virtual HTTP request replay in hosted mode** — UI panel exists; full handler re-entry is native-local.
3. **Scope-aware multi-file rename** — rename is now token-accurate within a document
   (skips strings, comments and substrings, and keeps locals separate from `.fields`),
   but has no scope model and does not cross files.
4. **Semantic tokens** — not implemented, and the capability is no longer advertised:
   declaring it while returning an empty token list made clients drop their TextMate
   grammar, so Rite source came back *less* highlighted. TextMate remains the highlighter.

#### P1 — Quality / polish

5. **Effect tracking follows names, not values** — a function passed by name
   (`each(shout)`) is caught, a lambda written inline carries its own marker, and a
   binding that holds an effectful function now carries the property forward, so
   `g ← shout` then `each(xs, g)`, and `f ← shout` then `f(1)`, are both caught. A
   lambda bound to a name is classified from its body.

   What remains uncovered is a function with no name to attach the property to: one
   read from a record field or list element (`each(xs, r.go)`), one received as a
   parameter (`◆ run(xs, f) ⟦ each(xs, f) ⟧`), or one returned by a call. Closing
   that needs effect polymorphism ("effectful exactly when the argument is"), and so a
   type system Rite does not have.

   **The blunt alternative was considered and rejected.** Requiring a marker wherever
   the analysis cannot tell would put one on every function that accepts a function,
   `map` included, at which point the marker distinguishes nothing.
   `docs/book/effects.md` states the boundary instead. Permissions bound what any of it
   can reach regardless.
6. **Game free-form sugar** — still prefer `@game.register_*`. The declarative
   `def item :name ⟦ … ⟧` form does not exist; `examples/text-rpg/game.ascii.rite` used to
   be written against it and is now a real transliteration of its glyph twin.
7. **`execute_command`** — three commands were advertised with no handler; the capability
   is withdrawn until they do something.
8. **Formatter sugar fidelity** — mostly closed. `..`, `..=`, `**` and `∘` build their
   `BinOp` variants rather than a call, so they survive `rite fmt` and the formatter's
   long-unreachable arms for them are now live; a single trailing block argument is
   recorded on `CallExpr` and printed back as `keep ⟦ … ⟧` rather than `keep(⟦ … ⟧)`.
   `desugar` lowers all of them to the same `NativeCall`, so nothing downstream changed.

   **`÷` and `∘` are glyph-only**, and the formatter now says so by printing them as
   `idiv(a, b)` / `compose(f, g)` in ASCII. Their supposed ASCII spellings do not lex
   as operators and cannot: both names are taken by the builtins they lower to, so a
   keyword would collide with `idiv(7, 2)`. Printing them infix in ASCII **changed the
   answer**: `7 ÷ 2` is 3, and `rite fmt --ascii` wrote `7 idiv 2`, which parses as two
   statements and is 7. `crates/rite-test/tests/dialect_value_parity.rs` runs both
   spellings and compares the values, which the text assertions did not.

   The statement sugars — `say`, `unless`, `for … in`, `while`, `loop` — now
   survive too. The parser still rewrites them (that keeps resolve, desugar and
   the effect analysis on one path), but wraps the rewrite in `Stmt::Sugared`,
   which carries the source shape; the formatter prints the shape, everything
   downstream walks the lowering. `statement_sugar_survives_formatting` in
   `crates/rite-fmt/tests/dialects.rs` pins it in both dialects.

   `rite fmt --check` is **still not a CI gate**, but the blocker moved: the
   remaining corpus diffs are dialect canonicalisation, not sugar loss. A run
   over `examples/` touches ten files, all the same two shapes — `keep { … }`
   braces canonicalised to `⟦ … ⟧` in glyph files, and dialect-neutral `use` /
   `..=` spellings replaced by `⊏` / `‥` in files the book deliberately writes
   mixed. Gating needs a decision about whether those spellings are canonical
   per-file or per-construct, not more formatter fidelity.
9. **Incremental relexing / CST** — no rowan green tree; recovery is best-effort parse.  
10. **`parallel` is the whole concurrency story.** It fans one function out over a
    list with a fixed in-flight window of 16 and joins before returning. Missing:
    a spawn/await handle for fire-now-collect-later, a configurable window, and
    running two *different* functions concurrently. All three would build on
    `RuntimeContext::fork()`, which already Arc-shares capabilities, handles and
    budget across branches. The scry-core field report reached for a bash fan-out
    because it did not find `parallel`; the book now cross-links it from the
    process and HTTP chapters.
11. **`?` is checked against host functions only.** `HOST_EFFECTS` carries a
    return-shape column, so `rite check` rejects `! @fs.exists(p)?` (E017).
    A `?` on a *user* function that answers a plain value still passes and
    fails at runtime — writing `examples/15-service` produced four of them.
    Closing it needs a "returns a result" property inferred over the call
    graph and closed to a fixed point, the way effect-ness already is: a body
    whose tail is `ok(…)`/`err(…)`, a `?`-propagating call, or a declared
    `result<T>` return. That is the same shape of analysis, not a type system.
12. **Resource limits cover external input.** `max_collection_size` and
    `max_string_size` bound what a script builds (`range`, `repeat`, `concat`)
    *and* what it takes in: `@fs.read`/`read_bytes`/`lines` check the file's
    size from metadata before the read, `@fs.read_chunk` checks the caller's
    requested length before allocating it, `@http.file` checks like
    `@fs.read_bytes`, `@process.run` drains both pipes concurrently and stops
    at the ceiling (killing the child; `.output()` used to buffer everything
    first), and `@db.query`/`query_prepared` stop collecting at
    `max_collection_size`. Outbound `@http` bodies are read streaming to
    `min(max_string_size, 64 MiB)` — remote input gets a hard default like
    `@tcp.recv`'s 16 MiB, and an over-size body is a catchable `err` since the
    remote's size is not the script's bug; the local ceilings are budget
    errors. `crates/rite-caps/tests/input_limits.rs` holds each path to its
    knob. `parallel` now runs at most 16 branches in flight, forking each
    window's contexts as it starts and absorbing them as it ends, so peak
    memory follows the ceiling instead of the list length. Still open:
    `rite run` exposes no `--max-string-size`/`--max-collection-size` flags
    (embedders set the budget directly; `cant` exposes them).

13. **Performance benchmarks** — `cargo bench -p rite-runtime` (criterion). Front end
    and interpreter are measured separately, so a parser regression and an eval
    regression cannot be mistaken for each other. Baseline, one dev machine, release:

    | Case | Time |
    |------|------|
    | `frontend/compile` small script | ~38 us |
    | `frontend/compile` 200 functions | ~1.6 ms |
    | `values/record_build` 5 fields | ~3.3 us |
    | `values/record_spread` | ~3.8 us |
    | `closures/closure_creation` x2000 | ~12.3 ms |
    | `pipelines/pipeline_map_keep` x5000 | ~12.8 ms |
    | `calls/fib_recursive` fib(20) | ~87 ms (~6.5 us/call) |
    | `floor/arithmetic_loop` x20000 | ~36 ms (~1.8 us/iteration) |

    The v1 LSP target (100-300 ms) has plenty of headroom: compiling 200 functions is
    ~1.6 ms, so analysis is nowhere near the budget.

    Those interpreter figures are after the sync-path change (see `is_sync` /
    `eval_sync` in rite-runtime): a node that cannot suspend is evaluated without
    allocating the boxed future an async tree-walker otherwise needs per node. Measured
    against the previous baseline that bought arithmetic -31%, pipelines -24%, record
    spread -21%, recursive calls -9%. What remains — ~1.8 us to evaluate
    `total := total + i * 2` once — is the floor for tree-walking at all, and is the
    number a bytecode VM would move.
13. **VS Code VSIX in CI** — package.json ready; not produced by a release job.  
14. **Example 07/08 HTTP** — blocks until shutdown (correct for servers); e2e ladder skips them.
15. **`@mcp` is partial by choice.**

    **Closed:** the client half. `@mcp.connect` opens a handle over stdio (a spawned
    subprocess, needing `--allow process`) or Streamable HTTP (needing
    `--allow net=<host>`, matched by the same `host_of` an outbound `@http.get` uses),
    and `tools` / `call_tool` / `resources` / `read_resource` / `prompts` /
    `get_prompt` / `close` take it. The grant is checked at `connect` and nowhere else.
    It negotiates the same two revisions the server answers, probing `server/discover`
    and falling back to `initialize` when the server has never heard of it. Absent on
    the client side: subscriptions, and the sampling and elicitation callbacks, which
    are a server asking the client to run a model or prompt a person and have no
    counterpart in a Rite script. A connected server's progress notifications are read
    off the wire and dropped, since `call_tool` answers one value.

    Of the protocol itself, three things are deliberately absent.
    `subscriptions/listen` and the `*ListChanged`
    notifications are not advertised, because a server's tool, resource and prompt
    tables are fixed when `@mcp.serve` starts and cannot change while it runs — the
    capability would be a claim a client could act on and never see honoured.
    `notifications/message` is not implemented because the Logging feature is deprecated
    upstream; `use @mcp.log` writes to stderr, which is that deprecation's own suggested
    migration and the only thing that works under stdio. Multi Round-Trip Requests
    (`resultType: "input_required"`) are not implemented, though every result is stamped
    through one encoder so adding the other value is a new arm rather than a refactor.
    Progress notifications reach the client on stdio; over HTTP a plain JSON response
    has no stream to carry them, so they are surfaced on stderr under `use @mcp.log`
    instead. Backward compatibility covers exactly one older revision (`2025-06-18`),
    engaged when a client sends `initialize`.
16. **`@mcp.serve` requires its `!`; `@http.listen` still does not.** The older
    construct escapes effect discipline only because it is not a `Call` — an accident of
    shape rather than a decision. The new one checks explicitly. Aligning `@http.listen`
    would break every existing script, so the two differ for now.
17. **`@proto` is native by build, not by nature.** `protox` and `prost-reflect` are
    pure Rust and do compile for wasm32 — checked, not assumed. They are behind a
    `proto` feature (on by default, off for the browser) because of size: measured
    against a serde_json-only baseline in a release `cdylib` with the workspace
    profile, `prost-reflect` alone costs +676 KB and adding the `.proto` compiler
    costs +1.1 MB, against a 962 KB `rite_wasm_bg.wasm`. The feature deliberately
    does not imply `native`, so switching it on for the browser is one word in
    `rite-wasm/Cargo.toml` if that trade ever changes.

    Three limits inside it, all documented in `docs/book/proto.md`: unknown fields
    are dropped on a decode/encode round trip; the well-known types (`Timestamp`,
    `Any`, `Struct`) decode as plain messages; and a `uint64` past `i64::MAX`
    answers `err`, because Rite has no unsigned 64-bit type and a magnitude-
    dependent result type is worse than a refusal.

#### P2 — Explicitly V2

DAP, package registry, JetBrains, collaborative Studio, cloud compile, bytecode VM.

---

## Architecture (current)

| Layer | Crates / apps |
|---|---|
| Language | `rite-syntax`, `rite-sem`, `rite-runtime`, `rite-caps`, `rite-compiler` |
| Tooling | `rite-fmt`, `rite-analysis`, `rite-lsp`, `rite-doc`, `rite-wasm`, `rite-cli` |
| Editors | `editors/vscode` |
| Studio | `apps/rite-studio` (playground) + `rite studio` Axum API |
| Product site | `apps/rite-web` (home, docs book, studio shell) → Cloudflare |
| Agent | `skills/rite` |
| Cant (sibling language) | `cant-syntax`, `cant-sem`, `cant`, `cant-cli` |
| Sigil (semantic renderer) | *planned:* `rite-sigil`, `rite-sigil-wasm`, `apps/sigil-web` |

### Cant

A **sibling front end**, not a Rite dialect: different composition semantics
(zero-or-more emissions, scatter, collect, ward, fork, bounded orbit), its own
lexer, parser and graph, executing by generating canonical ASCII Rite and passing
it through Rite's ordinary pipeline. Rite's grammar, `Dialect` enum, alias table,
IR and capability namespace are unchanged, and no `rite-*` crate depends on a
`cant-*` crate — enforced by `crates/cant-cli/tests/boundaries.rs`.

- [`docs/adr/0001-cant-sibling-frontend.md`](docs/adr/0001-cant-sibling-frontend.md)
- [`docs/adr/0002-cant-lowers-through-rite.md`](docs/adr/0002-cant-lowers-through-rite.md)
- [`docs/cant/internals.md`](docs/cant/internals.md) — pipeline, reusable Rite
  APIs, missing seams, and every conflict found between the spec and this tree
- [`docs/cant/checklist.md`](docs/cant/checklist.md) — per-criterion status

Status: **v0 complete, experimental.** Every published command works — `cant
version` / `check` / `parse` / `fmt` / `convert` / `graph` / `expand` / `explain`
/ `run` / `build` / `repl`, plus `cant -e '…'`. Programs execute on Rite's
runtime and compile through Rite's compiler; `crates/cant-cli/tests/differential.rs`
checks that `cant run`, `rite run <cant expand>` and the compiled binary agree.
`cant` ships in the release archives beside `rite` and `rite-lsp`, and
`docs/cant/graph-schema.md` freezes the graph JSON at **version 1** as the
contract Sigil consumes.

**Cant is removable.** No Rite source file mentions it at all; Rite's grammar,
conformance fixtures, examples, book and skill bundle never have. Thirteen shared
files carry a line each, all listed in `crates/cant-cli/tests/removable.rs` with
what deleting Cant does to them.

Three extractions landed in Rite for it, each independently useful and each
behaviour-preserving:

- `rite_core::render_snippet` — `Diagnostic::render` with the header lifted out,
  so any tool with spans and labels can draw a Rite-style excerpt.
- `rite::options::RuntimeOptions` — the shared meaning of `--allow` / `--deny` /
  `--timeout` / the four budget knobs. `rite run` uses it too, which is what
  proves it preserved behaviour, and it surfaced two latent bugs: a
  silently-discarded bad `--deny`, and three `ExecutionBudget` knobs `rite run`
  never exposed.
- `rite_render::svg_to_png` — arbitrary SVG to PNG, the part of `render_png` that
  was never about Rite. It builds Cant's social card without anyone installing an
  image toolchain.

### Sigil

The **semantic visual renderer**: a Cant graph projected into a deterministic
radial artifact — SVG, PNG, interactive HTML, or scene JSON — that stays legible
with every label removed. Not a runtime, not a visual programming language, and
not a Graphviz skin; `cant graph --format dot` remains the technical topology
view and does not supply Sigil's geometry.

- [`docs/adr/0003-sigil-is-a-renderer-not-a-runtime.md`](docs/adr/0003-sigil-is-a-renderer-not-a-runtime.md)
- [`docs/adr/0004-sigil-layout-is-non-semantic.md`](docs/adr/0004-sigil-layout-is-non-semantic.md)
- [`docs/adr/0005-one-renderer-in-rust.md`](docs/adr/0005-one-renderer-in-rust.md)
- [`docs/adr/0006-sigil-consumes-a-normalized-graph.md`](docs/adr/0006-sigil-consumes-a-normalized-graph.md)
- [`docs/adr/0007-veil-and-source-privacy.md`](docs/adr/0007-veil-and-source-privacy.md)
- [`docs/adr/0008-graphviz-stays-the-technical-view.md`](docs/adr/0008-graphviz-stays-the-technical-view.md)
- [`docs/adr/0009-glyph-names-a-token-sigil-names-an-artifact.md`](docs/adr/0009-glyph-names-a-token-sigil-names-an-artifact.md)
- [`docs/sigil/checklist.md`](docs/sigil/checklist.md) — per-criterion status
- [`docs/sigil/implementation-log.md`](docs/sigil/implementation-log.md) —
  deviations, discovered constraints, per-phase test results

Status: **Phases 0–8 complete, Phase 9 partial. Deploys from the tag; first
live deploy lands with the next release.**

`cant sigil` renders SVG, PNG, interactive HTML and scene JSON in three themes,
three traceries, four ornament levels and three disclosure modes.
`apps/sigil-web` runs the same renderer as WebAssembly in the browser — the
executed wasm32 build is compared byte-for-byte against a native fixture on
every site build — with selection, a Codex, a gallery, and every export the
CLI has including the interactive HTML page. No server round trip: there is no
render endpoint, and a test reads the Worker's source to keep it that way.

Of the checklist's criteria: **51 met with a test behind them, 14 partial with
the gap named, 1 not started.** All ten documentation pages exist —
`visual-language.md` is the one that teaches reading a sigil. What remains:
attaching the `sigil.rite.foo` zone in Cloudflare (the deploy itself is in
`release.yml`), and the partials named in `docs/sigil/checklist.md`;
`docs/sigil/implementation-log.md` records what each phase cost.

Two conflicts with the specification were resolved in the repository's favour and
recorded: the browser binding is `cant-sigil-wasm` rather than `rite-sigil-wasm`,
because it parses Cant source and ADR 0001 fixes the dependency edge by directory
name; and the gallery renders thumbnails live rather than baking them at build
time, which is stronger against drift.

### Key decisions

- Consolidated crates vs a fuller tooling split (boundaries documented).  
- Compiler embeds **ProgramIr** JSON, evaluates via `run_ir` (parity).  
- Dialects via parse→print (`grammar/aliases.json`).  
- Trailing blocks for `keep {…}`; **disabled** for match/if scrutinees.

---

## Acceptance snapshot

| Area | Status |
|---|---|
| MVP language + caps + HTTP + modules | Working |
| Conformance + differential | Working (+ match fixture) |
| Formatter / convert dialects | Working + property tests |
| LSP core features | Working (ranges improved) |
| VS Code baseline + full commands | Scaffold complete |
| WASM library API | Working (native host); pack optional |
| Studio local mode | Working |
| Docs book (17 chapters) | Filled |
| Diagnostic encyclopedia | Starter pages E020/E021/E024/E040 |
| Agent skill + machine manifests | Working |
| Example ladder e2e tests | Working (non-server scripts) |
| CONTRIBUTING | Added |

---

## Commands

```bash
cargo test --workspace
cargo build -p rite-cli -p rite-lsp --release

rite run examples/04-pattern-matching/main.rite --allow-all
rite convert file.rite --to ascii --stdout
rite studio --port 4041 --no-open
rite docs build && rite docs agent
rite describe language --json
```

---

## Testing summary

- Unit: lexer/parser (incl. match trailing-block), runtime, fmt dialects, analysis, wasm, permissions, HTTP handlers.  
- Conformance: arithmetic, pipeline, function, interpolation, match.  
- Differential: interpreter vs IR.  
- E2E ladder: `rite-test/tests/example_ladder.rs`.  

---

## Compatibility

V1 tooling does not change core language semantics. Public docs live under `docs/book/`; internal design notes stay out of this tree.
