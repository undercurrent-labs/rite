# Changelog

## [Unreleased]

### Added

- **`@crypto` — hashing, HMAC and the encodings that travel with them.**
  `@crypto.sha256(s)`, `@crypto.sha512(s)`, `@crypto.hmac_sha256(key, message)`,
  `@crypto.constant_time_eq(a, b)`, `@crypto.base64_encode` / `base64_decode`,
  `@crypto.hex_encode` / `hex_decode`, and `@crypto.random_bytes(n)`.

  Everything except `random_bytes` is a pure transform, so it takes no `!` marker
  and needs no `--allow`: `@crypto.sha256("abc")` observes nothing outside the
  program and answers the same on every run. That also makes the capability usable
  in Studio and anywhere else the pure evaluator runs. `random_bytes` reads the
  operating system's entropy pool, so it is marked and rides the existing `random`
  grant — and it deliberately ignores `@random.seed`, so pinning a seed for
  reproducible dice rolls does not pin your session tokens.

  The decoders answer a Result rather than failing the run, because their input is
  normally untrusted, and `base64_decode` is strict RFC 4648 — padded, canonical,
  standard alphabet — rather than guessing at malformed input.

  **No ciphers.** There is no `encrypt`, no AES, no RSA, and nothing that asks the
  caller to choose an IV or a mode; that shape is how ECB and reused nonces get
  shipped, and it belongs behind one opinionated `seal`/`open` in a future `cipher`
  package. Password hashing (argon2, bcrypt) is deferred for a different reason: it
  carries cost parameters and a stored-format contract, which is a design rather
  than a function. See [Hashing and encoding](docs/book/crypto.md).
- **`@udp` — datagram sockets.** `bind`, `local_addr`, `send_to`, `recv_from`, `close`.
  A socket is an opaque handle, the same representation a `@db` connection already has,
  and there is no connection state to manage: bind, exchange datagrams, close. Closing
  twice is fine, because a script closes on the way out and should not have to remember
  whether it already did.

  `recv_from(sock, timeout_ms)` answers `ok(⟨from, data, text⟩)`, or
  `err(⟨kind: "udp.timeout", …⟩)` when nothing arrives — **a timeout is a value, not a
  raise**. Waiting for a datagram that never comes is ordinary, so the script decides
  what it means; `?` on that line would hand it to the caller instead.

  Payloads are a string (sent as UTF-8) or a `bytes` value (sent verbatim) — the type
  `@fs.read_bytes` and `@http` response bodies already use. Received datagrams come back
  as `data` (bytes) plus `text` (lossy UTF-8). Bytes are still opaque in Rite, so a
  program can relay them but cannot yet build a binary packet from source; see the gap
  recorded in `IMPLEMENTATION.md`.

  Permissions are the two `@http` already applies, reached through the same code: the
  **bind address** allows loopback by default and needs `--allow net=<host>` for anything
  else, and the **destination** of every `send_to` is checked per host like an outbound
  `@http.get` — including loopback. Talking to yourself needs `--allow net=127.0.0.1`.

  Native only: the browser runtime has no socket layer and says so, as `@process` does.

- **Strings and numbers can be worked on.** Rite is pitched at tools and pipelines,
  where handling text is most of the job, and it had `lines`, `words` and `join` —
  nothing to split, trim, case, pad or slice with.

  Strings: `split`, `trim` / `trim_start` / `trim_end`, `replace`, `starts_with`,
  `ends_with`, `upper`, `lower`, `pad_start` / `pad_end`, `slice`, `index_of`.
  Numbers: `round`, `floor`, `ceil`, `sqrt`, `parse_int`, `parse_float`.

  Everything is character-indexed, matching `count` — `count("δ")` was already 1, and
  an API counting characters in one place and bytes in another only goes wrong on
  non-ASCII input. Indices may be negative, and out-of-range values clamp rather than
  fail so `slice` is safe on input you did not choose. `index_of` answers `none`
  rather than `-1`, because a sentinel that is also a valid index is how off-by-one
  bugs get written. `parse_int` and `parse_float` answer with a Result, so `?` handles
  bad input like anything else that can fail.

### Changed

- **`parallel` actually runs things together.** It dispatched straight to `map`,
  running every branch in sequence while the name promised otherwise — a comment in
  the source called it a "sequential fallback". Branches now overlap wherever they
  wait: eight 100 ms sleeps finish in about 100 ms rather than 800.

  It answers as if it had not. Results come back in input order however the branches
  finish, output is spliced in input order, and when several branches fail the one
  reported is the first in input order — so the same program prints the same thing
  twice running. Branches share the host, so a `@store` or `@db` write in one is
  visible to the others and to the parent.

  Concurrency, not parallelism: work that never waits gains nothing and should use
  `map`. Each branch needs its own evaluator context, which `map` does not pay for.

## [0.3.1] — 2026-07-30

Effects stop leaking through function boundaries, modules become a real module
system, and seven ordinary names stop silently evaluating to nothing.

### Fixed

- **Seven reserved words bound nothing as parameters.** `item`, `room`, `world`, `test`,
  `ok`, `err` and `some` could be *bound* but not *read*: `◆ f(item) ⟦ ^ item ⟧` returned
  `none`, and `map { |item| item * 2 }` returned a constant regardless of input — wrong
  answers, no diagnostic, `rite check` clean. Of the 31 reserved words these were the only
  silent ones. They now parse as expressions wherever they can be bound, and the
  declaration forms that need them (`◆ item :key ⟦ … ⟧`, `◆ test "…" ⟦ … ⟧`) are unchanged.

- **A module could not use another module.** Only the entry file's imports were brought
  into scope, so a function in one module calling into another reported an undefined name.
  The graph could only be an entry plus self-contained leaves. Modules import modules now,
  and a module's own imports stay private to it.

- **`compose` was never an ASCII spelling of `∘`.** The alias table advertised it, but no
  such keyword exists: `f compose g` evaluated to `f` and returned a wrong number instead
  of failing. The working form is the builtin call `compose(f, g)`, which is what the table
  now records — including in the agent skill bundle, which had been teaching the broken one.

### Changed

- **Every import binds a qualifier.** `use math` now gives `math.square` as well as
  `square`; an alias still keeps the module behind its own name. Two modules exporting the
  same name no longer make either unusable — the clash is reported only if you call the
  bare name, and it names both modules. Qualified calls are checked when you compile:
  `m.squre(9)` used to pass `rite check` and fail at runtime as `undefined name m__squre`,
  leaking the internal mangling at the reader.

- **Effects now travel with the call graph.** `!` marked only the place a host call was
  written, so wrapping one in a function made it disappear: `◆ greet(n) ⟦ ! @console.println(n) ⟧`
  was callable as `greet("x")` with no marker and `rite check` accepted it. The guarantee
  stopped at the first function boundary, which is exactly where code goes as it grows.

  A function that reaches the host now declares it — `◆! greet(name)` (ASCII `def! …`) —
  and callers mark the call. The compiler infers effect-ness from the body and closes it
  over the call graph, so a function calling an effectful function is effectful too,
  through any depth and through recursion. The declaration is the contract a caller sees;
  inference only checks the contract is honest. Declaring `◆!` on a body that happens to
  be pure is allowed, so a function can reserve the right to perform effects later.

  Passing an effectful function to another one (`each(shout)`) marks the call, since
  nothing on that line would otherwise say I/O happens. A lambda written inline already
  shows its own `!`, so it needs no second marker. A closure stored in a binding and
  passed later is not tracked — that needs types Rite does not have.

  Migration is one marker per effectful function and one per call. Two examples in the
  book needed it; nothing in `examples/` or `conformance/` did.

- **`print` and `println` need a marker.** They reach the terminal exactly as
  `@console.print` does, but took no marker, which made the whole discipline optional for
  anyone who used the short name.

- **No Windows binary is published.** CI stopped testing Windows on every change in 0.3.0,
  and shipping a binary nothing exercises is worse than shipping none — every Windows
  failure so far was an unportable *test*, but a real regression would now reach a user
  with nothing in its way. Rite still builds and runs there: use WSL, or
  `cargo install --path crates/rite-cli`. `rite update`, the installer, the release notes
  and the book all say so rather than pointing at an archive that is not there. Restoring
  it is three commented lines in the release workflow.

## [0.3.0] — 2026-07-30

`rite build` becomes a compiler. Also three fixes that are visible from a script, one of
which put wrong bytes on disk.

### Changed

- **`rite build` is a real backend.** It used to base64-encode the IR into the generated
  crate and call `run_ir`, so a compiled binary was the interpreter carrying its program as
  a payload — exactly as fast as `rite run`, after a multi-minute build. Statements and
  function bodies now lower to Rust: control flow becomes Rust control flow, operators
  become direct calls into `rite_runtime::ops`, and a call to a compiled function is a
  direct Rust call.

  Two measurements, release build, best of five, because one would misrepresent it:

  | program | interpreted | compiled | |
  |---------|------------|----------|---|
  | `fib(24)` — calls and arithmetic | 771 ms | 150 ms | **5.1×** |
  | pipelines over `map`/`keep`/`each` | 71 ms | 39 ms | **1.8×** |

  Pipelines were 1.0× — no improvement whatsoever — until closures compiled too. The
  remaining gap is that iteration still runs through `builtin_map` and `call_value` per
  element even when the closure body is compiled; only the body got faster.

  `Match`, `@console` and `@http.listen` still fall back. The generated file names what
  fell back, so this is visible without benchmarking.

  Worth recording how the first number was reached, too: the initial version compiled only
  top-level statements and measured **778 ms against 778 ms** — nothing, because `fib` is a
  *function* and function bodies were still interpreted. Compiling the bodies is the whole
  difference.

  Locals stay in `ctx.env` rather than becoming Rust `let` bindings: a Rite closure
  captures the environment, and promoting them without escape analysis would silently break
  capture.

- **Closures and pipelines compile.** A closure body is now a Rust function reached
  through a new `Value::NativeClosure`, capturing its environment by sharing exactly as an
  interpreted closure does — so `total := total + n` inside a compiled `each` body still
  assigns through to the scope that declared `total`. `:=` lowers too; without it an `each`
  loop driven by a mutable counter fell back and took everything inside it along.

- **`rite build` skips DuckDB when the program never uses `@db`.** It is a `bundled`
  dependency, so including it compiles the whole database from source. Cold builds of a
  one-line script, measured with an empty target directory: **425 s → 265 s** and 12 GB →
  9.4 GB. Still slow — the rest is compiling the Rite runtime itself, which a prebuilt
  support crate would address and this does not.

### Fixed

- **`map` rejected a compiled closure with "map expects function, got function".** It
  tested for `Value::Function` specifically while `type_name` reported both kinds as
  `function`, so the message named the same type twice. Callability is now one predicate,
  `Value::is_callable`, rather than a match arm per site. Caught by checking the compiled
  program's *output*, not its timing — it had "won" the benchmark by failing in 5 ms.
- **An atom written through a capability lost its name.** `@fs.write(p, #ok)` put the bytes
  `#0` on disk — the interner index — and `@fs.append`, `@console` and `@game.say` did the
  same. The builtins were fixed in 0.2.0; the capabilities had their own copies of the
  identical mistake, and writing wrong content to a user's file is the worst place for it.
- **Nothing in CI compiled the generated Rust.** Every test that builds a generated crate
  is `#[ignore]`d because it takes minutes, and the backend's own tests assert on emitted
  *text* — `code.contains("Box::pin")` passes on source that does not compile. A code
  generator therefore landed with its output never once compiled by the suite. The
  generated file is now parsed with `syn` on every run, which catches a stray brace or a
  malformed literal in milliseconds, and the end-to-end builds run in the release workflow
  where minutes are affordable. Verified by planting a dropped brace: the new gate fails,
  the fifteen text-based tests all pass.
- **A compiled binary dropped its result once it had printed.** `! @console.println("hi")`
  followed by `1 + 2` printed `hi` and swallowed the `3`, where `rite run` prints both — a
  standing parity break between the two commands, not a new one.
- **`rite build` failed outright under `RUSTFLAGS=-Dwarnings`.** Skipping DuckDB left
  `rite-caps` with unused imports in that configuration — which is now the default for any
  program without `@db` — and the generated crate imported a name it does not always use.
  Both configurations are warning-clean, and generated crates suppress lints the author of
  a Rite script cannot act on anyway. Found by running the end-to-end build tests before
  tagging rather than after.

## [0.2.0] — 2026-07-30

A correctness, security and honesty pass over the whole tree, from a full review.

**Four behaviour changes**, all in **Changed**: pipeline precedence (`→` binds tighter than
the operators), `@random` no longer returning the same sequence on every run, and two in the
REPL. If you have a script that depended on `@random` being effectively constant, seed it
explicitly with `@random.seed(n)`.

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
- **`rite doc <path>` documents your own scripts.** The path argument was accepted and
  thrown away, so it produced the generic language reference either way; meanwhile
  `parse_doc_comment` was public with no callers and the parser had been attaching doc
  comments to declarations all along. `///` on a declaration and `//!` at the top of a file
  now reach `scripts.md`, the JSON index, the search index and the HTML site, with
  `@param` / `@returns` / `@effects` / `@permission` tags and fenced examples.
  `rite docs build --scripts <path>` does the same from the maintained command family.
- Documentation for **doc comments** — a language feature since the lexer, and undocumented
  until now — plus the `@random` seeding contract, the REPL session model, and two rules the
  book never stated: match arms are newline-separated (a comma is a syntax error), and
  result patterns are juxtaposed (`ok data`, not `ok(data)`).
- **`rite_runtime::ops`** — operator semantics as public free functions, so an ahead-of-time
  backend can reach the same definition of `+` the interpreter uses instead of carrying a
  second copy.

### Changed

- **`@random` is random.** The default generator was seeded with a constant, so every
  `rite run` on every machine drew the identical sequence forever — `@random.int(1, 6)` was
  effectively a constant and a dice roller always rolled the same numbers. It is now seeded
  from the operating system. `@random.seed(n)` still pins a sequence when you want one, and
  now covers `uuid` too: that path called the system generator directly, so a run that
  asked to be reproducible produced a different identifier every time. Studio pins a seed,
  so editing and re-running shows what you changed rather than noise.
- **The REPL redefines names instead of refusing them.** `x ← 1` then `x ← 2` was a
  duplicate-binding error, and redefining a function failed *silently* while the old body
  stayed live. A redefinition now replaces the earlier one in place, keeping its position,
  so anything defined in between sees the new value.
- **The REPL performs an effect once.** An effectful binding was replayed before every
  later input, so `data ← ! @fs.read(f)` re-read the file each time and
  `r ← ! @http.post("/orders", …)` re-submitted the order. The session now stores the
  result rather than the expression. A `↢` declaration is remembered; a later `:=` is not,
  so mutations reset — documented in the book.
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
  `deploy` requires the Rust job, and every test binary runs before a failure is reported
  (`--no-fail-fast`) — without it a platform-specific break surfaced one failure per run.
  Linux and macOS run on push and PR; Windows is opt-in via **Run workflow**, since it
  takes ~36 minutes and its every failure so far was an unportable test rather than a
  broken Rite. The fixes that made it pass are all still in place.

### Fixed

- **Diagnostic columns counted bytes, not characters.** On any line containing a glyph —
  which in idiomatic Rite is most of them — every caret sat several columns right of what it
  pointed at, and the reported `file:line:col` was unusable for jumping. The same program
  written in ASCII reported the correct column, which is why it survived: the tests were
  ASCII. Carets now pad by display width, so a CJK string literal lines up too.
- **An atom reaching a string rendered as a number.** `str(#ok)` gave `"#0"`, and so did
  `"{status}"` and `[#a, #b] → join(", ")` and `panic(#boom)`. `@console.println` was correct
  the whole time, which is what hid it — the same atom printed two ways depending on which
  path it took to the screen.
- **Editor positions disagreed with each other.** `rite-analysis` carried three position
  implementations and two conventions: references used UTF-16 code units (what LSP means),
  symbols used character columns, diagnostics a third. Jumping to a name could land in a
  different place depending on whether you asked for the definition, a reference, or the
  diagnostic pointing at it. One implementation now, and `café` — a legal Rite identifier —
  no longer overshoots its own highlight by a column.
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

### Tests

- 255 → 743, and the gaps were where the bugs were: the byte-column, atom-rendering,
  `@random`, REPL and editor-position faults above were all found by writing tests for a
  thin crate rather than by using the language.
- **Interpreter/compiler parity** — 117 programs across the language surface, each run
  through the interpreter and through the IR path a compiled binary takes, comparing value,
  stdout *and* stderr. No cargo build, so it runs in milliseconds.
- **Three green tests were proving nothing.** Conformance fixtures declaring `allow = []`
  ran with every capability granted, because the loader returned `allow_all()` from both
  branches. Two `@db` sandbox tests asserted the absence of content from `/etc/passwd`, a
  file that does not exist on Windows — so on that platform they passed without the read
  ever happening. All now fail if the property they claim to check breaks.
- CI runs every test binary before reporting (`--no-fail-fast`); without it a
  platform-specific break surfaced one failure per 36-minute run. Windows tests are
  advisory while its remaining gaps are test portability rather than product faults; `fmt`,
  `clippy` and the build still gate there.

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
