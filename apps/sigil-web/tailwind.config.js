/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{vue,ts}"],
  theme: {
    extend: {
      colors: {
        // The `neon-ritual` palette, so the chamber and the artifact in it agree.
        abyss: "#05030A",
        violet: "#0B0714",
        spectral: "#EDEBFF",
        cyan: "#38F2FF",
        magenta: "#FF3CCF",
        ultraviolet: "#8E5CFF",
        gold: "#D8B35C",
        ember: "#FF6B4A",
      },
      fontFamily: {
        mono: ["ui-monospace", "SFMono-Regular", "Menlo", "Consolas", "monospace"],
      },
    },
  },
  plugins: [],
};
