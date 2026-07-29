import { createRouter, createWebHistory } from "vue-router";
import HomeView from "./views/HomeView.vue";
import DocsView from "./views/DocsView.vue";
import StudioView from "./views/StudioView.vue";
import AgentsView from "./views/AgentsView.vue";

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: "/", name: "home", component: HomeView, meta: { title: "Rite" } },
    {
      path: "/docs",
      name: "docs-index",
      component: DocsView,
      meta: { title: "Docs · Rite" },
    },
    {
      path: "/docs/:slug",
      name: "docs",
      component: DocsView,
      meta: { title: "Docs · Rite" },
    },
    {
      path: "/agents",
      name: "agents",
      component: AgentsView,
      meta: { title: "Agents · Rite" },
    },
    {
      path: "/studio",
      name: "studio",
      component: StudioView,
      meta: { title: "Studio · Rite", chrome: "studio" },
    },
    // Back-compat: bare hash shares used to live on /
    {
      path: "/play",
      redirect: (to) => ({ path: "/studio", hash: to.hash, query: to.query }),
    },
  ],
  scrollBehavior(to, _from, saved) {
    if (saved) return saved;
    if (to.hash) return { el: to.hash, behavior: "smooth" };
    return { top: 0 };
  },
});

router.afterEach((to) => {
  const title = (to.meta.title as string) || "Rite";
  if (typeof document !== "undefined") document.title = title;
});
