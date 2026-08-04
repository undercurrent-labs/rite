/**
 * The Cant engine, in the page.
 *
 * `cant-wasm` compiled with `wasm-pack`, loaded from `/wasm/cant_wasm.js` at
 * runtime. Everything Studio shows — diagnostics, the expansion, the graph, the
 * value — comes from the same crate the command line uses, so a program that
 * behaves one way here behaves that way in a terminal.
 *
 * There is no server. Nothing typed into Studio leaves the browser.
 */

/**
 * One diagnostic, as `CantDiagnostics::to_json` writes it.
 *
 * The headline is `title`, not `message` — `message` belongs to a *label*, and
 * the two are different things: the title says what is wrong, a label says where.
 * Studio rendered an empty line for every error until this matched the Rust.
 */
export type Diagnostic = {
  code?: string;
  severity?: string;
  title?: string;
  labels?: {
    message?: string;
    primary?: boolean;
    /** A file id plus a byte range — the span is nested, not flat. */
    span?: { file: number; span: { start: number; end: number } };
  }[];
  notes?: string[];
  help?: string;
  /** The Rite diagnostic this was remapped from, when there was one. */
  rite?: { code?: string; span?: unknown };
};

export type CheckResult = {
  ok: boolean;
  diagnostics: Diagnostic[];
  exit_code: number;
  rendered: string;
};

export type ExpandResult = {
  ok: boolean;
  rite: string | null;
  prefix: string | null;
  diagnostics: Diagnostic[];
  rendered: string;
};

export type RunResult = {
  ok: boolean;
  value: unknown;
  stdout: string;
  error: string | null;
  rite: string | null;
  diagnostics: Diagnostic[];
  rendered: string;
};

export type GraphResult = {
  ok: boolean;
  graph: CantGraph | null;
  diagnostics: Diagnostic[];
  rendered: string;
};

export type ExplainResult = {
  ok: boolean;
  text: string;
  capabilities: string[];
  effects: string[];
  max_orbit_items: number | null;
  hazards: string[];
};

/** The subset of `docs/cant/graph-schema.md` the picture needs. */
export type CantGraph = {
  version: string;
  entry?: number;
  exit?: number;
  nodes: GraphNode[];
  edges: GraphEdge[];
  subgraphs: GraphSubgraph[];
};

export type GraphNode = {
  id: number;
  kind: "source" | "stage" | "scatter" | "collect" | "ward" | "fork" | "orbit";
  expr?: { text: string; effectful?: boolean };
  predicate?: { text: string; effectful?: boolean };
  branches?: number[];
  body?: number;
  identity?: { text: string };
  max_items?: number;
  subgraph?: number;
};

export type GraphEdge = {
  from: { node: number };
  to: { node: number };
  ordinal?: number;
  role: "flow" | "enter" | "join" | "orbit_feedback";
};

export type GraphSubgraph = {
  id: number;
  owner: number;
  entry?: number;
  exit?: number;
  nodes: number[];
};

type Engine = {
  cant_check: (source: string) => CheckResult;
  cant_expand: (source: string) => ExpandResult;
  cant_graph: (source: string) => GraphResult;
  cant_dot: (source: string) => string;
  cant_explain: (source: string) => ExplainResult;
  cant_format: (source: string, dialect: string) => { ok: boolean; text?: string; error?: string };
  cant_convert: (source: string, dialect: string) => string;
  cant_run: (source: string) => RunResult;
  cant_version: () => Record<string, string>;
};

let enginePromise: Promise<Engine | null> | null = null;

/**
 * Load the engine once.
 *
 * The package is a real static asset in `public/wasm`, written by
 * `scripts/build-cant-wasm.sh` — not a bundled module. Vite's dev server refuses
 * to serve anything under `public/` *as a module*, so importing the URL directly
 * works in a built site and fails in `pnpm cant:dev`, which is the worst way
 * round: the failure would only appear in the one place nobody tests.
 *
 * So the glue is fetched as text and imported through a blob URL. One code path,
 * identical in dev and in production, and nothing about it depends on how the
 * server treats `public/`. The `.wasm` is then passed to `init` explicitly,
 * because a blob URL gives the glue no `import.meta.url` to resolve it from.
 */
export function loadEngine(): Promise<Engine | null> {
  if (enginePromise) return enginePromise;
  enginePromise = (async () => {
    const base = import.meta.env.BASE_URL || "/";
    const at = (file: string) => `${base}wasm/${file}`.replace(/([^:]\/)\/+/g, "$1");
    let blobUrl: string | null = null;
    try {
      const response = await fetch(at("cant_wasm.js"));
      if (!response.ok) throw new Error(`${response.status} fetching the engine`);
      const glue = await response.text();
      blobUrl = URL.createObjectURL(new Blob([glue], { type: "text/javascript" }));
      const mod = (await import(/* @vite-ignore */ blobUrl)) as Engine & {
        default?: (init?: { module_or_path: string }) => Promise<unknown>;
      };
      if (typeof mod.default === "function") {
        // `{ module_or_path }`, not a bare URL: the positional form is
        // deprecated and warns on every load.
        await mod.default({ module_or_path: at("cant_wasm_bg.wasm") });
      }
      return mod;
    } catch (err) {
      // A checkout where `pnpm cant:wasm` has not been run is the common case,
      // and it should say so on the page rather than only in the console.
      console.warn("[cant] engine failed to load", err);
      return null;
    } finally {
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    }
  })();
  return enginePromise;
}

export const EXAMPLES: { name: string; blurb: string; source: string }[] = [
  {
    name: "Filter",
    blurb: "Scatter a list, keep what passes, collect the rest.",
    source: "[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []",
  },
  {
    name: "Fork",
    blurb: "Three branches from one input, concatenated in order.",
    source: "5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []",
  },
  {
    name: "Orbit",
    blurb: "A bounded breadth-first walk that stops when nothing new arrives.",
    source: "[1, 2] -> * -> ~{ ?{ $ < 20 } -> $ * 2 } :by str :max 64 -> []",
  },
  {
    name: "Placeholder",
    blurb: "`$` puts the emission somewhere other than the first argument.",
    source: '"-" -> join(["a", "b"], $)',
  },
  {
    name: "Text",
    blurb: "Ordinary Rite expressions do the work between the operators.",
    source: 'lines("alpha\\nbb\\ngamma") -> * -> ?{ count($) > 2 } -> upper -> []',
  },
  {
    name: "Nested",
    blurb: "A fork whose branches are a ward and an orbit.",
    source: "4 -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []",
  },
  {
    name: "Reads a file",
    blurb: "Needs a host. Studio says so, and shows what it would run.",
    source: '"notes.txt" -> !@fs.read -> lines -> * -> ?{ $ != "" } -> []',
  },
];
