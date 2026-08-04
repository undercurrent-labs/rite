<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import SiteNav from "./components/SiteNav.vue";
import SiteFooter from "./components/SiteFooter.vue";

/*
 * Studio takes the whole viewport, the way the Rite site's does: a playground
 * that scrolls the page instead of its own panes is unusable, and a footer under
 * an editor is a footer nobody reaches.
 */
const route = useRoute();
const isStudio = computed(() => route.meta.chrome === "studio");
</script>

<template>
  <div class="flex min-h-screen flex-col" :class="isStudio ? 'h-screen overflow-hidden' : ''">
    <SiteNav />
    <main class="min-h-0 flex-1" :class="isStudio ? 'overflow-hidden' : ''">
      <RouterView />
    </main>
    <SiteFooter v-if="!isStudio" />
  </div>
</template>
