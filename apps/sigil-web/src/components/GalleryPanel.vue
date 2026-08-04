<script setup lang="ts">
/**
 * The gallery: every example in the repository, rendered.
 *
 * §20.8 asks for the cards to be generated from repository fixtures so they
 * cannot drift. The *sources* come from `examples/sigil/` at build time — Vite
 * reads the directory — and the thumbnails are rendered here, live, by the same
 * engine the canvas uses.
 *
 * That is stronger than baking images at build time, not weaker: a baked
 * thumbnail can be stale relative to the renderer that produced it, and this one
 * cannot be. It costs one render per card, which for six examples of a dozen
 * nodes is a few milliseconds.
 */
import { onMounted, ref } from "vue";
import { defaultOptions, renderCant } from "../lib/renderer";

defineProps<{ open: boolean }>();
const emit = defineEmits<{ open: [string]; close: [] }>();

const examples = __SIGIL_EXAMPLES__;

type Card = {
  name: string;
  source: string;
  svg?: string;
  summary?: string;
  tags: string[];
  /** Which tracery the thumbnail currently wears; hover advances it. */
  tracery: number;
};
const cards = ref<Card[]>(examples.map((e) => ({ ...e, tags: [], tracery: 0 })));

const TRACERIES = ["flowing", "concentric", "circuit"];

/**
 * Hovering a card cycles its tracery — the axis advertises itself.
 *
 * Pointer hover only, deliberately: keyboard focus should not fire renders
 * as someone tabs through, and the card's caption names what changed.
 */
async function cycleTracery(card: Card) {
  card.tracery = (card.tracery + 1) % TRACERIES.length;
  const result = await renderCant(`${card.name}.cant`, card.source, {
    ...defaultOptions(),
    ornament: "sparse",
    canonical: true,
    metadata: "minimal",
    tracery: TRACERIES[card.tracery],
  });
  if (result.svg) card.svg = result.svg;
}

/**
 * The constructs a card advertises, read off the rendered summary.
 *
 * From the scene's own census rather than by matching the source text — the
 * summary is generated from what was actually drawn, so a tag cannot claim a
 * construct the picture does not contain.
 */
function tagsOf(summary: string): string[] {
  return [
    "source",
    "stage",
    "ward",
    "scatter",
    "collect",
    "fork",
    "orbit",
    "invocation",
    "output seal",
  ].filter((word) => summary.includes(word));
}

onMounted(async () => {
  // Sequential rather than parallel: the engine is one WASM instance and a burst
  // of six would queue anyway, while blocking the canvas's first render.
  for (const card of cards.value) {
    const result = await renderCant(`${card.name}.cant`, card.source, {
      ...defaultOptions(),
      ornament: "sparse",
      canonical: true,
      // `minimal` metadata, which emits no element identifiers and no titles.
      //
      // Seven inline SVGs on one page otherwise means seven copies of
      // `id="sigil-glow"` and of every `id="node-…"` — invalid HTML, and an
      // `id` selector anywhere in the app could match a thumbnail instead of
      // the canvas. A decorative card needs neither: it is `aria-hidden`, and
      // its accessible name is the caption beside it.
      metadata: "minimal",
    });
    card.svg = result.svg;
    card.summary = result.summary;
    card.tags = result.summary ? tagsOf(result.summary) : [];
  }
});
</script>

<template>
  <aside
    v-if="open"
    class="panel fixed inset-x-0 bottom-0 z-20 max-h-[70vh] overflow-y-auto border-t p-3
           lg:inset-y-0 lg:left-auto lg:right-0 lg:max-h-none lg:w-[26rem] lg:border-l lg:border-t-0"
    aria-label="Gallery"
  >
    <div class="mb-3 flex items-center">
      <h2 class="text-[0.6rem] uppercase tracking-[0.25em] text-sigil-muted">Gallery</h2>
      <button class="instrument ml-auto" @click="emit('close')">close</button>
    </div>

    <ul class="grid grid-cols-2 gap-2">
      <li v-for="card in cards" :key="card.name">
        <button
          class="group w-full rounded-lg border border-sigil-border bg-sigil-card p-1.5 text-left
                 transition-colors hover:border-sigil-accent/60 focus-visible:border-sigil-accent"
          :title="`open in the editor — hovering cycles the tracery (${TRACERIES[card.tracery]})`"
          @mouseenter="cycleTracery(card)"
          @click="emit('open', card.name)"
        >
          <span
            v-if="card.svg"
            class="block aspect-square w-full overflow-hidden rounded-md bg-abyss
                   [&_svg]:h-full [&_svg]:w-full"
            aria-hidden="true"
            v-html="card.svg"
          />
          <span v-else class="block aspect-square w-full animate-pulse rounded-md bg-abyss" />
          <span
            class="mt-1 block font-mono text-[0.7rem] text-slate-200 group-hover:text-sigil-accent"
          >
            {{ card.name }}
          </span>
          <span class="mt-0.5 block text-[0.55rem] leading-tight text-sigil-muted">
            {{ card.tags.join(" · ") || "…" }}
            <template v-if="card.tracery !== 0"> · {{ TRACERIES[card.tracery] }}</template>
          </span>
        </button>
      </li>
    </ul>

    <p class="mt-3 text-[0.6rem] leading-relaxed text-sigil-muted">
      Rendered here, from the examples in the repository — veiled, canonical seed.
    </p>
  </aside>
</template>
