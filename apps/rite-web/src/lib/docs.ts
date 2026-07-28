/** Book chapter order (matches docs/book/README.md). */
export type DocChapter = {
  slug: string;
  title: string;
  file: string;
};

export const DOC_CHAPTERS: DocChapter[] = [
  { slug: "installation", title: "Installation", file: "installation.md" },
  { slug: "first-script", title: "First script", file: "first-script.md" },
  { slug: "values", title: "Values and atoms", file: "values.md" },
  { slug: "bindings", title: "Bindings", file: "bindings.md" },
  { slug: "functions", title: "Functions", file: "functions.md" },
  { slug: "pipelines", title: "Pipelines", file: "pipelines.md" },
  { slug: "collections", title: "Collections", file: "collections.md" },
  { slug: "matching", title: "Pattern matching", file: "matching.md" },
  { slug: "results", title: "Results and errors", file: "results.md" },
  { slug: "effects", title: "Effects and capabilities", file: "effects.md" },
  { slug: "files-json", title: "Files and JSON", file: "files-json.md" },
  { slug: "http", title: "HTTP services", file: "http.md" },
  { slug: "modules", title: "Modules", file: "modules.md" },
  { slug: "compiling", title: "Compiling to Rust", file: "compiling.md" },
  { slug: "rpg", title: "Text RPG", file: "rpg.md" },
  { slug: "embedding", title: "Embedding", file: "embedding.md" },
  { slug: "browser", title: "Browser & Studio", file: "browser.md" },
];

const rawModules = import.meta.glob("../../../../docs/book/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function fileFromPath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

const byFile = new Map<string, string>();
for (const [path, content] of Object.entries(rawModules)) {
  byFile.set(fileFromPath(path), content);
}

export function getDocMarkdown(slug: string): string | null {
  const ch = DOC_CHAPTERS.find((c) => c.slug === slug);
  if (!ch) return null;
  return byFile.get(ch.file) ?? null;
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
export function docsIndexMarkdown(): string {
  // Prefer the book README when present (keeps site + repo docs in sync).
  const readme = byFile.get("README.md");
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
    "API reference: `rite docs build` → `docs/generated/`.",
  ];
  return lines.join("\n");
}
