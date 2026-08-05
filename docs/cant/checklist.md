# Cant v0 implementation checklist

Every acceptance criterion in the MVP specification (§16), with the artifact that
satisfies it and the phase it lands in. `[x]` means done **and** covered by a
test that would fail if it regressed. `[ ]` means not started or not yet proven.

Legend for phases: **P0** audit/ADRs ✓ · **P1** skeleton, manifest, parser ✓ ·
**P2** formatter and conversion ✓ · **P3** graph and validation ✓ · **P4** expansion ✓ · **P5** run and build ✓ · **P6** explain, REPL, docs ✓ · **P7** release ✓ ·

## Boundaries

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| B1 | Rite grammar and dialect behaviour unchanged | P0–P7 | ADR 0001; `grammar/aliases.json` and `rite_fmt::Dialect` untouched; existing Rite suite green | [x] |
| B2 | Rite IR has no Cant-only variants | P4 | ADR 0002; `rite_sem::ExprIr` unchanged; Cant reaches IR only through generated source | [x] |
| B3 | Rite runtime has no public Cant capability | P5 | `rite_caps::HostCapabilities` registry unchanged | [x] |
| B4 | Rite crates do not depend on Cant crates | P1 | `crates/cant-cli/tests/boundaries.rs` — scans every `rite-*` manifest and source file | [x] |
| B5 | Cant is in the same monorepo and release pipeline | P1/P7 | workspace members; `cant` built, smoke-tested and packaged in every release archive, with checksums and a `version-manifest.json` entry | [x] |

## Authoring

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| A1 | Every program writable in ASCII on a normal keyboard | P1 | `grammar/cant/operators.toml` — every operator has an `ascii` spelling; `manifest_sync` asserts it is ASCII-only | [x] |
| A2 | Glyph forms accepted | P1 | `parser.rs::ascii_and_glyph_parse_to_the_same_program` and `fixtures.rs::every_dialect_pair_produces_the_same_program` | [x] |
| A3 | Glyph forms convertible | P2 | `cant convert --to ascii\|glyph`, splicing only the spans the parser recorded as operators; `fmt.rs::conversion_touches_only_structural_operators` | [x] |
| A4 | `cant -e '…'` works | P5 | top-level `-e` implies `run`; `cli.rs::run_dash_e_and_the_top_level_form_are_the_same_command` | [x] |
| A5 | File input works | P1 | `cant check <file>`, covered by `cant-cli/tests/cli.rs` | [x] |
| A6 | Stdin (`-`) works | P1 | `cant check -`, covered by `cant-cli/tests/cli.rs` | [x] |
| A7 | REPL works | P6 | `cant repl` — each line a whole program, `:expand`/`:graph`/`:explain` meta-commands, session-level permissions and budget | [x] |
| A8 | Shell quoting documented honestly | P6 | `docs/cant/cli.md` — quoted `-e`, the PowerShell `$` caveat, and no claim of unquoted portability | [x] |

## Semantics

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| S1 | Flow | P4/P5 | golden expansion **and** an execution fixture | [x] |
| S2 | Scatter | P4/P5 | its own type check, so a non-list reports `CANT-R003` naming the `*`; `scatter-not-a-list` fixture | [x] |
| S3 | Collect | P4/P5 | golden expansion and the `scatter-collect` execution fixture | [x] |
| S4 | Ward | P4/P5 | a conditional emission; the predicate is substituted rather than piped, because Rite rejects `$` outside a call. `ward-filters` fixture | [x] |
| S5 | Sequential fork | P4/P5 | branch chains called in order and concatenated; `fork` and `nested` fixtures | [x] |
| S6 | Bounded orbit | P4/P5 | FIFO worklist, seen-set, `:by`, `:max`; `orbit`, `orbit-dedup`, `orbit-identity`, `orbit-limit` fixtures | [x] |
| S7 | Ordering is deterministic | P4/P5 | `differential.rs::ordered_effects_match_between_the_two_paths` — fork branches left to right, scatter in list order, on both paths | [x] |
| S8 | Orbit cannot run unbounded | P3/P5 | `:max` validated in P3, enforced in generated code (`CANT-O002`), and Rite's step/time budget underneath it (`CANT-O001`); both have execution fixtures | [x] |
| S9 | Effects and permissions remain explicit and Rite-enforced | P3/P4/P5 | P3 rejects an effectful ward or `:by`; P4 proves a missing `!` is rejected by Rite's own resolver; P5's `capability-{denied,granted}` fixtures prove the grant is enforced identically on both paths | [x] |
| S10 | Program-boundary zero/one/many normalization | P4/P5 | `boundary-none` and `boundary-one` execution fixtures | [x] |

## Tooling

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| T1 | `cant version` | P1 | reports the tool, language, graph-schema and Rite versions; `cant-cli/tests/cli.rs` | [x] |
| T2 | `cant check` | P1/P3/P5 | P1 syntax; P3 adds graph validation (unknown modifier, effectful ward or `:by`, bad `:max`, unowned cycle) with no interface change; P5 adds Rite resolution through generated source | [~] |
| T3 | `cant fmt` | P2 | idempotent, comment- and string-preserving, `--check`/`--write`/`--compact`/`--width`; refuses a source with syntax errors | [x] |
| T4 | `cant convert` | P2 | see A3 | [x] |
| T5 | `cant expand` | P4 | `cant/tests/expand.rs` — valid (Rite accepts every expansion), deterministic, hygienic (prefix + source hash + node number), source-mapped; golden expansions in `conformance/cant/lowering/`. **Deviation:** not piped through `rite fmt` — see internals conflict 6 | [~] |
| T6 | `cant graph --format json\|dot` | P3 | `cant-cli/tests/cli.rs` — byte-identical between runs, DOT clusters subgraphs, the graph prints even when validation failed; `cant-sem/tests/dot_renders.rs` pipes every construct and fixture through Graphviz and requires no warning | [x] |
| T7 | `cant explain` | P6 | numbered steps from the **graph**, capabilities, effects, hazards, ordering; `explain.rs::the_output_is_never_a_debug_dump` asserts no `NodeKind`/`Span {`/`LeafExpr` ever appears | [x] |
| T8 | `cant run` | P5 | through generated Rite on Rite's runtime; `cant-cli/tests/{cli,differential}.rs` | [x] |
| T9 | `cant build` | P5 | through `rite_compiler::build_script`, from generated Rite kept at `.rite/cant/` so a compiled program stays auditable | [x] |
| T10 | Diagnostics point at Cant source | P4 | primary label on `.cant` spans; the three-deep cascade Rite reports for an unmarked host call collapses to the one the user wrote, and no generated identifier is ever shown | [x] |
| T11 | JSON diagnostics include the underlying Rite metadata | P4 | `expand.rs::json_diagnostics_preserve_the_underlying_rite_code_and_span` — Cant code, Cant span, generated span, Rite code, notes/help | [x] |
| T12 | Generated Rite is inspectable | P4 | `cant expand` is public and permanent (ADR 0002 §2); the output uses `for`/`while` rather than their desugared forms precisely so it can be read | [x] |
| T13 | Graph JSON and DOT are deterministic | P3 | identifiers assigned by a depth-first walk in source order; `graph.rs::json_and_dot_are_deterministic` | [x] |

## Parity

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| P1 | `cant run` == `rite run <cant expand>` | P5 | `differential.rs` over 15 execution fixtures — value, stdout, outcome, exit code, capability denial, ordered effects | [x] |
| P2 | Interpreted and compiled Cant fixtures match | P5 | same harness, `#[ignore]`d for cost: `cargo test -p cant-cli --test differential -- --ignored` | [x] |
| P3 | ASCII and glyph fixtures give equal graphs and values | P1/P3/P5 | AST equality in P1, graph equality in P3, and identical expansions — so identical values — because conversion is byte-preserving outside operator spans | [x] |

## Quality

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| Q1 | Existing Rite workspace tests pass | every | `cargo test --workspace --all-features --no-fail-fast` | [x] |
| Q2 | Cant tests run on Linux, macOS, Windows | P1/P7 | no POSIX-only fixtures, `std::env::temp_dir` rather than `/tmp`, `.exe` handled where a binary is spawned; the CI matrix builds and smoke-tests `cant` on every platform. Windows remains opt-in, as it is for Rite | [~] |
| Q3 | Parser/formatter fuzz smoke tests pass | P1/P2 | `no_panic.rs` (P1) and `fmt.rs` property tests over the fixture corpus plus a generator (P2) | [x] |
| Q4 | Every public example executable in CI | P6 | `cant-cli/tests/docs.rs` — every `examples/cant/*/main.cant` is **run**, every ` ```cant ` fence in the docs and ADRs is checked or run, and an `ignore` needs a written reason | [x] |
| Q5 | Architecture and deviations recorded | P0 | `docs/cant/internals.md`, ADRs 0001/0002, and this file | [x] |

## Property and fuzz obligations (spec §13.4)

| Property | Phase | Done |
|---|---|---|
| Formatting is idempotent — `format(format(x)) == format(x)` | P2 | [x] |
| ASCII/glyph graph equivalence — `parse(convert(x, d)) == parse(x)` | P1 (AST) / P3 (graph) | [x] |
| Strings and comments containing `->`, `\|{`, `~{`, `?{`, `[]` or glyphs are never altered | P1 (lexer) / P2 (formatter) | [x] |
| Generated Rite always parses | P4 | [x] |
| Source mappings are monotonic and in bounds | P4 | [x] |
| Orbit never exceeds its accepted-item limit | P4 | [x] |
| Invalid input never panics | P1 | [x] |
| Fuzzed graph deserialization cannot create unvalidated cycles | P3 | [x] |

## Phase 7 — release and the Sigil seam

| Criterion | Artifact / proof | Done |
|---|---|---|
| Rite releases can include both `rite` and `cant` | `release.yml` builds `-p cant-cli`, packages it into every archive, runs `cant version` on the native ones, and lists it in `version-manifest.json` | [x] |
| Checksums and manifest entries | the existing `SHA256SUMS` covers the archive; the manifest gained `binaries` and a `languages` block recording Cant's language and graph-schema versions and its `experimental` stability | [x] |
| CI matrix coverage | `cant-cli` builds in the debug and release steps on every platform, plus a smoke test of `version` / `-e` / `explain`; the `cant-site` job builds and typechecks the site | [x] |
| Cant docs on the product site | linked from the Rite footer as an experimental sibling — footer rather than nav, because nav placement would say Cant is part of that site | [x] |
| Graph JSON schema frozen as experimental | [`graph-schema.md`](graph-schema.md) is the contract; `cant-sem/tests/schema_freeze.rs` fails on any key added, removed or renamed, and on any key the document does not mention | [x] |
| A Sigil renderer's contract is documented | the same page: ports, edge roles, branch ordinals, reserved non-semantic layout, and the promise that **a consumer never has to parse Cant source** — asserted by `a_consumer_never_needs_the_source_text` | [x] |
| **Cant remains removable** | `cant-cli/tests/removable.rs` — **no Rite source file mentions Cant at all**, Rite's grammar, fixtures, examples and book never do, no Rite crate shells out to the binary, and every shared file that must be edited is listed with what the edit is | [x] |

## Open questions

Carried from `docs/cant/internals.md`. Each is **assigned to the phase that has
to answer it** rather than left floating — a question with no owner is one that
gets answered by accident, in whichever direction the first implementation
happened to go.

| # | Question | Blocks | Answered in |
|---|---|---|---|
| 1 | ~~How does a Cant program name a function it defines?~~ **Answered in P4:** builtins, Rite closures inside a leaf (`map($, { \|n\| n * n })`), and — when that is not enough — a `use` of a Rite module, which is one line the parser can delimit and which hands module resolution to Rite entirely. Cant does **not** learn to parse Rite declarations. `use` is implemented: leading `use name` lines parse, survive both dialects and the formatter, ride the graph as `uses`, and expand to Rite `use` lines — resolution, qualified access and effect discipline are Rite's own. Pinned by `conformance/cant/execution/use-module`. | — | ✓ decided |
| 2 | ~~Where do the permission and budget CLI flags live?~~ **Answered in P5:** `rite::options::RuntimeOptions` — the *meaning* of the strings, shared; the `clap` declarations stay each tool's own, and `rite` gains no argument-parser dependency. `rite run` was switched over to it, so the extraction is proven behaviour-preserving rather than asserted. Two latent bugs fell out: a bad `--deny` used to be discarded silently, and `rite run` exposed only two of the five budget knobs. | — | ✓ decided |
| 3 | ~~Does generated Rite print `!@fs.read` or `do host.fs.read`?~~ **Answered in P4:** `!@fs.read`, the compact form, because `cant expand` output is read next to the Cant that produced it and the two should look alike. `rite fmt --ascii` would rewrite it to `do host.fs.read`; see conflict 6 for why the output is not piped through it. | — | ✓ decided |

Two gaps recorded in Phase 0 are now closed:

- **Formatter/converter APIs reusable by editors** — `cant_syntax::fmt` exposes
  `format`, `convert`, `detect`, `convert_offset_map` and `map_offset` as
  library functions, so the deferred VS Code and Studio integrations have
  something to call that is not the CLI.
- **Cursor preservation across conversion** — `convert_offset_map` is exact
  rather than interpolated, because conversion only changes the length of
  operator spans. `rite-fmt`'s `LineSourceMap` has to approximate within a line;
  this does not.
