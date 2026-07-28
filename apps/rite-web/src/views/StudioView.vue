<script setup lang="ts">
// Studio playground — shared implementation from apps/rite-studio
import StudioApp from "@studio/App.vue";
</script>

<template>
  <div class="h-full min-h-0 flex flex-col studio-shell">
    <StudioApp />
  </div>
</template>

<style scoped>
.studio-shell {
  /* Fill viewport under site nav (~52px) */
  height: calc(100vh - 52px);
  height: calc(100dvh - 52px);
}
.studio-shell :deep(> div) {
  min-height: 0;
  height: 100%;
}
</style>
