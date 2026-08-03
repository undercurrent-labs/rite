/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

/** Injected by vite.config.ts — see the `define` block there. */
declare const __CANT_VERSION__: string;
declare const __RITE_HOST__: string;
declare const __CANT_OPERATORS__: {
  concept: string;
  token: string;
  ascii: string;
  glyph: string | null;
  ambiguous: boolean;
  description: string;
}[];
