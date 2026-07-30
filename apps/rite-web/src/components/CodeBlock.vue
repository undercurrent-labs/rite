<script setup lang="ts">
import { computed, ref } from "vue";
import { detectDialect, tokenize } from "@studio/highlight";
import type { CodeMode } from "../lib/markdown";
import { encodeShare } from "@studio/share";

const props = withDefaults(
  defineProps<{
    code: string;
    lang?: string;
    /** Fence annotation from the book; decides which run affordance appears. */
    mode?: CodeMode;
    /** Overrides the language chip (the home page labels its panes glyph/ascii). */
    label?: string;
  }>(),
  { lang: "", mode: "fragment", label: "" }
);

const source = computed(() => props.code.replace(/\n$/, ""));
const tokens = computed(() => tokenize(source.value, props.lang));
const chip = computed(() => props.label || props.lang || "text");
const isRite = computed(() => props.lang === "rite");

/**
 * `browser` blocks are executed in browser-safe mode by `rite docs check` on
 * every CI run, so Run here cannot promise something the book does not already
 * verify. Everything else opens in Studio instead of failing in place.
 */
const canRunInline = computed(() => isRite.value && props.mode === "browser");
const studioHref = computed(
  () => `/studio#s=${encodeShare({ source: source.value, dialect: detectDialect(source.value) })}`
);

const copied = ref(false);
const running = ref(false);
const output = ref<string | null>(null);
const failed = ref(false);

async function copy() {
  try {
    await navigator.clipboard.writeText(source.value);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    /* clipboard blocked — leave the label alone */
  }
}

async function run() {
  running.value = true;
  output.value = null;
  failed.value = false;
  try {
    // Loaded on demand: the WASM engine is ~830 KB and most readers never run anything.
    const { riteCall, formatOutput } = await import("@studio/riteApi");
    const result = (await riteCall("", "/api/v1/run", { source: source.value })) as {
      ok?: boolean;
      error?: string;
    };
    output.value = formatOutput("/api/v1/run", result) || "(no output)";
    failed.value = result?.ok === false;
  } catch (err) {
    failed.value = true;
    output.value = err instanceof Error ? err.message : String(err);
  } finally {
    running.value = false;
  }
}
</script>

<template>
  <figure class="code-block group">
    <figcaption class="code-block__bar">
      <span class="code-block__chip">{{ chip }}</span>
      <span
        v-if="mode === 'native_only'"
        class="code-block__note"
        title="Uses a capability the browser runtime does not provide"
        >needs the CLI</span
      >
      <span class="code-block__actions">
        <button type="button" class="code-block__btn" @click="copy">
          {{ copied ? "Copied" : "Copy" }}
        </button>
        <button
          v-if="canRunInline"
          type="button"
          class="code-block__btn code-block__btn--run"
          :disabled="running"
          @click="run"
        >
          {{ running ? "Running…" : "▶ Run" }}
        </button>
        <!-- RouterLink, not <a>: a bare anchor here would reload the whole app. -->
        <RouterLink v-else-if="isRite" :to="studioHref" class="code-block__btn"
          >Open in Studio</RouterLink
        >
      </span>
    </figcaption>

    <pre
      class="code-block__pre"
    ><code><span v-for="(t, i) in tokens" :key="i" :class="`tok-${t.kind}`">{{ t.text }}</span></code></pre>

    <div v-if="output !== null" class="code-block__output" :class="{ 'is-error': failed }">
      <span class="code-block__output-label">{{ failed ? "error" : "output" }}</span>
      <pre>{{ output }}</pre>
      <button type="button" class="code-block__dismiss" @click="output = null">Dismiss</button>
    </div>
  </figure>
</template>

<style scoped>
.code-block {
  @apply my-5 overflow-hidden rounded-lg border border-rite-border bg-rite-panel;
  /* A faint inner edge so the block reads as a lit panel rather than a flat box. */
  box-shadow: inset 0 1px 0 rgb(255 255 255 / 4%);
}

.code-block__bar {
  @apply flex items-center gap-2 border-b border-rite-border bg-rite-bg/50 px-3 py-1.5;
}

.code-block__chip {
  @apply font-mono text-[11px] uppercase tracking-[0.14em] text-rite-muted;
}

.code-block__note {
  @apply rounded border border-rite-pink/30 bg-rite-pink/10 px-1.5 py-0.5 text-[10px] text-rite-pink;
}

.code-block__actions {
  @apply ml-auto flex items-center gap-1.5;
}

.code-block__btn {
  @apply rounded border border-slate-700 bg-rite-panel px-2 py-0.5 font-sans text-xs text-slate-300 no-underline transition-colors;
}
.code-block__btn:hover {
  @apply border-rite-accent text-rite-accent;
}
.code-block__btn:disabled {
  @apply opacity-50;
}
.code-block__btn--run {
  @apply border-rite-green/40 text-rite-green;
}
.code-block__btn--run:hover {
  @apply border-rite-green text-rite-green;
}

.code-block__pre {
  @apply m-0 overflow-x-auto border-0 bg-transparent p-4 text-sm leading-relaxed;
}

.code-block__output {
  @apply border-t border-rite-border bg-rite-bg/60 px-4 py-3;
}
.code-block__output.is-error {
  @apply border-t-rite-pink/40;
}
.code-block__output-label {
  @apply mb-1 block font-mono text-[10px] uppercase tracking-[0.14em] text-rite-muted;
}
.code-block__output.is-error .code-block__output-label {
  @apply text-rite-pink;
}
.code-block__output pre {
  @apply m-0 overflow-x-auto whitespace-pre-wrap border-0 bg-transparent p-0 font-mono text-xs text-slate-200;
}
.code-block__dismiss {
  @apply mt-2 font-sans text-[11px] text-slate-400 hover:text-slate-200;
}
</style>
