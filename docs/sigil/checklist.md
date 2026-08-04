# Sigil v0 implementation checklist

Every acceptance criterion in `.internal/sigil_mvp.md` §29, with the artifact
that satisfies it and the phase it lands in. `[x]` means done **and** covered by
a test that would fail if it regressed. `[ ]` means not started or not yet
proven. `[~]` means partially landed, with the gap named.

The rule this file exists to enforce: a criterion is not `[x]` because a stub
exists, a TODO is written, a snapshot file was generated but never structurally
asserted, or a coordinate was hardcoded to make one fixture look right.

Phases: **P0** audit, ADRs, terminology, graph contract ✓ · **P1** normalized
graph · **P2** scene and layout · **P3** marks and canonical SVG · **P4**
ornament, themes, PNG, HTML · **P5** WASM and web foundation · **P6** interaction
and Codex · **P7** export and gallery · **P8** Cloudflare · **P9** hardening.

## Architecture

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| AR1 | Sigil does not execute programs | P1 | ADR 0003; `rite_sigil_cannot_execute_anything` reads the manifest and fails on `rite-runtime`, `rite-caps`, `rite-compiler`, `rite-repl`, `rite-lsp`, `tokio`, `axum`, `hyper`, `reqwest` | [x] |
| AR2 | Rite language semantics are unchanged | P0–P9 | ADR 0003; `grammar/aliases.json`, `rite.ebnf`, the lexer, `rite_sem::ExprIr` untouched; the existing Rite suite green at every phase | [x] |
| AR3 | Layout is non-semantic | P2 | ADR 0004; `the_graph_model_declares_no_coordinate_fields` scans the type, `a_serialized_graph_carries_no_geometry` scans an instance, `semantic_geometry_is_independent_of_the_ornament_layers` asserts the invariance; `LayoutHint` is not read | [x] |
| AR4 | Native and browser use the same Rust scene/layout renderer | P5 | `cant-sigil-wasm/tests/parity.rs` — 6 programs × 37 option sets, SVG and scene, plus both input paths agreeing. Does not yet compare a *browser-executed* build against a native fixture | [~] |
| AR5 | `rite-sigil` does not depend on Cant parsing internals | P1 | ADR 0006; `rite_sigil_does_not_know_what_cant_is` (manifest) and `no_rite_sigil_source_mentions_cant` (every source line, comments stripped) | [x] |
| AR6 | Cant adapts into a normalized Sigil graph | P1 | `cant_sem::sigil::to_sigil_graph`; `every_construct_adapts_into_something_renderable` over 8 programs × 2 option sets | [x] |
| AR7 | Rite core crates do not depend on Sigil | P1 | `no_rite_crate_depends_on_sigil` scans every non-Sigil, non-Cant crate manifest | [x] |

## Terminology

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| T1 | Individual language symbols are called glyphs | P0 | ADR 0009; `grammar/glyphs.toml`, `Kind::Glyph`, `.tok-glyph`, `"glyph"` palette key; `palette_sync.rs` fails if the three drift | [x] |
| T2 | Sigil refers to the visual program artifact | P0 | ADR 0009; every remaining lowercase "sigil" in the tree means the artifact | [x] |
| T3 | Public docs do not use both meanings ambiguously | P0 | README, `docs/book/{values,sugar}.md`, `docs/cant/internals.md`, CLI help and the regenerated `docs/generated/cli.md` | [x] |

## Input

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| I1 | Cant source renders through `cant sigil` | P3 | file, `-`, and `-e`; `cant-cli/tests/sigil.rs` | [x] |
| I2 | Graph JSON renders | P1/P3 | `cant sigil --graph`; `graph_json_renders_without_any_source` pipes `cant graph` into it | [x] |
| I3 | Invalid graph input produces structured diagnostics | P1 | 22 `SIGIL-*` codes, each carrying a `GraphRef` and an optional span; a test per rule in `validate.rs`; `no_two_codes_collide` and `every_code_has_documentation` | [x] |
| I4 | Schema versions are checked | P1 | `cant.graph` name-then-version in `validate_deserialized`; `rite.sigil.graph` via `SIGIL-V001`/`SIGIL-V002` | [x] |
| I5 | Unknown supported node kinds degrade safely | P1/P2 | `SigilNodeKind::Unknown(String)`; `an_unknown_kind_warns_by_default_and_errors_when_strict`, `an_unknown_node_kind_lays_out_without_panicking` | [x] |

## Semantics

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| S1 | Source, stage, flow, ward, scatter, collect, fork, orbit, effect, output, unknown have distinct visual grammar | P2/P3 | a skeleton per kind in `marks.rs`, asserted pairwise distinct over path data; every kind produces a scene element | [x] |
| S2 | Fork order is spatially stable | P2 | `fork_branches_occupy_sectors_in_ordinal_order` asserts clockwise order *and* that reversing the region array moves nothing | [x] |
| S3 | Orbit is visibly circular | P2 | `an_orbit_produces_a_ring_its_members_sit_on` — a `Circle` element plus every member on its circumference | [x] |
| S4 | Effects occupy the outer invocation boundary | P2 | `invocations_occupy_the_outer_boundary_band` and `effects_reach_the_invocation_boundary`; placement from the `effect` field, never a label scan | [x] |
| S5 | Semantic meaning does not depend only on color | P3 | `no_two_kinds_produce_the_same_mark`, `no_two_capability_families_produce_the_same_mark`, `the_monochrome_theme_gives_every_family_the_same_accent` | [x] |
| S6 | Ornament can be removed without changing semantic layout | P4 | `the_ornament_level_moves_no_semantic_geometry` — all four levels, over a fork/orbit/effect graph, asserting identical semantic elements *and* hit regions | [x] |

## Disclosure

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| D1 | Veiled mode shows no visible source labels | P3 | `veiled_output_never_draws_a_label` over hostile input × every metadata mode; a Veiled render emits no `<text>` at all | [x] |
| D2 | Inscribed mode shows minimal symbolic annotation | P4 | Inscriptions layer, abbreviated to 14 characters. Abbreviated *capability* marks remain unimplemented | [~] |
| D3 | Revealed mode provides readable labels and a full Codex | P4 | labels drawn upright beside each mark; a Codex per node in the HTML export. The in-app Codex is P6 | [x] |
| D4 | Codex can be hidden/collapsed | P6 | collapsed by default in both the app and the HTML export; no component test yet | [~] |
| D5 | Hover/focus can reveal values in the web app | P6 | hover and keyboard focus both reveal, in the app and the HTML export; no E2E yet | [~] |
| D6 | Deep Veil can suppress interactive revelation | P6 | suppresses tooltips *and* Codex labels in the app; no E2E yet | [~] |
| D7 | Metadata can be stripped completely | P3 | `metadata_none_contains_no_label_snippet_or_identifier` — no label, no title, no desc, no graph-derived id | [x] |

## Rendering

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| R1 | SVG is self-contained, deterministic, script-free, escaped | P3 | `tests/svg_security.rs` — 12 hostile strings × 36 option sets, asserting no script, no `on*` attribute, no external reference, well-formed XML, identical repeat renders | [x] |
| R2 | PNG works | P4 | `cant sigil --format png`, via `rite_render::svg_to_png` behind an off-by-default feature; scale guard; `tests/visual.rs` | [x] |
| R3 | Interactive HTML works offline | P4 | `cant sigil --format html`; `tests/html_export.rs` asserts no remote reference, one managed script, no inline handlers, no user text in executable position, collapsed Codex | [x] |
| R4 | Scene JSON is available | P2/P3 | `cant sigil --format scene-json` + 6 golden fixtures, structurally asserted | [x] |
| R5 | Three themes work | P3/P4 | WCAG 3:1 contrast per theme, distinct SVG per theme, and a raster check that each ground is the polarity it claims | [x] |
| R6 | Ornament levels work | P4 | `--ornament none\|sparse\|ritual\|maximal`, deterministic, on their own layers, no graph refs, plus the S6 invariance | [x] |
| R7 | Canonical and explicit seeds work | P3 | `--seed graph\|canonical\|random\|<int>` and `--canonical`; `canonical_output_is_reproducible_and_differs_from_the_default`, `an_explicit_seed_is_reproducible` | [x] |
| R8 | Render fingerprints work | P3 | `RenderFingerprint` carries graph, renderer, theme@version, seed, mode, metadata, format; `the_render_fingerprint_reports_what_produced_it`; absent under `--metadata none` | [x] |

## CLI

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| C1 | `cant sigil` accepts files, stdin, one-liners, graph JSON | P3 | four input forms, each its own test in `cant-cli/tests/sigil.rs` | [x] |
| C2 | Required format/theme/mode/legend/metadata/seed options work | P3/P4 | every value accepted and every bad value a usage error; defaults asserted through the fingerprint. `--legend`, `--ornament`, `--orientation`, `--embed-*`, `--open` are P4 | [~] |
| C3 | Diagnostics use stable `SIGIL-*` codes | P1–P3 | 22 codes, no collisions, all documented, all round-tripping through their rendered form; exit statuses follow Rite's contract | [x] |
| C4 | Cross-platform output paths and stdout behavior tested | P9 | `<basename>.sigil.svg` default, `-o -` for stdout; separator-neutral assertions, as the existing Windows-portable tests do | [ ] |

## Web application

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| W1 | Vue/TypeScript/Tailwind application exists | P5 | `apps/sigil-web`, running against the WASM engine; `typecheck` clean. Not yet a CI job | [~] |
| W2 | Cant and graph JSON input modes work | P5 | both tabs render; no component test yet | [~] |
| W3 | Canvas dominates the interface | P5 | the canvas is the flex-1 column and both panels collapse; no layout test yet | [~] |
| W4 | Panels can be hidden | P6 | E2E: all panels hidden leaves a fullscreen artifact | [ ] |
| W5 | Pan/zoom/fit/fullscreen work | P6 | implemented with keyboard-operable buttons; no E2E yet | [~] |
| W6 | Codex selection synchronizes with the canvas | P6 | component test both directions | [ ] |
| W7 | Mobile alternatives to hover work | P6 | E2E at a mobile viewport, tap and focus only | [ ] |
| W8 | Exports work | P7 | SVG, PNG (canvas), scene JSON, copy SVG, copy fingerprint in the app; HTML export is CLI-only | [~] |
| W9 | Built-in gallery works | P7 | generated from `fixtures/sigil/` at build time, so it cannot drift | [ ] |
| W10 | Source stays client-side | P5–P8 | ADR 0007; E2E asserts **no** network request carries source, graph JSON, or an exported artifact | [ ] |

## Cloudflare

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| CF1 | Separate Worker project exists | P8 | `apps/sigil-web/wrangler.jsonc`, worker `rite-sigil` | [ ] |
| CF2 | Workers Static Assets/Vite deployment works | P8 | `pnpm sigil:build` + wrangler dry run in CI | [ ] |
| CF3 | `sigil.rite.foo` is a Custom Domain | P8 | `custom_domain: true` route; `site.toml` gains the host and `site_domain_sync.rs` starts enforcing it | [ ] |
| CF4 | SPA fallback works | P8 | `not_found_handling: single-page-application`; smoke test on a deep route | [ ] |
| CF5 | Health/version/schema endpoints work | P8 | `/api/health`, `/api/version`, `/api/schema`; response shape test | [ ] |
| CF6 | Security headers are present | P8 | CSP without broad `unsafe-eval`, `nosniff`, `Referrer-Policy`, `Permissions-Policy`; header assertion test | [ ] |
| CF7 | Build/deploy scripts and CI checks exist | P8 | `pnpm sigil:{dev,build,preview,test,deploy}`; a `sigil-site` CI job independent of the other two, as `cant-site` is | [ ] |

## Quality

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| Q1 | Existing Rite/Cant tests remain green | P0–P9 | `cargo test --workspace --all-features`; 1329 → 1545 passing, 0 failing, at every phase | [x] through P4 |
| Q2 | Native/WASM scene parity passes | P5 | see AR4 | [~] |
| Q3 | Scene and SVG golden tests pass | P2/P3 | `fixtures/sigil/{scenes,svg}/`, structurally asserted rather than merely written | [x] |
| Q4 | Visual regressions are reviewed | P4 | `tests/visual.rs` — an 8×8 perceptual hash over rasterised output: stable across runs, contrast present per theme, ground polarity, ornament changes without burying | [x] |
| Q5 | Fuzz smoke tests pass | P9 | graph JSON parser, adapter, scene builder, mark generator, SVG serializer, metadata stripping | [ ] |
| Q6 | Malicious labels cannot inject markup | P1/P3 | `tests/svg_security.rs` — one escaper, one sanitizer, 12 hostile strings across 36 option sets, plus a hostile-identifier suite | [x] |
| Q7 | Large graph limits work | P1 | `a_graph_over_the_node_cap_is_refused_with_a_way_out` asserts the refusal names an alternative; `a_large_but_legal_graph_warns_once` | [~] `--simplify` itself is P3 |
| Q8 | Accessibility checklist passes | P6 | keyboard selection, focus indicators, structured Codex, reduced motion, no colour-only differentiation, screen-reader graph summary | [ ] |
| Q9 | Documentation examples are generated and tested | P7 | `examples/sigil/*.cant` → graph JSON, scene JSON, veiled + revealed SVG, PNG thumbnail, in CI | [ ] |

## Open design work

Not acceptance criteria, but things that must be right before this is a product
worth showing. Carried here so they are not lost between phases.

| # | What | Why it matters |
|---|---|---|
| ~~OD1~~ | ~~The composition does not fill the circle.~~ **Fixed.** Three causes: the spine divided by its length rather than length−1, so the gap grew as the program got *shorter*; its radius came from whole-graph depth, so deep branches bunched the backbone at the centre; and — the structural one — it allocated an angular slot to every spine node and then relocated some of them, reserving space for marks that would not be there. The sweep is now divided over the nodes that stay on the spiral, and a relocated node borrows a position without consuming a slot. | |
| OD2 | Edge routing minimizes nothing (§11.6). On a dense graph traces cross. | Crossings read as connections that are not there. |
| OD3 | Nested fork-inside-fork sectors subdivide by weight but are not recursively renormalized. | Deep nesting gets cramped before it gets illegible. |

## Non-negotiables that are not criteria but gate every `[x]`

These come from the quality bar in the brief. A criterion marked done while any
of these is true is marked wrong.

- No stubs, no TODOs standing in for behaviour.
- No fixture-specific hardcoded coordinates.
- No Graphviz subprocess wearing custom colours.
- No static background image standing in for generated geometry.
- No labels arranged around ordinary graph nodes.
- No browser-only JavaScript renderer diverging from native.
- No snapshot file that is written but never structurally asserted.
- No SVG path that can carry user markup through unescaped.
- No web app that sends source to the Worker.
