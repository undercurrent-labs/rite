<script setup lang="ts">
/** The decoder: what each mark is, read off the scene's legend. */
import { computed } from "vue";

const props = defineProps<{
  sceneJson?: string;
  summary?: string;
  fingerprint?: string;
  elapsedMs?: number;
  /**
   * Deep Veil suppresses *revelation*, not the panel.
   *
   * §13.1: a Veiled render may still be decoded through the Codex, and Deep Veil
   * is the setting that stops it. So the kinds stay — they are this renderer's
   * vocabulary, not the user's source — and the labels go.
   */
  deepVeil?: boolean;
  selected?: string | null;
}>();
defineEmits<{ close: []; select: [string | null] }>();

type Entry = {
  key: string;
  // Snake case: these are the scene's own field names. See SigilCanvas.
  graph_ref?: { kind: string; id: string };
  summary: string;
  label?: string;
  capabilities?: string[];
  region?: string;
  branchOrdinal?: number;
  warnings?: string[];
};

const entries = computed<Entry[]>(() => {
  if (!props.sceneJson) return [];
  try {
    return (JSON.parse(props.sceneJson).legend ?? []) as Entry[];
  } catch {
    return [];
  }
});
</script>

<template>
  <aside
    class="panel fixed inset-x-0 bottom-0 z-20 flex max-h-[70vh] flex-col border-t
           lg:static lg:z-auto lg:max-h-none lg:w-80 lg:min-h-0 lg:shrink-0 lg:border-l
           lg:border-t-0"
    aria-label="Codex"
  >
    <div class="flex shrink-0 items-center border-b border-ultraviolet/20 px-3 py-2">
      <h2 class="text-[0.6rem] uppercase tracking-[0.25em] text-spectral/40">Codex</h2>
      <button class="instrument ml-auto" @click="$emit('close')">close</button>
    </div>

    <p v-if="summary" class="shrink-0 border-b border-ultraviolet/20 p-3 text-[0.7rem] text-spectral/60">
      {{ summary }}
    </p>
    <p
      v-if="deepVeil"
      class="shrink-0 border-b border-ultraviolet/20 px-3 py-2 text-[0.65rem] text-gold/60"
    >
      Deep Veil — kinds only, no labels.
    </p>

    <ul class="min-h-0 flex-1 overflow-y-auto p-2 text-[0.7rem]">
      <li
        v-for="entry in entries"
        :key="entry.key"
        class="mb-1 cursor-pointer border border-transparent p-2 hover:border-cyan/40"
        :class="{ 'border-cyan bg-cyan/5': entry.graph_ref?.id === selected }"
        :aria-current="entry.graph_ref?.id === selected ? 'true' : 'false'"
        tabindex="0"
        @click="$emit('select', entry.graph_ref?.id === selected ? null : (entry.graph_ref?.id ?? null))"
        @keydown.enter.prevent="$emit('select', entry.graph_ref?.id ?? null)"
        @keydown.space.prevent="$emit('select', entry.graph_ref?.id ?? null)"
      >
        <span class="text-[0.6rem] uppercase tracking-widest text-cyan">{{ entry.summary }}</span>
        <span
          v-if="entry.label && !deepVeil"
          class="mt-0.5 block break-words text-spectral/70"
        >
          {{ entry.label }}
        </span>
        <span v-if="entry.capabilities?.length && !deepVeil" class="mt-0.5 block text-gold/60">
          touches {{ entry.capabilities.join(", ") }}
        </span>
        <span v-for="w in entry.warnings" :key="w" class="mt-0.5 block text-ember/80">{{ w }}</span>
      </li>
      <li v-if="!entries.length" class="p-2 text-spectral/30">nothing decoded yet</li>
    </ul>

    <div
      v-if="fingerprint"
      class="shrink-0 border-t border-ultraviolet/20 p-2 text-[0.6rem] leading-relaxed text-spectral/30"
    >
      <p class="break-all">{{ fingerprint }}</p>
      <p v-if="elapsedMs !== undefined">rendered in {{ elapsedMs.toFixed(1) }} ms</p>
    </div>
  </aside>
</template>
