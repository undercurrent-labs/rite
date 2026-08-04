# Accessibility

A sigil is a picture on purpose, so every claim here is about what exists
*besides* the pixels. The requirements are §23's; this page is where each one
actually lives.

## The accessible summary

Every scene carries a generated sentence — "This sigil contains one source,
two stages, two wards, one scatter, one collect, one fork, one orbit, and one
invocation." — built from the census of what was actually drawn, in the visual
grammar's centre-outward order. It is:

- the `aria-live` status region in the app, updated per render;
- the `<title>` of an HTML export;
- `SigilScene::summary()`, for any other consumer.

Generated from the drawn census rather than the source, so it cannot describe
a construct the picture does not contain.

## Titles never carry a label

Every mark's `<title>` — what a screen reader speaks on focus — is the node's
*kind* ("orbit", "fs invocation"), never its label. A title carrying source
text would put that text in a Veiled render's accessibility tree, which is
exactly the leak ADR 0007 separates disclosure from metadata to prevent. The
same rule gates capability names: an unknown capability family is spoken as
its safe name, never as the producer's raw string.

## Keyboard

- Every node in the app is focusable (`tabindex`), in **graph order** — the
  tab sequence follows the program, not the accident of where marks landed.
- Enter or Space selects; Escape clears the selection and any tooltip.
- Hit regions are never smaller than a comfortable target (22px radius),
  whatever the drawn mark's size — a hairline is not a keyboard target.
- Focus is a visible ring everywhere, because the browser default disappears
  against a near-black ground.

## Revelation without hover

Hover and keyboard focus reveal the same tooltip, so nothing depends on a
pointer. On narrow screens the side panels become sheets; nothing in the app
requires hovering at all. **Deep Veil** suppresses hover/focus revelation
deliberately — it is a privacy control, and it leaves the Codex's *kinds*
readable (§13.1).

## Motion

Every animation in the app — the render materialisation, the pulse on the
progress indicator — collapses to nothing under `prefers-reduced-motion`. The
artifact itself never animates.

## Where it is tested

Component tests assert the live region updates, panels collapse, and Deep Veil
keeps kinds while dropping labels; `svg_security.rs` asserts titles survive
hostile input without leaking it; hit-region sizing is asserted in the scene
fixtures (`hit_regions.len() == node_count`, floor on radius).
