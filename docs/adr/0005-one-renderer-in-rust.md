# ADR 0005 — One renderer, in Rust, shared by the CLI and the browser

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0003 — Sigil is a semantic renderer, not a runtime](0003-sigil-is-a-renderer-not-a-runtime.md) ·
  [ADR 0007 — Veiled rendering and source privacy are first-class](0007-veil-and-source-privacy.md)

## Context

Sigil has two surfaces: `cant sigil` on the command line and `apps/sigil-web` in
a browser tab. Both must produce the same artifact from the same program, because
the product promise is that a user renders locally in the browser, exports an
SVG, and gets the file the CLI would have written.

This repository has already run the experiment of letting a browser reimplement a
Rust behaviour. `crates/rite-render/src/lib.rs` says it out loud: the site's
TypeScript tokeniser is a second implementation of Rite's highlighter, kept in
step by `grammar/palette.json` and `crates/rite-cli/tests/palette_sync.rs` — a
manifest and a gate built specifically because two implementations of one thing
drift. That is a *tokeniser*, where the shared contract is eleven colours and a
list of token kinds. A layout engine's contract is every coordinate it emits.

There is also a straightforwardly attractive wrong answer available. D3 exists.
Writing a radial layout in TypeScript would be faster to iterate on, would
hot-reload, and would not need `wasm-pack`. It would also mean the SVG in the tab
and the SVG on disk are produced by different code, and "native and browser
scenes match" — an MVP acceptance criterion — would be a coincidence maintained
by hand.

## Decision

**One implementation, in Rust, in `rite-sigil`, compiled natively for the CLI and
to WebAssembly for the browser.**

Binding:

1. Graph normalization, validation, topology analysis, layout, mark generation,
   theme resolution, scene construction, and SVG serialization live in
   `rite-sigil` and nowhere else.
2. `rite-sigil-wasm` is a `wasm-bindgen` boundary and nothing more. It converts
   arguments, calls `rite-sigil`, and converts results. It contains no geometry.
3. `apps/sigil-web` **does not reimplement layout, marks, themes, or SVG
   generation in JavaScript.** TypeScript owns UI state, file input, editor
   integration, interaction overlays, export orchestration, browser-side
   rasterization fallback, and routing.
4. Native and WASM parity is a test, not a convention: for the canonical fixture
   set, native scene JSON and browser scene JSON must be equal, and native
   canonical SVG and browser canonical SVG must be equal. Differences are
   investigated, not absorbed into a tolerance.
5. The WASM dependency graph stays browser-pure — no Rite runtime, no
   capabilities, no filesystem, no process, no compiler, no async runtime it does
   not need. `crates/cant-wasm/Cargo.toml` already documents what happens when
   this slips: a workspace dependency silently pulled axum, hyper, tokio and mio
   into a `wasm32` build and failed inside `mio`. `rite-sigil` takes path
   dependencies with `default-features = false` for the same reason.
6. Interaction *decoration* is allowed in the browser — highlight classes,
   selection state, tooltips positioned over the SVG. Anything that changes
   where a semantic element **is** belongs in Rust.

## Consequences

**Good.** The parity criterion becomes checkable, and the golden fixtures serve
both platforms at once. A layout bug is fixed once.

**Good.** The determinism story in ADR 0004 survives contact with the browser.
Two implementations would have needed matching float formatting, matching hash
functions, and matching iteration order — three known sources of drift that
simply do not arise.

**Good.** Everything that touches untrusted input — graph JSON parsing, label
escaping, ID sanitization, limit enforcement — is written once, audited once, and
fuzzed once. A JavaScript renderer would have needed its own escaping story on
the side of the boundary where injection actually matters.

**Cost.** Iteration on the visual design is slower. Changing a curve means a
`wasm-pack` rebuild, not a hot reload. This is the real price and it is paid on
every aesthetic change, which for this product is most of them. It is mitigated
by the scene-JSON boundary: layout can be inspected and diffed without rendering,
and `cant sigil --format scene-json` is the fast loop.

**Cost.** Another WASM artifact to build, size-track, and ship. The repository
already builds two (`rite-wasm`, `cant-wasm`) with two scripts that were kept
separate deliberately; this adds a third rather than merging them, on the same
reasoning — Sigil's site must be buildable without Rite's.

**Risk accepted.** WASM start-up and render latency in the browser is a real
product risk against the §24 targets. The mitigation is measurement, cancellation
tokens, and a Web Worker if measurement justifies one — not a JavaScript fast
path, which would reintroduce the second implementation through the door marked
"performance".

## Alternatives rejected

**A TypeScript renderer for the web, Rust for the CLI.** Rejected: it makes the
central acceptance criterion unenforceable, and this repository already carries
one two-implementation drift gate and did not enjoy building it.

**A JavaScript renderer everywhere, with the CLI shelling out to Node.** Rejected:
it puts a Node runtime in the dependency path of a Rust CLI that ships as a single
static binary in a release archive, and `cant sigil` would stop working on a
machine that has `cant` and nothing else.

**Render to a bitmap in Rust and ship pixels to the browser.** Rejected: SVG is
the canonical format (§16.1), the artifact must be resolution-independent and
inspectable, and interactive hit regions require the element structure that
rasterization destroys.
