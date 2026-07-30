/**
 * Build-time highlighting tables, read from the language's own sources.
 *
 * Lives here rather than in either app because both the product site and the
 * standalone Studio build need the same defines, and Studio must not depend on
 * rite-web — rite-web imports Studio's sources, not the other way round.
 *
 * The tables come from grammar/ and from the capability manifest the CLI
 * generates, so adding a keyword or a host function highlights it in both apps
 * without anyone retyping a list. `cargo test -p rite-cli --test
 * editor_grammar_sync` holds those same sources to the lexer and the registry.
 */
import fs from "node:fs";
import path from "node:path";

const REPO_ROOT = path.resolve(import.meta.dirname ?? __dirname, "../../..");

const repoFile = (rel: string) => fs.readFileSync(path.resolve(REPO_ROOT, rel), "utf8");

/** `keywords = [ "def", … ]` out of the TOML, without a TOML dependency. */
function tomlStringArray(toml: string, key: string): string[] {
  const body = toml.match(new RegExp(`^${key}\\s*=\\s*\\[([^\\]]*)\\]`, "m"))?.[1];
  if (!body) throw new Error(`grammar: no ${key} array found`);
  return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

export type RiteGrammar = {
  keywords: string[];
  softKeywords: string[];
  glyphs: string[];
  capabilities: string[];
  capabilityFns: string[];
};

export function riteGrammar(): RiteGrammar {
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
    // Single-character sigils only; multi-char ASCII twins match as operators.
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
