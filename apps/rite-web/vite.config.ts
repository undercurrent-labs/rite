import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import fs from "node:fs";
import path from "node:path";

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

const repoFile = (rel: string) =>
  fs.readFileSync(path.resolve(__dirname, "../..", rel), "utf8");

/** `keywords = [ "def", … ]` out of the TOML, without a TOML dependency. */
function tomlStringArray(toml: string, key: string): string[] {
  const body = toml.match(new RegExp(`^${key}\\s*=\\s*\\[([^\\]]*)\\]`, "m"))?.[1];
  if (!body) throw new Error(`grammar: no ${key} array found`);
  return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/**
 * Highlighting tables, read from the language's own sources at build time.
 *
 * The site used to have no highlighter at all; the temptation with a new one is
 * to retype the keyword and sigil lists into the frontend, where they rot. The
 * VS Code grammar shows how that ends — its capability list is still missing
 * `csv` and `db`. These come from grammar/ and from the capability manifest the
 * CLI generates, so adding a keyword or a host function highlights it here too.
 */
function riteGrammar() {
  const keywordsToml = repoFile("grammar/keywords.toml");
  const aliases = JSON.parse(repoFile("grammar/aliases.json")) as {
    aliases: Record<string, { ascii: string; glyph: string }>;
  };
  const capabilities = JSON.parse(repoFile("skills/rite/machine/capabilities.json")) as Record<
    string,
    { name: string; permission: string }[]
  >;

  const glyphs = new Set<string>();
  for (const { glyph } of Object.values(aliases.aliases)) {
    // Single-character sigils only; multi-char ASCII twins are matched as operators.
    if ([...glyph].length === 1) glyphs.add(glyph);
  }

  const capabilityFns = new Set<string>();
  for (const fns of Object.values(capabilities)) {
    for (const fn of fns) capabilityFns.add(fn.name);
  }

  return {
    keywords: tomlStringArray(keywordsToml, "keywords"),
    softKeywords: tomlStringArray(keywordsToml, "soft_keywords"),
    glyphs: [...glyphs],
    capabilities: Object.keys(capabilities),
    capabilityFns: [...capabilityFns],
  };
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
