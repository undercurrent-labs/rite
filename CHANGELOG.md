# Changelog

## [Unreleased]

### Fixed

- **Rite Studio did not run locally.** `pnpm site:dev` loaded the engine by
  importing `/wasm/rite_wasm.js`, and Vite's dev server refuses to serve anything
  under `public/` as a module — it answered 500, and Studio reported "WASM not
  loaded" on a machine where the file was right there. A built site was fine, so
  the only broken configuration was the one you develop in. Both Studios now
  fetch the glue and import it through a blob URL: one code path, identical in
  dev and production.

### Changed

- The Rite site's footer no longer links Cant.

### Added

- **Cant versions on its own number, starting at `0.1.0`.** It shipped in Rite's
  `0.7.0` archive wearing Rite's version, which claimed seven minor cycles of
  stability for a v0 language whose operator vocabulary can still change. The
  release is still Rite's tag and `cant` still rides in it — one archive, two
  numbers, both in `version-manifest.json`. `cant version` reports its own
  beside the Rite it lowers to, and `cant::RITE_VERSION` now reads
  `rite_core::VERSION` so the second number is Rite's rather than a relabelling
  of the first. See ADR 0001, Amendment 2.

- **`rite update` installs every binary in the release archive**, not just
  `rite` and `rite-lsp`. The rule is "whatever the archive contains and is
  executable" rather than a list of names, so a release that gains a binary needs
  no edit — and nothing installed can be left frozen at an old version while
  `rite` moves on. This is how `cant` stays current: it has no updater of its
  own, and `cant update` says so and exits 2.

- **Cant Studio** — `cant.rite.foo/studio`. The real engine, compiled to
  WebAssembly: it checks as you type, draws the flow graph, shows the generated
  Rite, and runs the program. Execution goes through the expansion, exactly as
  the command line does, so Studio cannot disagree with a terminal about what a
  program means. Nothing typed into it leaves the browser, and there is no
  server to send it to.

  A capability the browser cannot serve — `@fs`, `@process`, `@db`, `@net` — is
  refused by name *before* the program runs, with the expansion still shown, so
  the answer is "run this elsewhere" rather than a failure inside generated code.

- **`cant-wasm`** — the browser-facing API behind Studio: `check`, `expand`,
  `graph`, `dot`, `explain`, `format`, `convert`, `run`, `version`. The `cant`
  crate gained a `native` feature (on by default) so the half that needs no host
  can be built without Rite's runtime, capabilities and compiler.

- Studio is laid out like Rite's: viewport-height, toolbar across the top,
  editor and panels side by side, each scrolling on its own. It was a centred
  page with a short editor and a long stretch of nothing under it.

- **[One-liners](docs/cant/one-liners.md)** — a field guide of recipes short
  enough to put in a shell, and the three things that surprise people: a list is
  one emission, `[]` wraps whatever is in flight, and `*` is scatter only when it
  is the whole stage.

## [0.7.0] — 2026-08-03

A second language in the same archive. Cant is terminal-typeable and
graph-oriented — a program is a flow of stages, and each one emits zero or more
values — and it runs by generating canonical Rite, so everything Rite already
enforces about effects, capabilities and budgets applies to it unchanged. Rite
itself gains three extracted APIs and two fixes; its grammar, IR and gates are
untouched.

The minor bump is the new executable. Nothing in Rite is breaking.

### Added

- **Cant**, a sibling language, shipped as a second executable in the same
  archives. Terminal-typeable and graph-oriented: every stage emits zero or more
  values, and scatter, collect, ward, fork and a bounded orbit change how many
  are in flight.

  ```bash
  cant -e '[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []'
  # [2, 4, 6]
  ```

  It is **not a Rite dialect.** It has its own lexer, parser and flow graph, and
  it executes by generating canonical ASCII Rite and passing that through Rite's
  ordinary front end — so it inherits Rite's values, effect discipline,
  capabilities, budgets, interpreter and native compiler without reimplementing
  any of them. `cant expand` prints exactly what runs, and a differential harness
  checks that `cant run`, `rite run <cant expand>` and the compiled binary agree
  on value, output and exit status.

  Rite's grammar, `Dialect` enum, alias table, IR and capability namespace are
  unchanged; no Rite source file mentions Cant at all. See
  `docs/adr/0001-cant-sibling-frontend.md` and `docs/cant/`.

  v0 is experimental: the operator vocabulary and the graph JSON schema
  (`docs/cant/graph-schema.md`, version 0) can still change.

- **`rite::options::RuntimeOptions`** — the shared meaning of `--allow`,
  `--deny`, `--timeout` and the four budget knobs, so a second executable cannot
  disagree with `rite run` about what `--allow fs:read=./data` means.
- **`rite_core::render_snippet`** — the caret-underlined source excerpt from
  `Diagnostic::render`, without needing a `rite_core::ErrorCode`.
- **`rite_render::svg_to_png`** — rasterise arbitrary SVG, not just highlighted
  Rite source.

### Fixed

- **A bad `--deny` was discarded silently.** A typo in a permission *revocation*
  left the permission in place — the failure mode where you believe you locked
  something down and did not. It is now an error, as a bad `--allow` always was.
- **LSP diagnostics linked to a domain this project does not own.** Every
  diagnostic offered a help link to `rite.dev`; it now points at the canonical
  host, and `crates/rite-cli/tests/site_domain_sync.rs` fails if any tracked file
  names a host `site.toml` does not declare.

### Changed

- The canonical host is **`rite.foo`**. `rite.undrc.dev` redirects and is not
  being retired: released binaries have it compiled into `SITE_BASE`, so
  `rite update` on an installed CLI still asks for it.
- **The sites deploy on the tag, not on every push to `main`.** The install
  one-liner names a version whose archives only exist once a release is
  published, so a site deployed from `main` spent every gap between merge and tag
  offering a download nobody could get. Both sites are still built by every CI
  run; only the publish moved.

## [0.6.2] — 2026-08-03

An MCP release. A Rite script could expose an HTTP service in a dozen lines but had no
way to be a Model Context Protocol server: the only route was to hand-roll JSON-RPC
framing over `@console.read_line`, which nobody was going to do. The shape below is the
`@http.listen` shape, because being an MCP server should cost what serving HTTP costs.

### Added

- **`@mcp.serve`** starts an MCP server and blocks until shutdown, over stdio (the
  default) or Streamable HTTP. The body is a declaration table — `tool`, `resource` and
  `prompt`, each with an ordinary Rite body:

  ```rite
  ! @mcp.serve "calculator" ⟦
    tool "add" "Add two numbers" |a: int, b: int| ⟦ ^ a + b ⟧
  ⟧
  ```

  Effectful, and unlike `@http.listen` it is held to that: the `!` is required. Serving
  over stdio needs no grant, being the process's own streams; an HTTP bind goes through
  the same policy as `@http.listen`, so loopback is free and anything else needs
  `--allow net=<host>`.

- **A tool's JSON Schema is derived from the types already declared on it.** `|a: int,
  b: [string]|` publishes an object schema requiring an integer and an array of strings;
  an unannotated parameter publishes the empty schema and stays required. There is no
  second description to keep in step, and argument validation is the same contract check
  a typed Rite call gets — so a client passing the wrong type receives the tool's own
  error, in band, with `isError: true` rather than as a dead connection.

- **`@mcp.tool_schema(f)`** answers the schema a function would be published with. Pure,
  so you can see what a tool advertises without starting anything.

- **`@mcp.progress(fraction, message)`** sends a `notifications/progress` on the stream
  of the call being served. Only meaningful inside a tool body; elsewhere it fails
  rather than reporting progress nobody could receive.

- **`use @mcp.log`** writes one structured JSON line per request to stderr. Stderr and
  not the protocol's own logging notifications, which the specification has deprecated —
  logging to stderr is its own suggested migration, and under stdio it is the only thing
  that works, because stdout is the wire.

  For that reason, everything a tool body prints goes to stderr while an stdio server is
  running. You do not have to arrange it; `! @console.println` inside a tool simply does
  not reach the protocol stream.

- **`docs/book/mcp.md`** and `examples/12-mcp/`.

The 2026-07-28 revision is implemented natively — stateless, with `server/discover` as
the one mandatory call and cache hints on the list results — and clients still speaking
`2025-06-18` are answered by a compatibility layer that engages when they send
`initialize`. Not implemented, deliberately: `subscriptions/listen` (a Rite server's
tables cannot change while it runs, so the capability is not advertised rather than
advertised and never fired), `notifications/message`, and Multi Round-Trip Requests.

### Fixed — `rite fmt` no longer discards type annotations

Formatting a file dropped declared types in two places, which for an `@mcp` declaration
would have silently changed the schema a server publishes:

- A route's parameter annotation was never printed at all, so `|req: any|` came back as
  `|req|`.
- `result<int>` printed as bare `result` and `⟨a: int, b: string⟩` as bare `record`,
  neither of which reparses to what was written.

Both now round-trip, in either dialect.

## [0.6.1] — 2026-08-01

An HTTP release. A Rite server could not return HTML: the media type was inferred
from the runtime type of the response body, so a string was always `text/plain`
and a browser rendered the markup as source text. A `headers` field on the
response record was accepted and silently dropped. There was also no way to serve
a file — the router required a pattern and a path to have the same number of
segments, so `/assets/css/app.css` could not be routed at all.

Together the changes below serve a static site or a built single-page app.

### Added

- **Response headers.** Any response may carry a `headers` record, and an explicit
  `content-type` replaces the one inferred from the body. `@http.response` takes
  them as a third argument — `@http.response(302, none, ⟨location: "/next"⟩)`. A
  two-argument call builds the same two-field record it always did.

  A header whose value is a **list** is sent once per element. That is the only
  way to set more than one cookie, since a record holds one value per key.

  Header names hold hyphens, so they need quoting as record keys —
  `⟨"content-type": "text/html; charset=utf-8"⟩`. Bare `content-type` lexes as a
  subtraction, and the failure is a server that does not start.

- **Catch-all routes.** A trailing `*name` captures the rest of the path,
  including slashes and including nothing: `/files/*rest` binds `req.path.rest`
  to `"deep/nested/a.txt"` for `/files/deep/nested/a.txt`, and to `""` for
  `/files`. Specific routes are matched **first**, whatever the declaration
  order, so a site-wide `GET "/*path"` does not shadow the API routes below it.

- **`@http.file(root, subpath)`** reads a file and builds the response for it,
  with a content type from the extension. A subpath that escapes `root` is
  refused — checked lexically and again after canonicalization, so a symlink
  pointing out of the tree is caught as well. The `fs:read` grant still applies
  on top of that. A directory resolves to its `index.html`, so `/` needs no
  special case. Effectful; returns a result, so a missing file is a value you
  decide about.

- **`req.form`** parses an `application/x-www-form-urlencoded` body into a record,
  as a result alongside `req.json`. The content type decides, not whether the
  bytes happen to parse, so a JSON body answers `err` and a handler can tell the
  two apart.

### Changed

- **A path that exists with a method it does not have answers `405`**, with an
  `Allow` header listing the methods that would have worked. It previously
  answered `404`, which describes the resource as absent when it is right there.
  If you assert on status codes, `POST` to a `GET`-only path is the case to check.

  Note the interaction with a catch-all: once `GET "/*path"` is declared, every
  path matches it, so other methods answer `405` rather than `404`.

## [0.6.0] — 2026-08-01

A strictness release, from two independent reviews of 0.4.1. Both named the same
four areas: pipelines, type annotations, fail-soft builtins, and effect tracking.
Working through them found nine further bugs, three of them cases where one part of
the language disagreed with another. A pipeline and a plain call gave different
answers for the same name. `rite fmt --ascii` changed what a program computed.
Writing the effect marker the compiler asked for turned the check off.

**Read this before upgrading.** Seven changes can break working code. Every one of
them is a place the old behaviour produced an answer rather than a complaint:

- **`→` is looser than the operators.** `a + b → str` was `a + (b → str)`; it is now
  `(a + b) → str`. The other side costs you parentheses: `xs → count > 2` is a parse
  error, and `(xs → count) > 2` is the fix. `rite check` names every site.
- **`?` requires a result.** `42?` was `42`. A function that can fail must now answer
  `ok(…)` on success too — see the entry below, it is the subtlest change here.
- **Incomparable values raise.** `"a" < 1` was `false`, and `"a" <= 1` and `"a" >= 1`
  were both **true**. `sort` on a mixed list handed the list back unsorted.
- **The higher-order builtins check their arguments.** `keep(42, f)` was `[]`,
  `all(42)` was `true`.
- **A wrong-typed count raises.** `repeat(2, "ab")` was `[]`, `range("a", "b")` was
  an empty range.
- **Effect tracking follows bindings**, so `g ← shout` then `each(xs, g)` needs the
  marker — and a marker over an expression that calls nothing is now an error, which
  catches `println!("x")`.
- **Type annotations are enforced.** They were parsed and dropped, while the
  generated reference said they were checked at runtime.

### Fixed — the grammar file describes the parser that exists

`grammar/rite.ebnf` called itself normative and was wrong about the thing both
reviews asked about: it placed `→` at the loosest precedence, which is an
arrangement the parser had already abandoned. It also omitted six operators and
described three productions that did not parse at all. `rite docs agent` copies it
verbatim into the agent bundle, so the drift was being published to anything reading
the machine-readable grammar.

It now matches, and says out loud that the parser is the definition.

### Fixed — a pipeline reaches the functions you wrote

`3 → dbl` for a `◆ dbl` you defined yourself failed at runtime with **unknown
builtin `dbl`**, naming a category the function was never in. Only the builtin
table was consulted for a bare name in stage position, so a pipeline — the most
visible thing in the language — composed with the standard library and nothing
else. `3 → dbl()` worked, which is how it went unnoticed: every bundled example
and every snippet in the book pipes into builtins.

The quieter half of the same bug was worse. A name that *is* a builtin resolved to
the builtin even where a definition shadowed it, so one program answered two
different things about the same name:

```
◆ count(xs) ⟦ ^ 99 ⟧
count([1, 2, 3])      // 99
[1, 2, 3] → count     // was 3
```

A bare stage now resolves the way a call already did — local binding, then
function, then builtin — so both lines answer 99, a closure in a binding works as
a stage, and an unknown stage name is a resolve error naming it rather than a
runtime complaint about builtins. **If a script defines a function whose name
matches a builtin and pipes into it, the pipeline now calls the definition.** That
is the answer the same name gave in call position all along.

### Fixed — `rite fmt --ascii` changed the answer

`x ← 7 ÷ 2` is `3`. Formatted to ASCII it became `x <- 7 idiv 2`, which is **`7`** —
it parses as two statements, so the division is gone. `f ∘ g` became `f compose g`,
which evaluates to `f`.

Neither `idiv` nor `compose` lexes as an operator, and neither can: both names are
taken by the builtins they lower to, so making them keywords would collide with
`idiv(7, 2)` and `compose(f, g)`. `÷` and `∘` are **glyph-only**, and the formatter
now says so by printing the call form in ASCII. That is the only rendering that
means the same thing and round-trips.

There is a test that runs both spellings and compares the *values* now, rather than
asserting on the text — text is what let this through.

### Fixed — `rite fmt` keeps the sugar you wrote

`1..=5` came back as `range_incl(1, 5)`, `f ∘ g` as `compose(f, g)`, `2 ** 8` as
`pow(2, 8)`, and `keep ⟦ |n| … ⟧` as `keep(⟦ |n| … ⟧)` — in the **glyph** dialect
too. Formatting `examples/02-pipelines/main.rite`, the example the pipelines chapter
is built on, rewrote it into a shape the book never uses; `examples/sugar/demo.rite`,
whose entire purpose is to show the sugar, lost most of it.

The parser was building the lowered call directly, so no `..` or `∘` ever reached
the AST and the formatter's own arms for them were unreachable. They build their
`BinOp` variants now and `desugar` lowers them exactly as before, so nothing
downstream sees a difference. A single trailing block argument is recorded on the
call, so `keep ⟦ … ⟧` prints back as itself.

The statement sugars — `say`, `unless`, `for … in`, `while` — are still expanded by
the parser and still print expanded. That, and not the operators, is what keeps
`rite fmt --check` from being a CI gate; `IMPLEMENTATION.md` says so.

### Fixed — the collection and string ceilings are enforced

`max_collection_size` and `max_string_size` were declared on `ExecutionBudget`,
given defaults of 1,000,000 and 10,000,000, copied through `child()` — and read
**nowhere in the workspace**. An embedder setting them got nothing.

That mattered more than a missing knob, because the step budget cannot see inside a
builtin: `range(0, 8000000)` is a handful of IR nodes and eight million elements, so
it completed under a **60-step** budget. Pushed further it aborted the process on the
allocation rather than raising, taking an embedder's host down with it.

They are checked before the allocation wherever the size is knowable up front —
`range`, `range_incl`, `repeat` and `concat` — and the failure is an ordinary budget
error, exit code 8. `repeat`'s own unrelated `1 << 26` constant is gone in favour of
the configured ceiling.

Still unbounded, and now written down in `IMPLEMENTATION.md` rather than implied:
`@fs` whole-file reads and `read_chunk`'s caller-supplied length, `@http` response
bodies, `@process.run` output capture, `@db.query` result sets, and `parallel`'s
eager one-context-per-element fork.

### Fixed — `@fs.write` with no content no longer empties the file

```
! @fs.write("notes.txt")     // wrote an empty file
```

The content argument defaulted to `""` when it was missing, and `std::fs::write`
truncates, so leaving it off — or misspelling the variable holding it — destroyed
whatever was there and answered `ok`. It is required now. `@fs.append` had the same
default, where it merely did nothing.

Every capability had grown its own argument handling, and the `unwrap_or` shape
behind this one was not unique to `@fs`: `@random.int("a", "b")` answered `0`,
`@clock.sleep("soon")` slept for 0 ms, `@csv.encode` of a non-list wrote an empty
CSV, and `@json.encode()` with nothing to encode wrote `"null"`. They share one argument layer
now, and its messages name the call and the position. `@fs`'s old helper said
"expected path string", leaving you to work out which of the three `@fs` calls on
the line had complained.

### Changed — a failed `match` says what did not match

```
match failure: no arm matched record value `⟨kind: 7⟩`
```

It used to say only `match failure: no arm matched`. The value was in scope on the
line above and simply not used.

### Changed — `?` requires a result, and the last fail-soft builtins raise

`42?` was `42`. The operator that says "this can fail" could be written over
something that cannot, and the case that costs is a call which *used* to answer a
result and stopped: the `?` goes on doing nothing, in silence. It now raises.

`ok(none)` is a result and still unwraps to `none` — that is how `@fs.read_line`
reports the end of a file — and `and_then` is unchanged as the combinator that
accepts a bare value.

**This has a consequence.** `?` returns `err` from the enclosing
function, so a function containing one can answer a failure — and must therefore
answer `ok(…)` on the way out too, or a caller cannot tell the two apart:

```
◆! describe(path) ⟦
  m ← ! @fs.metadata(path)?
  ^ ok(⟨path: path, len: m.len⟩)   // not the bare record
⟧
```

Two places in this repo were writing a `?` over something that never answered a
result — a tutorial and a conformance fixture. Both were doing nothing, and both
now say so.

**The higher-order family validates its arguments.** `map` was the only one that
did. `keep` and `group` answered an empty list for a non-list, `each`, `reduce` and
`find` answered `none`, and `all(42)` was `true` — every one of them a value a
correct call also produces. They are list-only, deliberately: `take("abcde", 2)`
has an obvious answer of the same kind, and mapping a function over a string does
not. The function argument is checked before the loop rather than at the first
element, so `keep([1, 2], 7)` names the mistake instead of failing inside it.

**And the last of the coercing arguments.** `count(42)` was `0`, `contains(42, 1)`
was `false`, `repeat(2, "ab")` was `[]`, `take("abcde", "2")` was `""`, and
`range("a", "b")` was an empty range — each because a wrong type became a default.
Absent still means the default, since several of these have a real one; present and
wrong now says so. `1 ∈ 42` raises for the same reason `contains` does.

### Changed — ordering is total or it is an error, and `sort` takes the comparator it documents

Comparison answered `Equal` for every pair it did not understand, so the
relational operators asserted things the equality operator denied:

```
"a" < 1      // was false
"a" <= 1     // was true
"a" >= 1     // was true      …about values that are not equal
```

`sort` inherited it, and that was the expensive part: `sort([3, "b", 1, "a", 2])`
handed back **the list unchanged** — not sorted, and not an error. The comparator
was not transitive either, so what order it did produce was unspecified.

Ordering is now defined for numbers, strings, `bool` (`false` first), bytes, and
lists (lexicographically, element by element then by length). Everything else
raises: two different kinds, two atoms, two records, and `NaN`. Atoms are symbols
and a record's fields are in insertion order, so ordering either would report how
the value was built rather than what it means. **Equality is untouched** — `"a" = 1`
is still `false`, not an error.

**`sort(seq, comparator)` now calls the comparator.** Two tutorials document it,
one of them explaining the sign convention in full, and the second argument was
dropped on the floor: `sort(files, ⟦ |a, b| b.len - a.len ⟧)` ran the default
comparator, which called every pair of records equal, so the list came back in its
original order looking sorted. The tutorial's own assertion is what caught it,
after the stricter ordering turned a wrong answer into an error.

Negative if the first argument comes first, positive if the second does, zero if
neither — and a comparator answering something other than a number is told so. This
is also what makes the stricter default affordable: a pair the language will not
order for you is a pair you can order yourself.

### Added — type annotations are checked, as the reference always said they were

`◆ f(x: int) → int` has been parsed, printed back by the formatter, and then
dropped. The generated reference has carried a section headed "Runtime type
contracts" — *"Optional annotations like `value: int` are checked at runtime on
function entry/exit"* — the whole time, and nothing behind it was true:

```
◆ typed(x: int) → int ⟦ ^ "not an int" ⟧
typed(true)                                // printed: not an int
```

Both are now errors, naming the function, the parameter and what arrived:

```
typed: parameter `x` expects int, got bool
typed: declared to return int, but returned string
```

The types are the value kinds — `int`, `float`, `number` (either), `string`,
`bool`, `atom`, `list`, `record`, `bytes`, `function`, `none` — plus `any` and
three composites: `[T]`, `result<T>`, and `⟨field: T, …⟩`. Checking is structural,
so an empty list satisfies `[int]` and a record may carry fields the annotation
does not name. A container reports where it stopped matching rather than restating
what was right:

```
f: parameter `xs` expects [int], but [1] is string rather than int
f: parameter `r` expects ⟨n: int⟩, but has no field `n`
```

`result<T>` **parses** now. The branch that should have handled it was an empty
`if` that matched the shape and did nothing, so `◆ f(x: result<int>)` was a parse
error with two more errors cascading after it.

A contract travels with the function *value*, not with the name it was declared
under, so it still applies through `f ← typed` and `each(xs, typed)`. Compiled
binaries enforce the same contracts: `rite build` emits the check around the same
`ops` functions the interpreter calls, so the two paths cannot drift.

Nothing that was unannotated changes, and nothing in the shipped corpus used an
annotation — so this breaks only programs that were already claiming something
untrue about themselves.

### Changed — `?` on a pipeline stage is rejected, and one on a pipeline input parses

`xs → f(a)?` bound the `?` to the *stage*. The interpreter read that as "call
`f(a)` without the piped value, unwrap the answer, then call the answer with
`xs`"; the compiler backend refused to lower it at all, so the two execution paths
disagreed about a program that compiled. Nothing in the book, the examples or the
fixtures used it. It is now `E016`, pointing at the `?` and naming the two places
it can go — on the pipeline's result, or on its input.

The second half was a parse bug of the same family as 0.5.0's `?`-before-`while`:

```
rows ← (! @json.read(path))? → keep ⟦ |r| r.active ⟧
```

`?` and prefix `?` (if) are one token, so the parser looks ahead to tell them
apart. The scan ran past `→ keep` and found the *stage's* trailing block, concluded
the `?` opened a conditional, and failed with "unexpected token →" — pointing at
the pipeline rather than at the `?`. No condition can begin with `→`, which makes
the fix unambiguous.

Also documented rather than left to be discovered: a `Result` travels through a
stage as an ordinary value and **does not** short-circuit — `and_then` is the
opt-in — and pipelines are eager, with every stage materialising its result.

### Changed — `→` is looser than the operators, and its result needs parentheses

**This changes how existing pipelines parse.** `a + b → str` was `a + (b → str)`,
which is not what it looks like and not what anyone writing it means. It is now
`(a + b) → str`.

The cost is on the other side, and it is not optional: an infix operator cannot be
looser than `+` on its left and tighter than `+` on its right. Reaching the input
side costs the result side, so an operator directly after a pipeline is now a parse
error — `E015` — naming the form that works:

```
xs → count > 2        // error[E015]: a pipeline's result cannot be an operand of `>`
(xs → count) > 2      // this
```

This arrangement has now been all three ways round, and the other two each answer
one case by quietly getting the other wrong:

| | `a + b → str` | `xs → count > 2` |
|---|---|---|
| Loose, stages as full expressions | `(a + b) → str` | `xs → (count > 2)` — died at runtime |
| Tight, stages at postfix | `a + (b → str)` — silently | `(xs → count) > 2` |
| **Loose, stages at postfix** | `(a + b) → str` | **E015** |

Stages are unchanged — a name, a call, or a trailing-block call, never a bare
operator expression. `|>` in F#, Elixir and Elm makes the same trade; there the
rejected case is a type error instead of a parse error.

**Migration** is mechanical: parenthesise any pipeline whose result feeds an
operator. Across the 101 `.rite` files in this repo it was one line in
`examples/06-cli-tool`, plus the fixtures and the chapter that taught the old rule.
`rite check` names every site.

### Changed — a marker over nothing is an error

`!` was checked in one direction only: leaving it out where an effect happens was
an error, and putting it where nothing happens was silence. So `x ← ! 42` passed,
and the marker could not be read as "something happens here" — the only reason to
write it.

The shape that made this expensive is the one anyone arriving from Rust writes
first:

```
println!("one")
```

`rite check` said `ok` and the program printed nothing. Statements split on
expression boundaries, so that line is **two** of them: a discarded reference to
`println`, then `!` applied to `"one"`. Both halves were individually legal.

A marker over an operand that calls nothing — no function, no capability, no
pipeline stage — is now `E021`, with the help text naming the `println!` case.
The marker goes before the call: `! println("one")`.

The test is whether anything is **called**, not whether the call is effectful.
`! each(xs, f)` for a parameter `f` stays legal, because whether `f` performs an
effect is exactly what Rite cannot always know — and rejecting the responsible
form would be the worse error. Nothing in the shipped corpus carried a stray
marker, so no example or chapter changed.

### Changed — naming an effectful function no longer hides it

Effect-ness travelled along the call graph by name, so giving a function a
different name took it off the graph:

```
◆! shout(n) ⟦ ! @console.println(str(n)) ⟧
◆ run(xs) ⟦
  g ← shout        // a rename
  each(xs, g)      // …and `each(xs, shout)` — the checked form — is gone
⟧
```

`rite check` said `ok` on that, and on `f ← shout` followed by `f("hi")`, which
printed from a plain `◆` with no marker anywhere in the file. One line of
indirection was the whole of it, and the second form is what anyone factoring a
call out of a loop writes.

A binding now carries the property of what it holds: a name that resolves to a
`◆!` function, or a lambda whose own body performs an effect. Calling through it,
or handing it to a higher-order function, is checked exactly as the original is.
**A function doing either must be declared `◆!` / `def!`, and its callers must
mark the call.** Nothing in the shipped corpus changed.

It follows a *name*. A function read from a record field, received as a parameter,
or returned by a call still passes unremarked: there is nothing to attach the
property to. The effects chapter lists what is and is not seen, and why closing the
rest needs a type system Rite does not have.

### Fixed — writing the marker no longer switches off the check

Passing an effectful function to a higher-order one requires `!` on the call, and
supplying it disabled the inference that the marker exists to drive:

```
◆! shout(n) ⟦ ! @console.println(str(n)) ⟧
◆ run(xs) ⟦ ! each(xs, shout) ⟧   // the ! the resolver asks for
◆ outer() ⟦ run([7]) ⟧            // plain ◆. no marker. printed 7.
```

`rite check` said `ok`. The call that records the effect sat inside the branch
that reports the missing marker, so taking the fix removed the record: `run` was
never inferred effectful, never required to be `◆!`, and its own callers were
never asked for anything. Complying with the discipline was what turned it off —
the one path a reader following the diagnostics would take.

It is recorded whether or not the marker is present now, matching the two checks
either side of it that always did. **A function that passes an effectful function
to a higher-order one must be declared `◆!` / `def!`, and its callers must mark
the call.** Scripts relying on the hole now get an error where they used to pass.

## [0.5.0] — 2026-08-01

The release where a lot of things that looked like they worked turned out not to,
and now do. Most were found by *running* the feature being documented rather than
by reading code or watching tests pass: a silently wrong builtin, a formatter that
turned a function into its own body, an embedded script whose output went nowhere,
and a rendered image with no text in it. Each had a green test suite over it at
the time.

**Read this before upgrading.** Three changes can break working code, all of them
cases where the old behaviour was a wrong answer delivered quietly:

- **Builtins that answered the wrong type now raise.** `sum(["1", "2"])` was `0`,
  the same `0` a correct empty list gives. `keys("abc")` was `[]`. `lines(xs)` on
  a list was `[]`, which reads exactly like an empty file. Each of these now says
  what is wrong at the call. If a script depended on one, it fails now — loudly,
  which is the point, but it fails.
- **`join` on a string joins its characters.** `join("abc", "-")` was `"abc"` and
  is now `"a-b-c"`.
- **Exit codes 3 and 4 mean what the published table always said.** A resolve
  failure exits **4** from `rite run` where it exited 3; a parse failure exits
  **3** from `rite check` where it exited 4. Anything matching on those numbers
  needs a look.

And one that is unlikely to bite but worth knowing: `{ || 42 }` is a function of
no arguments now, where it used to evaluate to `42`.

### Added — `and_then` calls its function

It lived in the pure builtin table, which cannot invoke a closure, so it ignored
the function it was given and answered its input: `and_then(ok(2), { |n| ok(n * 10) })`
gave `ok(2)`. A chain built on it looked like it worked and did nothing, which is
the worst way for a combinator to fail. `ok` calls the function, `err`
short-circuits unchanged, and a plain value passes through so it composes with
functions that do not wrap.

### Added — date arithmetic

`@clock.add(t, duration)` and `@clock.diff(a, b)`. `add` reuses the duration
vocabulary `@clock.duration` already speaks, so `"7d"` means the same everywhere,
and a negative duration expresses "thirty days ago" without a second function.
Both answer results — a string that is not a timestamp and a unit that does not
exist are both things a caller gets wrong — and both are unmarked, since shifting
a timestamp you already hold observes nothing outside the program. An
out-of-range shift is an `err`, not a panic.

### Added — `rite render`, pictures of code

`rite render <file>` draws highlighted Rite as SVG or PNG, so a README, a slide or
a docs page can show code that looks the way Studio shows it.

```
rite render greet.rite --output greet.svg
rite render greet.rite --format png --frame window --output greet.png
cat greet.rite | rite render - --frame box > greet.svg
```

`--format svg` is small and uses the viewer's own monospace font; `svg-font`
embeds the face and is self-contained at about a hundred times the size; `png`
rasterises. `--frame` is `text`, `box` or `window`. Layout is computed per column
rather than measured, which is what lets the small format still line up in a
viewer whose monospace font is not the one you have.

The highlighting is the language's own lexer and one shared palette — a new
keyword or host function cannot leave the pictures behind, because there is no
second list to update. Source that does not compile still renders, deliberately,
so a page explaining a mistake can show it.

`grammar/palette.json` is now the one colour table, with gates holding the site's
stylesheet to it: same colours, no colours of its own, an entry for every token
kind and no entries for kinds nothing emits. A fourth gate turns the stylesheet's
own comment into a checked property — every colour clears 4.5:1 against the panel
background, the worst being the comment grey at 6.32:1.

Studio gains **Save PNG**, rendering through the same crate over WASM and taking
the SVG to a canvas — so the browser needs no rasteriser, and there is no second
layout implementation in TypeScript to drift from `rite render`.

Two bugs found by looking at the output rather than at the tests, both of which
every assertion of the day was happy with:

The first PNG had the frame, the background and the window dots, and no text at
all. `usvg` resolves fonts through its own database and ignores an `@font-face`
data URL, so every glyph drew as nothing. There is a test that counts pixels now.

And whitespace was *drawn* rather than counted, with `xml:space="preserve"` asked
to hold it. Chrome collapses runs of spaces inside `<text>` whatever that
attribute says, so `^ n * n` rendered as `^n  *n` — glyphs in the wrong columns,
which is the one thing a picture of code must not do. Every visible segment is
placed at its own computed column now, and no drawn run contains a space. The
golden file could not have caught it: it was generated from the same mistake.

### Added — a host can call a function inside a guest script

`RiteEngine::load(name, src)` runs a script and keeps it, so the functions it
defined stay callable: `script.call("price", vec![order])`. Until now the engine
ran a script and handed back its value, and the functions went when the context
did — so passing data in meant writing it somewhere the guest could read and
running the whole file again per item, a file and a path grant standing in for an
argument. The embedding tutorial had to teach that workaround; it now teaches the
call.

The top level runs once, at `load` — that is what defines the functions, and for a
script with a `main` it runs `main` too. Holding the script holds that run, so a
mutable top-level binding keeps its value between calls and anything the script
opened stays open until it is dropped. Permissions and the budget apply to every
call, not only to the load. A missing function or the wrong number of arguments is
an error naming both, rather than `none` bound to a missing parameter that fails
somewhere else later.

**Atoms come back with their names.** An atom is an index into an interner and
`Display` has none to ask, so `format!("{value}")` renders `#0`. Every run of one
engine now shares an interner, and `engine.display(&value)` /
`script.display(&value)` resolve it — which also makes the same atom from two runs
the same value.

### Changed — exit codes 3 and 4 mean what the table always said

Rite's published contract reads "3 parse, 4 resolve". The binary did not do that:
`rite run` answered **3** for every rejected source and `rite check` answered
**4**, so the number said which command had complained rather than what was wrong
with the file. Diagnostic codes are grouped by phase (`E00x` lex, `E01x` parse,
`E02x` resolve), so the answer was there and nothing read it.

`rite run`, `rite check`, `rite semantic-ir` and `rite emit-rust` now all answer 3
for a source that would not parse and 4 for one that parsed and would not resolve.
A wrapper can act on the difference: 3 means the text is not Rite, 4 means it is
Rite referring to something that is not there.

**This changes an observable status.** A script with an undefined name or a
missing `!` marker exits 4 from `rite run` where it exited 3 before, and a file
that will not parse exits 3 from `rite check` where it exited 4. Anything matching
on the old numbers needs a look.

The differential runner caught its own inconsistency here: the IR path had 3
hardcoded, so the two execution paths disagreed about the same file the moment
resolve failures started answering 4.

### Changed — `@tcp` connections close with the run

`@tcp` kept its connections in a process-global map, so a socket outlived the run
that opened it: `rite run` only cleaned up because the process exited, and inside
`RiteEngine` — where the host keeps going — a guest that never called `@tcp.close`
leaked the connection for the lifetime of the host, with no way for the next run
to reach it. They live on the run's context now, as `@fs` handles do.

The original reasoning for the global was sound as far as it went: a `@tcp.listen`
handler runs its block in a fresh context, so a table on the *capability* would be
invisible to the block the handle is passed to. It does not follow that the table
must be global — the handler's own context is reachable where the connection is
registered, and is the right owner. A connection now closes when its handler
returns, and a client connection when the run ends.

### Added — `@fs` reads and writes without holding the whole file

Every `@fs` read was whole-file: `read` and `lines` are `read_to_string`,
`read_bytes` is `read`. Peak memory was the size of the file and nothing could be
processed as it arrived — `@fs.lines` was line-by-line as an *interface* only,
reading everything and then splitting, so at its peak it cost more than `read`.

`@fs.open(path, mode)` answers a handle: `read_line`, `read_chunk`, `write_chunk`,
`seek`, `flush`, `close`. Modes are `#read`, `#write` (creates, truncates) and
`#append` (creates, keeps). `read_line` reports the end of the file with `none`,
because an empty line is `""` and the two must not collide; `read_chunk` reports it
with an empty result, and a short read is not the end.

The convention is `@tcp`'s — open, opaque handle, close, and closing twice is
fine — with one deliberate difference. A `@tcp` connection lives in a
process-global; these live on the run's context, so **anything left open closes
when the run ends**. Under `rite run` that is invisible, since the process exits.
Inside an embedder it is the difference between a guest leaking a descriptor for
the lifetime of the host and not. One run may hold 1024 open at once; the next
`open` is an error naming `@fs.close`, rather than the operating system's
complaint arriving later from an unrelated call.

**The mode decides the permission, at `open`.** `#read` needs `fs:read` for that
path, `#write` and `#append` need `fs:write`, and nothing afterwards carries a path
to check — so a refused open is refused before the file is created or truncated.

`@tcp`'s sockets had the same leak, fixed just after — see above.

### Fixed — `?` on the line before a loop

`line ↢ ! @fs.read_line(h)?` followed by `while line != none ⟦ … ⟧` did not parse.
`?` and prefix `?` (if) are the same token, so the parser looks ahead to tell a
try-unwrap from the start of a conditional; the lookahead scanned past `while` and
found the *loop's own* `⟦`, concluded the `?` opened a conditional, and left it to
begin the next statement — which then parsed as `? while …` and failed with
"unexpected token While", pointing at the loop rather than at the line above it.
`loop` and `for` had it too. None of these can be bound as a name, so none can
open a condition, which makes the fix unambiguous.

### Fixed — an embedded script's output no longer disappears

`RiteEngine::run_source` built a `RuntimeContext`, the guest's `@console` output
buffered into it, and the context was dropped when the run returned. An embedded
`! @console.println("…")` therefore printed nothing and said nothing about it —
the host had no way to know the script had spoken at all.

Guest output now goes to the host's own stdout and stderr by default, as under
`rite run`, and `RiteEngineBuilder::with_output(sink)` redirects it to a log, a
buffer or a UI instead. The sink is called as the script writes, so a
long-running guest streams rather than holding everything until it finishes.

`with_default_builtins()` is deprecated: it has always been a no-op — builtins are
installed unconditionally — and a builder method that selects nothing is a trap.
It still compiles.

### Added — a tutorial for embedding

[Embedding Rite in a Rust program](docs/tutorials/embedding-rite.md): a host whose
pricing rules are a Rite file it does not trust, with grants in code, a budget,
and a record coming back. Its rules script runs in CI against a fixture; the Rust
half is compiled and run by hand, which the page says out loud.

`docs/book/embedding.md` was rewritten against the actual crate. It had been
hedging — "exact builder methods follow the crate API in your tree (`allow`,
`deny`, capability install)" — and `deny` and "capability install" do not exist,
while `run_path` was listed as "if exposed" when it has always been there. Every
snippet in the chapter now compiles; they were compiled together, as one program,
to check it.

### Fixed — a zero-argument closure is a function

`{ || 42 }` evaluated to `42`. `type_of` said `int`, and calling it failed with
`cannot call value of type int`. The only record of the `|…|` was the parameters
it named, and an empty list of them is indistinguishable from never having written
one — so desugar read a thunk as a bare block and gave back its body. Named
`◆ f()` was unaffected, which is how it went unnoticed.

The AST now records whether a parameter list was written, and a block with one
becomes a closure however many names it contains.

The formatter had the same blind spot and was the more dangerous half: it printed
the parameter list only when there were parameters to name, so `{ || 42 }` came
back as `⟦ 42 ⟧`. Formatting a correct program turned a function into its own
body, silently, and the failure showed up later at the call site.

### Changed — builtins read strings and bytes

**The sequence builtins were half a family.** `count`, `slice`, `reverse`,
`index_of`, `contains` and `repeat` read a string as a sequence of characters. The
rest counted list elements and answered an empty *list* for anything else, so
`drop("abcde", 2)` was `[]` and the mistake surfaced somewhere else entirely as
`upper expects a string, got list`. Measured rather than assumed, that was twelve
builtins silently answering the wrong type, not the two on record.

`take`, `drop`, `first`, `last`, `rest`, `init`, `reverse`, `sort`, `unique`,
`chunk` and `enumerate` now read lists, strings and bytes, and give back the kind
they were handed — `take("abcde", 2)` is `"ab"`, `take(bytes, 2)` is bytes.
Characters mean characters, so `take("héllo", 2)` is `"hé"`, agreeing with `slice`
and `count`. A byte is an int, which is what `byte_at` already answered.
`index_of` accepts lists and bytes, where it used to raise while `contains`
answered the same question about the same list happily. `sum`, `min`, `max` and
`join` read all three too: summing bytes is a checksum, and `min` uses the
ordering `sort` uses, so `min("cba")` and `first(sort("cba"))` finally agree.

**Where there is no sensible reading, they now say so instead of answering.**
`zip` and `flatten` are about the structure of a list of lists, which a string does
not have. `keys` and `values` want a record. `lines` and `words` want a string.
`collect_results` wants a list — it answered `ok([])`, which claims every result
succeeded, about a thing that was never a list of results. `sum` refuses
non-numbers rather than skipping them: `sum(["1", "2"])` was `0`, the same `0` a
correct empty list gives, which is the one wrong answer indistinguishable from a
right one.

The one deliberate hole left: `concat` still wraps a non-list argument as a single
element, because that is what a spread of plain values means and it is documented
that way.

### Added

- **`@process.exit(code)` ends a run with a chosen status.** `fail` meant exit 1
  and nothing else could be said, which every CLI eventually needs. The status is
  any number from 0 to 255; nothing after the call runs; no `^` and no middleware
  can intercept it; buffered output is still flushed. It needs **no permission**,
  for the reason `@process.args` needs none — the status you end with is a message
  to whoever ran you, not authority over anything, and gating it behind the grant
  that also permits running arbitrary binaries would be backwards.

  The range is deliberately not restricted to the codes the runtime does not use.
  Forwarding a child's status — `! @process.exit(r.status)` — is the most common
  reason to call it, and a rule that rejected 3–8 would fail only on the runs where
  a subprocess happened to return one, long after the tests passed. So `1`–`8` now
  mean two things: the runtime's own table when the runtime stopped the run, and
  whatever the script decided when it did. The runtime always announces itself on
  stderr, which is what a wrapper should read if it must tell them apart. A status
  outside 0–255 is an error at the call rather than a silent wrap to `code % 256`.

  Inside an `@http` or `@tcp` handler it ends the *process*: the server stops
  accepting, the request in flight gets `503`, and `@http.listen` ends the script
  with the status. `use @http.recover` does not intercept it — recover turns
  handler failures into described 500s, and an exit is not a failure.

### Fixed

- **`@process.run` rejects arguments that are not a list.** Anything else was
  silently treated as "no arguments", so `@process.run("sh", ⟦"-c", "…"⟧, ⟨⟩)` — a
  block where a list belongs — ran a bare `sh`, which read stdin to EOF and
  answered `ok(⟨status: 0⟩)`. A command that never did what was asked, reporting
  success. Same treatment the options record already had.

- **Two concurrent servers no longer share one stop switch.** Introduced while
  adding handler exits and caught by the isolation tests: a second `@http.listen`
  in a process dropped the first server's shutdown sender, which its accept loop
  reads as "shut down", so starting server two silently stopped server one.

### Changed

- **`expected.exit` in a conformance fixture is checked, on both paths.** It meant
  only "zero or not", tested interpreted-only: a fixture declaring `5` passed on any
  failure at all, and the IR path was never consulted for a case expected to fail.
  The status now has to be the declared one, and the two paths have to agree on it.
  Fixtures can also now expect a *successful* early exit, and `expected.stdout` is
  compared for a run that ended by failing — which is what tests the promise that
  output is flushed on every path.

- **One definition of the exit-code table.** `EvalError::exit_code` is now the only
  copy; `rite run`, the `main` generated for a compiled binary, and the conformance
  runner all call it, so a compiled program cannot reach a different conclusion
  about a failure than the interpreter did.

### Documentation

- **The exit-code table exists.** `cli-tool.md` linked to "the full exit-code table"
  in `effects.md`, which had no such table — and the codes as described elsewhere
  were not what the binary does. Every code in the new table was checked by running
  it: `3` and `4` turn out to distinguish *when* a source was rejected (`rite run`
  exits 3 whether a file failed to parse or to resolve; `rite check` exits 4 for
  both), not which phase found the problem.

## [0.4.1] — 2026-07-31

A correctness release. Five capability functions were shipped surface that did
nothing while the generated reference advertised them as working; a compiled binary
never ran `main`; and a path could leave a granted root through directories that do
not exist. Every one of them was found by writing documentation and *running* the
examples rather than reading the code — which is now enforced, with each tutorial's
final script executed and compared to the output printed beside it.

**One upgrade note.** `@clock.format` and `@clock.duration` now answer **results**
rather than plain values, because both can genuinely fail. They previously returned
their input unchanged and were documented as placeholders not to build on, so this
should affect nobody — but if you called them, add `?`.

### Added

- **`@clock.format` formats.** It took a pattern and ignored it, returning the
  timestamp unchanged. It now applies a strftime pattern — `%Y-%m-%d`,
  `%A, %d %B %Y` — and answers a result, because both arguments can be wrong: an
  unparseable timestamp gives `err(⟨kind: "clock.parse", …⟩)` and an unknown
  specifier gives `err(⟨kind: "clock.pattern", …⟩)`. The pattern is validated
  before use rather than handed straight to chrono, which panics on `%Q` — a
  script must not be able to abort its host by writing a bad format string.

- **`@clock.duration` normalizes durations.** It returned the integer it was
  given. It now reads a unit — `250ms`, `2s`, `5m`, `1h`, `1d`, and fractions like
  `1.5s` — and answers whole milliseconds, so `@clock.sleep(@clock.duration("2s")?)`
  says what it means. A bare number is still milliseconds, so both forms agree.

- **`@process.run` honours its third argument.** The options record was accepted
  and discarded, so `⟨cwd: "…"⟩` looked applied and did nothing. It now understands
  `cwd` and `env` (added to the inherited environment, since a child that loses
  `PATH` usually cannot start). An unrecognised key is an error rather than a
  silent default — a typo should not be indistinguishable from the default.

### Fixed — Studio

- **Switching dialect rewrites the editor.** The selector changed only what the
  *next* Format produced, so picking "ascii" left glyphs on screen — the one thing
  the control is named for was the one thing it did not do. Rite has one AST and two
  spellings, so converting is the honest reading of the choice. Source that cannot
  be parsed is left exactly as typed, with the reason in the diagnostics pane;
  silently replacing what someone wrote with a half-converted version would be worse
  than not converting.

### Fixed — compiler

- **A compiled binary never ran `main`.** `rite build` emitted a `rite_main` that
  ran the module's top-level statements and stopped, never consulting `ir.entry`,
  so the generated `rite_fn_main` sitting directly below it was never called. Since
  the book writes almost every example as `◆! main() ⟦ … ⟧`, essentially every
  compiled binary printed nothing and exited `0`.

  Nothing caught it, and each near-miss is worth recording: conformance fixtures are
  written as top-level statements, where both paths already agreed; `run_ir` handles
  the entry point correctly, so the in-process parity gate agreed too; and
  `codegen_is_valid_rust` only asks whether the generated Rust *parses*. The
  disagreement lived solely in generated Rust, so only building and running could
  see it — which is what the new `#[ignore]`d test does.

  Dispatched through the function registry rather than calling `rite_fn_main`
  directly, so a `main` the backend could not lower still runs via the interpreter
  fallback, exactly as its callers already do.

### Fixed — parser

- **`?` inside a call argument, followed by a statement taking a lambda, failed to
  parse.** `r ← id(@json.decode(raw)?)` on one line and `each(r, { |x| x })` on the
  next was rejected, with the error pointing at the *previous* line.

  `?` is both postfix try and prefix `if`, so the parser looks ahead to tell them
  apart. That scan begins inside whatever group the `?` sits in but starts its paren
  depth at zero, and stepped down with `saturating_sub` — which saturates at
  `i32::MIN`, not at zero. The closing `)` of the enclosing call took the depth to
  -1, the next statement's `(` brought it back to 0, and that statement's lambda `{`
  then looked like the body of a conditional. A closing delimiter at depth zero now
  ends the scan: it means the `?` has been left behind, so it was postfix try.

  This was found by writing a tutorial and running it, which is the point of running
  them.

### Security

- **A path could escape a granted root through directories that do not exist.**
  Permission checks resolve a path before testing containment, but only one missing
  component was ever resolved: with more than one, the path was kept as written and
  the containment test fell back to a textual prefix comparison. An absolute path
  like `<granted>/missing/../../secret` therefore *starts with* the granted root as
  a string while landing two levels above it, and was allowed.

  Resolution now walks up to the deepest directory that exists, canonicalizes that
  — so symlinks in it are followed — and folds the remaining components on by hand,
  resolving `..` as it goes. The tail cannot contain symlinks, because it does not
  exist, which is what makes resolving it lexically sound.

  The same fix removes a wrong denial: `@fs.mkdir("a/b")` where neither level exists
  was refused even with a grant covering the parent, because the unresolved relative
  path matched no root. Creating a directory inside a directory you were granted now
  works.

### Fixed

- **`@console.read_line` reads stdin.** A shim in the interpreter answered the
  empty string and shadowed the working implementation in `rite-caps`, so there was
  no way to prompt for input from a Rite script at all. The prompt is now printed by
  the runtime — which owns the output sink, and so can respect `--deny console` and
  keep ordering with buffered output — and the read is done by the capability. Line
  terminators are stripped for both `\n` and `\r\n`; end of input answers `""`.

- **`@game.say` can be called.** `say` is a keyword token, so `@game.say("…")`
  parsed as `@game.` followed by a bare `say` statement: the capability call
  vanished, the string went to stdout via `println`, and the runtime failed with
  `unknown @game.`. Keyword-spelled capability methods are now ordinary names after
  a `.`, which is what the surrounding cases already assumed.

- **`--allow env=PATH,HOME` grants two variables.** The list was stored as the
  single variable `"PATH,HOME"`, a name no environment can have, so the grant was
  accepted and granted nothing — and then denied at the point of use. `--deny`
  takes the same list form.

- **`@env.all` answers a scoped grant instead of refusing it.** It demanded the
  blanket `--allow env` even though the filter for the scoped case was already
  written, just unreachable. It now returns exactly the names granted, which
  reveals nothing `@env.get` would not answer one at a time. Granting nothing is
  still a denial.

- **Conformance `expected.value.json` was unchecked for every non-numeric case.**
  The comparison ended in `expected.parse::<i64>().ok() != value.as_int()`, which
  is `None != None` — false — whenever neither side was an integer, so a wrong
  string expectation reported nothing. Comparison is now by JSON value. The first
  thing this caught was a fixture asserting `"matched"` against `"#?0"`: atoms were
  being rendered with a fresh interner that could not name them, so
  `run_interpreted` now returns the interner alongside the value.

- `@http.response` declared arity 1 but takes a status *and* a body. Arity is a
  documentation field only, so this was wrong in the published reference rather
  than in behaviour.

- **The generated capability reference describes behaviour, not intent.** Several
  descriptors promised things the code did not do, and because the reference is
  generated from them, the promise was published. Those functions are now
  implemented (above) and their text matches; `@process.run` also documents that a
  non-startable command raises rather than answering `err`, and `@fs.remove` states
  plainly that it is recursive on directories.

- **A drift guard for the book's chapter list.** `DOC_CHAPTERS` and
  `docs/book/README.md` have to agree, and previously nothing checked — they had
  already drifted once into two different numberings on the same screen. The
  tutorial list got this guard when it was added; the book now has it too.

### Documentation

- **The book now covers every host function.** An audit against the capability
  registry found 34 of 102 never mentioned anywhere in the book or tutorials —
  including five capabilities with no chapter at all. That is now zero, enforced
  by nothing but the audit, so it is worth re-running when a capability is added.

- **Two new chapters where five capabilities had no home**: Environment (`@env`,
  `@clock`, `@random`, `@store`) and Processes (`@process`). Running another program
  is a different subject from reading a variable, and the sharpest permission in the
  set deserves its own page rather than a section inside someone else's.

- **Tutorials are executed, not just parsed.** Each one now ends with a complete
  script, and CI runs it against fixtures and compares its output to what the page
  prints — including a pinned modification time, so "which files are stale" has
  something to find. Tutorial fences are `native_only`, so `rite docs check` only
  ever proved their syntax was current; a tutorial could parse perfectly while
  describing behaviour that no longer existed. The markdown is the only copy of the
  script, so there is nothing to drift.

- **Five new tutorials.** *Building a CLI* — `@process.args`, splitting flags from
  positionals, and failing with a usage line on stderr and a non-zero status.
  *Testing what you built*, which leads with the thing that will bite someone:
  `rite test` grants **every** permission, so a test file is as trusted as the CLI
  itself. *An HTTP service with real routes*, whose client lives in the same file so
  the script proves its own routes — and which documents that `@store` does not
  persist across requests. *A DNS resolver over `@udp`*, the tutorial byte authoring
  exists for. And *Compiling to a binary*, on what happens to permissions when you
  ship one.

  Three of those cannot run in a CI gate for reasons no flag fixes — a server
  blocks, a cold `rite build` costs minutes, a DNS query needs the network — so they
  carry an explicit `local-only` marker and run under
  `cargo test -p rite-cli --test tutorial_scripts -- --ignored`. The marker is not a
  silent skip: a test requires the page to *tell the reader* it is not CI-verified,
  because the value of the gate is that printed output can be trusted, and an
  unmarked exception spends that trust.

- **New chapter: Network: sockets**, split out of HTTP services. `@udp` and `@tcp`
  were documented, but inside a 521-line chapter the sidebar labelled "HTTP
  services", which is a good way to look documented and read as missing. Both
  chapters now share a `Network:` prefix so they group in the sidebar.

- **A capability → chapter table** in the book index, so a reader who knows the
  sigil can find the prose without guessing which chapter adopted it.

- Expanded coverage of the previously undocumented `@fs` operations (`append`,
  `lines`, `exists`, `mkdir`, `remove`, `copy`, `move`), the `@json` file
  shortcuts, the rest of `@console`, and roughly twenty list and record builtins
  (`all`, `any`, `find`, `chunk`, `enumerate`, `zip`, `reduce`, `unique`,
  `flatten`, `type_of`, …).

- **Corrected two wrong claims in the book.** It said `rest` was a match pattern
  and not a pipeline stage — `xs → rest` works — and left `flatten` as "use
  flatten/builtin if available", which it is.

## [0.4.0] — 2026-07-31

Rite learns to talk to the network and to handle the bytes that come back:
`@crypto`, `@udp` and `@tcp` arrive, `parallel` starts actually running things
together, and strings, numbers and bytes get the operations a scripting language
is expected to have. Everything here is additive — 0.3.1 code keeps working.

### Added

- **`@fs.metadata` reports modification time and symlink-ness.** The record carried
  `len`, `is_file` and `is_dir` and nothing else, which made "which files changed
  since Tuesday" — the most common reason to call it — inexpressible.

  `mtime` is an RFC3339 UTC string, deliberately the same rendering `@clock.now`
  produces, so the two compare directly: `m.mtime > cutoff` is a real time
  comparison, because RFC3339 in UTC sorts lexicographically. `@clock.parse`
  accepts it unchanged. It is `none` where the filesystem records no such time.
  Date *arithmetic* still does not exist — a cutoff has to be a timestamp you
  already hold, not "seven days ago".

  `is_symlink` describes the path itself, while every other field describes what
  the path resolves to — `@fs.metadata` follows links, so a symlink to a file
  reports `is_file: true` with the target's length, as `ls -l` does. A broken link
  still answers `err(⟨kind: "io.not_found", …⟩)`: following it fails before
  anything can report on it, so a dangling symlink cannot be detected.

- **`@tcp` — byte streams, both ends.** `connect`, `send`, `recv`, `peer_addr`,
  `local_addr`, `close`, and a server:
  `! @tcp.listen "127.0.0.1:9000" ⟦ |conn| … ⟧`. A connection is an opaque
  handle, the representation `@udp` sockets and `@db` connections already have, and
  `close` on an already-closed one is fine.

  `peer_addr` is the far end and `local_addr` the near end, so a server can log the
  client it accepted and a client can read the source port it was given. Both are
  captured when the connection opens rather than queried on demand — they cannot
  change, and reading them is therefore never blocked by a `recv` that is still
  waiting, which is exactly when a server wants to know who has gone quiet.

  `recv(conn, max_bytes, timeout_ms)` distinguishes the two ways to get no bytes,
  because conflating them is how read loops go wrong: a peer that **closed cleanly**
  answers `ok` with **zero bytes** (end of stream — reading again says the same),
  while **nothing arriving in time** answers `err(⟨kind: "tcp.timeout", …⟩)` and
  leaves the connection open. Neither is a raise. Transport failures are
  `kind: "tcp.error"`.

  The server is callback-shaped, like `@http.listen`, and there is deliberately no
  `accept`: the block runs once per accepted connection in its own task, receives the
  connection, and **the connection is closed when the block returns**. A connection
  handed back to the script would need a lifetime the language cannot express — Rite
  has no destructors and no scope-bound resources — so `@tcp` reuses the one shape it
  already has instead of inventing rules for one it does not. `listen` blocks until
  Ctrl-C and prints the address it bound, so port `0` is usable.

  Payloads are the `bytes` type and the byte builtins (`from_hex`, `bytes`, `to_hex`,
  `to_text`, `concat`, `slice`, `byte_at`) — `send` takes a string (sent as UTF-8) or
  bytes (verbatim), and `recv` answers bytes. No `@tcp`-local encoding.

  Permissions are the two `@http` already applies, through the same code: the
  **listen address** allows loopback by default and needs `--allow net=<host>` for
  anything else; the **connect destination** is checked per host like an outbound
  `@http.get`, **including loopback**. A client that dials its own machine needs
  `--allow net=127.0.0.1`.

  Native only: the browser runtime has no socket layer and says so, as `@udp` does.

- **Bytes can be authored, not only relayed.** `Value::Bytes` could be counted and
  compared and nothing else, so a program could echo a datagram but not build one —
  the DNS query that motivated `@udp` was unwritable in Rite.

  `from_hex` (a Result — any byte, not only text-safe ones), `bytes` (from a list of
  `0`–`255` or a string's UTF-8), `to_hex`, `to_text` (a Result — bytes are not always
  text), and `byte_at`. `concat`, `slice` and `count` understand bytes now too, so a
  packet can be assembled from a header and a body and read back field by field.

  `count` measures bytes here rather than characters, which is the distinction the type
  exists to make. Out-of-range numbers are refused rather than truncated: a silently
  wrapped `0x1ff` is a packet that goes out wrong and gets debugged at the far end.

  `@crypto.hex_decode` is deliberately not this — it answers a string and rejects
  anything that is not valid UTF-8, which is right for hex-encoded *text* and useless
  for a DNS header.

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
  as `data` (bytes) plus `text` (lossy UTF-8). Binary packets can be built from source
  as well as relayed — see the byte builtins below, which landed in this same release
  and closed the gap `@udp` shipped with.

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

### Fixed

- **The docs site stopped offering Run on examples it cannot run.** Twenty-seven
  blocks across the book had a Run button that answered `capability `@json.encode`
  not registered`. The site decided the button from the fence annotation alone, but
  `rite docs check` runs *natively* — where the capabilities are compiled in — so a
  block could pass CI and still fail in the browser. The code now gets the final say
  over the annotation, and a block using a capability the browser lacks offers
  **Open in Studio** instead.

  The underlying gap is unchanged and recorded in `IMPLEMENTATION.md`: `rite-caps`
  sits behind rite-wasm's `native` feature, so the WASM bundle installs no capability
  host at all. `@console` works only because it reaches the output buffer through the
  context rather than the host.

- **`@http.listen` lost its handler block when the address was a binding.**
  Trailing-block call sugar stayed enabled while the listen address was parsed, so
  `@http.listen where ⟦…⟧` read as a *call to `where`* taking the block, leaving
  `listen` with none. Every example in the book passes a string literal, which is not
  callable, so nothing caught it.

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
