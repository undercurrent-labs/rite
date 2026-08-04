<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { highlightCant, highlightRite } from "../lib/highlight";
import { renderGraphSvg } from "../lib/graphLayout";
import {
  EXAMPLES,
  loadEngine,
  type CheckResult,
  type Diagnostic,
  type ExpandResult,
  type ExplainResult,
  type GraphResult,
  type RunResult,
} from "../lib/studio";
import { renderDescription } from "../lib/operators";

type Panel = "output" | "graph" | "rite" | "explain";

const PANELS: { id: Panel; label: string }[] = [
  { id: "output", label: "Output" },
  { id: "graph", label: "Graph" },
  { id: "rite", label: "Rite" },
  { id: "explain", label: "Explain" },
];

const source = ref(EXAMPLES[0].source);
const example = ref(0);
const spelling = ref<"ascii" | "glyph">("ascii");
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

/** Lay the program out, in the current spelling. */
async function format() {
  const engine = await loadEngine();
  if (!engine) return;
  const result = engine.cant_format(source.value, spelling.value);
  if (result.ok && result.text) source.value = result.text;
}

async function respell() {
  const engine = await loadEngine();
  if (!engine) return;
  // Format when the program parses — that lays it out as well as spelling it.
  // When it does not, fall back to the token-level conversion, which is safe on
  // a half-written program: the spelling toggle has to keep working while typing.
  const formatted = engine.cant_format(source.value, spelling.value);
  source.value =
    formatted.ok && formatted.text
      ? formatted.text
      : engine.cant_convert(source.value, spelling.value);
}

function loadExample(index: number) {
  example.value = index;
  source.value = EXAMPLES[index].source;
  spelling.value = "ascii";
  runResult.value = null;
  panel.value = "output";
}

onMounted(analyze);
watch(source, () => {
  void analyze();
});

/**
 * The highlighted layer sits under a transparent textarea, so the two have to
 * scroll as one. Anything else and the colours slide off the characters the
 * moment a program is taller or wider than the pane.
 */
const mirror = ref<HTMLElement | null>(null);
function syncScroll(event: Event) {
  const area = event.target as HTMLTextAreaElement;
  if (!mirror.value) return;
  mirror.value.scrollTop = area.scrollTop;
  mirror.value.scrollLeft = area.scrollLeft;
}

const highlighted = computed(() => highlightCant(source.value));
const generatedHtml = computed(() =>
  expansion.value?.rite ? highlightRite(expansion.value.rite) : ""
);
const ranHtml = computed(() =>
  runResult.value?.rite ? highlightRite(runResult.value.rite) : ""
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

/** The label that points at the thing itself, rather than at what followed it. */
function primaryLabel(d: Diagnostic): string | undefined {
  const label = d.labels?.find((l) => l.primary) ?? d.labels?.[0];
  return label?.message;
}
const problemCount = computed(
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

const valueText = computed(() => (runResult.value ? show(runResult.value.value) : ""));

/** Whether the editor still holds the example it was given. */
const untouched = computed(() => source.value === EXAMPLES[example.value].source);
</script>

<template>
  <div class="flex h-full min-h-0 flex-col">
    <!-- Toolbar -->
    <header
      class="flex shrink-0 flex-wrap items-center gap-2 border-b border-cant-border px-4 py-2"
    >
      <span class="hidden text-sm text-slate-500 sm:inline">Playground</span>
      <!--
        Status on the left, so the primary action stays flush with the right edge
        and lines up with the nav above it. A chip after `Run` pushed the button
        40px inward and the two bars stopped agreeing.
      -->
      <span
        v-if="check"
        class="mr-auto font-mono text-xs"
        :class="check.ok ? 'text-cant-green' : 'text-rose-400'"
      >
        {{ check.ok ? "ok" : `${problemCount} problem${problemCount === 1 ? "" : "s"}` }}
      </span>
      <span v-else class="mr-auto"></span>

      <label class="sr-only" for="cant-example">Example</label>
      <select
        id="cant-example"
        class="studio-input"
        :value="example"
        @change="loadExample(Number(($event.target as HTMLSelectElement).value))"
      >
        <option v-for="(ex, i) in EXAMPLES" :key="ex.name" :value="i">
          {{ ex.name }}
        </option>
      </select>

      <label class="sr-only" for="cant-spelling">Spelling</label>
      <select id="cant-spelling" v-model="spelling" class="studio-input" @change="respell">
        <option value="ascii">ascii</option>
        <option value="glyph">glyph</option>
      </select>

      <button
        type="button"
        class="studio-btn"
        :disabled="engineState !== 'ready'"
        @click="format"
      >
        Format
      </button>
      <button
        type="button"
        class="studio-btn studio-btn-primary"
        :disabled="running || engineState !== 'ready'"
        @click="run"
      >
        {{ running ? "running…" : "Run" }}
      </button>

    </header>

    <div
      v-if="engineState === 'missing'"
      class="shrink-0 border-b border-amber-500/40 bg-amber-500/10 px-4 py-2 text-sm text-amber-200"
    >
      The engine did not load. In a local checkout, build it first:
      <code class="font-mono">pnpm cant:wasm</code>.
    </div>

    <!-- Editor | panels -->
    <main class="grid min-h-0 flex-1 overflow-hidden lg:grid-cols-2">
      <section class="flex min-h-0 min-w-0 flex-col border-b border-cant-border lg:border-b-0">
        <div
          class="flex shrink-0 items-center justify-between gap-4 border-b border-cant-border/60 px-4 py-1.5"
        >
          <span class="font-mono text-xs uppercase tracking-wider text-slate-500">
            program.cant
          </span>
          <span v-if="untouched" class="truncate text-xs text-slate-600">
            {{ EXAMPLES[example].blurb }}
          </span>
        </div>

        <!--
          A textarea over a highlighted <pre>, in identical monospace metrics. A
          real editor component would be a dependency and a lot of behaviour to
          own; this is a text box that looks like the rest of the site.
        -->
        <div class="relative min-h-0 flex-1">
          <pre
            ref="mirror"
            class="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre p-4 font-mono text-sm leading-relaxed"
            aria-hidden="true"
          ><code v-html="highlighted"></code></pre>
          <textarea
            v-model="source"
            spellcheck="false"
            autocapitalize="off"
            autocomplete="off"
            wrap="off"
            class="absolute inset-0 h-full w-full resize-none overflow-auto whitespace-pre bg-transparent p-4 font-mono text-sm leading-relaxed text-transparent caret-cant-accent outline-none"
            aria-label="Cant program"
            @scroll="syncScroll"
          ></textarea>
        </div>

        <!-- Problems, where an editor would put them. -->
        <div
          v-if="check && !check.ok"
          class="max-h-44 shrink-0 space-y-1.5 overflow-auto border-t border-cant-border bg-cant-panel/40 p-3"
        >
          <div v-for="(d, i) in diagnostics" :key="i" class="text-sm leading-relaxed">
            <span class="font-mono text-xs text-rose-300">{{ d.code }}</span>
            <!--
              Diagnostics quote code in backticks, the way a terminal does. The
              site already knows how to turn those into code spans; rendering
              them raw put literal backticks in front of every error.
            -->
            <span class="ml-2 text-slate-200" v-html="renderDescription(d.title ?? '')"></span>
            <span
              v-if="primaryLabel(d)"
              class="ml-2 text-xs text-slate-400"
              v-html="`— ${renderDescription(primaryLabel(d) ?? '')}`"
            ></span>
            <span
              v-if="d.help"
              class="ml-2 text-xs text-slate-500"
              v-html="`help: ${renderDescription(d.help)}`"
            ></span>
            <span v-if="d.rite?.code" class="ml-2 font-mono text-xs text-slate-600">
              from Rite: {{ d.rite.code }}
            </span>
          </div>
        </div>
      </section>

      <section class="flex min-h-0 min-w-0 flex-col border-cant-border lg:border-l">
        <div class="flex shrink-0 gap-1 border-b border-cant-border px-3 py-1.5" role="tablist">
          <button
            v-for="tab in PANELS"
            :key="tab.id"
            type="button"
            role="tab"
            :aria-selected="panel === tab.id"
            class="rounded px-2.5 py-1 text-sm transition-colors"
            :class="
              panel === tab.id
                ? 'bg-slate-800/80 text-cant-accent'
                : 'text-slate-500 hover:text-slate-300'
            "
            @click="panel = tab.id"
          >
            {{ tab.label }}
          </button>
        </div>

        <div class="min-h-0 flex-1 overflow-auto p-4">
          <!-- Output -->
          <div v-show="panel === 'output'" class="space-y-4">
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
                  class="overflow-x-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-xs leading-relaxed"
                ><code v-html="ranHtml"></code></pre>
              </div>
            </template>
          </div>

          <!-- Graph -->
          <div v-show="panel === 'graph'">
            <div v-if="graphSvg" class="overflow-auto" v-html="graphSvg"></div>
            <p v-else class="text-sm text-slate-500">Nothing to draw yet.</p>
            <p class="mt-4 max-w-prose text-xs leading-relaxed text-slate-500">
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
                class="overflow-x-auto rounded-lg border border-cant-border bg-cant-panel p-3 font-mono text-xs leading-relaxed"
              ><code v-html="generatedHtml"></code></pre>
              <p class="mt-3 max-w-prose text-xs leading-relaxed text-slate-500">
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
              class="whitespace-pre-wrap font-mono text-xs leading-relaxed text-slate-300"
            >{{ explainResult.text }}</pre>
            <div v-if="explainResult?.capabilities?.length" class="mt-4 text-sm text-slate-400">
              Capabilities:
              <code
                v-for="cap in explainResult.capabilities"
                :key="cap"
                class="ml-1 font-mono text-cant-cyan"
                >{{ cap }}</code
              >
            </div>
          </div>
        </div>
      </section>
    </main>
  </div>
</template>
