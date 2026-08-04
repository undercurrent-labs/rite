# Sigil implementation log

What was decided, deviated from, or discovered while building Sigil, in the order
it happened. The specification at `.internal/sigil_mvp.md` is authoritative and
unchanged; this file records where the repository met it, where it did not, and
why.

Companion to [the acceptance checklist](checklist.md), which tracks *what* is
done. This one records *what it cost and what it turned out to mean*.

---

## Phase 0 — audit, ADRs, terminology, contracts

**Status: complete.** Baseline green before and after.

### Test status

| Gate | Before Phase 0 | After Phase 0 |
|---|---|---|
| `cargo fmt --all -- --check` | clean | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean | clean |
| `cargo test --workspace --all-features --no-fail-fast` | 1329 passed, 0 failed, 6 ignored, 139 binaries | 1332 passed, 0 failed, 6 ignored |
| `pnpm --dir apps/{rite-web,rite-studio,cant-web} typecheck` | clean | clean |
| `rite docs build` idempotent | yes | yes |

The three added tests are the new schema-freeze cases: schema-name refusal,
version-0 refusal, and per-node capability metadata.

### What the audit found

**The repository had already anticipated Sigil, and had done so correctly.**
`cant_sem::graph`'s module documentation says lowering reads the graph rather
than the AST specifically so that "what a future Sigil renderer displays is what
actually executes", and `LayoutHint` was reserved in Phase 3 with a doc comment
stating it is never semantic. ADR 0004 is therefore less an invention than a
promotion of an existing comment to a binding rule — and an extension of it:
Sigil does not read `LayoutHint` either, so a hostile or stale hint in a graph
JSON file cannot move a semantic mark.

**The `sigil` terminology footprint was smaller than expected and entirely
mechanical.** `grammar/sigils.toml` is read by no code at all — only
`crates/cant-cli/tests/boundaries.rs` names the path, to assert the file does not
mention Cant. The load-bearing usage was the `"sigil"` token kind, which flows
through `grammar/palette.json` → `rite_render::Kind::Sigil` → `.tok-sigil` →
`highlight.ts`, with `crates/rite-cli/tests/palette_sync.rs` requiring all four to
agree. That test made the rename safe: a partial migration fails CI rather than
producing silently unstyled tokens. See ADR 0009.

**`rite-render` is not reusable as a base for Sigil, and should not be forced
to be.** It renders *highlighted source text* — its model is runs of coloured
text at computed column positions, `Frame::{Text,Box,Window}` chrome, and a font
size. There is no geometry layer to share. Two things are worth reusing and
neither needs an extraction:

- The `png` feature pattern. `rite-render` gates `resvg` behind an off-by-default
  feature precisely so the browser build does not pull a rasteriser and a font
  stack it cannot use. `rite-sigil` copies the pattern rather than the code.
- `grammar/palette.json`'s discipline — one table, gated by a sync test — as the
  model for Sigil's theme manifests.

A shared SVG-serialization utility was considered and rejected for now:
`rite-render`'s SVG is a flat sequence of `<rect>`/`<text>` with inline fills and
no CSS classes, while Sigil's is layered groups with stable semantic IDs and
classes. Extracting a common escaper is the only genuinely coherent piece, and it
is four lines; duplicating it is cheaper than a crate boundary. Recorded here so
the decision is not re-litigated silently.

### Deviations from the specification

**1. `grammar/sigils.toml` migrated, but its `[[sigil]]` table became
`[[glyph]]`.** The spec asks for `grammar/glyphs.toml`; it does not name the
table key. Renaming both keeps the file internally consistent. No reader exists,
so nothing broke.

**2. The Cant graph schema was bumped to version 1 in Phase 0, not Phase 1.**
The spec's Phase 0 item 7 says "freeze or version the Cant graph contract needed
by Sigil", and the gaps were small and precisely known, so versioning now was
cheaper than carrying a documented gap into the adapter. What was added is in
`docs/cant/graph-schema.md`; the reasoning is below.

**3. No ADR was written for "Sigil is a semantic renderer" *and* "Graphviz stays
the technical view" as one document.** The brief lists six required ADR subjects;
they landed as seven documents (0003–0009), with terminology split out because it
has its own costs — a renamed public enum variant and a shipped CSS class — that
deserve their own consequences section.

### Cant graph gaps found, and what was done

The specification's §6.2 lists what the adapter must obtain. Measured against
`cant.graph` version 0:

| Required | Status before | Action |
|---|---|---|
| Graph schema identifier | **absent** — only a bare `version` | added `schema: "cant.graph"` |
| Graph schema version | present (`version`) | unchanged |
| Stable node IDs | present, depth-first in source order | unchanged |
| Stable edge identity | no `id`, but deterministic from `(from, to, ordinal, role)` | no change; Sigil synthesizes edge IDs from the tuple |
| Node kinds | present, closed enum | unchanged — `Effect`, `Output` and `Unknown` are Sigil's, derived by the adapter |
| Directed edges | present | unchanged |
| Branch ordering | present (`ordinal`, authoritative over array order) | unchanged |
| Region ownership | present (`subgraph`, `subgraphs[]`) | unchanged |
| Orbit/cycle metadata | present (`identity`, `max_items`, `orbit_feedback` role) | unchanged |
| **Effect/capability metadata** | **partial** — a per-leaf `effectful` bool, plus a program-wide `capabilities()` that re-scanned leaf text on every call | added per-node `capabilities: [{name, family}]` |
| Source spans | present, never dummy | unchanged |
| Labels/snippets | present (leaf `text`) | unchanged |
| Graph fingerprint | **absent** | not a Cant gap — the fingerprint is over the *normalized* graph and belongs to `rite-sigil` (ADR 0006) |

The capability gap was the only one that mattered architecturally. Without it,
Sigil deciding whether a node is a filesystem or a network invocation means
pattern-matching `@fs.` out of leaf text — inferring semantics from a label,
which the brief and ADR 0006 both prohibit by name, and which is wrong the first
time a leaf contains `"@fs.read"` inside a string. `cant_sem` already had a
careful textual scanner for its own summary; the fix was to run it once during
lowering and store the answer, so a program-wide summary and a consumer walking
nodes read the same field and cannot disagree.

`producer` was added at the same time because a stored graph that cannot say what
wrote it makes a bug report guesswork. It is explicitly excluded from anything a
consumer hashes: a renderer keying artifacts on the producer version would
invalidate every cached render on a release that changed no graph.

Version 0 graphs are refused rather than upgraded. The schema is experimental and
says so; a migration path for a format nobody has stored is cost with no reader.

### Constraints discovered

**The published-docs link gate.** `crates/cant-cli/tests/docs.rs` fails if a page
published on the Cant site links a page that is not published, because the reader
is on the site and the link lands there. `docs/cant/graph-schema.md` therefore
refers to ADR 0006 as a repository path in a code span rather than as a link.
Worth knowing before writing any published Sigil page.

**The generation guard is strict and correct.** `rite docs build` writes both the
agent bundle and the tracked generated reference, and CI fails if regenerating
rewrites a tracked file. The CLI help text change ("Sigils, keywords…" →
"Glyphs, keywords…") propagated to `docs/generated/cli.md`, which is tracked;
regenerating twice confirmed idempotence.

**`cant-wasm`'s dependency comment is a warning worth heeding.** It records that
declaring a workspace dependency without `default-features = false` silently
pulled axum, hyper, tokio and mio into a `wasm32` build and failed inside `mio`,
because cargo ignores `default-features = false` on a workspace dependency whose
table does not specify it. `rite-sigil` and `rite-sigil-wasm` take path
dependencies with explicit `default-features = false` for this reason.

### Left alone deliberately

- `CHANGELOG.md`'s historical prose still says "sigil" where a released entry
  said it. Rewriting shipped history to match new vocabulary would be a lie about
  what was published.
- `crates/rite-caps/tests/db_sandbox.rs` inserts the strings `'glyph'` and
  `'sigil'` as arbitrary SQL row values. They are test data, not terminology.
- `.internal/sigil_mvp.md` is unchanged, as instructed.

### Stale comments corrected

Two comments said Sigil "does not exist yet" and framed DOT export as a
placeholder until it did. ADR 0008 makes that relationship permanent rather than
transitional, so `crates/cant-sem/src/dot.rs` and the `cant graph` help text now
say that Graphviz is the technical view and Sigil is the stylized one, and that
neither replaces the other.

---

## Phase 1 — normalized graph

**Status: complete.**

Added `crates/rite-sigil` (`0.1.0`, its own version — it renders graphs from more
than one producer, and tying it to either language's number would make a renderer
release imply a language one), and the Cant adapter as `cant_sem::sigil`.

| Module | What it owns |
|---|---|
| `graph.rs` | `SigilGraph` and everything in it |
| `diagnostic.rs` | 22 `SIGIL-*` codes, `GraphRef`, `Diagnostics` |
| `canonical.rs` | canonical JSON, the fingerprint, the seeded PRNG |
| `limits.rs` | `RenderLimits`, `NormalizeOptions` |
| `validate.rs` | the untrusted-input boundary |

The dependency list is `rite-core`, `serde`, `serde_json`, `sha2`, `hex`, and
`tests/boundaries.rs` reads the manifest to prove it.

### Findings

**A comment explaining a rule can trip the test that enforces it.**
`crates/cant-cli/tests/boundaries.rs` scanned raw source text for `cant_sem` and
`cant::` to catch a Rite crate importing Cant. `rite-sigil` is full of doc
comments saying, by name, that it must not depend on `cant-sem` — which is ADR
0006 — so the boundary test fired on its own explanation. Fixed by stripping
comments before scanning, which is a correctness improvement: the test could not
previously tell an import from a sentence about one. `rite-sigil`'s own boundary
test assembles its needles with `concat!` so the file does not contain the
strings it forbids.

**`SIGIL-S006` (non-finite number) was written as a check and turned out to be
unreachable.** §6.4 asks for non-finite values to be rejected; `serde_json`
already does it, at a layer below. `1e400` fails to parse with "number out of
range", `NaN` and `Infinity` are not JSON, and `Number::from_f64` returns `None`
for both — so a non-finite value cannot be *constructed* in a `serde_json::Value`,
let alone deserialized into one. The branch was removed rather than kept: an
unreachable check wearing the appearance of a safety net is worse than no check,
because it invites trust. `non_finite_numbers_cannot_reach_validation_at_all`
pins the `serde_json` behaviour, so the omission becomes a real gap the moment
that stops being true. The code is retained for the scene bounds pass, where
`f64` arithmetic can genuinely produce one.

**Capability names are source text, and the model had to say so.** The first
version of `Capability` had a required `name: String`, and a fixture caught
`@fs.read` reaching the Codex of a scene built with labels off. A family (`fs`) is
a classification this renderer invented and layout cannot work without it; a name
is text the user wrote. They are now different fields with different
availability: `family` always, `name: Option<String>` only when labels were
requested. The privacy decision is made once, at the adapter, rather than filtered
out at each place that might display it.

### Deviations

**The adapter lives in `cant-sem`, not `cant`.** §5.1 permits either. `cant-sem`
owns `CantProgram`, and `cant`'s `native` feature pulls the runtime — putting the
adapter there would have made a browser build choose between the adapter and a
clean dependency graph.

---

## Phase 2 — scene model and semantic layout

**Status: complete.** Scene JSON only; no ornament, no SVG, as the phase requires.

`scene.rs` (the model), `analysis.rs` (topology), `layout.rs` (radial layout and
scene construction). Six golden fixtures under `fixtures/sigil/scenes/`, generated
from `examples/sigil/*.cant`.

The design and its reasoning are in [scene.md](scene.md).

### Findings

**Collision resolution destroyed orbit rings, and the fix was an ordering
decision rather than a tweak.** The collision pass nudges in polar coordinates
about the *canvas* centre. Applied to an orbit body member, that walks it
straight off the circle it is supposed to sit on — a ring member ended up 169
units from a ring of radius 52, and an orbit whose members are scattered *near* a
circle no longer says "this may go round again", which is the entire acceptance
criterion for S3.

Rings are now laid out **after** collision resolution, from the orbit node's
settled position, and settle as a unit. They need no separation pass of their own
because `ring_radius` sizes the circumference to hold them by construction. What
they can still do is overlap something outside the ring, and `report_ring_overlaps`
says so rather than leaving it silent — §11.7's last resort.

**`serde_json` does not round-trip every `f64` exactly, and this matters for
parity.** Writing `927.9171087042969` and reading it back yields
`927.9171087042968`. The writer emits the correct shortest representation; the
parser mis-rounds.

This was found by a fixture test asserting a scene equals itself after a JSON
round trip. It is load-bearing for acceptance criterion Q2: "native scene JSON
equals browser scene JSON" cannot be checked by deserializing both and comparing
structures, because deserialization is the lossy step. Comparisons compare
canonical text, produced from the live values on each side and never parsed back
— which is what `canonical.rs` was already for.
`float_round_trip_is_not_exact_which_is_why_text_is_canonical` pins it with the
concrete value.

**Depth had to be longest-path, not shortest.** A shortcut edge lets shortest-path
depth place a node at the same radius as its own predecessor, and two nodes at
the same radius on the same spoke overlap. Computed by bounded relaxation rather
than topological sort, so it stays total on a graph containing a non-feedback
cycle — which validation rejects, but which a caller using `analyze` directly
could still hand over.

### Deviations

**Fork branch sector boundaries are not drawn.** §9.8 permits lightly inscribed
boundaries and warns they must not resemble flow edges. In Phase 2 there is no
stroke vocabulary yet to make that distinction with, so branch membership is
expressed by where the members are. Revisit in Phase 3 with a real stroke
language.

**`SigilNodeKind::Literal` has no producer.** It is in the model because §7 lists
it and because a Rite projection will want it; nothing emits it today. It lays
out as ordinary flow.

**`--simplify`, `--max-nodes` and the CLI generally are Phase 3.** The limits
exist and are enforced; the flags that configure them do not.

### Test status

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --all-features --no-fail-fast` | **1442 passed, 0 failed, 6 ignored**, 143 binaries |
| `pnpm --dir apps/{rite-web,rite-studio,cant-web} typecheck` | clean |
| `rite docs build` idempotent | yes |

113 tests added since the pre-Sigil baseline of 1329.

### Measured performance

Release build, scene construction including JSON serialization:

| Nodes | Measured | §24 native target (scene + SVG) |
|---|---|---|
| 25 | < 1 ms | 25 ms |
| 100 | 4 ms | 100 ms |
| 500 | 46 ms | 1000 ms |

Scene JSON runs about 1.1 KiB per node. `crates/rite-sigil/tests/performance.rs`
measures rather than enforces, with assertions an order of magnitude loose: a
tight timing assertion on shared CI hardware fails for reasons unrelated to the
renderer, and a test that fails randomly gets disabled rather than investigated.

### Known limitations at the end of Phase 2

- No SVG, PNG, HTML, themes, ornament, marks, or disclosure modes. All Phase 3+.
- No `cant sigil` command. The library API exists; the CLI does not.
- No WASM crate and no web application.
- Marks are placeholders: `Geometry::Mark` carries centre, size and rotation with
  an empty `path`. Phase 3 fills it, which is why S1 and S5 are not `[x]`.
- Edge routing is a single bowed cubic. It minimizes nothing; §11.6's crossing
  minimization is not implemented, and on a dense graph traces will cross.
- Nested fork-inside-fork sectors subdivide by weight but are not recursively
  re-normalized, so deep nesting will get cramped before it gets illegible.
- `report_ring_overlaps` is O(n²) and runs on every render.

---

## Phase 3 — procedural marks and canonical SVG

**Status: complete except for inscriptions.** Marks, themes, the SVG serializer
and `cant sigil` have landed with their tests and goldens.

`marks.rs` (the constrained generator), `theme.rs` (three themes, contrast-gated),
`svg.rs` (the layered serializer). Six Veiled SVG goldens beside the scene ones,
in `fixtures/sigil/svg/`.

### The one finding that mattered

**A capability family's name leaked into a Veiled render's accessibility tree.**

`CapabilityFamily::Other(String)` carries whatever namespace the producer wrote.
`title_for` built an element title from `family.name()`, and for `Other` that
returns the producer's string — so a graph declaring a capability namespace of
`' onload='alert(1)` put that text into `<title>`, where a screen reader would
read it aloud from an artifact whose whole promise is that it shows nothing.

It got past the existing guard because that guard checked *labels*. A family
looked like renderer vocabulary — and for the nine known families it is. `Other`
is the one that is not, and it is the one nobody thinks about.

The fix is `CapabilityFamily::safe_name()`, which returns the family's own word
for the known nine and the fixed string `custom` for `Other`. The producer's
string is still available through `name()`, which is now documented as returning
user text, and it reaches the Codex where metadata mode governs it.

Found by the hostile-input matrix in `tests/svg_security.rs` — every disclosure
mode × every metadata mode × every theme, over twelve strings — which is the
argument for running that matrix rather than the default configuration.

### Deviations

**`--metadata full` does not embed source snippets yet.** The mode exists and
gates correctly, but nothing writes a metadata block, so `full` currently differs
from `safe` only in what it permits. The block arrives with the Codex in Phase 4.
`metadata_none_contains_no_label_snippet_or_identifier` is written so that it
keeps meaning what it means once data flows through.

**Inscribed and Revealed produce the same bytes as Veiled.** The serializer
honours them — it will not draw a `Text` element in Veiled mode and abbreviates
in Inscribed — but layout emits no `Text` elements at all, so there is nothing
for them to differ about. Inscriptions are the next thing to land, and D2/D3
stay unticked until they do.

**Themes are typed Rust constants, not a manifest.** `grammar/palette.json` is a
manifest because two independent implementations read it and drift is invisible.
Sigil has one renderer (ADR 0005), so a manifest would add a parse, a failure
mode, and a file to keep in sync with nothing on the other side.

### Test status

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --all-features --no-fail-fast` | **1484 passed, 0 failed, 6 ignored** |

40 tests added in this phase. The security suite alone renders 12 hostile strings
across 36 option combinations and asserts, on the bytes: no script, no event
handler attribute, no external reference, well-formed XML, no visible label in
Veiled, nothing in `metadata none`, and byte-identical repeat renders.

### A second finding, from the CLI

**A diagnostic's notes were never printed.** `SIGIL-S001` carries "try
`--simplify`, or use `cant graph` for a technical view" — the actionable half —
and `Display for SigilDiagnostic` rendered only the headline. A user over the
node cap got a number and no way forward. Caught by a CLI test asserting the
refusal names an alternative, which is the sort of thing a unit test on the
diagnostic type would have passed while the user-visible behaviour stayed broken.

### Known limitations at the end of Phase 3

- No PNG, no interactive HTML, no ornament, no Codex, no WASM, no web app.
- `--legend`, `--ornament`, `--orientation`, `--embed-graph`, `--embed-scene`,
  `--open`, `--height` and `--scale` are declared in the specification but not
  yet implemented; `cant sigil` rejects `--format png|html` explicitly rather
  than accepting and ignoring them.
- Inscriptions are not emitted, so two of three disclosure modes are inert.
- `--metadata full` embeds no snippets.
- The `void` and `parchment` themes exist and are contrast-checked but have no
  visual-regression coverage.

---

## Phase 4 — ornament, inscriptions, PNG, visual regression

**Status: complete except for the embedded metadata block.** Ornament,
inscriptions, PNG, interactive HTML and visual-regression tests have landed.

`ornament.rs` (four levels), inscriptions in `layout.rs`, `render_png` behind an
off-by-default feature, `tests/visual.rs`.

### Ornament is generated after placement, and that is the design

ADR 0004 requires ornament to be removable without relayout. The way to make
that true rather than careful is that `ornament::generate` takes the level and
the seed and **nothing from the placed scene** — it cannot avoid a node, because
it cannot see one. "Draw filigree in the gaps" would have needed to know where
the gaps are, and a gap depends on where the nodes landed, so an ornament that
avoided collisions would have made the semantic layout depend on the ornament
level through the collision pass.

`the_ornament_level_moves_no_semantic_geometry` asserts it over all four levels
against a graph with a fork, an orbit and an invocation in it — a linear chain
has no collisions to perturb, so it would have proved nothing.

### PNG is `rite-render`'s, and that was the point of the audit

`rite_render::svg_to_png` is arbitrary SVG to PNG — the part of that crate that
was never about Rite source, extracted for the Cant social card. Sigil calls it
behind an off-by-default `png` feature, the same pattern and for the same reason:
the browser build must not acquire a rasteriser and a font stack.

This is the reuse the Phase 0 audit was looking for and did not find in the
*rendering* model. The two renderers still share no abstraction — `rite-render`
draws highlighted text at column positions, Sigil draws layered geometry — and
forcing one would have distorted both. One audited function is the whole of it.

### The visual tests assert what a raster can actually answer

The first version asserted that the three themes produce distinct perceptual
hashes. They do not, and should not: a perceptual hash thresholds each cell
against the image's own mean, so it is blind to recolouring by construction —
which is exactly what makes it a good *composition* check. `neon-ritual` and
`void` draw the same shapes and hash identically.

So the raster asserts what only it can see: each theme's ground is the polarity
it claims (dark for two, light for parchment), each render has contrast in it
rather than being uniform, `maximal` ornament changes the picture without
burying it, and a graph twice the size is a different picture.

The PNG decoder and DEFLATE reader in that file are about two hundred lines and
exist so no dev-dependency is needed to read bytes this process just wrote.

### Findings

**`--mode revealed --metadata none` is contradictory, and now says so.** The two
axes are orthogonal by design, so the combination is meaningful — draw the
labels, embed nothing — but it is also what someone picks having confused "hide
it" with "do not embed it", and the artifact they get has their source written
across it. `cant sigil` warns (`SIGIL-C001`) rather than silently resolving it.

**The security checks had to learn where an attribute is.** Once inscriptions
existed, a label of `' onload='alert(1)` drew as escaped text containing the
literal ` onload=`, and a label of `javascript:alert(1)` drew as that string.
Both are inert — the quotes are escaped — but a whole-document substring search
cannot tell them from an attribute. The checks now scan only inside `<…>`, which
is where an attribute can exist. A security test that fires on the escaper
working correctly is one that gets deleted.

**A duplicated CSS class.** Ornament's layer class and its semantic class are
both `sigil-ornament`, so every ornament element carried
`class="sigil-ornament sigil-ornament"`. Valid, and visibly sloppy in an artifact
meant to be looked at.

### A third finding, from the HTML Codex

**The Codex leaked capability names under `--metadata none`.** The label was
gated on the metadata mode; the `touches` line one row below it was not, and a
legend entry's capability list carries the *name* — `@fs.read` — whenever the
graph carried one. So `none` kept the user's text out of one line and let it
back in through the next.

This is the third leak in the same family: the family name in a `<title>`
(Phase 3), the capability name in the graph model (Phase 1), and now this. Each
was a place where something that *looked* like renderer vocabulary was actually
the producer's string. The pattern is worth naming: whenever a field can hold
either, the gate has to be on the field, not on the concept.

### Known limitations at the end of Phase 4

- `--metadata full` still embeds no snippets; there is no metadata block.
- Inscribed abbreviates labels but has no abbreviated *capability* marks.
- No WASM, no web app, no Codex UI.
- `--legend`, `--orientation`, `--embed-graph`, `--embed-scene`, `--open` are
  specified and unimplemented.

---

## Phase 5 — WASM and the web application

**Status: foundation complete.** `cant-sigil-wasm`, `apps/sigil-web`, and a
parity gate. No browser test suite, no Cloudflare configuration.

### The crate is `cant-sigil-wasm`, and the specification says otherwise

`.internal/sigil_mvp.md` §5 names it `rite-sigil-wasm`; §5.1 has it depending on
"pure Cant syntax/graph crates". Both cannot hold in this repository: ADR 0001
fixes the dependency edge as `cant-* -> rite-*` and
`crates/cant-cli/tests/boundaries.rs` enforces it **by directory name**, so a
`rite-*` crate importing `cant-sem` fails the build.

Rendering pasted Cant source means parsing it, so the binding genuinely depends
on Cant. The naming rule predates the spec, is mechanically enforced, and encodes
something real — deleting Cant leaves a Rite that builds — so the name gave way
rather than the rule. `rite-sigil`, the renderer, stays Cant-free and Rite-side,
which is the boundary that actually carries the architecture. The crate is now in
`CANT_PATHS`: it cannot outlive Cant, and says so.

Found by the boundary test, not by review. This is the first place the
specification and the repository genuinely conflicted.

### Three bugs found by using it

**The metadata mode rotated the composition.** `--metadata full` asks the adapter
for labels; labels were part of the graph fingerprint; the fingerprint is the
default seed; the seed is the rotation. Asking for more *embedded* metadata
silently redrew the picture. Snippets had been excluded from the fingerprint for
exactly this reason and labels never were — inconsistent with the project's own
rule that nothing may infer meaning from a label. Labels, short labels and
capability names are now excluded; a capability's *family* stays, because it
decides which mark a node gets.

**A Veiled sigil could not have a full Codex.** Labels were carried only when the
artifact would draw them, so Veiled produced an empty decoder — while §13.1 says
a Veiled render may be decoded through the Codex, with Deep Veil to suppress it.
Tying the two together made the intended default unaskable. Labels now travel
unless `metadata none` forbids them; the picture stays clean because the
serializer never generates a text element in Veiled mode, which was already true
and is the guard doing the work.

This also corrected a test that asserted "a veiled render returns no source text
anywhere". That was the wrong guarantee. Veiled is about the **artifact**;
`metadata none` is the setting that means nothing anywhere. Both are now
asserted, including that the scene *does* keep labels — otherwise there is
nothing to decode.

**The composition used a corner of the circle.** Three causes, all now fixed.

Two were arithmetic: the spine divided by its length rather than length minus
one, so a chain of three covered two thirds of the sweep — the gap grew as the
program got *shorter*, which is backwards — and its radius came from whole-graph
depth, so a program with deep branches gave its own backbone tiny fractions and
bunched it near the centre while the branches spread past it.

The third was structural and is the one that mattered. The spine allocated an
angular slot to *every* spine node and the placement pass then moved some of them
elsewhere — an invocation to the outer boundary, an exit to the seal. Those slots
stayed empty. On `complex.cant`, three of five spine nodes move, so most of the
circle was reserved for marks that were never going to be there while the
survivors crowded together. The sweep is now divided over the nodes that will
still be on the spiral; a relocated node borrows a position between its
neighbours, so its spoke still points back along the flow it came from, without
consuming a slot.

Worth noting how it was found: not by a test, which had no way to say "this
looks empty", but by rendering one and looking at it. The tests said everything
was deterministic, bounded, and in the right band — all true, and none of it the
question being asked.

### Known limitations at the end of Phase 5

- Parity is asserted between two *native* calls of the same functions. It does
  not compare a browser-executed WASM build against a native fixture; that
  belongs in an E2E suite which does not exist.
- No component tests, no E2E tests, no accessibility audit of the app.
- No CI job builds the site.
- No Cloudflare configuration, no `/api/*` endpoints.
- No gallery, no local persistence, no selection/path highlighting.
- The app surfaces no accessible structured summary of its own; the scene has one.

---

## Phase 6 — interaction, in part

Selection with upstream-and-downstream path highlighting, Codex synchronisation
in both directions, Escape to clear, and the screen-reader summary in a live
region.

### Two silent failures, both about names

**The scene serializes `graph_ref`; the app read `graphRef`.** Only the WASM
boundary types use camelCase — the scene's own types keep their Rust field names
— and the mismatch fails quietly: no edges parse, so a selection lights only
itself. It looks like a highlighting bug rather than a naming one, and it would
have been easy to "fix" by loosening the matching instead.

**Which is what the first version did.** Falling back to
`elementId.includes(nodeId)` meant selecting `n1` also lit `n10` and `n11`, so
the selection lit the whole picture — and *looked* like it was working. Matching
is now exact: nodes by identity, edges only when both endpoints are in the
selection, so a trace to something outside does not read as part of the path.

Both were found by measuring in a real browser rather than by reading the code.
The lit-element counts are what showed it: 3 of 22 was too few, then 22 of 22 was
too many, and only 7-to-10 varying by node is right.

### The rest of Phase 6, and Phase 7's gallery

Mobile sheets, the gallery, an export preview, and eleven component tests under
`vitest`.

**The gallery renders live rather than baking thumbnails.** §20.8 asks for cards
generated from repository fixtures so they cannot drift. The sources are read
from `examples/sigil/` by Vite at build time; the pictures are produced in the
browser by the same engine the canvas uses. That is stronger than baking, not
weaker — a baked image can go stale against the renderer that made it, and one
rendered on the spot cannot.

**Seven inline SVGs on one page is seven copies of every element ID.** The
gallery duplicated `id="sigil-glow"` and every `id="node-…"`, which is invalid
HTML and means an `id` selector anywhere in the app could match a thumbnail
instead of the canvas. Fixed by rendering cards with `metadata: "minimal"`, which
emits no identifiers and no titles — a decorative card needs neither: it is
`aria-hidden` and its accessible name is the caption beside it.

The tests are about the chamber, not the engine: which control does what, which
panel collapses, what an export preview claims, and that nothing calls `fetch`.
The renderer is mocked, because what it does is tested in Rust where the
assertions can be exact.

### Known limitations

- No E2E suite over a *built* site. The privacy assertion spies on `fetch` in a
  component test, which is real but narrower than the criterion asks.
- No mobile-viewport test; the sheets are asserted by class, not by rendering.
- Examples are not generated as CI artifacts (Q9) — they are exercised by the
  fixture tests and shown in the gallery, which is not the same thing.
- Selection parses edge endpoints out of the element id — see OD4.
- No local persistence toggle.

---

## Phase 8 — Cloudflare, and Phase 9's fuzz layer

Worker, headers, endpoints, a CI job, and property tests over generated graphs.
Written but **not deployed**: the configuration is complete and dry-runs clean,
and no zone has been attached.

### Proving a negative about the Worker

The most important property of the Worker is one it does not have: an endpoint
that accepts a program. A behavioural test cannot show that — you cannot call a
route that is not there and learn anything — so `tests/worker.test.ts` reads the
Worker's own source and fails if it grows a request-body read, a `/api/render`
route, or an import of the renderer.

That is an unusual shape for a test and it is the right one here. ADR 0007's
privacy claim is architectural rather than procedural — there is nothing to
misconfigure because there is nothing there — and the way that stops being true
is somebody adding a convenience endpoint, which is exactly what reading the file
catches.

### `wasm-unsafe-eval` is required; `unsafe-eval` is not

Instantiating WebAssembly counts as evaluation to CSP, so a policy without
`wasm-unsafe-eval` means the renderer never starts. `unsafe-eval` is a strictly
larger grant and is not made. The test asserts both — including that
`unsafe-eval` does not appear other than as part of `wasm-unsafe-eval`, which a
naive substring check would have missed in the direction that matters.

One concession: `style-src 'unsafe-inline'`, because Vite inlines a small amount
of CSS and the asset pipeline does not hash it for us. Recorded in
`docs/sigil/deployment.md` rather than left as an unexplained looseness.

### OD4 closed

`SceneElement::ends` carries an edge's endpoints as fields. The app was parsing
them back out of the element *identifier* with a regular expression, which worked
only because the Cant adapter happens to build ids that way — a string format as
a structural dependency, and one that would have failed silently the moment edge
naming changed.

### The fuzz layer is `proptest`, not `cargo-fuzz`

The input is structured, so a generator that builds graphs reaches the
interesting shapes far more often than one mutating bytes, and it runs in the
ordinary test suite without a second toolchain. Seven properties over generated
graphs: no panic, finite coordinates, determinism, ornament invariance, no markup
injection, veiled draws no text, and the caps hold.

A coverage-guided target over the JSON *reader* is a different question and is
still absent — that is the one place byte-level mutation would earn its keep, and
Q5 stays partial because of it.

### Known limitations

- **Not deployed.** No zone attached, so CF2 and CF3 are configuration that has
  never been exercised against Cloudflare.
- No `cargo-fuzz` target over the graph JSON reader.
- No cross-platform CLI test run (C4), no browser matrix, no accessibility audit.
- `visual-language.md`, `cli.md`, `themes.md`, `accessibility.md` and
  `internals.md` are unwritten.

### Next milestone

Deploy, which is the only way CF2 and CF3 stop being claims. Then the remaining
documentation and the three open design items — edge-crossing minimization (OD2)
and recursive fork-sector renormalization (OD3) are the two that still show in
the picture.
