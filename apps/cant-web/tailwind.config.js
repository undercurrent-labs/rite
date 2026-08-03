/** @type {import('tailwindcss').Config} */

/*
 * Cant's palette is Rite's, with the accent moved.
 *
 * The two sites should read as siblings — same ground, same type, same spacing —
 * because that is what they are. What must not happen is a visitor mistaking one
 * for a section of the other, so the accent is the sigil pink already in Rite's
 * palette rather than a new colour: recognisably the same family, never the same
 * page. Background, panel, card, muted and border are byte-identical to
 * apps/rite-web/tailwind.config.js on purpose.
 */
export default {
  content: ["./index.html", "./src/**/*.{vue,js,ts}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        cant: {
          bg: "#0b0f14",
          panel: "#121821",
          card: "#161d28",
          // grammar/palette.json `sigil` — the colour Rite already draws its
          // glyphs in, which is the right note for a language made of them.
          accent: "#ff7edb",
          cyan: "#7ee0ff",
          green: "#c3e88d",
          amber: "#ffcb6b",
          muted: "#8b9bb4",
          border: "#1e293b",
        },
      },
      fontFamily: {
        sans: ['"DM Sans"', "system-ui", "sans-serif"],
        mono: ['"IBM Plex Mono"', "ui-monospace", "Menlo", "monospace"],
      },
      maxWidth: {
        prose: "68ch",
      },
    },
  },
  plugins: [],
};
