# Themes

A theme decides colour, stroke weight and glow — never geometry. The same
scene renders in every theme; that is asserted, not assumed, and it is why a
theme can be versioned separately (`THEME_VERSION` joins every render
fingerprint as `theme=<name>@<version>`).

The palettes live in `crates/rite-sigil/src/theme.rs`, each field commented
with its role. Three ship:

## `neon-ritual` (default)

Dark void, cyan flow, magenta structure, gold seals, ultraviolet regions and
ornament, glow on. The theme the chamber at `sigil.rite.foo` wears, and the one
the app's own chrome is tuned around.

| Role | Colour |
|---|---|
| ground | `#05030A` |
| flow strokes | `#38F2FF` |
| structure (forks, scatters) | `#FF3CCF` |
| seals, output, boundary | `#D8B35C` |
| invocations, regions, ornament | `#8E5CFF` |
| warnings | `#FF6B4A` |

## `void`

Monochrome: white-on-black, no glow. Deliberately the theme where a mark that
only works in colour *fails* — every node kind must differ in topology, and
`void` is where that rule is enforced by eye and by the monochrome golden. Also
the cheap-to-rasterise theme: zero glow radius means no filter pass.

## `parchment`

Occult manuscript: warm paper ground (`#EFE3C8`), ink strokes, muted red
structure and gold seals, no glow. The print theme — what a sigil looks like
when it is going on a wall rather than a screen.

## Rules every theme obeys

- **Colour never carries semantics alone.** Kinds differ in shape first; theme
  colour is reinforcement. `void` existing is the proof obligation.
- **Contrast is checked**, not eyeballed — the theme tests assert every
  semantic colour clears its ground.
- **Background is separable.** `--background transparent` or a hex overrides
  the theme's ground without touching its strokes, for compositing.
- **Glow is presentation.** Zero it (as `void` does) and nothing moves; it is
  drawn as an SVG filter, not baked into geometry.

Adding a theme means a new `Theme` constant with every role filled, a bump of
`THEME_VERSION`, the distinctness and contrast tests extended, and — because
themes are named in three places — the CLI's `--theme` message, the app's
control bar, and this page.
