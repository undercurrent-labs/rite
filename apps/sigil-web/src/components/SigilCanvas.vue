<script setup lang="ts">
/**
 * The artifact, and the ways to look at it.
 *
 * Pan, zoom, fit, fullscreen, and hover/focus revelation. The SVG is inserted
 * with `v-html`, which is normally the wrong tool — it is right here because the
 * markup is produced by the Rust serializer, which escapes every label and
 * sanitizes every identifier, and is asserted script-free by
 * `crates/rite-sigil/tests/svg_security.rs`. The alternative, re-parsing and
 * re-serializing it in JavaScript, would add a second escaping story on the side
 * of the boundary where injection actually matters.
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";

const props = defineProps<{
  svg?: string;
  sceneJson?: string;
  deepVeil: boolean;
  rendering: boolean;
  error: string | null;
  selected: string | null;
}>();
const emit = defineEmits<{ "update:selected": [string | null] }>();

const host = ref<HTMLElement | null>(null);
const wrap = ref<HTMLElement | null>(null);
const scale = ref(1);
const offset = ref({ x: 0, y: 0 });
const tip = ref<{ text: string; x: number; y: number } | null>(null);

const transform = computed(
  () => `translate(${offset.value.x}px, ${offset.value.y}px) scale(${scale.value})`
);

function fit() {
  scale.value = 1;
  offset.value = { x: 0, y: 0 };
}

function zoom(by: number) {
  scale.value = Math.min(8, Math.max(0.2, scale.value * by));
}

/**
 * Everything reachable from a node, and everything that reaches it.
 *
 * Read off the scene's edges rather than the SVG, because the SVG says which
 * marks exist and the scene says how they connect — and highlighting a *path*
 * needs the second. Both directions, because "what does this depend on" and
 * "what depends on this" are the two questions a selection is asked.
 */
const related = computed<{ nodes: Set<string>; edges: Set<string> }>(() => {
  const empty = { nodes: new Set<string>(), edges: new Set<string>() };
  const id = props.selected;
  if (!id || !props.sceneJson) return empty;

  // `graph_ref`, not `graphRef`. The scene's Rust types serialize with their own
  // field names, since only the *WASM boundary* types use camelCase, and reading the
  // wrong one fails silently: no edges parse, so a selection lights only itself
  // and looks like a highlighting bug rather than a naming one.
  type Element = { id: string; graph_ref?: { kind: string }; ends?: { from: string; to: string } };
  let elements: Element[] = [];
  try {
    elements = JSON.parse(props.sceneJson).elements ?? [];
  } catch {
    return { nodes: new Set([id]), edges: new Set() };
  }

  // Endpoints come off the element's own `ends` field. They used to be parsed
  // out of the *identifier* with a regular expression, which worked only because
  // the Cant adapter happened to build ids that way — a string format as a
  // structural dependency, and one that would have failed silently.
  const edges = elements
    .filter((e) => e.graph_ref?.kind === "edge" && e.ends)
    .map((e) => ({ element: e.id, from: e.ends!.from, to: e.ends!.to }));

  const nodes = new Set<string>([id]);
  for (const forward of [true, false]) {
    const frontier = [id];
    while (frontier.length) {
      const current = frontier.pop() as string;
      for (const edge of edges) {
        const [from, to] = forward ? [edge.from, edge.to] : [edge.to, edge.from];
        if (from === current && !nodes.has(to)) {
          nodes.add(to);
          frontier.push(to);
        }
      }
    }
  }

  // An edge is lit when *both* ends are, so a trace to a node outside the
  // selection does not read as part of the path.
  const lit = new Set<string>();
  for (const edge of edges) {
    if (nodes.has(edge.from) && nodes.has(edge.to)) lit.add(edge.element);
  }
  return { nodes, edges: lit };
});

/** Dim everything the selection does not reach. */
function paint() {
  const svg = host.value?.querySelector("svg");
  if (!svg) return;
  const { nodes, edges } = related.value;
  // Exact identity, not a substring test. Matching `graphId.includes("n1")` lit
  // `n10` and `n11` too, which meant a selection lit the whole picture and
  // looked like it was working.
  svg.querySelectorAll<SVGElement>('[id^="node-"], [id^="edge-"]').forEach((element) => {
    const on = !props.selected
      ? true
      : element.id.startsWith("node-")
        ? nodes.has(element.id.slice("node-".length))
        : edges.has(element.id);
    element.style.opacity = on ? "1" : "0.12";
    element.style.transition = "opacity .12s";
  });
}

watch([() => props.selected, () => props.svg], () => requestAnimationFrame(paint), {
  flush: "post",
});

let dragging = false;
let last = { x: 0, y: 0 };

function down(event: PointerEvent) {
  dragging = true;
  last = { x: event.clientX, y: event.clientY };
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
function move(event: PointerEvent) {
  if (!dragging) return;
  offset.value = {
    x: offset.value.x + (event.clientX - last.x),
    y: offset.value.y + (event.clientY - last.y),
  };
  last = { x: event.clientX, y: event.clientY };
}
function up(event: PointerEvent) {
  dragging = false;
  (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
}
function wheel(event: WheelEvent) {
  event.preventDefault();
  zoom(event.deltaY < 0 ? 1.12 : 1 / 1.12);
}

async function fullscreen() {
  if (document.fullscreenElement) await document.exitFullscreen();
  else await wrap.value?.requestFullscreen();
}

/**
 * Make every node focusable and revealing.
 *
 * Re-run whenever the SVG changes, because it is replaced wholesale. Reading the
 * `<title>` rather than a label is the point: the title is a semantic kind and
 * never source text, so a tooltip cannot show more than the disclosure mode
 * allowed.
 */
function wire() {
  const svg = host.value?.querySelector("svg");
  if (!svg) return;
  svg.removeAttribute("width");
  svg.removeAttribute("height");
  svg.setAttribute("class", "w-full h-full");

  svg.querySelectorAll<SVGElement>('[id^="node-"]').forEach((node) => {
    node.setAttribute("tabindex", "0");
    node.style.cursor = "pointer";
    const title = node.querySelector("title")?.textContent ?? "";

    const show = () => {
      if (props.deepVeil || !title) return;
      const box = node.getBoundingClientRect();
      const outer = wrap.value?.getBoundingClientRect();
      tip.value = {
        text: title,
        x: box.left + box.width / 2 - (outer?.left ?? 0),
        y: box.top - (outer?.top ?? 0) - 10,
      };
    };
    const hide = () => (tip.value = null);
    node.addEventListener("mouseenter", show);
    node.addEventListener("focus", show);
    node.addEventListener("mouseleave", hide);
    node.addEventListener("blur", hide);

    const graphId = node.id.replace(/^node-/, "");
    const pick = () => emit("update:selected", props.selected === graphId ? null : graphId);
    node.addEventListener("click", pick);
    node.addEventListener("keydown", (event) => {
      const key = (event as KeyboardEvent).key;
      if (key === "Enter" || key === " ") {
        event.preventDefault();
        pick();
      }
    });
  });
  paint();
}

watch(() => props.svg, () => requestAnimationFrame(wire), { flush: "post" });
watch(() => props.deepVeil, (on) => on && (tip.value = null));

function onKey(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  tip.value = null;
  emit("update:selected", null);
}
window.addEventListener("keydown", onKey);
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div ref="wrap" class="relative h-full w-full overflow-hidden bg-abyss">
    <div
      class="absolute inset-0 flex items-center justify-center touch-none"
      @pointerdown="down"
      @pointermove="move"
      @pointerup="up"
      @pointercancel="up"
      @wheel="wheel"
    >
      <div
        v-if="svg"
        ref="host"
        class="sigil-stage h-full w-full max-h-full max-w-full origin-center
               transition-transform duration-75"
        :style="{ transform }"
        v-html="svg"
      />
      <p v-else-if="error" class="max-w-md px-6 text-center text-xs text-ember">{{ error }}</p>
      <p v-else class="text-xs text-spectral/40">
        {{ rendering ? "inscribing…" : "nothing to draw" }}
      </p>
    </div>

    <div
      v-if="tip"
      class="pointer-events-none absolute z-10 -translate-x-1/2 -translate-y-full rounded border
             border-glow/60 bg-abyss/95 px-2 py-1 font-mono text-[0.7rem] text-glow"
      :style="{ left: `${tip.x}px`, top: `${tip.y}px` }"
      role="status"
      aria-live="polite"
    >
      {{ tip.text }}
    </div>

    <div class="absolute bottom-3 right-3 flex gap-1">
      <button class="instrument" aria-label="Zoom out" @click="zoom(1 / 1.25)">&minus;</button>
      <button class="instrument" aria-label="Zoom in" @click="zoom(1.25)">+</button>
      <button class="instrument" @click="fit">fit</button>
      <button class="instrument" @click="fullscreen">full</button>
    </div>

    <span
      v-if="rendering"
      class="absolute left-3 top-3 animate-pulse font-mono text-[0.65rem] tracking-widest
             text-glow/60"
    >
      inscribing…
    </span>
  </div>
</template>
