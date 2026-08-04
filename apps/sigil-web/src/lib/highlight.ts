/**
 * The Cant highlighter for the source panel.
 *
 * A port of the Cant site's own scanner (`apps/cant-web/src/lib/highlight.ts`),
 * driven by the same `grammar/cant/operators.toml` through `__CANT_OPERATORS__`
 * — so both sites colour exactly the operators the lexer recognises, from the
 * same manifest, with no hand-listed copy to drift.
 *
 * It is a scanner, not a set of regular expressions, for the same reason
 * `cant-syntax`'s lexer is: a string or a comment containing `->` or `?{` must
 * come out as a string or a comment. It is a display approximation of that
 * lexer — it does not resolve `*` as scatter versus multiply, because that
 * needs a parser and nothing in a source panel depends on the distinction.
 */

type Span = { text: string; cls: string | null };

const escapeHtml = (s: string): string =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Operator spellings, longest first so `->` beats `-` and `⊣⟦` beats `⊣`. */
const SPELLINGS: string[] = (() => {
  const out: string[] = [];
  for (const op of __CANT_OPERATORS__) {
    out.push(op.ascii);
    if (op.glyph && op.glyph !== op.ascii) out.push(op.glyph);
  }
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
      emit(rest.slice(0, j), "c-string");
      i += j;
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
    // Rite atom. Telling them apart needs the parser, so both get one colour.
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
