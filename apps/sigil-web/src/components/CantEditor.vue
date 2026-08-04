<script setup lang="ts">
/**
 * The source panel's editor: a textarea with the paint layered underneath.
 *
 * The classic overlay trick — a `<pre>` renders the highlighted source, and a
 * textarea with transparent text sits exactly on top of it carrying the caret,
 * the selection and every native editing behaviour. The two share one font,
 * one padding and one scroll position, or the illusion breaks; that agreement
 * is kept by sharing the class string rather than by repeating it.
 *
 * A real editor widget would do more, and §22 is why it is not one: the app
 * ships no third-party code, and a dependency the size of CodeMirror is a lot
 * to spend on colouring a twelve-operator language.
 */
import { computed, ref } from "vue";
import { highlightCant } from "../lib/highlight";

defineOptions({ inheritAttrs: false });

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ "update:modelValue": [string] }>();

const paint = ref<HTMLElement | null>(null);

// A trailing newline keeps the paint layer as tall as the textarea when the
// caret sits on a fresh last line — without it the layers scroll apart by
// exactly one line height at the bottom of the document.
const painted = computed(() => highlightCant(props.modelValue) + "\n");

/** The two layers agree on metrics because they are the same string. */
const METRICS = "m-0 whitespace-pre p-3 font-mono text-xs leading-relaxed";

function onInput(event: Event) {
  emit("update:modelValue", (event.target as HTMLTextAreaElement).value);
}

function onScroll(event: Event) {
  const area = event.target as HTMLTextAreaElement;
  if (!paint.value) return;
  paint.value.scrollTop = area.scrollTop;
  paint.value.scrollLeft = area.scrollLeft;
}

/** Tab indents rather than leaving the field — this is an editor, not a form. */
function onTab(event: KeyboardEvent) {
  const area = event.target as HTMLTextAreaElement;
  const { selectionStart, selectionEnd, value } = area;
  event.preventDefault();
  const next = value.slice(0, selectionStart) + "  " + value.slice(selectionEnd);
  emit("update:modelValue", next);
  requestAnimationFrame(() => {
    area.selectionStart = area.selectionEnd = selectionStart + 2;
  });
}
</script>

<template>
  <div class="relative min-h-[10rem] flex-1 overflow-hidden">
    <pre
      ref="paint"
      aria-hidden="true"
      :class="METRICS"
      class="pointer-events-none absolute inset-0 overflow-hidden text-slate-200"
      v-html="painted"
    />
    <textarea
      :value="modelValue"
      spellcheck="false"
      wrap="off"
      v-bind="$attrs"
      :class="METRICS"
      class="absolute inset-0 h-full w-full resize-none bg-transparent text-transparent
             caret-spectral outline-none placeholder:text-slate-600"
      @input="onInput"
      @scroll="onScroll"
      @keydown.tab="onTab"
    />
  </div>
</template>
