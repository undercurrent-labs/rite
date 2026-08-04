/**
 * The bridge to the Rust renderer.
 *
 * Everything that decides what a picture looks like is in `rite-sigil`, compiled
 * to WebAssembly. This file loads it, calls it, and cancels stale work. It does
 * not lay anything out, and must not start to — ADR 0005.
 *
 * # Loading through a blob URL
 *
 * The glue is fetched and imported through a blob rather than imported by path.
 * Vite's dev server refuses to serve files under `public/` as modules — it
 * answers 500 — so a direct `import("/wasm/…js")` works in a built site and
 * fails in the one you develop in. Both Rite Studios hit exactly this and were
 * fixed the same way; doing it here from the start means one code path in dev
 * and production.
 */

export interface RenderOptions {
  theme: string;
  mode: string;
  metadata: string;
  ornament: string;
  /** How traces are drawn: flowing, concentric, or circuit. */
  tracery: string;
  seed: string;
  background: string;
  canonical: boolean;
  simplify: boolean;
}

export interface Diagnostic {
  code: string;
  severity: string;
  message: string;
  graphId?: string;
  spanStart?: number;
  spanEnd?: number;
  notes?: string[];
}

export interface RenderResult {
  ok: boolean;
  svg?: string;
  sceneJson?: string;
  graphJson?: string;
  /** The self-contained interactive page; present only from renderHtml. */
  html?: string;
  fingerprint?: string;
  summary?: string;
  diagnostics: Diagnostic[];
  elapsedMs?: number;
}

export interface VersionInfo {
  renderer: string;
  graphSchema: number;
  sceneSchema: number;
  cantGraphSchema: string;
  themeVersion: number;
}

export const defaultOptions = (): RenderOptions => ({
  theme: "neon-ritual",
  mode: "veiled",
  metadata: "safe",
  ornament: "ritual",
  tracery: "flowing",
  seed: "graph",
  background: "theme",
  canonical: false,
  simplify: false,
});

type Wasm = {
  default: (input?: unknown) => Promise<unknown>;
  renderCant: (name: string, source: string, options?: string) => string;
  renderGraph: (graphJson: string, options?: string) => string;
  renderCantHtml: (name: string, source: string, options?: string) => string;
  renderGraphHtml: (graphJson: string, options?: string) => string;
  validateGraph: (graphJson: string) => string;
  version: () => string;
  supportedSchemas: () => string;
};

let engine: Wasm | null = null;
let loading: Promise<Wasm> | null = null;

/** Load the engine once; concurrent callers share the same promise. */
export function load(): Promise<Wasm> {
  if (engine) return Promise.resolve(engine);
  if (loading) return loading;

  loading = (async () => {
    const base = `${import.meta.env.BASE_URL ?? "/"}wasm/`.replace(/\/+/g, "/");
    const glueUrl = `${base}cant_sigil_wasm.js`;
    const response = await fetch(glueUrl);
    if (!response.ok) {
      throw new Error(`could not fetch the renderer (${response.status}) from ${glueUrl}`);
    }
    // The glue resolves its own `.wasm` relative to its module URL, which a blob
    // does not have — so the path is rewritten to an absolute one before the
    // blob is made.
    const source = (await response.text()).replace(
      /new URL\('cant_sigil_wasm_bg\.wasm',\s*import\.meta\.url\)/,
      JSON.stringify(`${base}cant_sigil_wasm_bg.wasm`)
    );
    const blob = new Blob([source], { type: "text/javascript" });
    const url = URL.createObjectURL(blob);
    try {
      const module = (await import(/* @vite-ignore */ url)) as Wasm;
      await module.default();
      engine = module;
      return module;
    } finally {
      URL.revokeObjectURL(url);
    }
  })();

  return loading;
}

/**
 * A monotonic token, so a slow render cannot overwrite a newer one.
 *
 * §19.2 asks for cancellation. WebAssembly cannot be interrupted mid-call, so
 * what is actually available is *discarding* — the render still finishes, and
 * its result is dropped if anything newer has started. Calling that
 * "cancellation" would overstate it.
 */
let generation = 0;

export function nextGeneration(): number {
  return ++generation;
}

export function isCurrent(token: number): boolean {
  return token === generation;
}

function parse(json: string): RenderResult {
  try {
    return JSON.parse(json) as RenderResult;
  } catch (error) {
    return {
      ok: false,
      diagnostics: [
        {
          code: "SIGIL-W001",
          severity: "error",
          message: `the renderer returned something unreadable: ${String(error)}`,
        },
      ],
    };
  }
}

function failure(message: string, code = "SIGIL-W001"): RenderResult {
  return { ok: false, diagnostics: [{ code, severity: "error", message }] };
}

/** Render Cant source. Never throws; a failure is a value. */
export async function renderCant(
  name: string,
  source: string,
  options: RenderOptions
): Promise<RenderResult> {
  let wasm: Wasm;
  try {
    wasm = await load();
  } catch (error) {
    return failure(String(error));
  }
  const started = performance.now();
  try {
    const result = parse(wasm.renderCant(name, source, JSON.stringify(options)));
    result.elapsedMs = performance.now() - started;
    return result;
  } catch (error) {
    // A panic in the renderer surfaces here as a thrown value. It is a bug
    // either way, but a blank page is a worse way to learn about it.
    return failure(`the renderer failed: ${String(error)}`);
  }
}

/** Render a `cant.graph` document. */
export async function renderGraph(
  graphJson: string,
  options: RenderOptions
): Promise<RenderResult> {
  let wasm: Wasm;
  try {
    wasm = await load();
  } catch (error) {
    return failure(String(error));
  }
  const started = performance.now();
  try {
    const result = parse(wasm.renderGraph(graphJson, JSON.stringify(options)));
    result.elapsedMs = performance.now() - started;
    return result;
  } catch (error) {
    return failure(`the renderer failed: ${String(error)}`);
  }
}

/**
 * Render to a self-contained interactive HTML page (§16, W8).
 *
 * A separate call rather than a field on every render: the page embeds the
 * SVG and a stylesheet over again, and exports are rare while renders are
 * constant. Built in this tab like everything else — nothing is uploaded.
 */
export async function renderHtml(
  tab: "cant" | "graph",
  nameOrJson: string,
  sourceOrNothing: string,
  options: RenderOptions
): Promise<RenderResult> {
  let wasm: Wasm;
  try {
    wasm = await load();
  } catch (error) {
    return failure(String(error));
  }
  try {
    const json =
      tab === "cant"
        ? wasm.renderCantHtml(nameOrJson, sourceOrNothing, JSON.stringify(options))
        : wasm.renderGraphHtml(nameOrJson, JSON.stringify(options));
    return parse(json);
  } catch (error) {
    return failure(`the renderer failed: ${String(error)}`);
  }
}

export async function version(): Promise<VersionInfo | null> {
  try {
    const wasm = await load();
    return JSON.parse(wasm.version()) as VersionInfo;
  } catch {
    return null;
  }
}
