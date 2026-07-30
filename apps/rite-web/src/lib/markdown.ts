import { marked, type Token, type Tokens } from "marked";

marked.setOptions({
  gfm: true,
  breaks: false,
});

/** Rewrite in-book relative links (foo.md, ./foo.md) to /docs/foo. */
function rewriteDocLinks(html: string): string {
  return html
    .replace(
      /href="(?:\.\/)?([a-z0-9-]+)\.md(#[^"]*)?"/gi,
      (_m, slug: string, hash: string = "") => `href="/docs/${slug}${hash || ""}"`
    )
    .replace(
      /href="README\.md(#[^"]*)?"/gi,
      (_m, hash: string = "") => `href="/docs${hash || ""}"`
    );
}

export function renderMarkdown(md: string): string {
  const raw = marked.parse(md, { async: false }) as string;
  return rewriteDocLinks(raw);
}

/**
 * How a ```rite fence is annotated in the book, which decides what the code
 * block offers the reader. The vocabulary is the doctest runner's, so
 * `rite docs check` is what keeps these honest — a block marked `browser` is
 * executed in browser-safe mode on every CI run.
 */
export type CodeMode = "browser" | "native_only" | "fragment";

export type DocSegment =
  | { kind: "html"; html: string }
  | { kind: "code"; code: string; lang: string; mode: CodeMode };

function parseFenceInfo(info: string): { lang: string; mode: CodeMode } {
  const [lang = "", rest = ""] = info.trim().split(/\s+/, 2);
  const mode: CodeMode =
    rest === "browser" ? "browser" : rest === "native_only" ? "native_only" : "fragment";
  return { lang, mode };
}

/**
 * Split markdown into rendered HTML runs and code blocks.
 *
 * Code blocks come back as data rather than HTML so they can be rendered as real
 * components — highlighted, with a copy button and a way to run them. Everything
 * else stays a single `v-html` chunk, which keeps prose rendering unchanged.
 */
export function segmentMarkdown(md: string): DocSegment[] {
  const tokens = marked.lexer(md);
  const segments: DocSegment[] = [];
  let buffer: Token[] = [];

  const flush = () => {
    if (!buffer.length) return;
    const chunk = buffer as Token[] & { links: Record<string, unknown> };
    // The parser needs the link reference table from the original lex.
    chunk.links = (tokens as unknown as { links: Record<string, unknown> }).links ?? {};
    segments.push({ kind: "html", html: rewriteDocLinks(marked.parser(chunk)) });
    buffer = [];
  };

  for (const token of tokens) {
    if (token.type === "code") {
      const code = token as Tokens.Code;
      flush();
      const { lang, mode } = parseFenceInfo(code.lang ?? "");
      segments.push({ kind: "code", code: code.text, lang, mode });
    } else {
      buffer.push(token);
    }
  }
  flush();
  return segments;
}
