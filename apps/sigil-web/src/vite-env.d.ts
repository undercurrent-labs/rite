/// <reference types="vite/client" />

declare const __SIGIL_VERSION__: string;
declare const __RITE_HOST__: string;
declare const __CANT_HOST__: string;
declare const __SIGIL_EXAMPLES__: { name: string; source: string }[];

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<Record<string, never>, Record<string, never>, unknown>;
  export default component;
}
