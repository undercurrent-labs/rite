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
| AR4 | Native and browser use the same Rust scene/layout renderer | P5 | ADR 0005; parity test — native scene JSON == browser scene JSON, native canonical SVG == browser canonical SVG, over the canonical fixture set | [ ] |
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
| I1 | Cant source renders through `cant sigil` | P3 | `cant sigil <file>`, `-`, `-e`; `cant-cli/tests/` | [ ] |
| I2 | Graph JSON renders | P1/P3 | `cant sigil --graph <file\|->`; adapter reads `cant.graph` v1 JSON with no parser present | [ ] |
| I3 | Invalid graph input produces structured diagnostics | P1 | 22 `SIGIL-*` codes, each carrying a `GraphRef` and an optional span; a test per rule in `validate.rs`; `no_two_codes_collide` and `every_code_has_documentation` | [x] |
| I4 | Schema versions are checked | P1 | `cant.graph` name-then-version in `validate_deserialized`; `rite.sigil.graph` via `SIGIL-V001`/`SIGIL-V002` | [x] |
| I5 | Unknown supported node kinds degrade safely | P1/P2 | `SigilNodeKind::Unknown(String)`; `an_unknown_kind_warns_by_default_and_errors_when_strict`, `an_unknown_node_kind_lays_out_without_panicking` | [x] |

## Semantics

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| S1 | Source, stage, flow, ward, scatter, collect, fork, orbit, effect, output, unknown have distinct visual grammar | P2/P3 | one scene golden per kind; a shape-distinctness test over generated mark path data | [ ] |
| S2 | Fork order is spatially stable | P2 | `fork_branches_occupy_sectors_in_ordinal_order` asserts clockwise order *and* that reversing the region array moves nothing | [x] |
| S3 | Orbit is visibly circular | P2 | `an_orbit_produces_a_ring_its_members_sit_on` — a `Circle` element plus every member on its circumference | [x] |
| S4 | Effects occupy the outer invocation boundary | P2 | `invocations_occupy_the_outer_boundary_band` and `effects_reach_the_invocation_boundary`; placement from the `effect` field, never a label scan | [x] |
| S5 | Semantic meaning does not depend only on color | P3 | monochrome (`void` theme) golden; every kind distinguishable by path geometry alone | [ ] |
| S6 | Ornament can be removed without changing semantic layout | P4 | property test: scene with ornament `none` and `maximal` have byte-identical semantic-layer elements | [ ] |

## Disclosure

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| D1 | Veiled mode shows no visible source labels | P3 | ADR 0007; golden SVG suite asserts no visible text node carries label content | [ ] |
| D2 | Inscribed mode shows minimal symbolic annotation | P4 | golden; abbreviated capability family marks only, no full expressions | [ ] |
| D3 | Revealed mode provides readable labels and a full Codex | P4 | golden + Codex entry count equals node count | [ ] |
| D4 | Codex can be hidden/collapsed | P6 | `apps/sigil-web` component test | [ ] |
| D5 | Hover/focus can reveal values in the web app | P6 | component + E2E test, keyboard focus included | [ ] |
| D6 | Deep Veil can suppress interactive revelation | P6 | E2E: with Deep Veil on, hover and focus produce no tooltip | [ ] |
| D7 | Metadata can be stripped completely | P4 | `--metadata none`; property test over the malicious-label fixture set asserts no source snippet survives anywhere in the output | [ ] |

## Rendering

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| R1 | SVG is self-contained, deterministic, script-free, escaped | P3 | canonical SVG goldens; assertions: parses in a strict XML parser, no `<script>`, no `on*` attribute, no external reference, no `foreignObject` | [ ] |
| R2 | PNG works | P4 | `--format png`; reuses `rite-render`'s audited `resvg` path where it fits | [ ] |
| R3 | Interactive HTML works offline | P4 | self-contained export; test asserts zero external references and a working Codex toggle | [ ] |
| R4 | Scene JSON is available | P2 | `rite_sigil::build_scene` + 6 golden fixtures, structurally asserted. **CLI surface is P3** — the format exists, `cant sigil` does not yet | [~] |
| R5 | Three themes work | P4 | `neon-ritual`, `void`, `parchment`; a golden and a contrast check each | [ ] |
| R6 | Ornament levels work | P4 | `none`/`sparse`/`ritual`/`maximal`; goldens + the S6 invariance property | [ ] |
| R7 | Canonical and explicit seeds work | P3 | `--seed graph\|canonical\|<int>\|random`; determinism property test | [ ] |
| R8 | Render fingerprints work | P3 | graph fingerprint + renderer version + theme version + seed + mode + geometry options; stable across runs, absent under `--metadata none` | [ ] |

## CLI

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| C1 | `cant sigil` accepts files, stdin, one-liners, graph JSON | P3 | `cant-cli/tests/` — four input forms | [ ] |
| C2 | Required format/theme/mode/legend/metadata/seed options work | P3/P4 | one test per option; defaults asserted: svg, neon-ritual, veiled, safe, graph seed, ritual ornament | [ ] |
| C3 | Diagnostics use stable `SIGIL-*` codes | P1–P3 | code table in `rite-sigil`; a test asserts every emitted code is documented | [ ] |
| C4 | Cross-platform output paths and stdout behavior tested | P9 | `<basename>.sigil.svg` default, `-o -` for stdout; separator-neutral assertions, as the existing Windows-portable tests do | [ ] |

## Web application

| # | Criterion | Phase | Artifact / proof | Done |
|---|---|---|---|---|
| W1 | Vue/TypeScript/Tailwind application exists | P5 | `apps/sigil-web`; `typecheck` is a CI gate, as it is for the other two apps | [ ] |
| W2 | Cant and graph JSON input modes work | P5 | component tests | [ ] |
| W3 | Canvas dominates the interface | P5 | layout test on the desktop breakpoint | [ ] |
| W4 | Panels can be hidden | P6 | E2E: all panels hidden leaves a fullscreen artifact | [ ] |
| W5 | Pan/zoom/fit/fullscreen work | P6 | component + E2E, with keyboard-operable zoom (see A3) | [ ] |
| W6 | Codex selection synchronizes with the canvas | P6 | component test both directions | [ ] |
| W7 | Mobile alternatives to hover work | P6 | E2E at a mobile viewport, tap and focus only | [ ] |
| W8 | Exports work | P7 | SVG, PNG, HTML, scene JSON, copy SVG, copy fingerprint | [ ] |
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
| Q1 | Existing Rite/Cant tests remain green | P0–P9 | `cargo test --workspace --all-features`; 1329 → 1442 passing, 0 failing, at every phase | [x] through P2 |
| Q2 | Native/WASM scene parity passes | P5 | see AR4 | [ ] |
| Q3 | Scene and SVG golden tests pass | P2/P3 | `fixtures/sigil/{scenes,svg}/`, structurally asserted rather than merely written | [ ] |
| Q4 | Visual regressions are reviewed | P4 | PNG perceptual-hash + bounded pixel diff, per theme | [ ] |
| Q5 | Fuzz smoke tests pass | P9 | graph JSON parser, adapter, scene builder, mark generator, SVG serializer, metadata stripping | [ ] |
| Q6 | Malicious labels cannot inject markup | P1/P3 | `malicious_labels_do_not_escape_into_element_ids`, `a_hostile_identifier_is_sanitized_in_the_element_id`; SVG escaping is P3 | [~] |
| Q7 | Large graph limits work | P1 | `a_graph_over_the_node_cap_is_refused_with_a_way_out` asserts the refusal names an alternative; `a_large_but_legal_graph_warns_once` | [~] `--simplify` itself is P3 |
| Q8 | Accessibility checklist passes | P6 | keyboard selection, focus indicators, structured Codex, reduced motion, no colour-only differentiation, screen-reader graph summary | [ ] |
| Q9 | Documentation examples are generated and tested | P7 | `examples/sigil/*.cant` → graph JSON, scene JSON, veiled + revealed SVG, PNG thumbnail, in CI | [ ] |

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
