<script setup lang="ts">
import SiteNav from "./components/SiteNav.vue";
import SiteFooter from "./components/SiteFooter.vue";
</script>

<template>
  <div class="flex min-h-screen flex-col">
    <SiteNav />
    <main class="min-h-0 flex-1">
      <RouterView />
    </main>
    <SiteFooter />
  </div>
</template>
