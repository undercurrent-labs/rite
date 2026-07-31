<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  adjacentTutorials,
  getTutorialMarkdown,
  TUTORIALS,
  tutorialBySlug,
} from "../lib/tutorials";
import { segmentMarkdown } from "../lib/markdown";
import CodeBlock from "../components/CodeBlock.vue";

const route = useRoute();
const router = useRouter();

const slug = computed(() => {
  const s = route.params.slug;
  return typeof s === "string" && s.length ? s : "";
});

const tutorial = computed(() => (slug.value ? tutorialBySlug(slug.value) : undefined));

/** null = no such tutorial, undefined = still loading its chunk. */
const markdown = ref<string | null | undefined>(undefined);

watch(
  slug,
  async (current) => {
    if (!current) {
      markdown.value = undefined;
      return;
    }
    markdown.value = undefined;
    const text = await getTutorialMarkdown(current);
    // A fast second navigation must not be overwritten by the slower first load.
    if (slug.value === current) markdown.value = text;
  },
  { immediate: true }
);

// `tutorials` base: a bare `foo.md` link inside a tutorial means a sibling
// tutorial, not a book chapter.
const segments = computed(() =>
  markdown.value ? segmentMarkdown(markdown.value, "tutorials") : []
);

const neighbors = computed(() => (slug.value ? adjacentTutorials(slug.value) : {}));

/**
 * Rendered markdown is injected as raw HTML, so its links are plain anchors and
 * would reload the whole app. Route them instead, leaving external links,
 * new-tab clicks and same-page anchors alone.
 */
function onDocClick(ev: MouseEvent) {
  if (ev.defaultPrevented || ev.button !== 0) return;
  if (ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return;
  const anchor = (ev.target as HTMLElement | null)?.closest("a");
  if (!anchor) return;
  if (anchor.target && anchor.target !== "_self") return;
  const href = anchor.getAttribute("href");
  if (!href || !href.startsWith("/")) return;
  ev.preventDefault();
  router.push(href);
}

watch(
  [tutorial, slug],
  ([t, s]) => {
    document.title = t ? `${t.title} · Tutorials · Rite` : s ? "Not found · Rite" : "Tutorials · Rite";
  },
  { immediate: true }
);
</script>

<template>
  <!-- Index -->
  <div v-if="!slug" class="mx-auto max-w-4xl px-4 py-10">
    <h1 class="text-2xl font-semibold text-slate-100">Tutorials</h1>
    <p class="mt-3 max-w-prose text-slate-400">
      Project-shaped guides. Each builds a small working thing end to end and explains the
      decisions along the way — as opposed to
      <RouterLink to="/docs" class="text-rite-accent hover:underline">the book</RouterLink>, which
      covers one topic per chapter and is meant to be read in order.
    </p>
    <p class="mt-2 max-w-prose text-sm text-rite-muted">
      Every example was run to produce the output printed beside it.
    </p>

    <ul class="mt-8 space-y-3">
      <li v-for="(t, i) in TUTORIALS" :key="t.slug">
        <RouterLink
          :to="`/tutorials/${t.slug}`"
          class="group block rounded-lg border border-rite-border bg-rite-panel p-4 transition-colors hover:border-rite-accent/50"
        >
          <div class="flex items-baseline gap-3">
            <span class="font-mono text-xs text-rite-muted">{{ i + 1 }}</span>
            <h2 class="font-medium text-slate-100 group-hover:text-rite-accent">{{ t.title }}</h2>
          </div>
          <p class="mt-1.5 pl-7 text-sm text-slate-400">{{ t.blurb }}</p>
          <dl class="mt-3 flex flex-wrap gap-x-6 gap-y-1 pl-7 text-xs">
            <div class="flex gap-1.5">
              <dt class="text-rite-muted">Builds</dt>
              <dd class="text-slate-300">{{ t.builds }}</dd>
            </div>
            <div class="flex gap-1.5">
              <dt class="text-rite-muted">Needs</dt>
              <dd class="text-slate-300">{{ t.needs }}</dd>
            </div>
          </dl>
        </RouterLink>
      </li>
    </ul>

    <p class="mt-8 text-sm text-slate-400">
      More are planned — an HTTP service with real routes, a CLI with argument parsing,
      compiling to a binary, embedding Rite in a Rust program, and a DNS resolver over
      <code class="text-rite-pink">@udp</code>.
    </p>
  </div>

  <!-- A single tutorial -->
  <div v-else class="mx-auto flex max-w-6xl gap-8 px-4 py-8 md:py-10">
    <aside class="hidden w-56 shrink-0 md:block">
      <div class="sticky top-20">
        <p class="mb-3 text-xs font-mono uppercase tracking-wider text-rite-muted">Tutorials</p>
        <nav class="space-y-0.5 text-sm">
          <RouterLink
            v-for="(t, i) in TUTORIALS"
            :key="t.slug"
            :to="`/tutorials/${t.slug}`"
            class="block rounded-md px-2 py-1.5"
            :class="
              slug === t.slug
                ? 'bg-slate-800/80 text-rite-accent'
                : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
            "
          >
            <span class="mr-1.5 font-mono text-[10px] text-slate-400">{{ i + 1 }}</span>
            {{ t.title }}
          </RouterLink>
        </nav>
        <div class="mt-6 border-t border-rite-border pt-4 text-sm">
          <RouterLink to="/docs" class="text-slate-400 hover:text-rite-accent">
            The book →
          </RouterLink>
        </div>
      </div>
    </aside>

    <article class="min-w-0 flex-1">
      <div v-if="markdown === null" class="prose-rite">
        <h1>Not found</h1>
        <p>
          No tutorial named <code>{{ slug }}</code>.
          <RouterLink to="/tutorials">Back to the list</RouterLink>.
        </p>
      </div>
      <p v-else-if="markdown === undefined" class="text-slate-400">Loading…</p>
      <div v-else class="prose-rite max-w-prose">
        <template v-for="(seg, i) in segments" :key="i">
          <CodeBlock
            v-if="seg.kind === 'code'"
            :code="seg.code"
            :lang="seg.lang"
            :mode="seg.mode"
          />
          <div v-else @click="onDocClick" v-html="seg.html" />
        </template>
      </div>

      <nav
        v-if="markdown && (neighbors.prev || neighbors.next)"
        class="mt-12 flex max-w-prose flex-wrap items-center justify-between gap-4 border-t border-rite-border pt-6 text-sm"
      >
        <RouterLink
          v-if="neighbors.prev"
          :to="`/tutorials/${neighbors.prev.slug}`"
          class="text-slate-400 hover:text-rite-accent"
        >
          ← {{ neighbors.prev.title }}
        </RouterLink>
        <span v-else />
        <RouterLink
          v-if="neighbors.next"
          :to="`/tutorials/${neighbors.next.slug}`"
          class="text-slate-400 hover:text-rite-accent"
        >
          {{ neighbors.next.title }} →
        </RouterLink>
      </nav>
    </article>
  </div>
</template>
