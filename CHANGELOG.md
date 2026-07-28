# Changelog

## [0.1.4] — 2026-07-28

### Fixed

- HTTP handlers: `! @console.println` (and other console output) now flushes to the server process after each request (was trapped in a per-request buffer)
- `use @http.log` / `use @http.recover` actually wire middleware (were no-ops)

### Added

- Access log middleware `@http.log` → stderr: `rite: GET /path 200 3ms`
- Glyph **`⊏`** as dual of `use` (imports + HTTP middleware plug-in)

## [0.1.3] — 2026-07-28

### Added — Sugar pack

- **Ranges:** `1..n` exclusive, `1..=n` / `1‥n` inclusive
- **Pipeline stages:** `rest`/`tail`, `take`/`drop`, `init`, `reverse`, `words`, `lines`, `join`, `enumerate`, field projection `→ .name`
- **Control:** ASCII `else`, `unless`/`¿`, `for`/`∀ … ∈`, `loop n`, `while`
- **Assign:** `+=` `-=` `*=` `/=` `%=`
- **Numeric:** `**` / `pow`, `÷` / `idiv`, `abs`, `clamp`, `repeat`, `concat`
- **Logic:** `xor` / `⊻` (plus existing `∧∨¬`)
- **Results:** `✓`/`✗` marks, `is_ok`/`is_err`/`unwrap_or`/`or_else`
- **Print:** `say` / `¶`
- **Compose:** `f ∘ g` / `compose(f, g)`
- Docs: `docs/book/sugar.md`; example: `examples/sugar/demo.rite`
- Tests: `crates/rite-caps/tests/sugar_pack.rs`

### Notes

- List/record `..spread` inside literals is deferred (use `concat` / record `+` merge). Match rest patterns unchanged.

## [0.1.2] — 2026-07-28

### Fixed

- Nested local functions (`◆` / `def` inside a body) bind correctly and close over outer params
- Early `^` / `return` from nested if/match/blocks exits the enclosing function
- Top-level postfix `?` on `err` yields the err value as the script result
- Lexer no longer hangs on unknown multi-byte symbols; glyph ops `∧` `∨` `¬` tokenize
- Prefix if (`? cond ⟦…⟧`) on the next line is not stolen as postfix try on the previous expr

### Added

- Bulletproof edge-case suites (eval, parse, CLI, REPL, WASM) and expanded conformance fixtures
- Docs: nested helpers, ASCII if uses `:`, multi-value HTTP return, match rest vs pipeline

## [0.1.1] — 2026-07-28

### Fixed

- Installer: status logs no longer pollute the release URL; require `bash` (not `sh`/`dash`)
- REPL: wall-clock timeout no longer fires after idle; session prelude keeps bindings/functions
- Studio (`rite studio`): nested Tokio runtime panic on `/api/v1/run`
- Release CI: Windows zip packaging; Mac Intel build on `macos-latest`; rustup target install

### Added

- Cloudflare deploy from GitHub Actions on `main`
- Thorough HTTP e2e tests (ephemeral port, methods, concurrency, permissions)
- Docs: one-liners & REPL guide (`docs/book/one-liners.md`)

## [0.1.0] — 2026-07-28

### Added

- Initial Rite v1 language implementation
- Glyphic and ASCII dual syntax with formatter
- Tree-walking async interpreter
- Ahead-of-time Rust compilation backend
- Capability system: console, fs, json, clock, env, process, random, http, game, store
- Sinatra-style HTTP service DSL
- Event-driven text RPG DSL
- CLI: run, build, check, fmt, repl, test, doc, explain, ast, ir, capabilities
- Documentation generator (Markdown, HTML, JSON)
- Conformance suite and differential interpreter/compiler tests
