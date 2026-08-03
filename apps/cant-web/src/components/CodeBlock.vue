<script setup lang="ts">
import { computed, ref } from "vue";
import { highlightCant, highlightRite } from "../lib/highlight";

const props = withDefaults(
  defineProps<{ code: string; lang?: "cant" | "rite"; label?: string }>(),
  { lang: "cant" }
);

const html = computed(() =>
  props.lang === "cant" ? highlightCant(props.code) : highlightRite(props.code)
);

const copied = ref(false);

async function copy() {
  try {
    await navigator.clipboard.writeText(props.code);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1200);
  } catch {
    // Clipboard access can be refused (insecure origin, permission). Say
    // nothing rather than pretending it worked.
    copied.value = false;
  }
}
</script>

<template>
  <div class="group relative">
    <div
      v-if="label"
      class="mb-1.5 font-mono text-xs uppercase tracking-wider text-slate-500"
    >
      {{ label }}
    </div>
    <pre
      class="overflow-x-auto rounded-lg border border-cant-border bg-cant-panel p-4 font-mono text-sm leading-relaxed"
    ><code v-html="html"></code></pre>
    <button
      type="button"
      class="absolute right-2 rounded border border-cant-border bg-cant-card px-2 py-1 text-xs text-slate-400 opacity-0 transition-opacity hover:text-slate-100 focus:opacity-100 group-hover:opacity-100"
      :class="label ? 'top-8' : 'top-2'"
      @click="copy"
    >
      {{ copied ? "copied" : "copy" }}
    </button>
  </div>
</template>
