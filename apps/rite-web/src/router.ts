import { createRouter, createWebHistory } from "vue-router";
import HomeView from "./views/HomeView.vue";

/*
 * Only the landing page is eager. Studio in particular pulls in the whole
 * playground and its runtime bridge, which a reader who never leaves /docs
 * should not be made to download.
 */
const DocsView = () => import("./views/DocsView.vue");
const StudioView = () => import("./views/StudioView.vue");
const AgentsView = () => import("./views/AgentsView.vue");
const NotFoundView = () => import("./views/NotFoundView.vue");

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
    // Before /docs/:slug, or "reference" would match as a chapter slug.
    {
      path: "/docs/reference/:slug",
      name: "reference",
      component: DocsView,
      meta: { title: "Reference · Rite" },
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
    // Cloudflare's SPA fallback answers every unknown path with index.html, so
    // without this the router matches nothing and renders a blank page.
    {
      path: "/:pathMatch(.*)*",
      name: "not-found",
      component: NotFoundView,
      meta: { title: "Not found · Rite" },
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
