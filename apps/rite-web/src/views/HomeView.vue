<script setup lang="ts">
import CodeBlock from "../components/CodeBlock.vue";
import { useLatestTag } from "../lib/release";

const { tag: latestTag, resolved: tagResolved } = useLatestTag();

const installSnippet = `curl -fsSL https://rite.undrc.dev/install | bash
# → ~/.local/bin/rite  (+ rite-lsp)
export PATH="$HOME/.local/bin:$PATH"
rite version`;

const pillars = [
  {
    title: "Glyphic + ASCII",
    body: "Write with ◆ and ⟦ ⟧ when you want density, or def / [[ ]] when you want keys. Same AST — format either way.",
    accent: "text-rite-accent",
  },
  {
    title: "Effects & capabilities",
    body: "Host calls are explicit (! / do). Permissions gate FS, HTTP, and process so scripts stay honest about what they touch.",
    accent: "text-rite-pink",
  },
  {
    title: "Interpreter + IR",
    body: "Tree-walk semantics you can reason about, shared Program IR, AOT compile, embed from Rust, and run pure scripts in the browser.",
    accent: "text-rite-green",
  },
];

const sampleGlyph = `◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))`;

const sampleAscii = `def square(n) [[
  return n * n
]]
do host.console.println(str(square(12)))`;
</script>

<template>
  <div>
    <!-- Hero -->
    <section class="relative overflow-hidden border-b border-rite-border">
      <div
        class="pointer-events-none absolute inset-0 opacity-40"
        style="
          background:
            radial-gradient(ellipse 80% 50% at 20% -10%, rgba(126, 224, 255, 0.18), transparent 50%),
            radial-gradient(ellipse 60% 40% at 90% 10%, rgba(255, 126, 219, 0.12), transparent 45%);
        "
      />
      <div class="relative mx-auto max-w-6xl px-4 py-16 md:py-24">
        <p class="mb-3 font-mono text-xs uppercase tracking-[0.2em] text-rite-muted">
          scripting · capabilities · rust
        </p>
        <h1 class="max-w-3xl text-4xl font-semibold tracking-tight text-white md:text-5xl md:leading-tight">
          Scripts that look like
          <span class="text-rite-accent">sigils</span>
          and behave like
          <span class="text-rite-pink">contracts</span>.
        </h1>
        <p class="mt-5 max-w-2xl text-lg text-slate-400">
          Rite is a Rust-backed language for tools, pipelines, and embeds — dual glyph/ASCII syntax,
          explicit effects, and host permissions. Run it in the CLI, compile to native, or try pure
          scripts in the browser.
        </p>
        <div class="mt-8 flex flex-wrap gap-3">
          <RouterLink
            to="/studio"
            class="inline-flex items-center rounded-lg bg-rite-accent px-5 py-2.5 text-sm font-semibold text-rite-bg hover:bg-sky-200"
          >
            Open Studio
          </RouterLink>
          <RouterLink
            to="/docs/first-script"
            class="inline-flex items-center rounded-lg border border-slate-700 bg-rite-panel px-5 py-2.5 text-sm font-medium text-slate-100 hover:border-slate-500"
          >
            Read the book
          </RouterLink>
          <RouterLink
            to="/docs/installation"
            class="inline-flex items-center rounded-lg px-5 py-2.5 text-sm font-medium text-slate-400 hover:text-slate-100"
          >
            Install →
          </RouterLink>
          <RouterLink
            to="/agents"
            class="inline-flex items-center rounded-lg px-5 py-2.5 text-sm font-medium text-slate-400 hover:text-slate-100"
          >
            Agents &amp; skill
          </RouterLink>
        </div>
      </div>
    </section>

    <!-- Dual syntax -->
    <section class="mx-auto max-w-6xl px-4 py-16">
      <div class="mb-8 flex flex-wrap items-end justify-between gap-4">
        <div>
          <h2 class="text-2xl font-semibold text-white">One program, two skins</h2>
          <p class="mt-2 max-w-xl text-slate-400">
            Glyphic form for dense, readable scripts. ASCII when you need plain keyboards and diffs.
            Studio can convert either way.
          </p>
        </div>
        <RouterLink to="/studio" class="text-sm text-rite-accent hover:underline">
          Run this in Studio →
        </RouterLink>
      </div>
      <div class="grid gap-4 md:grid-cols-2">
        <CodeBlock :code="sampleGlyph" lang="rite" label="glyph" mode="browser" class="!my-0" />
        <CodeBlock :code="sampleAscii" lang="rite" label="ascii" mode="browser" class="!my-0" />
      </div>
      <p class="mt-3 font-mono text-sm text-rite-green">→ 144</p>
    </section>

    <!-- Pillars -->
    <section class="border-y border-rite-border bg-rite-panel/40">
      <div class="mx-auto grid max-w-6xl gap-6 px-4 py-16 md:grid-cols-3">
        <article
          v-for="p in pillars"
          :key="p.title"
          class="rounded-xl border border-rite-border bg-rite-card p-6"
        >
          <h3 class="text-lg font-semibold" :class="p.accent">{{ p.title }}</h3>
          <p class="mt-3 text-sm leading-relaxed text-slate-400">{{ p.body }}</p>
        </article>
      </div>
    </section>

    <!-- Paths -->
    <section class="mx-auto max-w-6xl px-4 py-16">
      <h2 class="text-2xl font-semibold text-white">Where to go next</h2>
      <div class="mt-8 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <RouterLink
          to="/studio"
          class="group rounded-xl border border-rite-border bg-rite-panel p-5 transition hover:border-rite-accent/40"
        >
          <div class="text-rite-accent font-medium">Studio</div>
          <p class="mt-2 text-sm text-slate-400">
            Browser playground with WASM. Share snippets, format, check, run pure code.
          </p>
          <span class="mt-3 inline-block text-sm text-slate-400 group-hover:text-rite-accent"
            >Open →</span
          >
        </RouterLink>
        <RouterLink
          to="/docs"
          class="group rounded-xl border border-rite-border bg-rite-panel p-5 transition hover:border-rite-pink/40"
        >
          <div class="text-rite-pink font-medium">Guided book</div>
          <p class="mt-2 text-sm text-slate-400">
            Install through pipelines, matching, capabilities, HTTP, modules, and embedding.
          </p>
          <span class="mt-3 inline-block text-sm text-slate-400 group-hover:text-rite-pink"
            >Start reading →</span
          >
        </RouterLink>
        <RouterLink
          to="/docs/installation"
          class="group rounded-xl border border-rite-border bg-rite-panel p-5 transition hover:border-rite-green/40"
        >
          <div class="text-rite-green font-medium">CLI & tooling</div>
          <p class="mt-2 text-sm text-slate-400">
            <code class="text-xs text-rite-green">rite run</code>,
            <code class="text-xs text-rite-green">fmt</code>,
            <code class="text-xs text-rite-green">check</code>, LSP, VS Code, compile to native.
          </p>
          <span class="mt-3 inline-block text-sm text-slate-400 group-hover:text-rite-green"
            >Install →</span
          >
        </RouterLink>
      </div>
    </section>

    <!-- Install strip -->
    <section class="border-t border-rite-border bg-rite-panel/30">
      <div class="mx-auto max-w-6xl px-4 py-12">
        <h2 class="text-lg font-semibold text-white">Install the CLI</h2>
        <p class="mt-2 max-w-2xl text-sm text-slate-400">
          No clone required — binaries from GitHub Releases, verified with SHA-256.
        </p>
        <CodeBlock class="mt-4" :code="installSnippet" lang="bash" />
        <p class="mt-3 text-sm text-slate-400">
          {{ tagResolved ? "Latest release" : "Packaged with this site" }}
          <code class="text-rite-green text-xs">{{ latestTag }}</code>
          · pin with
          <code class="text-rite-green text-xs">RITE_VERSION={{ latestTag }}</code>
          · details in
          <RouterLink to="/docs/installation" class="text-rite-accent hover:underline"
            >Installation</RouterLink
          >
          ·
          <RouterLink to="/agents" class="text-rite-accent hover:underline">Agents</RouterLink>
          · or try
          <RouterLink to="/studio" class="text-rite-accent hover:underline">Studio</RouterLink>
          with zero install.
        </p>
      </div>
    </section>

  </div>
</template>
