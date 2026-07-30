<script setup lang="ts">
import { ref } from "vue";

const props = withDefaults(
  defineProps<{
    /** Text shown in the block and written to the clipboard. */
    code: string;
    /** Tailwind classes for the <pre>, so callers keep their own surface style. */
    preClass?: string;
  }>(),
  { preClass: "" }
);

const copied = ref(false);

async function copy() {
  try {
    await navigator.clipboard.writeText(props.code);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    /* clipboard blocked (insecure context / denied) — leave the label alone */
  }
}
</script>

<template>
  <div class="relative">
    <pre :class="preClass">{{ code }}</pre>
    <button
      type="button"
      class="absolute right-2 top-2 rounded border border-slate-700 bg-rite-bg/80 px-2 py-1 text-xs text-slate-300 hover:border-rite-accent hover:text-rite-accent"
      :aria-label="copied ? 'Copied to clipboard' : 'Copy to clipboard'"
      @click="copy"
    >
      {{ copied ? "Copied" : "Copy" }}
    </button>
  </div>
</template>
