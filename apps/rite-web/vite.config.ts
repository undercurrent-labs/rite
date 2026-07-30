import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import fs from "node:fs";
import path from "node:path";
import { riteGrammar } from "../rite-studio/build/grammar";

/**
 * Single source of truth for the version the site advertises: the workspace
 * manifest. Hardcoding it here drifted to three different numbers once already.
 */
function workspaceVersion(): string {
  const manifest = fs.readFileSync(path.resolve(__dirname, "../../Cargo.toml"), "utf8");
  const section = manifest.split(/^\[workspace\.package\]$/m)[1];
  const found = section?.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
  if (!found) throw new Error("could not read [workspace.package] version from Cargo.toml");
  return `v${found}`;
}

export default defineConfig({
  define: {
    __RITE_VERSION__: JSON.stringify(workspaceVersion()),
    // Only advertise the VSIX mirror when the pipeline actually put one here.
    __HAS_VSIX__: JSON.stringify(
      fs.existsSync(path.resolve(__dirname, "public/vscode/rite.vsix"))
    ),
    __RITE_GRAMMAR__: JSON.stringify(riteGrammar()),
  },
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
