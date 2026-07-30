<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  adjacentChapters,
  chapterBySlug,
  DOC_CHAPTERS,
  docsIndexMarkdown,
  getDocMarkdown,
} from "../lib/docs";
import { segmentMarkdown } from "../lib/markdown";
import CodeBlock from "../components/CodeBlock.vue";

const route = useRoute();
const router = useRouter();

function onChapterSelect(ev: Event) {
  const v = (ev.target as HTMLSelectElement).value;
  router.push(v ? `/docs/${v}` : "/docs");
}

/**
 * Rendered markdown is injected as raw HTML, so its in-book links are plain
 * anchors and would reload the whole app. Route them instead, leaving external
 * links, new-tab clicks and same-page anchors alone.
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

const slug = computed(() => {
  const s = route.params.slug;
  if (typeof s === "string" && s.length) return s;
  return "";
});

const chapter = computed(() => (slug.value ? chapterBySlug(slug.value) : undefined));

/** null = no such chapter, undefined = still loading this chapter's chunk. */
const markdown = ref<string | null | undefined>(undefined);

watch(
  slug,
  async (current) => {
    markdown.value = undefined;
    const text = current ? await getDocMarkdown(current) : await docsIndexMarkdown();
    // A fast second navigation must not be overwritten by the slower first load.
    if (slug.value === current) markdown.value = text;
  },
  { immediate: true }
);

/** Code blocks render as components; prose stays a single v-html run per gap. */
const segments = computed(() => (markdown.value ? segmentMarkdown(markdown.value) : []));

const neighbors = computed(() => (slug.value ? adjacentChapters(slug.value) : {}));

watch(
  chapter,
  (ch) => {
    if (ch) document.title = `${ch.title} · Docs · Rite`;
    else document.title = "Docs · Rite";
  },
  { immediate: true }
);
</script>

<template>
  <div class="mx-auto flex max-w-6xl gap-8 px-4 py-8 md:py-10">
    <!-- Sidebar -->
    <aside class="hidden w-56 shrink-0 md:block">
      <div class="sticky top-20 max-h-[calc(100vh-6rem)] overflow-y-auto pr-2">
        <p class="mb-3 text-xs font-mono uppercase tracking-wider text-rite-muted">Book</p>
        <nav class="space-y-0.5 text-sm">
          <RouterLink
            to="/docs"
            class="block rounded-md px-2 py-1.5"
            :class="
              !slug
                ? 'bg-slate-800/80 text-rite-accent'
                : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
            "
          >
            Overview
          </RouterLink>
          <RouterLink
            v-for="(ch, i) in DOC_CHAPTERS"
            :key="ch.slug"
            :to="`/docs/${ch.slug}`"
            class="block rounded-md px-2 py-1.5"
            :class="
              slug === ch.slug
                ? 'bg-slate-800/80 text-rite-accent'
                : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
            "
          >
            <span class="mr-1.5 font-mono text-[10px] text-slate-400">{{ i + 1 }}</span>
            {{ ch.title }}
          </RouterLink>
        </nav>
        <div class="mt-6 border-t border-rite-border pt-4">
          <RouterLink
            to="/studio"
            class="text-sm text-rite-pink hover:underline"
          >
            Open Studio →
          </RouterLink>
        </div>
      </div>
    </aside>

    <!-- Mobile chapter select -->
    <div class="mb-4 w-full md:hidden">
      <label class="mb-1 block text-xs text-rite-muted">Chapter</label>
      <select
        class="w-full rounded-md border border-slate-700 bg-rite-panel px-3 py-2 text-sm"
        :value="slug || ''"
        @change="onChapterSelect"
      >
        <option value="">Overview</option>
        <option v-for="ch in DOC_CHAPTERS" :key="ch.slug" :value="ch.slug">
          {{ ch.title }}
        </option>
      </select>
    </div>

    <!-- Content -->
    <article class="min-w-0 flex-1">
      <div v-if="markdown === null" class="prose-rite">
        <h1>Not found</h1>
        <p>
          No chapter named <code>{{ slug }}</code>.
          <RouterLink to="/docs">Back to overview</RouterLink>.
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
        v-if="slug && (neighbors.prev || neighbors.next)"
        class="mt-12 flex max-w-prose flex-wrap items-center justify-between gap-4 border-t border-rite-border pt-6 text-sm"
      >
        <RouterLink
          v-if="neighbors.prev"
          :to="`/docs/${neighbors.prev.slug}`"
          class="text-slate-400 hover:text-rite-accent"
        >
          ← {{ neighbors.prev.title }}
        </RouterLink>
        <span v-else />
        <RouterLink
          v-if="neighbors.next"
          :to="`/docs/${neighbors.next.slug}`"
          class="text-slate-400 hover:text-rite-accent"
        >
          {{ neighbors.next.title }} →
        </RouterLink>
      </nav>

      <p v-if="slug === 'browser' || slug === 'first-script'" class="mt-8 max-w-prose text-sm text-slate-400">
        Try pure examples in
        <RouterLink to="/studio" class="text-rite-accent hover:underline">Studio</RouterLink>
        without installing.
      </p>
    </article>
  </div>
</template>
