import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import fs from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(__dirname, "../..");

/**
 * The version the site advertises, read from the workspace manifest.
 *
 * Same reasoning as `apps/rite-web/vite.config.ts`: hardcoding it drifted to
 * three different numbers there once already, and a second site is a second
 * place for that to happen.
 */
function workspaceVersion(): string {
  const manifest = fs.readFileSync(path.join(repoRoot, "Cargo.toml"), "utf8");
  const section = manifest.split(/^\[workspace\.package\]$/m)[1];
  const found = section?.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
  if (!found) throw new Error("could not read [workspace.package] version from Cargo.toml");
  return `v${found}`;
}

/** The primary site's host, so the "sibling of Rite" links are never hardcoded. */
function riteHost(): string {
  const manifest = fs.readFileSync(path.join(repoRoot, "site.toml"), "utf8");
  const found = manifest.match(/^\s*primary\s*=\s*"([^"]+)"/m)?.[1];
  if (!found) throw new Error("could not read `primary` from site.toml");
  return found;
}

export type OperatorSpec = {
  concept: string;
  token: string;
  ascii: string;
  glyph: string | null;
  ambiguous: boolean;
  description: string;
};

/**
 * The operator vocabulary, read from `grammar/cant/operators.toml` at build time.
 *
 * The site renders the same table the lexer reads. A third hand-written copy of
 * twelve operators — after the manifest and the EBNF — is exactly the drift the
 * manifest exists to prevent, and a docs table that disagrees with the parser is
 * worse than no table.
 *
 * The restricted-TOML reader is the same subset `cant-syntax`'s reader accepts;
 * if this throws, the manifest gained syntax that the Rust side would also
 * reject, so failing the build is the correct outcome.
 */
function cantOperators(): OperatorSpec[] {
  const text = fs.readFileSync(path.join(repoRoot, "grammar/cant/operators.toml"), "utf8");
  const operators: OperatorSpec[] = [];
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
    const need = (key: string): string => {
      const value = current?.[key];
      if (typeof value !== "string") {
        throw new Error(`grammar/cant/operators.toml: operator missing \`${key}\``);
      }
      return value;
    };
    operators.push({
      concept: need("concept"),
      token: need("token"),
      ascii: need("ascii"),
      glyph: typeof current.glyph === "string" ? current.glyph : null,
      ambiguous: current.ambiguous === true,
      description: need("description"),
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
    const value =
      rest === "true" ? true : rest === "false" ? false : rest.replace(/^"|"$/g, "");
    if (current) current[key] = value;
  }
  push();

  if (operators.length === 0) throw new Error("grammar/cant/operators.toml has no operators");
  return operators;
}

export default defineConfig({
  define: {
    __CANT_VERSION__: JSON.stringify(workspaceVersion()),
    __RITE_HOST__: JSON.stringify(riteHost()),
    __CANT_OPERATORS__: JSON.stringify(cantOperators()),
  },
  plugins: [vue()],
  resolve: {
    alias: {
      "@cant": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    // Not 5173: the Rite site owns that, and running both at once is the normal
    // case while working on either.
    port: 5174,
    fs: {
      // docs/cant/*.md is imported as raw text from outside the app root.
      allow: [repoRoot],
    },
  },
});
