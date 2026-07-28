/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts}",
    "../rite-studio/src/**/*.{vue,js,ts}",
  ],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        rite: {
          bg: "#0b0f14",
          panel: "#121821",
          card: "#161d28",
          accent: "#7ee0ff",
          pink: "#ff7edb",
          green: "#c3e88d",
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
