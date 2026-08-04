import { createRouter, createWebHistory } from "vue-router";
import HomeView from "./views/HomeView.vue";

// Only the landing page is eager: a visitor who never opens the docs should not
// download the markdown renderer.
const DocsView = () => import("./views/DocsView.vue");
// Studio pulls in the graph renderer and, at runtime, a megabyte of engine.
// Nobody reading the front page should pay for either.
const StudioView = () => import("./views/StudioView.vue");
const NotFoundView = () => import("./views/NotFoundView.vue");

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    { path: "/", name: "home", component: HomeView, meta: { title: "Cant" } },
    {
      path: "/docs",
      name: "docs-index",
      component: DocsView,
      meta: { title: "Docs · Cant" },
    },
    {
      path: "/docs/:slug",
      name: "docs",
      component: DocsView,
      meta: { title: "Docs · Cant" },
    },
    {
      path: "/studio",
      name: "studio",
      component: StudioView,
      // `chrome: "studio"` makes App.vue lock the shell to the viewport and drop
      // the footer, so the panes scroll instead of the page.
      meta: { title: "Studio · Cant", chrome: "studio" },
    },
    { path: "/:pathMatch(.*)*", name: "not-found", component: NotFoundView },
  ],
  scrollBehavior(to, _from, saved) {
    if (saved) return saved;
    if (to.hash) return { el: to.hash, top: 80 };
    return { top: 0 };
  },
});

router.afterEach((to) => {
  const title = (to.meta.title as string | undefined) ?? "Cant";
  document.title = title;
});
