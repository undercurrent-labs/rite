# ADR 0007 — Veiled rendering and source privacy are first-class

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0003 — Sigil is a semantic renderer, not a runtime](0003-sigil-is-a-renderer-not-a-runtime.md) ·
  [ADR 0005 — One renderer, in Rust](0005-one-renderer-in-rust.md)

## Context

The product claim is that a user can render a program they are not willing to
publish, hide every label, and share the result. Two separate promises are folded
into that sentence, and they fail in different ways.

**The artifact must not leak.** A "Veiled" SVG with the function names still
sitting in `<title>` elements, a `data-label` attribute, or an embedded metadata
block is not veiled. It looks veiled, which is worse than not being veiled at
all, because the user shared it believing something false. Visible labels and
embedded metadata are independent axes and treating them as one setting is the
bug.

**The source must not travel.** The hosted app renders in the browser (ADR 0005),
so there is no technical need to send anything anywhere. But there are half a
dozen ordinary web-app conveniences that leak source without anyone deciding to:
a shareable permalink that base64s the program into the URL — where it lands in
history, in the Referer header, and in any analytics script; an error reporter
that attaches the editor buffer; a "recent programs" list synced for convenience;
a server-side PNG rasterizer added because canvas export was fiddly.

Veiled mode is also the *default* artistic mode, which means the failure is not
hypothetical for one careful user — it is the path everyone takes.

## Decision

**Disclosure and metadata are separate, enforced, tested policies; and source
never leaves the browser.**

### Disclosure is not metadata

1. `--mode <veiled|inscribed|revealed>` controls **visible** text. `--metadata
   <full|safe|minimal|none>` controls **embedded** data. They are orthogonal and
   neither implies the other.
2. `veiled` renders no source labels, function names, capability names, node IDs,
   or runtime-looking annotations, and no visible legend. Only marks and geometry.
3. `metadata none` removes source labels and snippets entirely — from `<title>`,
   from `<desc>`, from any metadata block, and from every attribute. What remains
   is valid SVG structure.
4. `metadata safe` — the default — carries semantic kinds and stable IDs but **no
   source snippets**. It is the default because accessibility (§23) needs
   `<title>` to exist, and a semantic kind is not the user's source.
5. These are asserted, not asserted-to. The golden SVG suite fails if a veiled
   render contains visible label text, and if a `metadata none` render contains
   any source snippet. A malicious-label fixture set proves user text cannot
   escape into markup.
6. `Deep Veil` in the web app suppresses interactive revelation as well —
   hover and focus tooltips included. A veil that any mouseover removes is a
   presentation choice, not a privacy control, and both must exist.

### Source stays on the machine it was typed on

7. No server render endpoint exists, and none may be added. The Worker serves
   static assets plus `/api/health`, `/api/version`, `/api/schema`, all of which
   are read-only and take no body.
8. No source, graph JSON, label, snippet, filename, or exported artifact is
   transmitted, logged, or persisted remotely — including via analytics. There is
   no analytics in v0; if any is added later the same prohibition binds it.
9. **No source in URLs.** No permalink, no query parameter, no hash fragment
   carrying a program. Sharing a render means exporting a file, which is the
   operation whose privacy the user can actually see.
10. Nothing is persisted by default. Local-only persistence is opt-in, explicitly
    labelled, clearable, and never synced.
11. An end-to-end test asserts the negative: exercising the app — load, edit,
    render, switch modes, export — issues **no** network request carrying source.

## Consequences

**Good.** "Veiled" means something a test can check, so it stays true as the
renderer changes. The two-axis split is what makes "unlabelled but accessible"
and "unlabelled and inert" both expressible, which one setting could not do.

**Good.** The privacy claim is architectural rather than procedural. There is no
endpoint to misconfigure, because there is no endpoint.

**Cost.** No share links. This is the most user-visible cost and it is a real
feature the product does not get — sending someone a render means sending them a
file. It is the direct consequence of requirement 9 and it is deliberate.

**Cost.** Export is harder. PNG must be produced in the browser rather than by
asking a server to rasterize, so the web app carries a rasterization path the
CLI does not need.

**Cost.** Metadata modes multiply the golden-test matrix: every disclosure mode
against every metadata mode, per theme. Mitigated by asserting *properties*
(no label text present, no snippet present) over the full matrix and keeping byte
goldens to a canonical subset.

**Risk accepted.** Determinism is in tension with privacy. A render fingerprint
derived from the graph is, by construction, a stable identifier for a program:
two people holding the same "anonymous" sigil can tell they hold the same
program. Sigil does not claim to be steganography — §3.2 lists that as a
non-goal — and the documentation says so plainly rather than implying an
anonymity the artifact does not provide. `--metadata none` removes the
fingerprint from the file; it cannot remove the geometry, which is the picture.

## Alternatives rejected

**One `--mode` flag controlling visible text and metadata together.** Rejected:
the two useful configurations are "no visible labels but accessible titles" and
"no visible labels and nothing embedded", and a single axis cannot express both.
This is the specific failure the split exists to prevent.

**Veiled as a CSS/visibility layer over a fully-labelled SVG.** Rejected outright.
The text would still be in the file, findable with a text editor by anyone the
user sent it to. Veiled must mean *not generated*, not *not displayed*.

**Encrypted or steganographic embedding, so the Codex can decode an artifact
without the source.** Rejected: it is a listed non-goal, it invites a security
claim the project cannot back, and the honest version — export the scene JSON
alongside the SVG — already exists.

**Permalinks with the program in the fragment, since a fragment is not sent to
the server.** Rejected. Fragments land in browser history, in shoulder-surfing
range, in bookmark sync, and in anything that reads `location.href` — and the
distinction is far too subtle to be the thing standing between a user and
publishing their source.
