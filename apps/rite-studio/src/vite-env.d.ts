/// <reference types="vite/client" />

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<{}, {}, any>;
  export default component;
}

/**
 * Highlighting tables injected by vite.config.ts from grammar/ and the
 * capability manifest. Declared here because both apps compile these sources.
 */
declare const __RITE_GRAMMAR__: {
  keywords: string[];
  softKeywords: string[];
  glyphs: string[];
  capabilities: string[];
  capabilityFns: string[];
};
