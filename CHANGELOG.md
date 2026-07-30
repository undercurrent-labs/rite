# Changelog

## [0.2.0] — 2026-07-30

A correctness, security and honesty pass over the whole tree, from a full review.
One behaviour change to the grammar (pipeline precedence) — see **Changed**.

### Security

- **Compiled binaries enforce permissions.** `rite build` hardcoded `allow_all()` into the
  generated program, so a binary built with no `--allow` flags had full filesystem and
  process access while the docs promised enforcement. The real `PermissionSet` is now
  baked in and checked.
- **`@db` no longer escapes the sandbox.** `--allow db` granted arbitrary file read *and*
  write through DuckDB's own SQL (`read_csv`, `COPY TO`). External access is off and the
  configuration locked, so a script cannot `SET` it back on.
- **`@http.listen` requires `net` for non-loopback binds.** The old check substring-matched
  the address, so `0.0.0.0:port` bound with no permission at all.
- **`@fs.glob` is scoped.** It returned matches from anywhere regardless of the granted
  read root, leaking paths such as `~/.ssh`.
- **`rite studio` authenticates.** The session token was generated, printed and never
  checked, while `/version` reported `token_required: true`. Tokens are enforced, `Host` is
  validated against loopback (DNS rebinding), and executed scripts get restricted
  permissions unless started with `--allow-all`.
- **`--deny console` works.** Console calls bypass the capability host, so the permission
  check was unreachable dead code and a denied script printed anyway.
- **`rite update` fails closed.** Checksum verification was skipped when the sums file was
  absent, undownloadable, or missing the archive. It also refuses to overwrite a
  `target/debug` build artifact.
- **Effect markers are enforced consistently.** `@db.*`, `@csv.*` and every `@fs` read
  needed no `!`; one canonical effect table now drives `E021`, with a parity test against
  the capability descriptors. A bare capability mention (`n ← @clock.now`) also needs the
  marker — it calls the function.

### Added

- **Outbound HTTP**: `@http.get`, `@http.post`, `@http.request`, gated per host by `net` —
  which previously granted nothing at all. The response has the same shape a handler
  receives.
- **`@process.args`** — a script's own arguments, replacing a `RITE_ARGV` environment
  bridge. Needs no grant; works in compiled binaries.
- **Record spread**: `⟨..base, k: v⟩`, defined as the `+` merge operator spelled
  positionally, so `⟨..a, ..b⟩ = a + b` holds by construction.
- **Streaming output** — `rite run` prints as the script runs instead of buffering to exit.
- **Benchmarks** — `cargo bench -p rite-runtime`, front end measured separately from the
  interpreter.
- **`rite docs serve` / `docs open` / `describe diagnostic`** do real work; they used to
  print success and do nothing. `--trace` is implemented.
- Documentation for string interpolation, escapes and raw strings — previously undocumented
  despite being used throughout the examples.

### Changed

- **`→` binds tighter than the operators.** `xs → count > 2` now means
  `(xs → count) > 2`; it used to parse as `xs → (count > 2)` and fail at runtime with
  "cannot call value of type bool". Every binary operator after a stage was affected.
  The trade: a bare binary expression as pipeline input groups to the right, so
  `a + b → f` is `a + (b → f)` — parenthesise to pipe the sum.
- **Raw strings no longer interpolate.** `r"{x}"` is literal, as raw implies.
- **`rite fmt` preserves comments and layout.** It deleted every comment, including `//!`
  and `///`, and the LSP ran it on save. It also keeps multi-line records, lists and
  pipelines multi-line, keeps one-line blocks inline, and no longer drops route parameter
  lists or rewrites `use @http.log` into an internal symbol. A fail-safe refuses to write
  if output would gain diagnostics.
- **`rite fmt` needs an explicit path** (or `--all`); it used to default to the whole tree.
- The LSP no longer advertises semantic tokens or `execute_command` — declaring the former
  while returning nothing made editors drop their TextMate grammar.
- CI: clippy is a hard gate (it had `continue-on-error` and a command cargo rejected),
  `deploy` requires the Rust job, and the matrix covers macOS and Windows.

### Fixed

- **Any non-ASCII character in a comment or multi-line string panicked** the lexer, and so
  `run`, `check`, `fmt`, the LSP and Studio. `/* résumé */` was enough.
- **Closures were dynamically scoped** when a caller shadowed a captured name: an adder
  built with `10` returned `1005` instead of `15` if the caller happened to bind `n`.
- **A line starting with `(` or `[` was applied to the previous statement.** `a ← 1`
  followed by `[9]` parsed as `a ← 1[9]` and silently bound `a` to `none`.
- Six panics that killed the process are now errors: `i64::MIN / -1`, `idiv`, `pow`,
  `clamp`, `range`, `repeat`.
- `∉` evaluated both operands twice, so side effects ran twice.
- Script output was discarded on every error path.
- HTTP handlers could not see module scope — any top-level binding was `undefined name` at
  request time. Mutable module state now has server lifetime.
- The `!` marker was lost through `?`, so `! @fs.write(p, d)?` was rejected.
- `def Name ⟨…⟩` data declarations did not resolve.
- Doc comments were never harvested: `FunctionDecl.doc` was always `None`, so nothing read
  `///` from real sources. Hover and completion show it now.
- `find_references` matched inside strings and comments; rename replaced substrings
  document-wide, corrupting `max` when renaming `x`.
- The agent bundle could truncate its own `SKILL.md`; its capability manifest was three
  releases stale and advertised the wrong effect flags.
- `rite check` reported `E026` on module examples that `rite run` executed fine.

### Performance

- Nodes that cannot suspend are evaluated without allocating a future: arithmetic -31%,
  pipeline map/keep -24%, record spread -21%, recursive calls -9%.

## [0.1.9] — 2026-07-29

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
