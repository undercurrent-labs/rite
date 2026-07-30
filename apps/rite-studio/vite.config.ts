import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { riteGrammar } from "./build/grammar";

export default defineConfig({
  define: {
    __RITE_GRAMMAR__: JSON.stringify(riteGrammar()),
  },
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:4041",
    },
  },
  build: {
    outDir: "dist",
    // public/wasm is copied as-is; load at runtime from /wasm/*
    assetsInlineLimit: 0,
  },
  // Ensure wasm MIME / assets from public are left alone
  optimizeDeps: {
    exclude: ["rite_wasm"],
  },
});
