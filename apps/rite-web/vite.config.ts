import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "node:path";

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      "@studio": path.resolve(__dirname, "../rite-studio/src"),
      "@web": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 5173,
    fs: {
      // Allow importing book markdown + studio sources
      allow: [path.resolve(__dirname, "../..")],
    },
    proxy: {
      "/api": "http://127.0.0.1:4041",
    },
  },
  build: {
    outDir: "dist",
    assetsInlineLimit: 0,
    emptyOutDir: true,
  },
  optimizeDeps: {
    exclude: ["rite_wasm"],
  },
});
