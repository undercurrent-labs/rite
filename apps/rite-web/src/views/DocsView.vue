<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  adjacentChapters,
  chapterBySlug,
  DOC_CHAPTERS,
  docsIndexMarkdown,
  getDocMarkdown,
  getReferenceMarkdown,
  REFERENCE_PAGES,
  referenceBySlug,
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

/** The generated reference lives under /docs/reference/ so its slugs cannot
 *  collide with the book's — both have an `http` page, for instance. */
const isReference = computed(() => route.name === "reference");

const chapter = computed(() => {
  if (!slug.value) return undefined;
  return isReference.value ? referenceBySlug(slug.value) : chapterBySlug(slug.value);
});

/** null = no such chapter, undefined = still loading this chapter's chunk. */
const markdown = ref<string | null | undefined>(undefined);

watch(
  [slug, isReference],
  async ([current, reference]) => {
    markdown.value = undefined;
    const text = !current
      ? await docsIndexMarkdown()
      : reference
        ? await getReferenceMarkdown(current)
        : await getDocMarkdown(current);
    // A fast second navigation must not be overwritten by the slower first load.
    if (slug.value === current && isReference.value === reference) markdown.value = text;
  },
  { immediate: true }
);

/** Code blocks render as components; prose stays a single v-html run per gap. */
const segments = computed(() => (markdown.value ? segmentMarkdown(markdown.value) : []));

// Prev/next walks the book only; the reference is a lookup, not a reading order.
const neighbors = computed(() =>
  slug.value && !isReference.value ? adjacentChapters(slug.value) : {}
);

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

        <p class="mb-2 mt-6 text-xs font-mono uppercase tracking-wider text-rite-muted">
          Reference
        </p>
        <nav class="space-y-0.5 text-sm">
          <RouterLink
            v-for="page in REFERENCE_PAGES"
            :key="page.slug"
            :to="`/docs/reference/${page.slug}`"
            class="block rounded-md px-2 py-1.5"
            :class="
              isReference && slug === page.slug
                ? 'bg-slate-800/80 text-rite-accent'
                : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
            "
          >
            {{ page.title }}
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
        <optgroup label="Book">
          <option v-for="ch in DOC_CHAPTERS" :key="ch.slug" :value="ch.slug">
            {{ ch.title }}
          </option>
        </optgroup>
        <optgroup label="Reference">
          <option
            v-for="page in REFERENCE_PAGES"
            :key="page.slug"
            :value="`reference/${page.slug}`"
          >
            {{ page.title }}
          </option>
        </optgroup>
      </select>
    </div>

    <!-- Content -->
    <article class="min-w-0 flex-1">
      <div v-if="markdown === null" class="prose-rite">
        <h1>Not found</h1>
        <p>
          No {{ isReference ? "reference page" : "chapter" }} named <code>{{ slug }}</code>.
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
