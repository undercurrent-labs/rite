<script setup lang="ts">
import { computed, ref } from "vue";
import { tokenize } from "./highlight";

/**
 * A highlighted editor built the boring, reliable way: a coloured `<pre>` behind
 * a transparent `<textarea>`.
 *
 * The textarea keeps every native behaviour that matters — caret, selection,
 * undo, IME, spellcheck-off, mobile keyboards — which a contenteditable would
 * quietly break. The only requirement is that both layers lay text out
 * identically, so font, size, line-height, padding and wrapping are set once
 * (`.layer`) and shared rather than repeated per element.
 */
const props = withDefaults(
  defineProps<{
    modelValue: string;
    lang?: string;
    /** Set explicitly so a <label for> can reach the textarea, not the wrapper. */
    inputId?: string;
  }>(),
  { lang: "rite", inputId: undefined }
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
  cursor: [pos: { line: number; character: number }];
}>();

const input = ref<HTMLTextAreaElement | null>(null);
const mirror = ref<HTMLPreElement | null>(null);

const tokens = computed(() => tokenize(props.modelValue, props.lang));

function onInput(event: Event) {
  const el = event.target as HTMLTextAreaElement;
  emit("update:modelValue", el.value);
  reportCursor(el);
}

/** The mirror is scrolled to follow the textarea; it never scrolls itself. */
function onScroll() {
  if (!input.value || !mirror.value) return;
  mirror.value.scrollTop = input.value.scrollTop;
  mirror.value.scrollLeft = input.value.scrollLeft;
}

function reportCursor(el: HTMLTextAreaElement) {
  const before = el.value.slice(0, el.selectionStart ?? 0);
  const lines = before.split("\n");
  emit("cursor", {
    line: lines.length - 1,
    character: (lines[lines.length - 1] ?? "").length,
  });
}

function onSelect(event: Event) {
  reportCursor(event.target as HTMLTextAreaElement);
}

/** Tab should indent, not escape to the next control, inside a code editor. */
function onTab(event: KeyboardEvent) {
  const el = event.target as HTMLTextAreaElement;
  event.preventDefault();
  const start = el.selectionStart ?? 0;
  const end = el.selectionEnd ?? 0;
  const next = `${props.modelValue.slice(0, start)}  ${props.modelValue.slice(end)}`;
  emit("update:modelValue", next);
  requestAnimationFrame(() => {
    el.selectionStart = el.selectionEnd = start + 2;
  });
}

defineExpose({ focus: () => input.value?.focus() });
</script>

<template>
  <div class="editor">
    <!--
      Trailing newline: a textarea reserves a line after a final "\n" but a <pre>
      does not, so without this the two drift apart by one line at the bottom.
    -->
    <pre ref="mirror" class="layer editor__mirror" aria-hidden="true"><code><span
      v-for="(t, i) in tokens"
      :key="i"
      :class="`tok-${t.kind}`"
    >{{ t.text }}</span>{{ "\n" }}</code></pre>
    <textarea
      :id="inputId"
      ref="input"
      class="layer editor__input"
      :value="modelValue"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      @input="onInput"
      @scroll="onScroll"
      @select="onSelect"
      @click="onSelect"
      @keyup="onSelect"
      @keydown.tab="onTab"
    />
  </div>
</template>

<style scoped>
.editor {
  position: relative;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: theme("colors.rite.panel");
}

/*
 * Every property here affects where a glyph lands. The two layers must agree on
 * all of them or the caret drifts away from the text behind it.
 */
.layer {
  position: absolute;
  inset: 0;
  margin: 0;
  padding: 1rem;
  border: 0;
  font-family: theme("fontFamily.mono");
  font-size: 0.875rem;
  line-height: 1.625;
  letter-spacing: normal;
  tab-size: 2;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: normal;
}

.editor__mirror {
  overflow: hidden;
  pointer-events: none;
  background: transparent;
}

.editor__input {
  overflow: auto;
  resize: none;
  background: transparent;
  color: transparent;
  caret-color: theme("colors.rite.accent");
  outline: none;
}

/* Selection has to stay translucent — the glyphs are on the layer underneath. */
.editor__input::selection {
  background: rgb(126 224 255 / 25%);
}
</style>
