/** @type {import('tailwindcss').Config} */

/*
 * Sigil's palette is Rite's, with the accent moved — the same rule Cant follows.
 *
 * Background, panel, card, muted and border are byte-identical to
 * apps/rite-web/tailwind.config.js and apps/cant-web/tailwind.config.js, so the
 * three sites read as one family. The accent is `keyword` from
 * grammar/palette.json — Rite took capability cyan, Cant took glyph pink, and
 * the violet is the remaining note in the same register.
 *
 * The chrome and the artifact are deliberately *not* the same palette. The
 * canvas is a stage: it keeps the renderer's own neon-ritual ground
 * (crates/rite-sigil/src/theme.rs) so the artifact sits in its own colour
 * world, framed by chrome that belongs to the family.
 */
export default {
  content: ["./index.html", "./src/**/*.{vue,ts}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        sigil: {
          bg: "#0b0f14",
          panel: "#121821",
          card: "#161d28",
          // grammar/palette.json `keyword`.
          accent: "#c792ea",
          cyan: "#7ee0ff",
          pink: "#ff7edb",
          green: "#c3e88d",
          amber: "#ffcb6b",
          muted: "#8b9bb4",
          border: "#1e293b",
        },
        // The stage and what sits on it, from the renderer's neon-ritual theme.
        abyss: "#05030A",
        spectral: "#EDEBFF",
        glow: "#38F2FF",
        gold: "#D8B35C",
        ember: "#FF6B4A",
      },
      fontFamily: {
        sans: ['"DM Sans"', "system-ui", "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "Menlo", "monospace"],
      },
    },
  },
  plugins: [],
};
