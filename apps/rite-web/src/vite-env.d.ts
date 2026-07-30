/// <reference types="vite/client" />

/** Injected by vite.config.ts from the workspace Cargo.toml (e.g. "v0.3.0"). */
declare const __RITE_VERSION__: string;

/** True when public/vscode/rite.vsix existed at build time. */
declare const __HAS_VSIX__: boolean;

declare module "*.md?raw" {
  const content: string;
  export default content;
}

declare module "*.vue" {
  import type { DefineComponent } from "vue";
  const component: DefineComponent<object, object, unknown>;
  export default component;
}
