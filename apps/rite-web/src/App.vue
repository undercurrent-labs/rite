<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import SiteNav from "./components/SiteNav.vue";
import SiteFooter from "./components/SiteFooter.vue";

const route = useRoute();
const isStudio = computed(() => route.meta.chrome === "studio");
</script>

<template>
  <div class="min-h-screen flex flex-col" :class="isStudio ? 'h-screen overflow-hidden' : ''">
    <SiteNav :compact="isStudio" />
    <main class="flex-1 min-h-0" :class="isStudio ? 'overflow-hidden' : ''">
      <RouterView />
    </main>
    <SiteFooter v-if="!isStudio" />
  </div>
</template>
