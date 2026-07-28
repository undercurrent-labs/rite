<script setup lang="ts">
import { ref, watch } from "vue";
import { EXAMPLES } from "./examples";
import { decodeShare, encodeShare } from "./share";
import { convertWithMap, formatOutput, riteCall } from "./riteApi";

const source = ref(`◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
`);
const dialect = ref<"glyph" | "ascii">("glyph");
const panel = ref<"out" | "diag" | "ast" | "ir" | "rust" | "http" | "trace">("out");
const output = ref(
  "Ready. Hosted Studio runs pure scripts in-browser via WASM (console, arithmetic, match…). Full FS/HTTP needs local `rite studio`."
);
const busy = ref(false);
const apiBase = ref(import.meta.env.VITE_RITE_API || "");
const cursor = ref({ line: 0, character: 0 });
const httpRoutes = ref<string[]>([]);
const httpMethod = ref("GET");
const httpPath = ref("/health");
const httpBody = ref("{}");
const httpResponse = ref("");

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

async function run() {
  panel.value = "out";
  const j: any = await api("/api/v1/run", { source: source.value });
  output.value = formatOutput("/api/v1/run", j);
  if (j?.virtual_http?.routes) {
    httpRoutes.value = j.virtual_http.routes;
    panel.value = "http";
    httpResponse.value =
      "Virtual HTTP session ready. Full request replay needs local `rite studio`; routes listed below.";
  }
}
async function check() {
  panel.value = "diag";
  const j = await api("/api/v1/analyze", { source: source.value });
  output.value = formatOutput("/api/v1/analyze", j);
}
async function format() {
  const mapped = await convertWithMap(apiBase.value, source.value, dialect.value, cursor.value);
  source.value = mapped.text;
  cursor.value = { line: mapped.line, character: mapped.character };
  output.value = formatOutput("/api/v1/format", mapped.raw);
}
async function emitRust() {
  panel.value = "rust";
  const j: any = await api("/api/v1/emit-rust", { source: source.value });
  output.value = j.rust || formatOutput("/api/v1/emit-rust", j);
}
async function showAst() {
  panel.value = "ast";
  const j = await api("/api/v1/parse", { source: source.value });
  output.value = formatOutput("/api/v1/parse", j);
}
async function sendHttp() {
  // Local studio may support session APIs later; for now echo virtual plan
  httpResponse.value = JSON.stringify(
    {
      method: httpMethod.value,
      path: httpPath.value,
      body: httpBody.value,
      routes: httpRoutes.value,
      note: "Hosted WASM returns virtual routes; full request replay uses local rite studio + native listen.",
    },
    null,
    2
  );
  panel.value = "http";
}
function loadExample(id: string) {
  const ex = EXAMPLES.find((e) => e.id === id);
  if (ex) {
    source.value = dialect.value === "ascii" && ex.ascii ? ex.ascii : ex.source;
    output.value = `Loaded ${ex.title}\n${ex.description}`;
  }
}
function onCursor(ev: Event) {
  const el = ev.target as HTMLTextAreaElement;
  const pos = el.selectionStart ?? 0;
  const before = source.value.slice(0, pos);
  const lines = before.split("\n");
  cursor.value = {
    line: lines.length - 1,
    character: (lines[lines.length - 1] ?? "").length,
  };
}
</script>

<template>
  <div class="h-full min-h-0 flex flex-col bg-rite-bg text-slate-100">
    <header class="flex flex-wrap items-center gap-2 border-b border-slate-800 px-3 py-2 shrink-0">
      <span class="text-sm text-slate-500 mr-auto hidden sm:inline">Playground</span>
      <select
        class="bg-slate-900 border border-slate-700 rounded px-2 py-1 text-sm"
        @change="loadExample(($event.target as HTMLSelectElement).value)"
      >
        <option value="">Examples…</option>
        <option v-for="ex in EXAMPLES" :key="ex.id" :value="ex.id">{{ ex.title }}</option>
      </select>
      <select v-model="dialect" class="bg-slate-900 border border-slate-700 rounded px-2 py-1 text-sm">
        <option value="glyph">glyph</option>
        <option value="ascii">ascii</option>
      </select>
      <button class="btn" :disabled="busy" @click="run">Run</button>
      <button class="btn" :disabled="busy" @click="check">Check</button>
      <button class="btn" :disabled="busy" @click="format">Format</button>
      <button class="btn" :disabled="busy" @click="showAst">AST</button>
      <button class="btn" :disabled="busy" @click="emitRust">Rust</button>
    </header>

    <main class="flex-1 grid md:grid-cols-2 min-h-0 overflow-hidden">
      <textarea
        v-model="source"
        class="w-full h-[40vh] md:h-full bg-rite-panel p-4 font-mono text-sm outline-none border-0 resize-none"
        spellcheck="false"
        @click="onCursor"
        @keyup="onCursor"
      />
      <section class="flex flex-col border-l border-slate-800 min-h-[35vh] md:min-h-0 overflow-hidden">
        <div class="flex flex-wrap gap-1 p-2 border-b border-slate-800 text-sm">
          <button
            v-for="t in ['out', 'diag', 'ast', 'ir', 'rust', 'http', 'trace']"
            :key="t"
            class="px-2 py-1 rounded border border-transparent"
            :class="panel === t ? 'border-rite-pink text-rite-pink' : 'text-slate-400'"
            @click="panel = t as any"
          >
            {{ t }}
          </button>
        </div>
        <div v-if="panel === 'http'" class="p-3 space-y-2 text-sm border-b border-slate-800">
          <div class="flex flex-wrap gap-2">
            <select v-model="httpMethod" class="bg-slate-900 border border-slate-700 rounded px-2 py-1">
              <option>GET</option>
              <option>POST</option>
              <option>PUT</option>
              <option>DELETE</option>
            </select>
            <input v-model="httpPath" class="flex-1 bg-slate-900 border border-slate-700 rounded px-2 py-1 font-mono" />
            <button class="btn" @click="sendHttp">Send</button>
          </div>
          <textarea v-model="httpBody" class="w-full h-20 bg-slate-900 border border-slate-700 rounded p-2 font-mono text-xs" />
          <div class="text-xs text-slate-400">Routes: {{ httpRoutes.join(" · ") || "(none yet — Run an @http.listen script)" }}</div>
          <pre class="text-xs whitespace-pre-wrap">{{ httpResponse }}</pre>
        </div>
        <pre v-else class="flex-1 overflow-auto p-4 text-xs text-slate-300 whitespace-pre-wrap">{{ output }}</pre>
      </section>
    </main>
  </div>
</template>

<style scoped>
.btn {
  @apply bg-slate-900 border border-slate-700 rounded px-3 py-1 text-sm hover:border-rite-accent disabled:opacity-50;
}
</style>
