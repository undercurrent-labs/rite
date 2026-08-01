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
| Streaming output | **Done** — `RuntimeContext::sink`; `rite run` prints as the script runs |
| Script arguments | **Done** — `@process.args`, also in compiled binaries |
| Script exit codes | **Done** — `@process.exit(code)`, 0–255, no permission; same status from `rite run` and a compiled binary |

### Resolved since (this pass)

| Was | Now |
|---|---|
| A module could not `use` another module — the graph was one level deep | Modules import modules; their imports stay private to them |
| `use math` gave no qualifier, so two modules exporting one name could not both be used | Every import binds a qualifier: `math.square` as well as `square` |
| A typo in a qualified call passed `rite check` and failed at runtime naming `m__squre` | Checked when compiling, reported as `module \`m\` has no public \`squre\`` |
| Colliding exports reported a duplicate at the call site, naming neither module | Named, with the qualified forms offered as the fix |
| `item`, `room`, `world`, `test`, `ok`, `err`, `some` as parameter names bound nothing and read back as `none` | They parse as expressions wherever they can be bound |
| `!` marked only the site of a host call, so a function wrapping one was callable unmarked | Effects propagate: `◆!` declares them, inference checks the body and closes over the call graph, callers mark the call |
| `println("x")` reached the terminal with no marker, making the discipline optional | `print`/`println` take a marker like any host call |

### Remaining gaps (after this pass)

1. **WASM host I/O** — browser run evaluates pure scripts + `@console`; FS, HTTP listen,
   outbound HTTP, `@db`, `@udp`, `@tcp` and `@process` need the native host. The wasm
   build installs no capability host at all (`rite-caps` is behind the `native`
   feature), so the capabilities that *are* pure value transforms — `@json`, `@csv`,
   and all of `@crypto` except `random_bytes` — are unreachable there too, despite
   needing nothing the browser lacks. Closing that is a packaging change in
   `rite-wasm`, not a change to those capabilities.

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

   Still lowered: the statement sugars — `say`, `unless`, `for … in`, `while` — which
   `items.rs` rewrites into their expanded forms before the AST exists, so `rite fmt`
   prints the expansion. `examples/sugar/demo.rite` is the file that shows it.

   Because of those, `rite fmt --check` is **not yet a CI gate**: it would demand
   rewriting the corpus into shapes the book does not teach. Retaining the statement
   sugars unblocks it.
9. **Incremental relexing / CST** — no rowan green tree; recovery is best-effort parse.  
12. **Resource limits are partial** — `max_collection_size` and `max_string_size` are
    enforced where a collection's size is knowable before it is built (`range`,
    `range_incl`, `repeat`, `concat`), where the runaway cases were. Not yet
    bounded: `@fs.read`/`read_bytes`/`lines` and `@fs.read_chunk`'s caller-supplied
    length, `@http` response bodies, `@process.run` output capture, and `@db.query`
    result sets. Each can still buffer an unbounded amount from outside the program.
    `@http.listen` (1 MiB), `@tcp.recv` (16 MiB) and `@udp.recv_from` (65535) are the
    ones already capped, and are the model. `parallel` still forks one deep-cloned
    context per element eagerly, with no concurrency ceiling.

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
