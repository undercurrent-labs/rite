import { marked } from "marked";
import { highlightCant, highlightRite } from "./highlight";

marked.setOptions({ gfm: true, breaks: false });

const REPO = "https://github.com/undercurrent-labs/rite";
const BLOB = `${REPO}/blob/main`;

/**
 * The generated graph pictures, resolved to bundled URLs.
 *
 * The documents reference them relatively, as `graphs/orbit.svg`, so the same
 * markdown renders on GitHub. Vite hashes and serves them from here, which means
 * there is one copy of each file rather than a build step that keeps two in
 * step.
 */
const GRAPHS = import.meta.glob("../../../../docs/cant/graphs/*.svg", {
  query: "?url",
  import: "default",
  eager: true,
}) as Record<string, string>;

const graphUrl = (name: string): string | undefined => {
  const entry = Object.entries(GRAPHS).find(([path]) => path.endsWith(`/${name}`));
  return entry?.[1];
};

/** Point `<img src="graphs/x.svg">` at the bundled asset. */
function rewriteImages(html: string): string {
  return html.replace(/src="graphs\/([^"]+)"/g, (whole, name: string) => {
    const url = graphUrl(name);
    return url ? `src="${url}"` : whole;
  });
}

/**
 * Markdown file name → site route.
 *
 * Only the documents the site publishes. A file that is in the repository but
 * not here falls through to the GitHub branch below, which is what should
 * happen — a route to a page that does not exist renders as "no document named
 * …", which is less useful than a link that leaves.
 */
const DOC_ROUTES: Record<string, string> = {
  "README.md": "/docs/overview",
  "tutorial.md": "/docs/tutorial",
  "language.md": "/docs/language",
  "one-liners.md": "/docs/one-liners",
  "projects.md": "/docs/projects",
  "cli.md": "/docs/cli",
  "diagnostics.md": "/docs/diagnostics",
  "graph-schema.md": "/docs/graph-schema",
};

/**
 * Rewrite the links in a source document to something a browser can follow.
 *
 * These files are written to resolve on disk and in a GitHub preview — that is
 * why `docs/cant/README.md` links `../adr/0001-…md` — so the site rewrites
 * rather than the documents accommodating the site. Three cases:
 *
 * 1. a document the site publishes → its route;
 * 2. a file in the repository → GitHub, because there is nothing here to show;
 * 3. anything else → left alone.
 */
function rewriteLinks(html: string): string {
  return html.replace(/href="([^"]+)"/g, (whole, href: string) => {
    if (/^(https?:|mailto:|#|\/)/i.test(href)) return whole;

    const [pathPart, hash = ""] = href.split(/(#.*)$/);
    const file = pathPart.split("/").pop() ?? "";

    // A published document, and not a path that points outside docs/cant or
    // docs/adr — `../book/effects.md` is Rite's, and belongs on Rite's site.
    const route = DOC_ROUTES[file];
    const looksLikeCantDoc =
      !pathPart.includes("/") ||
      pathPart.startsWith("../adr/") ||
      pathPart.startsWith("../cant/");
    if (route && looksLikeCantDoc) {
      return `href="${route}${hash}"`;
    }

    // A repository path, written relative to docs/cant or docs/adr.
    const cleaned = pathPart.replace(/^(\.\.\/)+/, "");
    if (
      /^(crates|grammar|examples|conformance|docs|scripts|apps|editors)\//.test(cleaned) ||
      /\.(rs|toml|json|ebnf|cant|rite|sh|ya?ml)$/.test(cleaned)
    ) {
      return `href="${BLOB}/${cleaned}${hash}" target="_blank" rel="noopener noreferrer"`;
    }
    return whole;
  });
}

/**
 * Syntax-highlight fenced blocks.
 *
 * `marked` has already escaped the code, so it is unescaped once here and
 * re-escaped by the highlighter — which is the only way to hand the scanner the
 * characters the author actually wrote. `&amp;` must be undone last or it would
 * resurrect entities the author typed literally.
 */
function highlightFences(html: string): string {
  return html.replace(
    /<pre><code class="language-(cant|rite)">([\s\S]*?)<\/code><\/pre>/g,
    (_whole, lang: string, escaped: string) => {
      const source = escaped
        .replace(/&lt;/g, "<")
        .replace(/&gt;/g, ">")
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
        .replace(/&amp;/g, "&");
      const body = lang === "cant" ? highlightCant(source) : highlightRite(source);
      return `<pre class="code-block" data-lang="${lang}"><code>${body}</code></pre>`;
    }
  );
}

/** Give every `<h2>`/`<h3>` an id, so the page can carry a table of contents. */
function addHeadingIds(html: string): string {
  return html.replace(/<(h[23])>([\s\S]*?)<\/\1>/g, (_whole, tag: string, inner: string) => {
    const text = inner.replace(/<[^>]+>/g, "");
    const id = text
      .toLowerCase()
      .replace(/[^a-z0-9\s-]/g, "")
      .trim()
      .replace(/\s+/g, "-");
    return `<${tag} id="${id}">${inner}</${tag}>`;
  });
}

export type Heading = { id: string; text: string; level: 2 | 3 };

export function headingsOf(html: string): Heading[] {
  const out: Heading[] = [];
  const re = /<(h[23]) id="([^"]+)">([\s\S]*?)<\/\1>/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(html))) {
    out.push({
      id: m[2],
      text: m[3].replace(/<[^>]+>/g, "").trim(),
      level: m[1] === "h2" ? 2 : 3,
    });
  }
  return out;
}

export function renderMarkdown(md: string): string {
  const raw = marked.parse(md, { async: false }) as string;
  return addHeadingIds(rewriteImages(highlightFences(rewriteLinks(raw))));
}
