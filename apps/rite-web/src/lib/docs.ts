/** Book chapter order (matches docs/book/README.md). */
export type DocChapter = {
  slug: string;
  title: string;
  file: string;
};

/**
 * Order and titles are the same list the book README prints, so the numbering in
 * the sidebar matches the numbering a reader sees on the /docs index page.
 * Change both together.
 */
export const DOC_CHAPTERS: DocChapter[] = [
  { slug: "installation", title: "Installation", file: "installation.md" },
  { slug: "first-script", title: "First script", file: "first-script.md" },
  { slug: "one-liners", title: "One-liners & REPL", file: "one-liners.md" },
  { slug: "values", title: "Values and atoms", file: "values.md" },
  { slug: "bindings", title: "Bindings", file: "bindings.md" },
  { slug: "functions", title: "Functions", file: "functions.md" },
  { slug: "pipelines", title: "Pipelines", file: "pipelines.md" },
  { slug: "collections", title: "Collections", file: "collections.md" },
  { slug: "matching", title: "Pattern matching", file: "matching.md" },
  { slug: "results", title: "Results and errors", file: "results.md" },
  { slug: "sugar", title: "Syntax sugar", file: "sugar.md" },
  { slug: "effects", title: "Effects and capabilities", file: "effects.md" },
  { slug: "files-json", title: "Files, JSON, and CSV", file: "files-json.md" },
  { slug: "crypto", title: "Hashing and encoding", file: "crypto.md" },
  { slug: "db", title: "Databases", file: "db.md" },
  { slug: "http", title: "Network: HTTP services", file: "http.md" },
  { slug: "sockets", title: "Network: sockets", file: "sockets.md" },
  { slug: "mcp", title: "Model Context Protocol", file: "mcp.md" },
  { slug: "environment", title: "Environment", file: "environment.md" },
  { slug: "processes", title: "Processes", file: "processes.md" },
  { slug: "modules", title: "Modules", file: "modules.md" },
  { slug: "compiling", title: "Compiling to Rust", file: "compiling.md" },
  { slug: "rpg", title: "Text RPG", file: "rpg.md" },
  { slug: "embedding", title: "Embedding", file: "embedding.md" },
  { slug: "browser", title: "Browser & Studio", file: "browser.md" },
  { slug: "agents", title: "Agents & the skill bundle", file: "agents.md" },
  { slug: "testing", title: "Testing", file: "testing.md" },
  { slug: "contributing-tests", title: "Contributing tests", file: "contributing-tests.md" },
];

/**
 * Generated reference pages, published alongside the book.
 *
 * Listed explicitly rather than globbed: `rite docs build` also writes a dozen
 * one-paragraph placeholders, and only these two are real documents. They are
 * regenerated from the capability registry and from clap's command tree, so
 * they cannot drift from the implementation — CI regenerates and fails if
 * either changed.
 */
export const REFERENCE_PAGES: DocChapter[] = [
  { slug: "capabilities", title: "Capability reference", file: "capabilities.md" },
  { slug: "cli", title: "CLI reference", file: "cli.md" },
];

/**
 * Lazy on purpose. Eager loading inlined every chapter (~91 KB of markdown) into
 * the entry chunk, so a visitor to the homepage downloaded the whole book. Each
 * page is now its own chunk, fetched when it is opened.
 */
const rawModules = import.meta.glob("../../../../docs/book/*.md", {
  query: "?raw",
  import: "default",
}) as Record<string, () => Promise<string>>;

const referenceModules = import.meta.glob("../../../../docs/generated/{capabilities,cli}.md", {
  query: "?raw",
  import: "default",
}) as Record<string, () => Promise<string>>;

function fileFromPath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

const byFile = new Map<string, () => Promise<string>>();
for (const [path, load] of Object.entries({ ...rawModules, ...referenceModules })) {
  byFile.set(fileFromPath(path), load);
}

const cache = new Map<string, string>();

async function loadFile(file: string): Promise<string | null> {
  const cached = cache.get(file);
  if (cached !== undefined) return cached;
  const load = byFile.get(file);
  if (!load) return null;
  const text = await load();
  cache.set(file, text);
  return text;
}

export async function getDocMarkdown(slug: string): Promise<string | null> {
  const ch = DOC_CHAPTERS.find((c) => c.slug === slug);
  if (!ch) return null;
  return loadFile(ch.file);
}

export async function getReferenceMarkdown(slug: string): Promise<string | null> {
  const page = REFERENCE_PAGES.find((p) => p.slug === slug);
  if (!page) return null;
  return loadFile(page.file);
}

export function referenceBySlug(slug: string): DocChapter | undefined {
  return REFERENCE_PAGES.find((p) => p.slug === slug);
}

export function chapterBySlug(slug: string): DocChapter | undefined {
  return DOC_CHAPTERS.find((c) => c.slug === slug);
}

export function adjacentChapters(slug: string): {
  prev?: DocChapter;
  next?: DocChapter;
} {
  const i = DOC_CHAPTERS.findIndex((c) => c.slug === slug);
  if (i < 0) return {};
  return {
    prev: i > 0 ? DOC_CHAPTERS[i - 1] : undefined,
    next: i < DOC_CHAPTERS.length - 1 ? DOC_CHAPTERS[i + 1] : undefined,
  };
}

/** Index page body when visiting /docs without a slug. */
export async function docsIndexMarkdown(): Promise<string> {
  // Prefer the book README when present (keeps site + repo docs in sync).
  const readme = await loadFile("README.md");
  if (readme && readme.trim().length > 0) {
    return readme;
  }
  const lines = [
    "# Rite guided book",
    "",
    "A path from install to embedding. Each chapter works with the CLI; many pure examples also run in [Studio](/studio).",
    "",
    ...DOC_CHAPTERS.map((c, i) => `${i + 1}. [${c.title}](/docs/${c.slug})`),
    "",
    "",
    ...REFERENCE_PAGES.map((p) => `- [${p.title}](/docs/reference/${p.slug})`),
  ];
  return lines.join("\n");
}
