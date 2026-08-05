<script setup lang="ts">
import CodeBlock from "../components/CodeBlock.vue";

// A 404 in the language the site is about. It parses.
const snippet = `request -> ?{ $.found } -> render -> []`;
</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-20">
    <p class="font-mono text-sm text-cant-accent">404</p>
    <h1 class="mt-2 text-3xl font-semibold tracking-tight text-white">
      Nothing was emitted here
    </h1>
    <p class="mt-4 max-w-prose text-slate-400">
      The ward did not pass. Nothing downstream ran, which is what a ward is
      for — and is also why this page is empty.
    </p>
    <div class="mt-8 max-w-xl">
      <CodeBlock :code="snippet" />
    </div>
    <div class="mt-8 flex flex-wrap gap-3">
      <RouterLink
        to="/"
        class="rounded-md border border-cant-accent/40 bg-cant-accent/10 px-4 py-2 text-sm font-medium text-cant-accent hover:bg-cant-accent/20"
        >Home</RouterLink
      >
      <RouterLink
        to="/docs"
        class="rounded-md border border-cant-border px-4 py-2 text-sm text-slate-300 hover:bg-slate-800/50"
        >Docs</RouterLink
      >
    </div>
  </div>
</template>
