# Changelog

## Unreleased

### Fixed

- **Skill package CI**: absolute `OUT` path + Python zip (relative `dist/skill` after `cd stage` broke zip)
- **VSIX CI**: regenerate standalone `package-lock.json` (pnpm-linked lock broke clean `npm install`)
- **Packaging gates**: `cargo test -p rite-cli --test packaging` + `bash scripts/check-packaging.sh` / `package-vsix.sh`

## [0.1.8] — 2026-07-29

### Added

- **`rite skill install|update|status|path`** — install agent skill into Grok/Claude/Cursor (cached under `~/.local/share/rite`, state in `~/.config/rite/config.json`)
- **`rite update` / `self-update`** — check/install CLI from GitHub Releases; report skill freshness vs last pull
- **`rite vscode install|download|info`** — fetch `.vsix` and install via `code`/`cursor`
- **Site `/agents`** — agent-friendly install docs + skill/vsix download endpoints
- **Release assets**: `rite-agent-skill.tar.gz` / `.zip`, `rite.vsix`
- Packaging scripts: `scripts/package-skill.sh`; site build copies skill to `/skill/`

## [0.1.7] — 2026-07-29

### Added

- **Implicit `run`**: `rite script.rite` (and shebang `#!/usr/bin/env rite`) when the first positional arg is not a subcommand
- Docs: shebang / executable scripts section in first-script guide

## [0.1.6] — 2026-07-28

### Added

- **`@csv`** capability (mirror `@json`): `decode` / `encode` / `read` / `write` with headers, delimiter, skip_empty options
- **Custom HTTP middleware**: `use { |req, next| … }` with callable `next(req)`; `req.headers` (lowercase); Bearer auth example in `examples/08-middleware`
- **Modules polish**: relative `use ./path` / `use ../path`, fixed `use mod as alias` → `alias.fn`, `pub use` re-exports
- **`@db` (DuckDB)**: `open` / `close` / `exec` / `query` / `prepare` / `query_prepared` / `exec_prepared` / `begin` / `commit` / `rollback`; permissions `--allow db` and `--allow db=path`
- **Branding**: logo mark, favicon, OG image for site + Studio + README

### Docs

- Book chapters: CSV section, `db.md`, middleware auth, modules relative/alias/re-export

## [0.1.5] — 2026-07-28

### Added — Test suite hardening

- **HTTP observability suite** (`http_observability.rs`): middleware registration, access log on/off, handler console flush, recover → 500, glyph `⊏ @http.log`
- **Test I/O capture** (`begin_test_io_capture` / `take_test_io_capture` / `last_registered_middleware`) so side effects are assertable in-process
- **Sugar dual-dialect suite** (`sugar_dual_dialect.rs`)
- **Example gates** + **docs contract** CLI tests
- Contributor guide: `docs/book/testing.md`

### Fixed

- (from 0.1.4) HTTP console flush + real `@http.log` / recover — regressions now locked

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
