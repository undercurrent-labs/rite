<script setup lang="ts">
/**
 * The chamber.
 *
 * Three columns on a desktop — source, canvas, Codex — with the canvas dominant
 * and every panel collapsible so the artifact can take the whole viewport
 * (§20.2). On a narrow screen the panels become sheets and nothing depends on
 * hover.
 *
 * Everything happens here. There is no server call in this file and none in the
 * app: the renderer is WebAssembly in this tab, the source never leaves it, and
 * nothing is written to a URL. See `docs/adr/0007-veil-and-source-privacy.md`.
 */
import { computed, onMounted, ref, watch } from "vue";
import {
  defaultOptions,
  isCurrent,
  nextGeneration,
  renderCant,
  renderGraph,
  version,
  type Diagnostic,
  type RenderResult,
} from "./lib/renderer";
import SigilCanvas from "./components/SigilCanvas.vue";
import CodexPanel from "./components/CodexPanel.vue";
import ControlBar from "./components/ControlBar.vue";
import GalleryPanel from "./components/GalleryPanel.vue";

const examples = __SIGIL_EXAMPLES__;

const tab = ref<"cant" | "graph">("cant");
const source = ref(examples.find((e) => e.name === "complex")?.source ?? examples[0]?.source ?? "");
const graphText = ref("");
const options = ref(defaultOptions());
const liveRender = ref(true);

const result = ref<RenderResult | null>(null);
const rendering = ref(false);
const engineError = ref<string | null>(null);
const versions = ref<Awaited<ReturnType<typeof version>>>(null);

const showSource = ref(true);
const showCodex = ref(false);
const deepVeil = ref(false);
const selected = ref<string | null>(null);
const showGallery = ref(false);

/**
 * What an export will actually contain (§20.7).
 *
 * Shown rather than assumed, because the two axes that decide it are
 * independent: `--mode revealed --metadata none` draws the labels and embeds
 * nothing, which is meaningful and is also what someone picks having confused
 * "hide it" with "do not embed it".
 */
const exportNotice = computed(() => {
  const mode = options.value.mode;
  const metadata = options.value.metadata;
  if (mode !== "veiled" && metadata === "none") {
    return {
      tone: "warn" as const,
      text: `${mode} draws your labels into the artifact — metadata none only stops them being embedded`,
    };
  }
  if (mode !== "veiled") {
    return { tone: "warn" as const, text: `${mode} draws labels into the artifact` };
  }
  if (metadata === "none") {
    return { tone: "safe" as const, text: "veiled, nothing embedded — no source in the file" };
  }
  if (metadata === "full") {
    return { tone: "warn" as const, text: "veiled picture, but full metadata is embedded" };
  }
  return { tone: "safe" as const, text: "veiled, semantic metadata only — no source snippets" };
});

const diagnostics = computed<Diagnostic[]>(() => result.value?.diagnostics ?? []);
const errors = computed(() => diagnostics.value.filter((d) => d.severity === "error"));
const warnings = computed(() => diagnostics.value.filter((d) => d.severity !== "error"));

/**
 * Debounced, discarding, and generation-checked.
 *
 * A render that finishes after a newer one started is dropped rather than
 * painted — §19.2's stale-render storm, which on a fast typist is otherwise a
 * canvas that flickers between two programs.
 */
let debounce: number | undefined;

function scheduleRender(delay = 220) {
  window.clearTimeout(debounce);
  debounce = window.setTimeout(render, delay);
}

async function render() {
  const token = nextGeneration();
  rendering.value = true;
  const current = { ...options.value };
  const outcome =
    tab.value === "cant"
      ? await renderCant("sigil.cant", source.value, current)
      : await renderGraph(graphText.value, current);

  if (!isCurrent(token)) return;
  result.value = outcome;
  // A selection points at a node in the picture that was on screen. After a new
  // render it may name something that is no longer there, and a highlight over a
  // node the user did not choose is worse than no highlight.
  selected.value = null;
  rendering.value = false;
}

onMounted(async () => {
  versions.value = await version();
  if (!versions.value) {
    engineError.value =
      "the renderer did not load — the WASM package may be missing from public/wasm";
  }
  render();
});

watch([source, graphText, tab], () => {
  if (liveRender.value) scheduleRender();
});
// Options are cheap and deliberate: render at once rather than after a pause.
watch(options, () => render(), { deep: true });

function useExample(name: string) {
  const example = examples.find((e) => e.name === name);
  if (!example) return;
  tab.value = "cant";
  source.value = example.source;
  render();
}

/** Exports are built here and downloaded here. Nothing is uploaded. */
function download(content: string, filename: string, type: string) {
  const blob = new Blob([content], { type });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

function exportSvg() {
  if (result.value?.svg) download(result.value.svg, "sigil.svg", "image/svg+xml");
}

function exportScene() {
  if (result.value?.sceneJson)
    download(result.value.sceneJson, "sigil.scene.json", "application/json");
}

/**
 * PNG through a canvas, in this tab.
 *
 * The native CLI rasterises with `resvg`; the browser has a rasteriser already
 * and shipping a second one into the WASM bundle to avoid using it would be a
 * megabyte spent on nothing. §4.3 permits a browser-specific rasterisation
 * fallback for exactly this.
 */
async function exportPng() {
  const svg = result.value?.svg;
  if (!svg) return;
  const blob = new Blob([svg], { type: "image/svg+xml" });
  const url = URL.createObjectURL(blob);
  try {
    const image = new Image();
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error("the artifact could not be rasterised"));
      image.src = url;
    });
    const canvas = document.createElement("canvas");
    canvas.width = 1600;
    canvas.height = 1600;
    const context = canvas.getContext("2d");
    if (!context) return;
    context.drawImage(image, 0, 0, 1600, 1600);
    canvas.toBlob((png) => {
      if (!png) return;
      const pngUrl = URL.createObjectURL(png);
      const anchor = document.createElement("a");
      anchor.href = pngUrl;
      anchor.download = "sigil.png";
      anchor.click();
      URL.revokeObjectURL(pngUrl);
    }, "image/png");
  } finally {
    URL.revokeObjectURL(url);
  }
}

async function copySvg() {
  if (result.value?.svg) await navigator.clipboard.writeText(result.value.svg);
}

async function copyFingerprint() {
  if (result.value?.fingerprint) await navigator.clipboard.writeText(result.value.fingerprint);
}
</script>

<template>
  <div class="flex h-screen flex-col overflow-hidden">
    <header
      class="panel flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b px-4 py-2"
    >
      <h1 class="text-sm tracking-[0.3em] text-cyan">SIGIL</h1>
      <p class="hidden text-[0.65rem] text-spectral/40 sm:block">
        a program&rsquo;s topology as a ritual artifact
      </p>

      <div class="ml-auto flex items-center gap-2">
        <button
          class="instrument"
          :aria-pressed="showSource"
          @click="showSource = !showSource"
        >
          Source
        </button>
        <button class="instrument" :aria-pressed="showCodex" @click="showCodex = !showCodex">
          Codex
        </button>
        <button
          class="instrument"
          :aria-pressed="showGallery"
          @click="showGallery = !showGallery"
        >
          Gallery
        </button>
        <span class="hidden text-[0.6rem] text-spectral/30 lg:inline">
          {{ versions?.renderer ? `renderer ${versions.renderer}` : "" }}
        </span>
      </div>
    </header>

    <ControlBar v-model:options="options" v-model:deep-veil="deepVeil" />

    <div class="flex min-h-0 flex-1 flex-col lg:flex-row">
      <!-- Source -->
      <section
        v-if="showSource"
        class="panel fixed inset-x-0 bottom-0 z-20 flex max-h-[70vh] flex-col border-t
               lg:static lg:z-auto lg:max-h-none lg:w-[26rem] lg:min-h-0 lg:shrink-0
               lg:border-r lg:border-t-0"
        aria-label="Source"
      >
        <div class="flex shrink-0 gap-1 border-b border-ultraviolet/20 px-2 py-1.5">
          <button
            class="instrument"
            :class="{ 'is-active': tab === 'cant' }"
            @click="tab = 'cant'"
          >
            Cant
          </button>
          <button
            class="instrument"
            :class="{ 'is-active': tab === 'graph' }"
            @click="tab = 'graph'"
          >
            Graph JSON
          </button>
          <button class="instrument ml-auto" @click="render">Render</button>
        </div>

        <textarea
          v-if="tab === 'cant'"
          v-model="source"
          spellcheck="false"
          aria-label="Cant source"
          class="min-h-[10rem] flex-1 resize-none bg-transparent p-3 text-xs leading-relaxed
                 text-spectral/90 outline-none"
        />
        <textarea
          v-else
          v-model="graphText"
          spellcheck="false"
          aria-label="Cant graph JSON"
          placeholder="Paste the output of `cant graph program.cant --format json`"
          class="min-h-[10rem] flex-1 resize-none bg-transparent p-3 text-xs leading-relaxed
                 text-spectral/90 outline-none placeholder:text-spectral/25"
        />

        <div class="shrink-0 border-t border-ultraviolet/20 p-2">
          <span class="instrument-label">Examples</span>
          <div class="flex flex-wrap gap-1">
            <button
              v-for="example in examples"
              :key="example.name"
              class="instrument"
              @click="useExample(example.name)"
            >
              {{ example.name }}
            </button>
          </div>
        </div>

        <div
          v-if="errors.length || warnings.length"
          class="max-h-40 shrink-0 overflow-y-auto border-t border-ultraviolet/20 p-2 text-[0.65rem]"
        >
          <p v-for="(d, i) in errors" :key="`e${i}`" class="mb-1 text-ember">
            <span class="opacity-60">{{ d.code }}</span> {{ d.message }}
          </p>
          <p v-for="(d, i) in warnings" :key="`w${i}`" class="mb-1 text-gold/80">
            <span class="opacity-60">{{ d.code }}</span> {{ d.message }}
          </p>
        </div>
      </section>

      <!-- The artifact -->
      <main class="relative min-h-0 flex-1">
        <SigilCanvas
          v-model:selected="selected"
          :svg="result?.svg"
          :scene-json="result?.sceneJson"
          :deep-veil="deepVeil"
          :rendering="rendering"
          :error="engineError"
        />
      </main>

      <!-- Codex -->
      <CodexPanel
        v-if="showCodex"
        :scene-json="result?.sceneJson"
        :summary="result?.summary"
        :fingerprint="result?.fingerprint"
        :elapsed-ms="result?.elapsedMs"
        :deep-veil="deepVeil"
        :selected="selected"
        @close="showCodex = false"
        @select="selected = $event"
      />
    </div>

    <p class="sr-only" role="status" aria-live="polite">{{ result?.summary }}</p>

    <GalleryPanel
      :open="showGallery"
      @open="useExample($event); showGallery = false"
      @close="showGallery = false"
    />

    <footer
      class="panel flex shrink-0 flex-wrap items-center gap-2 border-t px-3 py-1.5 text-[0.65rem]"
    >
      <span class="text-spectral/35">Export</span>
      <button class="instrument" :disabled="!result?.svg" @click="exportSvg">SVG</button>
      <button class="instrument" :disabled="!result?.svg" @click="exportPng">PNG</button>
      <button class="instrument" :disabled="!result?.sceneJson" @click="exportScene">
        Scene
      </button>
      <button class="instrument" :disabled="!result?.svg" @click="copySvg">Copy SVG</button>
      <button class="instrument" :disabled="!result?.fingerprint" @click="copyFingerprint">
        Copy fingerprint
      </button>

      <span
        class="ml-3 border-l border-ultraviolet/20 pl-3"
        :class="exportNotice.tone === 'warn' ? 'text-ember/80' : 'text-spectral/40'"
      >
        {{ exportNotice.text }}
      </span>

      <label class="ml-auto flex items-center gap-1.5 text-spectral/40">
        <input v-model="liveRender" type="checkbox" class="accent-cyan" />
        live
      </label>
      <span class="text-gold/60" title="Nothing here is uploaded.">
        renders in this tab &middot; source never leaves it
      </span>
    </footer>
  </div>
</template>
