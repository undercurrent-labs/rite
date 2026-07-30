<script setup lang="ts">
import { computed } from "vue";
import CodeBlock from "../components/CodeBlock.vue";
import { useLatestTag } from "../lib/release";

/** Site static files (must be real assets in dist — not SPA HTML). */
const skillTarSite = "/skill/rite-agent-skill.tar.gz";
const skillZipSite = "/skill/rite-agent-skill.zip";
const vsixSite = "/vscode/rite.vsix";
/**
 * The VSIX mirror only exists when the deploy pipeline copied a packaged
 * extension into public/vscode/. Linking it unconditionally served the SPA's
 * index.html as a .vsix download.
 */
const hasVsixMirror = __HAS_VSIX__;
/** GitHub Releases latest/download — reliable binary downloads. */
const skillTarGh =
  "https://github.com/undercurrent-labs/rite/releases/latest/download/rite-agent-skill.tar.gz";
const skillZipGh =
  "https://github.com/undercurrent-labs/rite/releases/latest/download/rite-agent-skill.zip";
const vsixGh =
  "https://github.com/undercurrent-labs/rite/releases/latest/download/rite.vsix";
const installCli = "curl -fsSL https://rite.undrc.dev/install | bash";

const { tag: latestTag, resolved: tagResolved } = useLatestTag();

const cliSnippet = computed(
  () => `${installCli}
# pin: RITE_VERSION=${latestTag.value} curl -fsSL https://rite.undrc.dev/install | bash`
);
const skillSnippet = `rite skill install --target all
# or:  rite skill install --target grok`;
const vscodeSnippet = `rite vscode install
rite vscode download --out ./rite.vsix`;
const updateSnippet = `rite update --check
rite update`;

</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-12">
    <p class="mb-2 font-mono text-xs uppercase tracking-[0.2em] text-rite-muted">
      agents · skills · editors
    </p>
    <h1 class="text-3xl font-semibold tracking-tight text-white md:text-4xl">
      Agent skill &amp; tooling
    </h1>
    <p class="mt-4 text-lg text-slate-400">
      Give coding agents authoritative Rite context (syntax, capabilities, diagnostics) and install
      the CLI / VS&nbsp;Code extension without cloning the monorepo.
    </p>

    <!-- Quick install -->
    <section class="mt-12 rounded-xl border border-rite-border bg-rite-panel p-6">
      <h2 class="text-xl font-semibold text-white">One-liners</h2>
      <p class="mt-2 text-sm text-slate-400">
        {{ tagResolved ? "Latest release:" : "Packaged with this site:" }}
        <code class="text-rite-green">{{ latestTag }}</code>
      </p>
      <div class="mt-4 space-y-4 font-mono text-sm">
        <div>
          <p class="mb-1 text-xs text-rite-muted">CLI (+ LSP)</p>
          <CodeBlock :code="cliSnippet" lang="bash" />
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">Agent skill (Grok / Claude / Cursor)</p>
          <CodeBlock :code="skillSnippet" lang="bash" />
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">VS Code / Cursor extension</p>
          <CodeBlock :code="vscodeSnippet" lang="bash" />
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">Self-update (CLI + skill freshness)</p>
          <CodeBlock :code="updateSnippet" lang="bash" />
        </div>
      </div>
    </section>

    <!-- Downloads -->
    <section class="mt-10">
      <h2 class="text-xl font-semibold text-white">Direct downloads</h2>
      <p class="mt-2 text-sm text-slate-400">
        GitHub Releases is authoritative. Site mirrors are published under
        <code class="text-slate-300">/skill/</code> and
        <code class="text-slate-300">/vscode/</code> by the deploy pipeline, and are only
        linked here when they actually shipped.
      </p>
      <ul class="mt-4 space-y-3 text-slate-300">
        <li>
          <a :href="skillTarGh" class="text-rite-accent hover:underline" target="_blank" rel="noopener"
            >rite-agent-skill.tar.gz</a
          >
          <span class="text-slate-400"> (GitHub)</span>
          ·
          <a :href="skillTarSite" class="text-slate-400 hover:text-rite-accent hover:underline"
            >site mirror</a
          >
        </li>
        <li>
          <a :href="skillZipGh" class="text-rite-accent hover:underline" target="_blank" rel="noopener"
            >rite-agent-skill.zip</a
          >
          <span class="text-slate-400"> (GitHub)</span>
          ·
          <a :href="skillZipSite" class="text-slate-400 hover:text-rite-accent hover:underline"
            >site mirror</a
          >
        </li>
        <li>
          <a :href="vsixGh" class="text-rite-accent hover:underline" target="_blank" rel="noopener"
            >rite.vsix</a
          >
          <span class="text-slate-400"> (GitHub)</span>
          <template v-if="hasVsixMirror">
            ·
            <a :href="vsixSite" class="text-slate-300 hover:text-rite-accent hover:underline"
              >site mirror</a
            >
          </template>
        </li>
        <li>
          <a
            href="https://github.com/undercurrent-labs/rite/releases"
            class="text-rite-accent hover:underline"
            target="_blank"
            rel="noopener"
            >All GitHub Releases assets</a
          >
        </li>
      </ul>
    </section>

    <!-- Skill details -->
    <section class="mt-12 prose-rite">
      <h2 class="text-xl font-semibold text-white">What the skill contains</h2>
      <pre class="mt-3 overflow-x-auto rounded-lg border border-rite-border bg-rite-panel p-4 font-mono text-xs text-slate-300">skills/rite/
  SKILL.md              # rules agents must follow
  machine/              # grammar, aliases, capabilities, diagnostics (JSON)
  references/           # quick-reference, capabilities, diagnostics
  examples/scripts/     # sample .rite</pre>
      <p class="mt-4 text-slate-400">
        After <code class="text-rite-green">rite skill install</code>, the skill is cached under
        <code class="text-slate-300">~/.local/share/rite/skill/rite</code> and linked into agent
        discovery paths (e.g. <code class="text-slate-300">~/.grok/skills/rite</code>). Config records
        last pull time and version in <code class="text-slate-300">~/.config/rite/config.json</code>.
      </p>
    </section>

    <!-- Manual agent install -->
    <section class="mt-12">
      <h2 class="text-xl font-semibold text-white">Manual skill install (no CLI yet)</h2>
      <pre class="mt-3 overflow-x-auto rounded-lg bg-black/40 p-4 font-mono text-sm text-slate-300"># GitHub (recommended)
curl -fsSL -o skill.tgz \
  https://github.com/undercurrent-labs/rite/releases/latest/download/rite-agent-skill.tar.gz
# or site mirror: https://rite.undrc.dev/skill/rite-agent-skill.tar.gz

mkdir -p ~/.grok/skills
tar -xzf skill.tgz -C ~/.grok/skills
# → ~/.grok/skills/rite/SKILL.md

# Claude / Cursor
mkdir -p ~/.claude/skills &amp;&amp; tar -xzf skill.tgz -C ~/.claude/skills
mkdir -p ~/.cursor/skills &amp;&amp; tar -xzf skill.tgz -C ~/.cursor/skills</pre>
    </section>

    <!-- LLM-facing summary -->
    <section class="mt-12 rounded-xl border border-rite-accent/20 bg-rite-accent/5 p-6">
      <h2 class="text-lg font-semibold text-rite-accent">For agents reading this page</h2>
      <ul class="mt-3 list-disc space-y-2 pl-5 text-sm text-slate-300">
        <li>
          Prefer the skill bundle over inventing syntax —
          <code class="text-rite-green">machine/aliases.json</code> and
          <code class="text-rite-green">machine/grammar.ebnf</code> are authoritative.
        </li>
        <li>
          Effectful host calls need <code class="text-rite-green">!</code> /
          <code class="text-rite-green">do</code> (diagnostic E021).
        </li>
        <li>
          Validate with <code class="text-rite-green">rite check</code> and
          <code class="text-rite-green">rite run --allow-all</code> when the CLI is available.
        </li>
        <li>
          Machine introspection:
          <code class="text-rite-green">rite describe language --json</code>.
        </li>
      </ul>
    </section>

    <p class="mt-10 text-sm text-slate-400">
      Docs:
      <RouterLink to="/docs/installation" class="text-rite-accent hover:underline">Installation</RouterLink>
      ·
      <RouterLink to="/docs/first-script" class="text-rite-accent hover:underline">First script</RouterLink>
      ·
      <a href="https://github.com/undercurrent-labs/rite" class="text-rite-accent hover:underline"
        >Source</a
      >
    </p>
  </div>
</template>
