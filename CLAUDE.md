# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Rite is a scripting language implemented in Rust: dual-dialect syntax (glyph `◆ ← → ⟦⟧` / ASCII `def <- -> [[ ]]`), explicit effect markers, capability-based permissions, a tree-walking interpreter, an AOT compiler, LSP, WASM build, VS Code extension, and a Vue Studio/product site. Cargo workspace at the root plus a pnpm workspace over `apps/*` and `editors/*`.

Toolchain is pinned in `rust-toolchain.toml` (1.97.1) and pnpm is pinned to 9.0.0 in `package.json` — CI matches both exactly.

## Commands

```bash
# Build. Integration tests spawn target/debug/rite, so build the CLI before testing.
cargo build -p rite-cli -p rite-lsp
cargo test --workspace --all-features --no-fail-fast

# The three CI gates, in the order CI runs them
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings   # hard gate, tree is clean
cargo test --workspace --all-features --no-fail-fast

# Single test target / single test
cargo test -p rite-caps --test http_handlers
cargo test -p rite-test --test conformance_gate
cargo test -p rite-runtime sync_path -- --nocapture

# Heavy tests are #[ignore]d (each does a cold cargo build of a generated crate, minutes each)
cargo test -p rite-compiler -- --ignored

# Run the CLI from source
cargo run -p rite-cli -- run examples/01-values/main.rite --allow-all

# Docs + agent skill (both regenerate tracked/derived output; see CI guard below)
cargo run -p rite-cli -- docs build
cargo run -p rite-cli -- docs check          # doctests over fenced `rite` blocks in docs/
./target/release/rite docs agent --output skills/rite

# Packaging gate — run before push/release
bash scripts/check-packaging.sh
RITE_SKIP_VSIX=1 cargo test -p rite-cli --test packaging -- --nocapture   # skips the node/npm VSIX half

# Benchmarks (criterion; frontend and interpreter measured separately)
cargo bench -p rite-runtime

# Web side
pnpm install
pnpm site:dev            # http://127.0.0.1:5173 → / /docs /studio
pnpm site:build          # WASM + apps/rite-web/dist
pnpm wasm:build          # scripts/build-wasm.sh → studio + web public/wasm

# xtask is a thin wrapper: cargo run -p xtask -- test | fmt | clippy | doc | examples
```

## Architecture

**Front end is one pipeline**, entered through `rite_sem::compile_to_ir` / `compile_path`:

```
parse (rite-syntax) → module load (rite-sem::modules) → resolve (rite-sem::resolve)
  → desugar (rite-sem::desugar) → ProgramIr (rite-sem::ir)
```

`ProgramIr` is the boundary between front end and every consumer: interpreter, compiler, analysis, WASM.

**Two execution paths that must agree.** The tree-walking interpreter in `rite-runtime` is normative. `rite build` (`rite-compiler`) lowers IR to Rust, emits a crate under `.rite/build/<sha>/` that embeds the `ProgramIr` as base64 JSON, and falls back per-statement to the interpreter for anything the backend can't express. Statements the backend *does* compile must produce identical results. This invariant is checked by `crates/rite-test/tests/interpreter_ir_parity.rs` (in-process, milliseconds) and by every conformance case, which runs both ways. **Any change to evaluation semantics has to land in both `rite-runtime` and `rite-compiler/src/codegen.rs`, or parity tests fail.** Shared operator semantics live in `rite-runtime/src/ops.rs` specifically so generated code can call them.

The generated crate takes its `rite-*` deps from a local checkout when one is found, otherwise from git (`RITE_SOURCE_DIR` overrides; `RITE_BUILD_GIT_REF` picks the ref). Publishing to crates.io is not an option — see the `DepSource` comment in `rite-compiler/src/lib.rs` before "fixing" this.

**Effects propagate.** `!` marks a host call; `◆!` / `def!` declares a function that performs one. `resolve.rs` infers effect-ness from bodies and closes it over the call graph to a fixed point, so a function calling an effectful function is itself effectful. The declaration is the contract callers see; inference only checks the contract is honest. Adding a host function means adding it to the effect table below, or it silently becomes pure.

**Effects are checked in two places and cross-validated.** `rite-sem/src/resolve.rs` holds the canonical effect table (which `@host.fn` calls require a `!` / `do` marker); `rite-caps` carries an `effectful:` flag per `NativeFunctionDescriptor`. `crates/rite-caps/tests/effect_parity.rs` fails if they disagree in either direction, so a new host function must be added to both. Reads are effects: `@fs.read`, `@env.get`, `@db.query` need `!` for the same reason `@clock.now` does.

**Permissions** (`rite-caps/src/permissions.rs`) default-secure: console/clock/random allowed, fs/net/env/process/db denied; any default is revocable with `--deny`. Grants are canonicalized against the CWD at *build* time for compiled binaries. `--allow net=host` gates both `@http.listen` bind addresses and outbound `@http.get` targets.

**Dialects are parse→print, not two grammars.** `grammar/aliases.json` is the single table mapping concept → ASCII spelling → glyph → token. `rite-fmt` formats/converts through it. When you touch it, keep `editors/vscode/syntaxes/rite.tmLanguage.json` in sync (TextMate is the only highlighter — semantic tokens are deliberately not advertised; see `IMPLEMENTATION.md`).

**Modules merge into one flat scope.** `merge_exports_into_entry` copies every imported module's public functions into the entry AST, and `inject_dependencies` does the same for each module before it is resolved (which is what lets a module `use` another). Qualified access is name-mangling: `math.square` becomes the global `math__square`, rewritten in `desugar` and validated in `resolve` — so a qualifier must be bound in both places or you get either a false "undefined name" or a runtime failure naming the mangled symbol. A local binding shadows a module name, so both sites check that the mangled global exists rather than trusting the import alone.

**Analysis is shared, not LSP-specific.** Put logic in `rite-analysis` (snapshots, `WorkspaceIndex`) so `rite-lsp`, Studio, and WASM all get it. `rite-lsp` should stay thin.

**Crate map** (`crates/`): `rite-core` (spans, diagnostics, error codes) · `rite-syntax` (lexer, parser, AST) · `rite-sem` (resolve, modules, desugar, IR) · `rite-runtime` (values, evaluator, budget, ops) · `rite-caps` (host capabilities + permissions) · `rite-compiler` (IR → Rust → cargo) · `rite-fmt` · `rite-doc` · `rite-test` (conformance + differential harness) · `rite-repl` · `rite-analysis` · `rite-lsp` · `rite-wasm` · `rite` (embedding API, `RiteEngine`) · `rite-cli`.

## Conformance fixtures

`conformance/<area>/<case_name>/` with `case.rite` plus sidecars: `expected.exit`, optional `expected.value.json`, optional `expected.stdout`, `permissions.toml`. The runner (`rite-test/src/conformance.rs`) executes each case interpreted **and** through `run_ir_mode` and requires both to match each other and the expectations. A fixture that can't be read is a failure, not a skip. Adding language behaviour without a fixture here is incomplete work.

## Conventions worth knowing

- **Diagnostics carry stable codes.** Add to `rite-core/src/error_codes.rs`, emit with spans, and add a page under `docs/diagnostics/E0xx.md`. Codes are grouped: E0xx lex, E01x parse, E02x resolve/module, E03x runtime, E04x permission, E05x compile, E06x http.
- **CI fails if generation rewrites tracked files.** `rite docs agent` writing into `skills/` must be idempotent — regenerate and confirm `git diff --quiet -- skills/ docs/ examples/ apps/` before pushing. This guard exists because an in-place regeneration once truncated `SKILL.md` to zero bytes.
- **Line endings are pinned to LF by `.gitattributes`** on every platform; CRLF breaks both `rite fmt --check` and the generation guard.
- **Windows CI is opt-in** (`workflow_dispatch` / `gh workflow run ci.yml`), not removed — the portability fixes stay in place.
- **HTTP tests share process-global state** via `RITE_HTTP_TEST` / `RITE_HTTP_TEST_SECS` env vars and a lock; new HTTP tests must take `http_test_lock()`.
- **Exit codes are part of the contract:** 0 success, 1 runtime, 2 usage, 3 parse, 4 resolve, 5 permission, 6 compile, 7 test, 8 budget.
- Commit messages follow conventional-commit prefixes with a plain-English subject (`fix(caps): …`, `perf(build): …`).

## Adding things (see CONTRIBUTING.md for the full checklists)

- **Syntax**: `grammar/rite.ebnf` + `aliases.json` → lexer/parser → `rite-sem` desugar/IR → `rite-runtime` eval → conformance fixture + parser test → `docs/book/` + regenerate the agent bundle.
- **Capability**: implement in `rite-caps` with a `NativeFunctionDescriptor`, register in `HostCapabilities`, add it to the effect table in `rite-sem/src/resolve.rs`, add permission tests, and note browser-safety in `rite-wasm` if native-only.

## Docs

`IMPLEMENTATION.md` is the honest status page — known gaps (WASM host I/O, aliased imports, formatter sugar fidelity, no CST/incremental relex) and the benchmark baseline live there; read it before assuming a feature is complete. User docs are `docs/book/`; `docs/generated/` is `rite doc` output and gitignored. Internal design notes are deliberately not in this tree.
