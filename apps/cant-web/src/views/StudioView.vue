<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { highlightCant, highlightRite } from "../lib/highlight";
import { renderGraphSvg } from "../lib/graphLayout";
import {
  EXAMPLES,
  loadEngine,
  type CheckResult,
  type ExpandResult,
  type ExplainResult,
  type GraphResult,
  type RunResult,
} from "../lib/studio";

type Panel = "output" | "graph" | "rite" | "explain";

const source = ref(EXAMPLES[0].source);
const panel = ref<Panel>("output");
const engineState = ref<"loading" | "ready" | "missing">("loading");
const running = ref(false);

const check = ref<CheckResult | null>(null);
const expansion = ref<ExpandResult | null>(null);
const runResult = ref<RunResult | null>(null);
const graphResult = ref<GraphResult | null>(null);
const explainResult = ref<ExplainResult | null>(null);

/**
 * Everything but `run` is cheap and re-derived on every keystroke, so the
 * diagnostics and the picture follow the cursor. Running is explicit: a program
 * can print, and printing on every keystroke would be unpleasant.
 */
async function analyze() {
  const engine = await loadEngine();
  if (!engine) {
    engineState.value = "missing";
    return;
  }
  engineState.value = "ready";
  check.value = engine.cant_check(source.value);
  expansion.value = engine.cant_expand(source.value);
  graphResult.value = engine.cant_graph(source.value);
  explainResult.value = engine.cant_explain(source.value);
}

async function run() {
  const engine = await loadEngine();
  if (!engine) return;
  running.value = true;
  try {
    runResult.value = engine.cant_run(source.value);
    panel.value = "output";
  } finally {
    running.value = false;
  }
}

async function convert(dialect: "ascii" | "glyph") {
  const engine = await loadEngine();
  if (!engine) return;
  // Format when the program parses — that lays it out as well as spelling it.
  // When it does not, fall back to the token-level conversion, which is safe on
  // a half-written program: a spelling toggle has to keep working while typing.
  const formatted = engine.cant_format(source.value, dialect);
  source.value =
    formatted.ok && formatted.text
      ? formatted.text
      : engine.cant_convert(source.value, dialect);
}

function load(index: number) {
  source.value = EXAMPLES[index].source;
  runResult.value = null;
  panel.value = "output";
}

onMounted(analyze);
watch(source, () => {
  void analyze();
});

const highlighted = computed(() => highlightCant(source.value));
const expansionHtml = computed(() =>
  runResult.value?.rite ? highlightRite(runResult.value.rite) : ""
);
/** The expansion is available without running: it is a property of the text. */
const generatedHtml = computed(() =>
  expansion.value?.rite ? highlightRite(expansion.value.rite) : ""
);

const graphSvg = computed(() => {
  const graph = graphResult.value?.graph;
  if (!graph || !graph.nodes?.length) return "";
  try {
    return renderGraphSvg(graph);
  } catch {
    // A picture is a convenience. If the layout cannot draw something, the JSON
    // and the diagnostics are still right, and a blank panel beats a blank page.
    return "";
  }
});

const diagnostics = computed(() => check.value?.diagnostics ?? []);
const errorCount = computed(
  () => diagnostics.value.filter((d) => d.severity !== "warning").length
);

/**
 * A value, spelled the way Cant would print it.
 *
 * `JSON.stringify` alone gives `[2,4,6]`, and the CLI prints `[2, 4, 6]`. Two
 * spellings of the same answer across two tools is a small thing that makes
 * people doubt whether it *is* the same answer.
 */
function show(value: unknown): string {
  if (value === null || value === undefined) return "none";
  if (Array.isArray(value)) return `[${value.map(show).join(", ")}]`;
  if (typeof value === "object") {
    const pairs = Object.entries(value as Record<string, unknown>).map(
      ([k, v]) => `${k}: ${show(v)}`
    );
    return `<< ${pairs.join(", ")} >>`;
  }
  return JSON.stringify(value);
}

const valueText = computed(() =>
  runResult.value ? show(runResult.value.value) : ""
);
</script>

<template>
  <div class="mx-auto max-w-7xl px-4 py-8">
    <header class="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold tracking-tight text-white">Studio</h1>
        <p class="mt-1 max-w-prose text-sm text-slate-400">
          Cant in the page. The same crate the command line uses, compiled to
          WebAssembly — it expands your program to Rite and runs the Rite, exactly
          as <code class="font-mono text-slate-300">cant run</code> does. Nothing
          you type leaves the browser.
        </p>
      </div>
      <div class="flex flex-wrap gap-2">
        <button
          v-for="(example, i) in EXAMPLES"
          :key="example.name"
          type="button"
          class="rounded border border-cant-border px-2.5 py-1 text-xs text-slate-400 hover:border-cant-accent/40 hover:text-slate-100"
          :title="example.blurb"
          @click="load(i)"
        >
          {{ example.name }}
        </button>
      </div>
    </header>

    <div
      v-if="engineState === 'missing'"
      class="mb-6 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm text-amber-200"
    >
      The engine did not load. In a local checkout, build it first:
      <code class="font-mono">pnpm cant:wasm</code>.
    </div>

    <div class="grid gap-5 lg:grid-cols-2">
      <!-- Editor -->
      <section>
        <div class="mb-2 flex items-center justify-between">
          <span class="font-mono text-xs uppercase tracking-wider text-slate-500">
            program.cant
          </span>
          <div class="flex gap-2">
            <button
              type="button"
              class="rounded border border-cant-border px-2 py-0.5 text-xs text-slate-400 hover:text-slate-100"
              @click="convert('ascii')"
            >
              ascii
            </button>
            <button
              type="button"
              class="rounded border border-cant-border px-2 py-0.5 text-xs text-slate-400 hover:text-slate-100"
              @click="convert('glyph')"
            >
              glyph
            </button>
          </div>
        </div>

        <!--
          A textarea over a highlighted <pre>, both in the same monospace metrics.
          A real editor component would be a dependency and a lot of behaviour to
          own; this is a text box that looks like the rest of the site.
        -->
        <div class="relative min-h-[9rem] rounded-lg border border-cant-border bg-cant-panel">
          <pre
            class="pointer-events-none overflow-x-auto p-4 font-mono text-sm leading-relaxed"
            aria-hidden="true"
          ><code v-html="highlighted"></code><br /></pre>
          <textarea
            v-model="source"
            spellcheck="false"
            autocapitalize="off"
            autocomplete="off"
            class="absolute inset-0 h-full w-full resize-none bg-transparent p-4 font-mono text-sm leading-relaxed text-transparent caret-cant-accent outline-none"
            aria-label="Cant program"
          ></textarea>
        </div>

        <div class="mt-3 flex items-center gap-3">
          <button
            type="button"
            class="rounded-md border border-cant-accent/40 bg-cant-accent/10 px-4 py-1.5 text-sm font-medium text-cant-accent hover:bg-cant-accent/20 disabled:opacity-50"
            :disabled="running || engineState !== 'ready'"
            @click="run"
          >
            {{ running ? "running…" : "Run" }}
          </button>
          <span v-if="check" class="text-xs" :class="check.ok ? 'text-cant-green' : 'text-rose-400'">
            {{ check.ok ? "ok" : `${errorCount} problem${errorCount === 1 ? "" : "s"}` }}
          </span>
        </div>

        <!-- Diagnostics -->
        <div v-if="check && !check.ok" class="mt-4 space-y-2">
          <div
            v-for="(d, i) in diagnostics"
            :key="i"
            class="rounded-lg border border-rose-500/30 bg-rose-500/5 p-3 text-sm"
          >
            <div class="font-mono text-xs text-rose-300">{{ d.code }}</div>
            <div class="mt-1 text-slate-200">{{ d.message }}</div>
            <div v-if="d.help" class="mt-1.5 text-xs text-slate-400">help: {{ d.help }}</div>
            <div v-if="d.rite?.code" class="mt-1.5 font-mono text-xs text-slate-500">
              from Rite: {{ d.rite.code }}
            </div>
          </div>
        </div>
      </section>

      <!-- Panels -->
      <section class="min-w-0">
        <div class="mb-2 flex gap-1 border-b border-cant-border">
          <button
            v-for="tab in (['output', 'graph', 'rite', 'explain'] as Panel[])"
            :key="tab"
            type="button"
            class="-mb-px border-b-2 px-3 py-1.5 text-sm capitalize transition-colors"
            :class="
              panel === tab
                ? 'border-cant-accent text-cant-accent'
                : 'border-transparent text-slate-500 hover:text-slate-300'
            "
            @click="panel = tab"
          >
            {{ tab === "rite" ? "Rite" : tab }}
          </button>
        </div>

        <!-- Output -->
        <div v-show="panel === 'output'" class="space-y-3">
          <p v-if="!runResult" class="text-sm text-slate-500">
            Press <span class="text-slate-300">Run</span>. The value, anything printed,
            and the Rite that produced them appear here.
          </p>
          <template v-else>
            <div
              v-if="runResult.error"
              class="rounded-lg border border-rose-500/30 bg-rose-500/5 p-3 text-sm text-rose-200"
            >
              {{ runResult.error }}
            </div>
            <div v-if="runResult.stdout">
              <div class="mb-1 font-mono text-xs uppercase tracking-wider text-slate-500">
                stdout
              </div>
              <pre
                class="overflow-x-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-sm text-slate-300"
              >{{ runResult.stdout }}</pre>
            </div>
            <div v-if="runResult.ok">
              <div class="mb-1 font-mono text-xs uppercase tracking-wider text-slate-500">
                value
              </div>
              <pre
                class="overflow-x-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-sm text-cant-green"
              >{{ valueText }}</pre>
            </div>
            <div v-if="runResult.rite">
              <div class="mb-1 font-mono text-xs uppercase tracking-wider text-slate-500">
                what ran
              </div>
              <pre
                class="max-h-80 overflow-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-xs leading-relaxed"
              ><code v-html="expansionHtml"></code></pre>
            </div>
          </template>
        </div>

        <!-- Graph -->
        <div v-show="panel === 'graph'">
          <div
            v-if="graphSvg"
            class="overflow-auto rounded-lg border border-cant-border bg-cant-panel p-3"
            v-html="graphSvg"
          ></div>
          <p v-else class="text-sm text-slate-500">Nothing to draw yet.</p>
          <p class="mt-2 text-xs text-slate-500">
            Clusters are subgraphs — a fork branch or an orbit body. Dashed edges enter
            and rejoin them; the pink edge is an orbit's feedback, the only cycle a Cant
            program can contain. The same shape
            <code class="font-mono">cant graph</code> emits as JSON.
          </p>
        </div>

        <!-- Generated Rite -->
        <div v-show="panel === 'rite'">
          <template v-if="expansion?.rite">
            <pre
              class="max-h-[32rem] overflow-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-xs leading-relaxed"
            ><code v-html="generatedHtml"></code></pre>
            <p class="mt-2 text-xs text-slate-500">
              Ordinary Rite, and exactly what runs — this is
              <code class="font-mono">cant expand</code>. Every generated name carries
              the prefix
              <code class="font-mono text-slate-400">{{ expansion.prefix }}</code
              >, so it cannot collide with anything you wrote.
            </p>
          </template>
          <p v-else class="text-sm text-slate-500">
            No expansion: Cant does not generate Rite for a program it has already
            rejected, because printing a guess as though it were the program is how an
            audit tool starts lying.
          </p>
        </div>

        <!-- Explain -->
        <div v-show="panel === 'explain'">
          <pre
            v-if="explainResult?.text"
            class="overflow-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-xs leading-relaxed text-slate-300"
          >{{ explainResult.text }}</pre>
          <div
            v-if="explainResult?.capabilities?.length"
            class="mt-3 text-sm text-slate-400"
          >
            Capabilities:
            <code
              v-for="cap in explainResult.capabilities"
              :key="cap"
              class="ml-1 font-mono text-cant-cyan"
              >{{ cap }}</code
            >
          </div>
        </div>
      </section>
    </div>
  </div>
</template>
