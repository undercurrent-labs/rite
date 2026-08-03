/**
 * A small Cant highlighter for the site.
 *
 * Driven by `grammar/cant/operators.toml` through `OPERATORS`, so the site
 * colours exactly the operators the lexer recognises — a hand-listed set here
 * would be a fourth copy of the vocabulary.
 *
 * It is a scanner, not a set of regular expressions, for the same reason
 * `cant-syntax`'s lexer is: a string or a comment containing `->` or `?{` must
 * come out as a string or a comment. Replacing operator characters across the
 * whole line would paint the inside of `"a -> b"`, which is precisely the bug
 * the language's own tooling is built to avoid. This is a display approximation
 * of that lexer — it does not resolve `*` as scatter versus multiply, because
 * that needs a parser and nothing on a page depends on the distinction.
 */
import { OPERATORS } from "./operators";

type Span = { text: string; cls: string | null };

const escapeHtml = (s: string): string =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Operator spellings, longest first so `->` beats `-` and `⊣⟦` beats `⊣`. */
const SPELLINGS: string[] = (() => {
  const out: string[] = [];
  for (const op of OPERATORS) {
    out.push(op.ascii);
    if (op.glyph && op.glyph !== op.ascii) out.push(op.glyph);
  }
  // `}` and `⟧` close blocks and are in the manifest; `{` only ever arrives
  // attached to a sigil, so it is not listed and needs no case here.
  return [...new Set(out)].sort((a, b) => b.length - a.length);
})();

const isIdentStart = (c: string) => /[A-Za-z_]/.test(c);
const isIdent = (c: string) => /[A-Za-z0-9_]/.test(c);

function scan(source: string): Span[] {
  const spans: Span[] = [];
  let i = 0;
  let plain = "";

  const flush = () => {
    if (plain) {
      spans.push({ text: plain, cls: null });
      plain = "";
    }
  };
  const emit = (text: string, cls: string) => {
    flush();
    spans.push({ text, cls });
  };

  while (i < source.length) {
    const rest = source.slice(i);

    // Comments and strings whole, before any operator is considered.
    if (rest.startsWith("//")) {
      const end = rest.indexOf("\n");
      const text = end < 0 ? rest : rest.slice(0, end);
      emit(text, "c-comment");
      i += text.length;
      continue;
    }
    if (rest.startsWith("/*")) {
      const end = rest.indexOf("*/", 2);
      const text = end < 0 ? rest : rest.slice(0, end + 2);
      emit(text, "c-comment");
      i += text.length;
      continue;
    }
    if (rest[0] === '"') {
      let j = 1;
      while (j < rest.length) {
        if (rest[j] === "\\") j += 2;
        else if (rest[j] === '"') {
          j += 1;
          break;
        } else j += 1;
      }
      const text = rest.slice(0, j);
      emit(text, "c-string");
      i += text.length;
      continue;
    }

    // `!@fs.read` reads as one thing, so it is coloured as one thing.
    const cap = /^!?@[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*/.exec(rest);
    if (cap) {
      emit(cap[0], "c-capability");
      i += cap[0].length;
      continue;
    }

    // `:max` after a block is a modifier; `:error` inside an expression is a
    // Rite atom. Both are `:name`, and telling them apart needs the parser —
    // they are given the same colour rather than a guessed one.
    const atom = /^:[A-Za-z_][A-Za-z0-9_]*/.exec(rest);
    if (atom) {
      emit(atom[0], "c-atom");
      i += atom[0].length;
      continue;
    }

    if (rest[0] === "$") {
      emit("$", "c-dollar");
      i += 1;
      continue;
    }

    const num = /^\d[\d_]*(\.\d+)?/.exec(rest);
    if (num) {
      emit(num[0], "c-number");
      i += num[0].length;
      continue;
    }

    const spelling = SPELLINGS.find((s) => rest.startsWith(s));
    if (spelling) {
      emit(spelling, "c-op");
      i += spelling.length;
      continue;
    }

    if (isIdentStart(rest[0])) {
      let j = 0;
      while (j < rest.length && isIdent(rest[j])) j += 1;
      plain += rest.slice(0, j);
      i += j;
      continue;
    }

    plain += rest[0];
    i += 1;
  }
  flush();
  return spans;
}

export function highlightCant(source: string): string {
  return scan(source)
    .map((s) => (s.cls ? `<span class="${s.cls}">${escapeHtml(s.text)}</span>` : escapeHtml(s.text)))
    .join("");
}

/**
 * Rite, highlighted well enough to read in a `cant expand` comparison.
 *
 * Deliberately thin: the site's job is to show that generated Rite is ordinary
 * Rite, not to reimplement Rite's highlighter. Keywords, strings, comments and
 * capability calls, and nothing else.
 */
const RITE_KEYWORDS = [
  "def",
  "return",
  "if",
  "else",
  "match",
  "do",
  "use",
  "as",
  "pub",
  "true",
  "false",
  "none",
  "while",
  "for",
  "and",
  "or",
  "not",
];

export function highlightRite(source: string): string {
  const spans = scan(source).map((s) => {
    if (s.cls) return s;
    return s;
  });
  return spans
    .map((s) => {
      if (s.cls) return `<span class="${s.cls}">${escapeHtml(s.text)}</span>`;
      return escapeHtml(s.text).replace(
        new RegExp(`\\b(${RITE_KEYWORDS.join("|")})\\b`, "g"),
        '<span class="c-keyword">$1</span>'
      );
    })
    .join("");
}
