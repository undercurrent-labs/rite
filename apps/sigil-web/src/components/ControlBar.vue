<script setup lang="ts">
/** The instrument row: everything that changes the artifact, in one place. */
import type { RenderOptions } from "../lib/renderer";

const props = defineProps<{ options: RenderOptions; deepVeil: boolean }>();
const emit = defineEmits<{
  "update:options": [RenderOptions];
  "update:deepVeil": [boolean];
}>();

function set<K extends keyof RenderOptions>(key: K, value: RenderOptions[K]) {
  emit("update:options", { ...props.options, [key]: value });
}

const modes = [
  { id: "veiled", hint: "No labels. Marks and geometry only." },
  { id: "inscribed", hint: "Short annotations." },
  { id: "revealed", hint: "Readable labels." },
];
const themes = ["neon-ritual", "void", "parchment"];
const ornaments = ["none", "sparse", "ritual", "maximal"];
const metadata = ["full", "safe", "minimal", "none"];
</script>

<template>
  <div
    class="panel flex shrink-0 flex-wrap items-end gap-x-5 gap-y-2 border-b px-4 py-2"
    role="group"
    aria-label="Render controls"
  >
    <div>
      <span class="instrument-label">Veil</span>
      <div class="flex gap-1">
        <button
          v-for="mode in modes"
          :key="mode.id"
          class="instrument capitalize"
          :class="{ 'is-active': options.mode === mode.id }"
          :title="mode.hint"
          @click="set('mode', mode.id)"
        >
          {{ mode.id }}
        </button>
      </div>
    </div>

    <div>
      <span class="instrument-label">Theme</span>
      <div class="flex gap-1">
        <button
          v-for="theme in themes"
          :key="theme"
          class="instrument"
          :class="{ 'is-active': options.theme === theme }"
          @click="set('theme', theme)"
        >
          {{ theme }}
        </button>
      </div>
    </div>

    <div>
      <span class="instrument-label">Ornament</span>
      <div class="flex gap-1">
        <button
          v-for="level in ornaments"
          :key="level"
          class="instrument"
          :class="{ 'is-active': options.ornament === level }"
          @click="set('ornament', level)"
        >
          {{ level }}
        </button>
      </div>
    </div>

    <div>
      <label class="instrument-label" for="sigil-metadata">Metadata</label>
      <select
        id="sigil-metadata"
        class="instrument"
        :value="options.metadata"
        @change="set('metadata', ($event.target as HTMLSelectElement).value)"
      >
        <option v-for="m in metadata" :key="m" :value="m">{{ m }}</option>
      </select>
    </div>

    <div>
      <label class="instrument-label" for="sigil-seed">Seed</label>
      <input
        id="sigil-seed"
        class="instrument w-28"
        :value="options.seed"
        @change="set('seed', ($event.target as HTMLInputElement).value)"
      />
    </div>

    <div class="flex gap-1">
      <button
        class="instrument"
        :aria-pressed="options.canonical"
        title="A documented fixed orientation and seed, for reproducible output."
        @click="set('canonical', !options.canonical)"
      >
        canonical
      </button>
      <button
        class="instrument"
        :aria-pressed="deepVeil"
        title="Suppress hover and focus revelation as well."
        @click="emit('update:deepVeil', !deepVeil)"
      >
        deep veil
      </button>
    </div>
  </div>
</template>
