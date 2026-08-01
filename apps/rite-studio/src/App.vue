<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { EXAMPLES } from "./examples";
import { decodeShare, encodeShare } from "./share";
import { convertWithMap, formatOutput, riteCall, renderSvg } from "./riteApi";
import CodeEditor from "./CodeEditor.vue";
import { tokenize } from "./highlight";

/**
 * One pane per action. Each button writes only its own pane, so switching tabs
 * never shows the output of a different action as if it were this one's.
 */
const PANELS = ["out", "diag", "ast", "ir", "rust", "http"] as const;
type Panel = (typeof PANELS)[number];

const PANEL_LABELS: Record<Panel, string> = {
  out: "output",
  diag: "diagnostics",
  ast: "ast",
  ir: "ir",
  rust: "rust",
  http: "http",
};

const PANEL_EMPTY: Record<Panel, string> = {
  out: "Press Run.",
  diag: "Press Check.",
  ast: "Press AST.",
  ir: "Press IR.",
  rust: "Press Rust.",
  http: "Run a script containing @http.listen to list its routes.",
};

const source = ref(`◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
`);
const dialect = ref<"glyph" | "ascii">("glyph");
const panel = ref<Panel>("out");
const content = reactive<Record<Panel, string>>({
  out: "",
  diag: "",
  ast: "",
  ir: "",
  rust: "",
  http: "",
});
const busy = ref(false);
const apiBase = ref(import.meta.env.VITE_RITE_API || "");
const cursor = ref({ line: 0, character: 0 });
const httpRoutes = ref<string[]>([]);
const httpAddr = ref("127.0.0.1:4040");
const httpMethod = ref("GET");
const httpPath = ref("/health");
const copied = ref("");

const shown = computed(() => content[panel.value] || PANEL_EMPTY[panel.value]);

const curlCommand = computed(
  () => `curl -sS -X ${httpMethod.value} http://${httpAddr.value}${httpPath.value}`
);

/**
 * The browser build reports its bind address as `virtual://rite-studio` and its
 * routes as source fragments (`GET "/health" ⟦`). Neither is usable as-is: the
 * first produced `http://virtual://…` in the curl line, the second showed a
 * dangling block glyph. Normalise both into what the CLI would really serve.
 */
function usableAddr(addr: unknown): string | null {
  if (typeof addr !== "string") return null;
  return /^[\w.-]+:\d+$/.test(addr) && !addr.startsWith("0.0.0.0:0") ? addr : null;
}

function cleanRoute(route: string): string {
  return route
    .replace(/[⟦{]\s*$/, "")
    .replace(/\[\[\s*$/, "")
    .trim();
}

/** `GET "/health"` → method and path, so the curl line targets a real route. */
function parseRoute(route: string): { method: string; path: string } | null {
  const m = cleanRoute(route).match(/^([A-Z]+)\s+"([^"]+)"/);
  return m ? { method: m[1], path: m[2] } : null;
}

// Load share fragment
if (typeof location !== "undefined" && location.hash.startsWith("#s=")) {
  try {
    const state = decodeShare(location.hash.slice(3));
    if (state.source) source.value = state.source;
    if (state.dialect) dialect.value = state.dialect as "glyph" | "ascii";
  } catch {
    /* ignore */
  }
}

/**
 * Switching dialect rewrites the editor.
 *
 * The selector used to change only what the *next* Format produced, so picking
 * "ascii" left glyphs on screen and looked broken — the one thing the control is
 * named for was the one thing it did not do. Rite has a single AST and two
 * spellings, so converting is the honest reading of the choice.
 *
 * Registered after the share-fragment hydration above, which runs at setup, so
 * restoring `#s=…` with `dialect: "ascii"` does not fire a conversion of source
 * that is already ASCII.
 */
watch(dialect, async (to) => {
  if (!source.value.trim()) return;
  busy.value = true;
  try {
    const mapped = await convertWithMap(apiBase.value, source.value, to, cursor.value);
    source.value = mapped.text;
    cursor.value = { line: mapped.line, character: mapped.character };
  } catch (e) {
    // Leave the editor exactly as it was: silently replacing what someone typed
    // with a half-converted version is worse than not converting. Source that does
    // not parse cannot be converted, and that is the usual reason to land here.
    panel.value = "diag";
    content.diag = `Could not convert to ${to}: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    busy.value = false;
  }
});

watch([source, dialect], () => {
  const frag = encodeShare({ source: source.value, dialect: dialect.value });
  if (typeof history !== "undefined" && typeof location !== "undefined") {
    // Preserve path so nested /studio#s=… shares keep working on the product site.
    history.replaceState(null, "", `${location.pathname}${location.search}#s=${frag}`);
  }
});

async function api(path: string, body: Record<string, unknown>) {
  busy.value = true;
  try {
    return await riteCall(apiBase.value, path, body);
  } finally {
    busy.value = false;
  }
}

/**
 * Save the editor's contents as a PNG.
 *
 * The picture comes from `rite-render` through WASM, then goes through a canvas
 * to become a PNG — so what is saved is exactly what `rite render` would draw,
 * and the browser never needs a rasteriser. Studio offers PNG only: an SVG is
 * what the CLI is for, and one format here is one thing to explain.
 */
async function savePng() {
  busy.value = true;
  try {
    const svg = await renderSvg(source.value, "window");
    if (!svg) {
      panel.value = "diag";
      content.diag =
        "Could not render an image: the WASM module is not available in this build.";
      return;
    }

    // The SVG carries its own font, so the canvas needs nothing from the page.
    const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    try {
      const img = new Image();
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve();
        img.onerror = () => reject(new Error("the browser could not read the rendered SVG"));
        img.src = url;
      });

      // Twice the CSS size, so it stays sharp when it lands somewhere retina.
      const scale = 2;
      const canvas = document.createElement("canvas");
      canvas.width = img.width * scale;
      canvas.height = img.height * scale;
      const ctx = canvas.getContext("2d");
      if (!ctx) throw new Error("no 2d canvas context");
      ctx.scale(scale, scale);
      ctx.drawImage(img, 0, 0);

      const png = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, "image/png")
      );
      if (!png) throw new Error("the canvas produced no PNG");

      const link = document.createElement("a");
      link.href = URL.createObjectURL(png);
      link.download = "rite.png";
      link.click();
      // Revoked on the next tick: revoking immediately can beat the download.
      setTimeout(() => URL.revokeObjectURL(link.href), 10_000);
    } finally {
      URL.revokeObjectURL(url);
    }
  } catch (e) {
    panel.value = "diag";
    content.diag = `Could not save an image: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    busy.value = false;
  }
}

function copyShareLink() {
  void flash("link", location.href);
}

async function flash(what: string, text: string) {
  try {
    await navigator.clipboard.writeText(text);
    copied.value = what;
    setTimeout(() => (copied.value = ""), 1500);
  } catch {
    copied.value = "";
  }
}

async function run() {
  panel.value = "out";
  const j: any = await api("/api/v1/run", { source: source.value });
  content.out = formatOutput("/api/v1/run", j);
  if (j?.virtual_http?.routes) {
    httpRoutes.value = (j.virtual_http.routes as string[]).map(cleanRoute);
    httpAddr.value = usableAddr(j.virtual_http.addr) ?? "127.0.0.1:4040";
    const first = httpRoutes.value.map(parseRoute).find(Boolean);
    if (first) {
      httpMethod.value = first.method;
      httpPath.value = first.path;
    }
    content.http = "";
    panel.value = "http";
  }
}
async function check() {
  panel.value = "diag";
  content.diag = formatOutput("/api/v1/analyze", await api("/api/v1/analyze", { source: source.value }));
}
async function format() {
  const mapped = await convertWithMap(apiBase.value, source.value, dialect.value, cursor.value);
  source.value = mapped.text;
  cursor.value = { line: mapped.line, character: mapped.character };
  panel.value = "out";
  content.out = formatOutput("/api/v1/format", mapped.raw);
}
async function showAst() {
  panel.value = "ast";
  content.ast = formatOutput("/api/v1/parse", await api("/api/v1/parse", { source: source.value }));
}
async function showIr() {
  panel.value = "ir";
  content.ir = formatOutput("/api/v1/ir", await api("/api/v1/ir", { source: source.value }));
}
async function emitRust() {
  panel.value = "rust";
  const j: any = await api("/api/v1/emit-rust", { source: source.value });
  content.rust = j?.rust || formatOutput("/api/v1/emit-rust", j);
}
function loadExample(id: string) {
  const ex = EXAMPLES.find((e) => e.id === id);
  if (!ex) return;
  source.value = dialect.value === "ascii" ? ex.ascii : ex.source;
  panel.value = "out";
  content.out = `Loaded ${ex.title} — ${ex.description}`;
}
/**
 * What each pane actually contains, so it is highlighted as itself: the parse
 * and analysis panes are JSON documents, `rust` is generated Rust, and `out`
 * carries Rite values and printed text.
 */
const PANEL_LANGS: Record<Panel, string> = {
  out: "rite",
  diag: "json",
  ast: "json",
  ir: "json",
  rust: "rust",
  http: "bash",
};

const shownTokens = computed(() => tokenize(shown.value, PANEL_LANGS[panel.value]));
const curlTokens = computed(() => tokenize(`rite run server.rite\n${curlCommand.value}`, "bash"));
</script>

<template>
  <div class="h-full min-h-0 flex flex-col bg-rite-bg text-slate-100">
    <header class="flex flex-wrap items-center gap-2 border-b border-slate-800 px-3 py-2 shrink-0">
      <span class="text-sm text-slate-400 mr-auto hidden sm:inline">Playground</span>

      <label class="sr-only" for="studio-example">Load example</label>
      <select
        id="studio-example"
        class="bg-slate-900 border border-slate-700 rounded px-2 py-1 text-sm"
        @change="loadExample(($event.target as HTMLSelectElement).value)"
      >
        <option value="">Examples…</option>
        <option v-for="ex in EXAMPLES" :key="ex.id" :value="ex.id">{{ ex.title }}</option>
      </select>

      <label class="sr-only" for="studio-dialect">Dialect</label>
      <select
        id="studio-dialect"
        v-model="dialect"
        class="bg-slate-900 border border-slate-700 rounded px-2 py-1 text-sm"
      >
        <option value="glyph">glyph</option>
        <option value="ascii">ascii</option>
      </select>

      <button class="btn" :disabled="busy" @click="run">Run</button>
      <button class="btn" :disabled="busy" @click="check">Check</button>
      <button class="btn" :disabled="busy" @click="format">Format</button>
      <button class="btn" :disabled="busy" @click="showAst">AST</button>
      <button class="btn" :disabled="busy" @click="showIr">IR</button>
      <button class="btn" :disabled="busy" @click="emitRust">Rust</button>
      <button
        class="btn"
        :disabled="busy"
        title="Save this script as a PNG, highlighted"
        @click="savePng"
      >
        Save PNG
      </button>
      <button
        class="btn"
        title="Copy a link that reopens this script"
        @click="copyShareLink"
      >
        {{ copied === "link" ? "Copied" : "Copy link" }}
      </button>
    </header>

    <main class="flex-1 grid md:grid-cols-2 min-h-0 overflow-hidden">
      <label class="sr-only" for="studio-source">Rite source</label>
      <CodeEditor
        v-model="source"
        input-id="studio-source"
        lang="rite"
        class="h-[40vh] md:h-full"
        @cursor="cursor = $event"
      />
      <section class="flex flex-col border-l border-slate-800 min-h-[35vh] md:min-h-0 overflow-hidden">
        <div class="flex flex-wrap gap-1 p-2 border-b border-slate-800 text-sm" role="tablist">
          <button
            v-for="t in PANELS"
            :key="t"
            role="tab"
            :aria-selected="panel === t"
            class="px-2 py-1 rounded border border-transparent"
            :class="panel === t ? 'border-rite-pink text-rite-pink' : 'text-slate-400'"
            @click="panel = t"
          >
            {{ PANEL_LABELS[t] }}
          </button>
        </div>

        <div v-if="panel === 'http'" class="flex-1 overflow-auto p-3 space-y-3 text-sm">
          <p class="text-slate-400">
            Routes:
            <span class="font-mono text-slate-200">{{
              httpRoutes.join(" · ") || "(none yet)"
            }}</span>
          </p>
          <p v-if="!httpRoutes.length" class="text-slate-400">
            {{ PANEL_EMPTY.http }}
          </p>
          <template v-else>
            <p class="text-slate-400">
              The browser build lists a script's route table but does not bind a socket.
              Run the same script under the CLI, then call it:
            </p>
            <div class="flex flex-wrap gap-2">
              <label class="sr-only" for="studio-method">Method</label>
              <select
                id="studio-method"
                v-model="httpMethod"
                class="bg-slate-900 border border-slate-700 rounded px-2 py-1"
              >
                <option>GET</option>
                <option>POST</option>
                <option>PUT</option>
                <option>DELETE</option>
              </select>
              <label class="sr-only" for="studio-path">Request path</label>
              <input
                id="studio-path"
                v-model="httpPath"
                class="flex-1 bg-slate-900 border border-slate-700 rounded px-2 py-1 font-mono"
              />
            </div>
            <pre
              class="overflow-x-auto rounded bg-black/40 p-3 font-mono text-xs"
            ><code><span v-for="(t, i) in curlTokens" :key="i" :class="`tok-${t.kind}`">{{ t.text }}</span></code></pre>
            <button class="btn" @click="flash('curl', curlCommand)">
              {{ copied === "curl" ? "Copied" : "Copy curl" }}
            </button>
          </template>
        </div>

        <pre
          v-else
          class="flex-1 overflow-auto p-4 text-xs whitespace-pre-wrap"
        ><code><span v-for="(t, i) in shownTokens" :key="i" :class="`tok-${t.kind}`">{{ t.text }}</span></code></pre>
      </section>
    </main>
  </div>
</template>

<style scoped>
.btn {
  @apply bg-slate-900 border border-slate-700 rounded px-3 py-1 text-sm hover:border-rite-accent disabled:opacity-50;
}
.sr-only {
  @apply absolute w-px h-px p-0 -m-px overflow-hidden whitespace-nowrap border-0;
  clip: rect(0, 0, 0, 0);
}
</style>
