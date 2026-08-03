<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { CANT_DOCS, adjacentDocs, docBySlug, getDocMarkdown } from "../lib/docs";
import { headingsOf, renderMarkdown, type Heading } from "../lib/markdown";

const route = useRoute();
const slug = computed(() => (route.params.slug as string | undefined) ?? null);
const doc = computed(() => (slug.value ? docBySlug(slug.value) : undefined));

const html = ref("");
const headings = ref<Heading[]>([]);
const missing = ref(false);
const loading = ref(false);

async function load(current: string | null) {
  if (!current) {
    html.value = "";
    headings.value = [];
    missing.value = false;
    return;
  }
  loading.value = true;
  const md = await getDocMarkdown(current);
  loading.value = false;
  if (md === null) {
    missing.value = true;
    html.value = "";
    headings.value = [];
    return;
  }
  missing.value = false;
  html.value = renderMarkdown(md);
  headings.value = headingsOf(html.value);
}

watch(slug, load, { immediate: true });

const neighbours = computed(() => (slug.value ? adjacentDocs(slug.value) : {}));
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-10">
    <div class="grid gap-10 lg:grid-cols-[15rem_minmax(0,1fr)]">
      <!-- Sidebar -->
      <aside class="lg:sticky lg:top-20 lg:self-start">
        <p class="mb-2 font-mono text-xs uppercase tracking-wider text-slate-500">
          Contents
        </p>
        <nav class="space-y-0.5 text-sm">
          <RouterLink
            v-for="d in CANT_DOCS"
            :key="d.slug"
            :to="`/docs/${d.slug}`"
            class="block rounded px-2 py-1.5 transition-colors"
            :class="
              slug === d.slug
                ? 'bg-slate-800/80 text-cant-accent'
                : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
            "
          >
            {{ d.title }}
          </RouterLink>
        </nav>

        <!--
          On-page contents, second so the document list stays at eye level. Only
          h2s: the documents are long and a three-level tree competes with the
          prose instead of helping someone find their place in it.
        -->
        <template v-if="headings.filter((h) => h.level === 2).length > 2">
          <p class="mb-2 mt-6 font-mono text-xs uppercase tracking-wider text-slate-500">
            On this page
          </p>
          <nav class="space-y-0.5 border-l border-slate-800 text-sm">
            <a
              v-for="h in headings.filter((x) => x.level === 2)"
              :key="h.id"
              :href="`#${h.id}`"
              class="block border-l-2 border-transparent py-1 pl-3 text-slate-500 hover:border-cant-accent/50 hover:text-slate-300"
            >
              {{ h.text }}
            </a>
          </nav>
        </template>
      </aside>

      <!-- Body -->
      <div class="min-w-0">
        <!-- Index -->
        <template v-if="!slug">
          <h1 class="text-3xl font-semibold tracking-tight text-white">Cant documentation</h1>
          <p class="mt-3 max-w-prose text-slate-400">
            Four pages: what the language is, how it works, how to run it, and the
            shape of the graph it produces.
          </p>

          <div class="mt-8 space-y-3">
            <RouterLink
              v-for="d in CANT_DOCS"
              :key="d.slug"
              :to="`/docs/${d.slug}`"
              class="block rounded-lg border border-cant-border bg-cant-panel/50 p-4 transition-colors hover:border-cant-accent/40"
            >
              <div class="font-medium text-slate-100">{{ d.title }}</div>
              <div class="mt-1 text-sm text-slate-400">{{ d.blurb }}</div>
            </RouterLink>
          </div>

        </template>

        <!-- A document -->
        <template v-else-if="missing">
          <h1 class="text-2xl font-semibold text-white">No document named “{{ slug }}”</h1>
          <p class="mt-3 text-slate-400">
            <RouterLink to="/docs" class="text-cant-accent hover:underline"
              >Back to the index</RouterLink
            >.
          </p>
        </template>

        <template v-else>
          <div v-if="loading" class="text-slate-500">Loading…</div>
          <article v-else class="prose-cant max-w-prose" v-html="html"></article>

          <nav
            v-if="!loading && (neighbours.prev || neighbours.next)"
            class="mt-14 flex justify-between gap-4 border-t border-slate-800 pt-6 text-sm"
          >
            <RouterLink
              v-if="neighbours.prev"
              :to="`/docs/${neighbours.prev.slug}`"
              class="text-slate-400 hover:text-cant-accent"
            >
              ← {{ neighbours.prev.title }}
            </RouterLink>
            <span v-else></span>
            <RouterLink
              v-if="neighbours.next"
              :to="`/docs/${neighbours.next.slug}`"
              class="text-right text-slate-400 hover:text-cant-accent"
            >
              {{ neighbours.next.title }} →
            </RouterLink>
          </nav>
        </template>
      </div>
    </div>
  </div>
</template>
