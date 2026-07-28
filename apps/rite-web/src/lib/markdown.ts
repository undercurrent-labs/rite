import { marked } from "marked";

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
