/**
 * The Cant documents the site publishes.
 *
 * They are the same files a reader gets on GitHub, imported as raw text rather
 * than copied. A separate web-only version of the language reference would be a
 * second thing to keep true.
 */
export type CantDoc = {
  slug: string;
  title: string;
  file: string;
  /** One line for the index page. */
  blurb: string;
};

export const CANT_DOCS: CantDoc[] = [
  {
    slug: "overview",
    title: "Overview",
    file: "README.md",
    blurb: "What Cant is, the operators, and how to read a program.",
  },
  {
    slug: "language",
    title: "The language",
    file: "language.md",
    blurb: "Emissions, stages, flow, scatter, collect, ward, fork, orbit, effects.",
  },
  {
    slug: "cli",
    title: "Command line",
    file: "cli.md",
    blurb: "Running programs, quoting, exit codes, diagnostics, permissions.",
  },
  {
    slug: "graph-schema",
    title: "Graph schema",
    file: "graph-schema.md",
    blurb: "The JSON a graph is serialized as, and what a consumer can rely on.",
  },
];

/**
 * The published files, named individually.
 *
 * `docs/cant/*.md` would be shorter and wrong: Vite bundles every match, so a
 * document that is in the repository but not on the site still ships as a
 * fetchable chunk. The list has to stay in step with `CANT_DOCS` above, which
 * `published_documents_are_the_documents_bundled` in
 * `crates/cant-cli/tests/docs.rs` checks.
 */
const docModules = import.meta.glob(
  "../../../../docs/cant/{README,language,cli,graph-schema}.md",
  {
    query: "?raw",
    import: "default",
  },
) as Record<string, () => Promise<string>>;

function fileName(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] || p;
}

const byFile = new Map<string, () => Promise<string>>();
for (const [p, load] of Object.entries(docModules)) {
  byFile.set(fileName(p), load);
}

const cache = new Map<string, string>();

export function allDocs(): CantDoc[] {
  return CANT_DOCS;
}

export function docBySlug(slug: string): CantDoc | undefined {
  return allDocs().find((d) => d.slug === slug);
}

export async function getDocMarkdown(slug: string): Promise<string | null> {
  const doc = docBySlug(slug);
  if (!doc) return null;
  const cached = cache.get(doc.file);
  if (cached !== undefined) return cached;
  const load = byFile.get(doc.file);
  if (!load) return null;
  const text = await load();
  cache.set(doc.file, text);
  return text;
}

export function adjacentDocs(slug: string): { prev?: CantDoc; next?: CantDoc } {
  const all = allDocs();
  const i = all.findIndex((d) => d.slug === slug);
  if (i < 0) return {};
  return { prev: all[i - 1], next: all[i + 1] };
}
