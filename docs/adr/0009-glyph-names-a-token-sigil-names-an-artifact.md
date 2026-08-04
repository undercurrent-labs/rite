# ADR 0009 — Glyph names a token; Sigil names an artifact

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** nothing
- **Related:** [ADR 0003 — Sigil is a semantic renderer, not a runtime](0003-sigil-is-a-renderer-not-a-runtime.md) ·
  [ADR 0001 — Cant is a sibling front end](0001-cant-sibling-frontend.md)

## Context

"Sigil" already means something in this repository, and it means the wrong thing.

`grammar/sigils.toml` is the table of Rite's token symbols — `◆` is a sigil, `←`
is a sigil, `⟦` is a sigil. The word spread from there. `rite_render::Kind::Sigil`
is a highlighter token class. `grammar/palette.json` has a `"sigil"` colour with
a glow. `apps/rite-studio/src/highlight.css` styles `.tok-sigil` and turns its
glow off under `prefers-reduced-motion`. `rite_fmt` has a private `fn sigil`.
`rite_syntax` has `try_sigil`. `rite doc` emits a section with `id: "sigils"` —
titled, already, "Glyph / ASCII syntax table".

That last one is the tell. The repository had already started saying *glyph* for
this concept and never finished. `grammar/aliases.json` is the canonical
concept→ASCII→glyph table and calls the column `glyph`. `rite describe syntax` is
documented as "Glyph and ASCII spellings for every construct". `cant convert --to
glyph`, `cant_syntax`'s `Dialect::Glyph`, and `rite_fmt::Dialect::Glyph` all use
it. Two words are in service for one concept, and the one with fewer users is
about to be needed for something else entirely.

Because Sigil — the renderer — is a visual artifact generated from a whole
program's semantic topology. If `◆` is also a sigil, then "the sigil for this
program" and "the sigil in this program" are different kinds of thing said the
same way, in the same docs, about the same language. That is not a naming
preference; it makes sentences in the documentation unparseable.

## Decision

**"Glyph" is a visual spelling of one language token or operator. "Sigil" is the
visual artifact generated from a program's semantic graph, and the renderer that
produces it. Neither word may be used for the other's meaning.**

The migration, which changes no syntax and no semantics:

1. `grammar/sigils.toml` → `grammar/glyphs.toml`, with `[[sigil]]` → `[[glyph]]`
   and `canonical` retained. The file is documentation-only — nothing in the
   workspace reads it, which is why the rename is safe and also why it had drifted
   from the vocabulary everything else uses.
2. `grammar/palette.json`: the `"sigil"` token kind → `"glyph"`. Colour and glow
   are unchanged, so no rendered output changes colour.
3. `rite_render::Kind::Sigil` → `Kind::Glyph`. This is a public enum in a
   published crate; the variant is renamed rather than deprecated, because
   `rite-render`'s only consumers are in this workspace.
4. `.tok-sigil` → `.tok-glyph` in `apps/rite-studio/src/highlight.css`, and the
   `"sigil"` token kind → `"glyph"` in `apps/rite-studio/src/highlight.ts`.
   `crates/rite-cli/tests/palette_sync.rs` requires all three to agree, so a
   partial rename fails CI rather than producing unstyled tokens.
5. Private helpers follow: `rite_fmt`'s `fn sigil` → `fn glyph`, `rite_syntax`'s
   `try_sigil` → `try_glyph`.
6. `rite doc`'s section `id: "sigils"` → `"glyphs"`, and its `.sigil` CSS class →
   `.glyph`.
7. Prose in `README.md`, `docs/book/`, `docs/cant/`, the CLI's own help text, and
   the app views uses *glyph* for a token symbol. `docs/generated/cli.md` and the
   agent bundle are regenerated, because CI fails if generation rewrites a
   tracked file.
8. From here, `Sigil` capitalized is the renderer and its artifacts; `sigil`
   lowercase in a Rite or Cant context is a bug.

What is explicitly **not** changed: `rite-syntax` token names, `grammar/aliases.json`,
`grammar/rite.ebnf`, the lexer's behaviour, the formatter's output, or any
spelling a user types. This is a rename of words about the language, not of the
language.

## Consequences

**Good.** One concept, one word, and it is the word the majority of the codebase
was already using. `rite describe syntax` no longer describes "glyph spellings"
under a manifest called `sigils.toml`.

**Good.** The renderer gets an unambiguous name before it has any users, which is
the only cheap time to do this.

**Cost.** A public enum variant changes name, and `grammar/sigils.toml` is a path
that could appear in someone's tooling. Both are in-repo-only in practice — the
manifest has no reader at all and `rite-render` is not depended on outside this
workspace — but the rename is a breaking change on paper and belongs in the
changelog.

**Cost.** A CSS class in a shipped stylesheet changes. Anyone with a user
stylesheet targeting `.tok-sigil` loses it. Weighed against the confusion, and
the class is an implementation detail of Studio's highlighter rather than a
documented extension point.

**Risk accepted.** "Sigil" survives in `.internal/sigil_mvp.md`, in the historical
prose of `CHANGELOG.md`, and in the doc comments of `cant_sem` that anticipated
"a future Sigil renderer" — where it already meant the artifact and was correct.
Those are left alone. A grep for the word will therefore still return hits, and
what matters is that none of them mean *token symbol*.

## Alternatives rejected

**Keep both meanings and disambiguate by context.** Rejected. The two meanings
collide most densely in exactly the documents that have to explain both — the
visual-language docs, which describe how a program's *glyphs* become a *sigil*.

**Name the renderer something else and leave the token terminology alone.**
Rejected. The specification names the product Sigil throughout, the domain is
`sigil.rite.foo`, and the product identity — a program's semantic topology as a
ritual seal — is precisely what the word means. The token usage is the one that
was borrowed loosely, and it already had a better name available.

**Deprecate `Kind::Sigil` with a type alias and remove it in a later release.**
Rejected: `rite-render` is not published outside this workspace, so the alias
would exist purely to be deleted, and a workspace where both spellings compile is
how a half-finished rename becomes permanent.
