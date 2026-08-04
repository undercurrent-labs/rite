import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(__dirname, "../..");

/**
 * The version the app advertises, read from the crate that ships the renderer.
 *
 * Read rather than hardcoded, for the reason the other two sites read theirs: a
 * hardcoded number drifted to three different values on the Rite site once.
 */
function sigilVersion(): string {
  const manifest = fs.readFileSync(path.join(repoRoot, "crates/rite-sigil/Cargo.toml"), "utf8");
  const section = manifest.split(/^\[package\]$/m)[1] ?? manifest;
  const found = section.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
  if (!found) {
    throw new Error("could not read a literal version from crates/rite-sigil/Cargo.toml");
  }
  return `v${found}`;
}

/** A host from `site.toml`, so no domain is hardcoded in three places. */
function host(key: string): string {
  const manifest = fs.readFileSync(path.join(repoRoot, "site.toml"), "utf8");
  const found = manifest.match(new RegExp(`^\\s*${key}\\s*=\\s*"([^"]+)"`, "m"))?.[1];
  if (!found) throw new Error(`could not read \`${key}\` from site.toml`);
  return found;
}

/**
 * The built-in examples, read from `examples/sigil/` at build time.
 *
 * Generated from the repository rather than transcribed, so the gallery cannot
 * drift from the fixtures the tests use — §20.8's requirement, and the failure
 * it prevents is an example that stopped parsing months ago.
 */
function examples(): { name: string; source: string }[] {
  const dir = path.join(repoRoot, "examples/sigil");
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith(".cant"))
    .sort()
    .map((file) => ({
      name: file.replace(/\.cant$/, ""),
      source: fs.readFileSync(path.join(dir, file), "utf8").trim(),
    }));
}

export default defineConfig({
  // `vitest` reads this config, so the `define` block above is available to the
  // tests — which is what lets a component test use the same repository-read
  // examples the app does, rather than a transcribed copy that can drift.
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.ts"],
  },
  define: {
    __SIGIL_VERSION__: JSON.stringify(sigilVersion()),
    __RITE_HOST__: JSON.stringify(host("primary")),
    __CANT_HOST__: JSON.stringify(host("cant")),
    __SIGIL_EXAMPLES__: JSON.stringify(examples()),
  },
  plugins: [vue()],
  resolve: {
    alias: { "@sigil": path.resolve(__dirname, "./src") },
  },
  server: {
    // Not 5173 or 5174: the Rite and Cant sites own those, and running more
    // than one at a time is the normal case while working on any of them.
    port: 5175,
    strictPort: false,
  },
});
