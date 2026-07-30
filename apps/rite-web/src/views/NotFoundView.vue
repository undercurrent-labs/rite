<script setup lang="ts">
import { useRoute } from "vue-router";

const route = useRoute();
</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-24">
    <p class="mb-3 font-mono text-xs uppercase tracking-[0.2em] text-rite-muted">error · 404</p>
    <h1 class="text-3xl font-semibold tracking-tight text-white md:text-4xl">No such page</h1>
    <p class="mt-4 text-lg text-slate-400">
      Nothing is served at
      <code class="font-mono text-base text-rite-green">{{ route.path }}</code>
      .
    </p>
    <div class="mt-8 flex flex-wrap gap-3">
      <RouterLink
        to="/"
        class="inline-flex items-center rounded-lg bg-rite-accent px-5 py-2.5 text-sm font-semibold text-rite-bg hover:bg-sky-200"
      >
        Home
      </RouterLink>
      <RouterLink
        to="/docs"
        class="inline-flex items-center rounded-lg border border-slate-700 bg-rite-panel px-5 py-2.5 text-sm font-medium text-slate-100 hover:border-slate-500"
      >
        Read the book
      </RouterLink>
      <RouterLink
        to="/studio"
        class="inline-flex items-center rounded-lg px-5 py-2.5 text-sm font-medium text-slate-400 hover:text-slate-100"
      >
        Studio →
      </RouterLink>
    </div>
  </div>
</template>
