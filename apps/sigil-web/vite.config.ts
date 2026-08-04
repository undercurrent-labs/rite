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

/**
 * What the `/api/*` endpoints report.
 *
 * Read from the crates rather than restated, so a version the Worker advertises
 * cannot disagree with the renderer that is actually compiled into the page.
 * The commit is whatever CI put in the environment; absent locally, and the
 * endpoint says `unknown` rather than inventing one.
 */
function buildInfo() {
  const cantSem = fs.readFileSync(path.join(repoRoot, "crates/cant-sem/src/lib.rs"), "utf8");
  const cantGraph = cantSem.match(/GRAPH_SCHEMA_VERSION: &str = "([^"]+)"/)?.[1] ?? "unknown";
  const sigil = fs.readFileSync(path.join(repoRoot, "crates/rite-sigil/src/graph.rs"), "utf8");
  const sigilGraph = sigil.match(/GRAPH_SCHEMA_VERSION: u32 = (\d+)/)?.[1] ?? "unknown";
  const scene = fs.readFileSync(path.join(repoRoot, "crates/rite-sigil/src/scene.rs"), "utf8");
  const sceneSchema = scene.match(/SCENE_SCHEMA_VERSION: u32 = (\d+)/)?.[1] ?? "unknown";

  return {
    commit: process.env.GITHUB_SHA ?? process.env.COMMIT_SHA ?? "unknown",
    renderer: sigilVersion(),
    schemas: {
      "cant.graph": [cantGraph],
      "rite.sigil.graph": [Number(sigilGraph)],
      "rite.sigil.scene": [Number(sceneSchema)],
    },
  };
}

/**
 * The Cant operator vocabulary, read from `grammar/cant/operators.toml` at
 * build time — the same file `cant-syntax` and the Cant site read, parsed with
 * the same restricted-TOML reader the Cant site uses. The source panel's
 * highlighter is driven by this, so it colours exactly the operators the lexer
 * recognises rather than a hand-listed copy that can drift.
 */
function cantOperators(): { ascii: string; glyph: string | null }[] {
  const text = fs.readFileSync(path.join(repoRoot, "grammar/cant/operators.toml"), "utf8");
  const operators: { ascii: string; glyph: string | null }[] = [];
  let current: Record<string, string | boolean> | null = null;

  const stripComment = (line: string): string => {
    let inString = false;
    for (let i = 0; i < line.length; i++) {
      if (line[i] === '"') inString = !inString;
      else if (line[i] === "#" && !inString) return line.slice(0, i);
    }
    return line;
  };

  const push = () => {
    if (!current) return;
    if (typeof current.ascii !== "string") {
      throw new Error("grammar/cant/operators.toml: operator missing `ascii`");
    }
    operators.push({
      ascii: current.ascii,
      glyph: typeof current.glyph === "string" ? current.glyph : null,
    });
    current = null;
  };

  for (const raw of text.split("\n")) {
    const line = stripComment(raw).trim();
    if (!line) continue;
    if (line === "[[operator]]") {
      push();
      current = {};
      continue;
    }
    if (line.startsWith("[")) throw new Error(`unsupported table header: ${line}`);
    const eq = line.indexOf("=");
    if (eq < 0) throw new Error(`expected \`key = value\`, found: ${line}`);
    const key = line.slice(0, eq).trim();
    const rest = line.slice(eq + 1).trim();
    const value = rest === "true" ? true : rest === "false" ? false : rest.replace(/^"|"$/g, "");
    if (current) current[key] = value;
  }
  push();

  if (operators.length === 0) throw new Error("grammar/cant/operators.toml has no operators");
  return operators;
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
    __SIGIL_BUILD__: JSON.stringify(buildInfo()),
    __CANT_OPERATORS__: JSON.stringify(cantOperators()),
  },
  plugins: [
    vue(),
    // `build-info.json`, for the Worker's `/api/version` and `/api/schema`.
    // An emitted asset rather than a `define`: the Worker is bundled by
    // wrangler and never sees Vite's constants — which is how the first deploy
    // answered version queries with an exception while the tests stayed green.
    {
      name: "sigil-build-info",
      generateBundle() {
        this.emitFile({
          type: "asset",
          fileName: "build-info.json",
          source: JSON.stringify({ app: sigilVersion(), ...buildInfo() }, null, 2),
        });
      },
    },
  ],
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
