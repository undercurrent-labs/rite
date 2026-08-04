<script setup lang="ts">
import { useRoute } from "vue-router";
import { RITE_URL } from "../lib/operators";

const route = useRoute();

const links = [
  { to: "/studio", label: "Studio", match: /^\/studio/ },
  { to: "/docs", label: "Docs", match: /^\/docs/ },
];

const github = "https://github.com/undercurrent-labs/rite/tree/main/docs/cant";
</script>

<template>
  <header class="sticky top-0 z-40 border-b border-cant-border bg-cant-bg/90 backdrop-blur">
    <div class="mx-auto flex max-w-6xl items-center gap-3 px-4 py-3">
      <!--
        The wordmark states the relationship rather than explaining it: Rite's
        name, in Rite's style, struck through, and Cant's flow operator pointing
        away from it. `→` because it is Cant's own arrow — a generic chevron
        would say "next", and this says "derived from, and not the same thing".
      -->
      <RouterLink
        to="/"
        class="group flex shrink-0 items-center gap-2 font-semibold tracking-wide"
        aria-label="Cant — a sibling to Rite"
      >
        <img
          src="/brand/logo.svg"
          alt=""
          width="28"
          height="28"
          class="h-7 w-7 rounded-md border border-cant-accent/30 bg-cant-panel object-cover group-hover:border-cant-accent/60"
        />
        <span class="flex items-baseline gap-1.5">
          <span
            class="text-slate-500 line-through decoration-cant-accent/70 decoration-2"
            aria-hidden="true"
            >Rite</span
          >
          <span class="font-mono text-cant-accent" aria-hidden="true">→</span>
          <span class="text-cant-accent group-hover:text-cant-accent/80">Cant</span>
        </span>
      </RouterLink>

      <nav class="ml-4 flex items-center gap-1 text-sm">
        <RouterLink
          v-for="l in links"
          :key="l.to"
          :to="l.to"
          class="rounded-md px-3 py-1.5 transition-colors"
          :class="
            l.match.test(route.path)
              ? 'bg-slate-800/80 text-cant-accent'
              : 'text-slate-400 hover:bg-slate-800/40 hover:text-slate-100'
          "
        >
          {{ l.label }}
        </RouterLink>
      </nav>

      <div class="ml-auto flex items-center gap-2">
        <!--
          Same shape as the Rite site's nav: Studio is both an ordinary link on
          the left and a call to action on the right, in the site's own accent —
          pink here, cyan there. Hidden on Studio itself, where it would offer
          the page you are already on.
        -->
        <RouterLink
          v-if="route.path !== '/studio'"
          to="/studio"
          class="hidden items-center rounded-md border border-cant-accent/40 bg-cant-accent/10 px-3 py-1.5 text-sm font-medium text-cant-accent hover:bg-cant-accent/20 sm:inline-flex"
        >
          Try Studio
        </RouterLink>
        <!--
          The sibling link is the point of having two sites: someone who lands
          here looking for the language that actually runs today should be one
          click from it, and the relationship should be visible in the chrome
          rather than explained in a paragraph.
        -->
        <a
          :href="RITE_URL"
          class="hidden items-center rounded-md border border-cant-cyan/30 px-3 py-1.5 text-sm text-cant-cyan hover:bg-cant-cyan/10 sm:inline-flex"
        >
          Rite ↗
        </a>
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
