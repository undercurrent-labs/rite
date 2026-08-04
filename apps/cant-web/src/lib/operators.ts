/**
 * The Cant operator vocabulary, injected at build time from
 * `grammar/cant/operators.toml` — the same file `cant-syntax` reads.
 *
 * There is no hand-written copy of this table on the site. A docs table that
 * disagrees with the parser is worse than no table, and twelve operators across
 * a manifest, an EBNF and a web page is three chances to drift.
 */
export type OperatorSpec = {
  concept: string;
  token: string;
  ascii: string;
  glyph: string | null;
  ambiguous: boolean;
  description: string;
};

declare const __CANT_OPERATORS__: OperatorSpec[];
declare const __CANT_VERSION__: string;
declare const __RITE_HOST__: string;
declare const __SIGIL_HOST__: string;

export const OPERATORS: OperatorSpec[] = __CANT_OPERATORS__;
export const CANT_VERSION: string = __CANT_VERSION__;
export const RITE_URL = `https://${__RITE_HOST__}`;
export const SIGIL_URL = `https://${__SIGIL_HOST__}`;

/**
 * Human-readable names for the concepts, in the order the docs introduce them.
 *
 * The manifest's `concept` is a stable machine key (`ward_open`, `block_close`);
 * these are what a reader should see. Anything the manifest gains but this map
 * has not is rendered from the key rather than dropped — a new operator should
 * appear on the site the day it lands, looking slightly unpolished, rather than
 * silently not appear at all.
 */
const DISPLAY_NAMES: Record<string, string> = {
  flow: "Flow",
  scatter: "Scatter",
  collect: "Collect",
  ward_open: "Ward",
  fork_open: "Fork",
  orbit_open: "Orbit",
  block_close: "Block close",
  branch_separator: "Branch separator",
  current_value: "Current value",
  effect: "Effect",
  capability: "Capability",
  modifier: "Modifier",
};

export function displayName(concept: string): string {
  return DISPLAY_NAMES[concept] ?? concept.replace(/_/g, " ");
}

/**
 * The operators worth putting in front of someone learning the language.
 *
 * `block_close` and `branch_separator` are punctuation for forms that are listed
 * with their opener, so showing them as vocabulary items of their own would
 * inflate a twelve-operator language into something that looks harder than it is.
 */
const PUNCTUATION = new Set(["block_close", "branch_separator"]);

export const VOCABULARY = OPERATORS.filter((o) => !PUNCTUATION.has(o.concept));

/**
 * How an operator is written in a program, rather than in isolation.
 *
 * `?{` on its own is not something anyone types; `?{ p }` is.
 */
const USAGE: Record<string, { ascii: string; glyph: string }> = {
  ward_open: { ascii: "?{ p }", glyph: "⊣⟦ p ⟧" },
  fork_open: { ascii: "|{ a ; b }", glyph: "⫴⟦ a ; b ⟧" },
  orbit_open: { ascii: "~{ body }", glyph: "⟲⟦ body ⟧" },
  modifier: { ascii: ":name value", glyph: ":name value" },
};

export function asciiUsage(op: OperatorSpec): string {
  return USAGE[op.concept]?.ascii ?? op.ascii;
}

export function glyphUsage(op: OperatorSpec): string | null {
  const usage = USAGE[op.concept];
  if (usage) return usage.glyph;
  return op.glyph;
}

/**
 * Is the glyph a genuinely different spelling, or the same character?
 *
 * `$`, `!`, `@` and `:` have no glyph form — the manifest either omits one or
 * records the identical character — and printing "→ same" twelve times says
 * nothing. Used to grey those cells out.
 */
export function hasDistinctGlyph(op: OperatorSpec): boolean {
  const glyph = glyphUsage(op);
  return glyph !== null && glyph !== asciiUsage(op);
}

/**
 * Render the backtick spans in a manifest description as code.
 *
 * The `description` fields are written as markdown so they read well in
 * `grammar/cant/operators.toml` itself — "also the head of `!=` inside a leaf".
 * Printed raw on a page that is otherwise typeset, the backticks look like a
 * mistake. This is the only markdown the field is allowed to contain, so a full
 * renderer would be a dependency for one construct.
 *
 * Escapes first: the descriptions contain `<` and `>` (`>=`, `<-`), and this
 * output is bound with `v-html`.
 */
export function renderDescription(text: string): string {
  const escaped = text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
  return escaped.replace(
    /`([^`]+)`/g,
    '<code class="font-mono text-slate-300">$1</code>'
  );
}
