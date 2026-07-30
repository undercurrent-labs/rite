<script setup lang="ts">
const github = "https://github.com/undercurrent-labs/rite";
const labs = "https://undrc.dev";
</script>

<template>
  <footer class="border-t border-rite-border py-8 text-center text-sm text-slate-400">
    <p class="mb-3 flex justify-center">
      <img src="/brand/logo.png" alt="Rite" width="40" height="40" class="h-10 w-10 rounded-lg opacity-90" />
    </p>
    <p>
      Rite ·
      <RouterLink to="/docs" class="hover:text-slate-300">Docs</RouterLink>
      ·
      <RouterLink to="/studio" class="hover:text-slate-300">Studio</RouterLink>
      ·
      <a :href="github" class="hover:text-slate-300" target="_blank" rel="noopener noreferrer"
        >GitHub</a
      >
    </p>
    <p class="mt-3 text-xs text-slate-400 max-w-lg mx-auto leading-relaxed px-4">
      Hosted at
      <a href="https://rite.undrc.dev" class="text-slate-400 hover:text-slate-300">rite.undrc.dev</a>
      by
      <a :href="labs" class="text-slate-400 hover:text-slate-300" target="_blank" rel="noopener noreferrer"
        >Undercurrent Labs LLC</a
      >.
    </p>
    <p class="mt-2 text-xs text-slate-400">
      A language can be visually strange while remaining semantically disciplined.
    </p>
  </footer>
</template>
