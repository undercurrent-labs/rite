<script setup lang="ts">
import { CANT_VERSION, RITE_URL } from "../lib/operators";

const host = RITE_URL.replace(/^https?:\/\//, "");
</script>

<template>
  <footer class="border-t border-cant-border bg-cant-bg">
    <div
      class="mx-auto flex max-w-6xl flex-col gap-3 px-4 py-8 text-sm text-slate-500 sm:flex-row sm:items-center"
    >
      <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span class="font-mono text-cant-accent">cant {{ CANT_VERSION }}</span>
        <span class="text-slate-700">·</span>
        <span>experimental</span>
      </div>
      <div class="flex flex-wrap items-center gap-x-4 gap-y-1 sm:ml-auto">
        <RouterLink to="/docs" class="hover:text-slate-300">Docs</RouterLink>
        <a :href="RITE_URL" class="text-cant-cyan hover:text-cant-cyan/80">{{ host }}</a>
        <a
          href="https://github.com/undercurrent-labs/rite"
          target="_blank"
          rel="noopener noreferrer"
          class="hover:text-slate-300"
          >GitHub</a
        >
      </div>
    </div>
  </footer>
</template>
