/**
 * Hosted vs local Rite execution:
 * - Prefer a real local/host API when `/api/v1/version` returns JSON.
 * - Else load WASM from `/wasm/rite_wasm.js` (wasm-pack output in public/).
 *
 * Note: Cloudflare SPA `not_found_handling` returns 200 HTML for missing routes,
 * so we must not treat bare 200 as an API.
 */

export type FormatResult = {
  ok?: boolean;
  text?: string;
  dialect?: string;
  source_map?: { out_to_in: number[]; in_len: number; out_len: number };
  error?: string;
};

export type RunResult = {
  ok?: boolean;
  value?: unknown;
  stdout?: string;
  stderr?: string;
  error?: string;
  virtual_http?: { addr?: string; routes?: string[] };
};

type WasmMod = {
  wasm_parse: (s: string) => unknown;
  wasm_analyze: (s: string) => unknown;
  wasm_format: (s: string, d: string) => unknown;
  wasm_convert: (s: string, d: string) => unknown;
  wasm_render_svg: (s: string, frame: string) => string;
  wasm_run: (s: string, allowAll: boolean) => unknown;
  wasm_emit_rust: (s: string) => unknown;
  wasm_syntax_tree: (s: string) => unknown;
  wasm_semantic_ir: (s: string) => unknown;
  wasm_complete: (s: string, line: number, ch: number) => unknown;
  wasm_hover: (s: string, line: number, ch: number) => unknown;
  wasm_map_position: (
    mapJson: string,
    oldS: string,
    newS: string,
    line: number,
    ch: number
  ) => { line: number; character: number };
};

let wasmPromise: Promise<WasmMod | null> | null = null;
let apiProbe: Promise<boolean> | null = null;

function isJsonContentType(ct: string | null): boolean {
  if (!ct) return false;
  const lower = ct.toLowerCase();
  return lower.includes("application/json") || lower.includes("+json");
}

/**
 * Load the engine once.
 *
 * The package is a real static asset in `public/wasm` — not a bundled module.
 * Importing its URL directly works in a built site and fails in `pnpm site:dev`,
 * because Vite's dev server refuses to serve anything under `public/` *as a
 * module*: the request comes back 500 and Studio falls through to "WASM not
 * loaded" on a machine where the file is right there. That is the worst way
 * round, since the only broken configuration is the one you develop in.
 *
 * So the glue is fetched as text and imported through a blob URL. One code path,
 * identical in dev and in production, and nothing about it depends on how the
 * server treats `public/`. The `.wasm` is then passed to `init` explicitly,
 * because a blob URL gives the glue no `import.meta.url` to resolve it from.
 */
async function loadWasm(): Promise<WasmMod | null> {
  if (wasmPromise) return wasmPromise;
  wasmPromise = (async () => {
    const base = import.meta.env.BASE_URL || "/";
    const at = (file: string) => `${base}wasm/${file}`.replace(/([^:]\/)\/+/g, "$1");
    let blobUrl: string | null = null;
    try {
      const response = await fetch(at("rite_wasm.js"));
      if (!response.ok) throw new Error(`${response.status} fetching the engine`);
      const glue = await response.text();
      blobUrl = URL.createObjectURL(new Blob([glue], { type: "text/javascript" }));
      const mod = (await import(/* @vite-ignore */ blobUrl)) as WasmMod & {
        default?: (init?: { module_or_path: string }) => Promise<unknown>;
      };
      if (typeof mod.default === "function") {
        // `{ module_or_path }`, not a bare URL: the positional form is
        // deprecated and warns on every load.
        await mod.default({ module_or_path: at("rite_wasm_bg.wasm") });
      }
      return mod;
    } catch (err) {
      console.warn("[rite] WASM load failed", err);
      return null;
    } finally {
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    }
  })();
  return wasmPromise;
}

/** True only when a real Rite HTTP API answers with JSON (not SPA HTML fallback). */
export async function apiAvailable(base: string): Promise<boolean> {
  if (apiProbe && base === "") return apiProbe;
  const probe = (async () => {
    try {
      const r = await fetch(`${base}/api/v1/version`, {
        method: "GET",
        headers: { accept: "application/json" },
      });
      if (!r.ok) return false;
      if (!isJsonContentType(r.headers.get("content-type"))) return false;
      const j = await r.json();
      // Accept either a version string or an explicit ok flag.
      return typeof j === "object" && j !== null && ("version" in j || "ok" in j || "rite" in j);
    } catch {
      return false;
    }
  })();
  if (base === "") apiProbe = probe;
  return probe;
}

export async function riteCall(
  base: string,
  path: string,
  body: Record<string, unknown>
): Promise<unknown> {
  const hasApi = await apiAvailable(base);

  if (hasApi) {
    try {
      const r = await fetch(`${base}${path}`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          accept: "application/json",
        },
        body: JSON.stringify(body),
      });
      if (r.ok && isJsonContentType(r.headers.get("content-type"))) {
        return await r.json();
      }
      // Non-JSON or error — fall through to WASM rather than showing HTML/405 bodies.
    } catch {
      /* fall through to wasm */
    }
  }

  const wasm = await loadWasm();
  if (!wasm) {
    return {
      ok: false,
      error:
        "No local API and WASM not loaded. Build with `bash scripts/build-wasm.sh` or run `rite studio`.",
    };
  }

  const source = String(body.source ?? "");
  const dialect = String(body.dialect ?? "glyph");

  try {
    switch (path) {
      case "/api/v1/parse":
        return wasm.wasm_parse(source);
      case "/api/v1/analyze":
      case "/api/v1/check":
        return wasm.wasm_analyze(source);
      case "/api/v1/format":
        return { ok: true, ...(wasm.wasm_format(source, dialect) as object) };
      case "/api/v1/run":
        return wasm.wasm_run(source, true);
      case "/api/v1/emit-rust":
        return wasm.wasm_emit_rust(source);
      // No native studio route for this yet; the native call 404s and lands here.
      case "/api/v1/ir":
        return wasm.wasm_semantic_ir(source);
      default:
        return { ok: false, error: `unsupported offline path ${path}` };
    }
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

export async function convertWithMap(
  base: string,
  source: string,
  dialect: string,
  cursor: { line: number; character: number }
): Promise<{ text: string; line: number; character: number; raw: unknown }> {
  const raw = (await riteCall(base, "/api/v1/format", { source, dialect })) as FormatResult;
  const text = raw.text ?? source;
  let line = cursor.line;
  let character = cursor.character;
  if (raw.source_map) {
    const wasm = await loadWasm();
    if (wasm?.wasm_map_position) {
      try {
        const mapped = wasm.wasm_map_position(
          JSON.stringify(raw.source_map),
          source,
          text,
          cursor.line,
          cursor.character
        );
        line = mapped.line;
        character = mapped.character;
      } catch {
        /* keep cursor */
      }
    } else {
      const lines = text.split("\n");
      line = Math.min(cursor.line, Math.max(0, lines.length - 1));
      character = Math.min(cursor.character, (lines[line] ?? "").length);
    }
  }
  return { text, line, character, raw };
}

/** Pretty-print a run/check result for the Studio output panel. */
/**
 * The SVG Studio saves as a picture.
 *
 * Rendered by `rite-render` through WASM rather than drawn here: a second layout
 * in TypeScript would be a second thing to keep in step with `rite render`, and
 * the whole point of the Rust renderer is that there is only one. Studio asks for
 * the self-contained form so the saved image carries its font.
 *
 * Needs the WASM module, which Studio already loads to run code. `null` means the
 * module is genuinely not there; a module that *is* there and fails throws, so
 * the caller can say what went wrong. Catching both as `null` reported a
 * rendering failure as a missing module, which sent me looking in the wrong place
 * for ten minutes.
 */
export async function renderSvg(source: string, frame: string): Promise<string | null> {
  const wasm = await loadWasm();
  if (!wasm?.wasm_render_svg) return null;
  return wasm.wasm_render_svg(source, frame);
}

export function formatOutput(path: string, result: unknown): string {
  if (result == null) return "(empty)";
  if (typeof result !== "object") return String(result);

  const r = result as RunResult & FormatResult & { diagnostics?: unknown; rust?: string };
  if (path === "/api/v1/run" || ("stdout" in r && ("ok" in r || "value" in r))) {
    const lines: string[] = [];
    if (r.stdout) lines.push(r.stdout.replace(/\n$/, ""));
    if (r.stderr) lines.push(r.stderr.replace(/\n$/, ""));
    if (r.error) lines.push(`error: ${r.error}`);
    if (!r.stdout && !r.stderr && !r.error) {
      if (r.value !== undefined && r.value !== null) {
        lines.push(
          typeof r.value === "string" ? r.value : JSON.stringify(r.value, null, 2)
        );
      } else {
        lines.push(r.ok ? "(ok, no output)" : "(failed)");
      }
    } else if (r.value !== undefined && r.value !== null && r.value !== "" && typeof r.value !== "object") {
      lines.push(`→ ${r.value}`);
    }
    if (r.virtual_http?.routes?.length) {
      lines.push(`virtual HTTP: ${r.virtual_http.routes.join(" · ")}`);
    }
    return lines.filter((l) => l.length > 0).join("\n") || JSON.stringify(result, null, 2);
  }
  return JSON.stringify(result, null, 2);
}
