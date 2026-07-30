import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import "./style.css";
import "@studio/highlight.css";

createApp(App).use(router).mount("#app");
