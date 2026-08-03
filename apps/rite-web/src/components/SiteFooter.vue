<script setup lang="ts">
const github = "https://github.com/undercurrent-labs/rite";
const labs = "https://undrc.dev";

/*
 * Cant is linked from the footer rather than the nav on purpose.
 *
 * It is a sibling language, not a section of Rite — putting it beside Docs and
 * Studio would say it is part of this site, which is the relationship
 * docs/adr/0001-cant-sibling-frontend.md exists to deny. A footer link says
 * "there is another one of these", which is exactly true.
 */
const cant = "https://cant.rite.foo";
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
      <a href="https://rite.foo" class="text-slate-400 hover:text-slate-300">rite.foo</a>
      by
      <a :href="labs" class="text-slate-400 hover:text-slate-300" target="_blank" rel="noopener noreferrer"
        >Undercurrent Labs LLC</a
      >.
    </p>
    <p class="mt-3 text-xs text-slate-500">
      <a :href="cant" class="text-slate-400 hover:text-slate-300">Cant</a> — an
      experimental sibling language that lowers to Rite
    </p>
    <p class="mt-2 text-xs italic text-slate-400">
      a language can be visually strange while remaining semantically disciplined
    </p>
  </footer>
</template>
