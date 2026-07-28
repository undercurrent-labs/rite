<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";

const props = withDefaults(
  defineProps<{
    compact?: boolean;
  }>(),
  { compact: false }
);

const route = useRoute();

const links = [
  { to: "/docs", label: "Docs", match: /^\/docs/ },
  { to: "/studio", label: "Studio", match: /^\/studio/ },
];

const github = "https://github.com/undercurrent-labs/rite";

function isActive(match: RegExp) {
  return match.test(route.path);
}

const barClass = computed(() =>
  props.compact
    ? "border-b border-rite-border bg-rite-bg/95 backdrop-blur sticky top-0 z-40"
    : "border-b border-rite-border bg-rite-bg/90 backdrop-blur sticky top-0 z-40"
);
</script>

<template>
  <header :class="barClass">
    <div
      class="mx-auto flex max-w-6xl items-center gap-3 px-4 py-3"
      :class="compact ? 'max-w-none px-3' : ''"
    >
      <RouterLink
        to="/"
        class="group flex items-center gap-2 font-semibold tracking-wide text-rite-accent shrink-0"
      >
        <span
          class="inline-flex h-7 w-7 items-center justify-center rounded-md border border-rite-accent/30 bg-rite-panel text-sm group-hover:border-rite-accent/60"
          aria-hidden="true"
          >◆</span
        >
        <span class="text-slate-100 group-hover:text-white">Rite</span>
      </RouterLink>

      <nav class="ml-4 flex items-center gap-1 text-sm">
        <RouterLink
          v-for="l in links"
          :key="l.to"
          :to="l.to"
          class="rounded-md px-3 py-1.5 transition-colors"
          :class="
            isActive(l.match)
              ? 'bg-slate-800/80 text-rite-accent'
              : 'text-slate-400 hover:text-slate-100 hover:bg-slate-800/40'
          "
        >
          {{ l.label }}
        </RouterLink>
      </nav>

      <div class="ml-auto flex items-center gap-2">
        <RouterLink
          v-if="!compact && route.path !== '/studio'"
          to="/studio"
          class="hidden sm:inline-flex items-center rounded-md border border-rite-accent/40 bg-rite-accent/10 px-3 py-1.5 text-sm font-medium text-rite-accent hover:bg-rite-accent/20"
        >
          Try Studio
        </RouterLink>
        <a
          :href="github"
          target="_blank"
          rel="noopener noreferrer"
          class="rounded-md px-3 py-1.5 text-sm text-slate-400 hover:text-slate-100"
        >
          GitHub
        </a>
      </div>
    </div>
  </header>
</template>
