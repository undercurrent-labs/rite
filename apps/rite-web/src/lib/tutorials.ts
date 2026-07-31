/**
 * Tutorials are a separate section from the book, not more chapters in it.
 *
 * The book is one topic per chapter, numbered, meant to be read in order. A
 * tutorial is a project: longer, task-shaped, and picked from a list rather than
 * read through. Folding them into `DOC_CHAPTERS` would have stretched a
 * 24-chapter reading order to 30-odd and made "start at the top" wrong advice.
 *
 * They carry metadata the book has no use for — what you end up with, what you
 * need first — because that is what someone chooses a tutorial by.
 */
export type Tutorial = {
  slug: string;
  title: string;
  file: string;
  /** One line, shown on the index card. */
  blurb: string;
  /** What the reader has at the end. */
  builds: string;
  /** Prerequisites in plain words — "nothing" is a real and common answer. */
  needs: string;
};

/**
 * Order is the recommended reading order and must match `docs/tutorials/README.md`,
 * which renders as the index page's own table. Nothing enforces that; check it by
 * eye, the same way the book's two chapter lists are kept in step.
 */
export const TUTORIALS: Tutorial[] = [
  {
    slug: "json-pipeline",
    title: "Reshaping JSON",
    file: "json-pipeline.md",
    blurb: "Read a file of orders, keep what shipped, group by customer, rank the result.",
    builds: "A report generator",
    needs: "nothing",
  },
  {
    slug: "cli-tool",
    title: "Building a CLI",
    file: "cli-tool.md",
    blurb: "Read your own arguments, split flags from names, and fail the way a shell expects.",
    builds: "A command-line greeter",
    needs: "nothing",
  },
  {
    slug: "testing-what-you-built",
    title: "Testing what you built",
    file: "testing-what-you-built.md",
    blurb: "Write tests for the CLI, and learn that `rite test` grants every permission.",
    builds: "A test suite for the greeter",
    needs: "Building a CLI",
  },
  {
    slug: "http-service",
    title: "An HTTP service with real routes",
    file: "http-service.md",
    blurb: "Path and query parameters, a JSON body, recovery, and a client that proves each route.",
    builds: "A JSON API that tests itself",
    needs: "nothing",
  },
  {
    slug: "dns-resolver",
    title: "A DNS resolver over @udp",
    file: "dns-resolver.md",
    blurb: "Encode a question into wire-format bytes, send one datagram, read an address back.",
    builds: "A minimal DNS client",
    needs: "a reachable nameserver",
  },
  {
    slug: "compiling-a-binary",
    title: "Compiling to a binary",
    file: "compiling-a-binary.md",
    blurb: "Turn a script into a native executable, and see what happens to its permissions.",
    builds: "A shippable word-count tool",
    needs: "a Rust toolchain",
  },
  {
    slug: "fs-audit",
    title: "Auditing a directory",
    file: "fs-audit.md",
    blurb: "Expand a glob, ask each file about itself, total it up, flag what has gone stale.",
    builds: "A directory-sizing CLI tool",
    needs: "nothing",
  },
];

/**
 * Lazy, for the same reason the book's chapters are: a tutorial is long, and a
 * visitor to the index should not download every one of them to read the list.
 */
const rawModules = import.meta.glob("../../../../docs/tutorials/*.md", {
  query: "?raw",
  import: "default",
}) as Record<string, () => Promise<string>>;

const byFile = new Map<string, () => Promise<string>>();
for (const [path, load] of Object.entries(rawModules)) {
  const file = path.replace(/\\/g, "/").split("/").pop() || path;
  byFile.set(file, load);
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

export async function getTutorialMarkdown(slug: string): Promise<string | null> {
  const t = TUTORIALS.find((x) => x.slug === slug);
  if (!t) return null;
  return loadFile(t.file);
}

export function tutorialBySlug(slug: string): Tutorial | undefined {
  return TUTORIALS.find((t) => t.slug === slug);
}

export function adjacentTutorials(slug: string): { prev?: Tutorial; next?: Tutorial } {
  const i = TUTORIALS.findIndex((t) => t.slug === slug);
  if (i < 0) return {};
  return {
    prev: i > 0 ? TUTORIALS[i - 1] : undefined,
    next: i < TUTORIALS.length - 1 ? TUTORIALS[i + 1] : undefined,
  };
}
