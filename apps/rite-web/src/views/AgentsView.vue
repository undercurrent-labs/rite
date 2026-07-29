<script setup lang="ts">
import { onMounted, ref } from "vue";

const skillTar = "/skill/rite-agent-skill.tar.gz";
const skillZip = "/skill/rite-agent-skill.zip";
const vsix = "/vscode/rite.vsix";
const installCli = "curl -fsSL https://rite.undrc.dev/install | bash";
const latestTag = ref("v0.1.8");

onMounted(async () => {
  try {
    const res = await fetch(
      "https://api.github.com/repos/undercurrent-labs/rite/releases/latest",
      { headers: { Accept: "application/vnd.github+json" } }
    );
    if (!res.ok) return;
    const data = (await res.json()) as { tag_name?: string };
    if (data.tag_name) latestTag.value = data.tag_name;
  } catch {
    /* keep fallback */
  }
});
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
        Latest release:
        <code class="text-rite-green">{{ latestTag }}</code>
      </p>
      <div class="mt-4 space-y-4 font-mono text-sm">
        <div>
          <p class="mb-1 text-xs text-rite-muted">CLI (+ LSP)</p>
          <pre class="overflow-x-auto rounded-lg bg-black/40 p-3 text-rite-green">{{ installCli }}
# pin: RITE_VERSION={{ latestTag }} curl -fsSL https://rite.undrc.dev/install | bash</pre>
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">Agent skill (Grok / Claude / Cursor)</p>
          <pre class="overflow-x-auto rounded-lg bg-black/40 p-3 text-rite-green">rite skill install --target all
# or:  rite skill install --target grok</pre>
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">VS Code / Cursor extension</p>
          <pre class="overflow-x-auto rounded-lg bg-black/40 p-3 text-rite-green">rite vscode install
rite vscode download --out ./rite.vsix</pre>
        </div>
        <div>
          <p class="mb-1 text-xs text-rite-muted">Self-update (CLI + skill freshness)</p>
          <pre class="overflow-x-auto rounded-lg bg-black/40 p-3 text-rite-green">rite update --check
rite update</pre>
        </div>
      </div>
    </section>

    <!-- Downloads -->
    <section class="mt-10">
      <h2 class="text-xl font-semibold text-white">Direct downloads</h2>
      <ul class="mt-4 space-y-3 text-slate-300">
        <li>
          <a :href="skillTar" class="text-rite-accent hover:underline">rite-agent-skill.tar.gz</a>
          <span class="text-slate-500"> — skill bundle for agents</span>
        </li>
        <li>
          <a :href="skillZip" class="text-rite-accent hover:underline">rite-agent-skill.zip</a>
          <span class="text-slate-500"> — same bundle as zip</span>
        </li>
        <li>
          <a :href="vsix" class="text-rite-accent hover:underline">rite.vsix</a>
          <span class="text-slate-500"> — VS Code / Cursor extension (when published)</span>
        </li>
        <li>
          <a
            href="https://github.com/undercurrent-labs/rite/releases"
            class="text-rite-accent hover:underline"
            target="_blank"
            rel="noopener"
            >GitHub Releases</a
          >
          <span class="text-slate-500"> — platform binaries, checksums, skill, vsix</span>
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
      <pre class="mt-3 overflow-x-auto rounded-lg bg-black/40 p-4 font-mono text-sm text-slate-300">curl -fsSL https://rite.undrc.dev/skill/rite-agent-skill.tar.gz -o skill.tgz
mkdir -p ~/.grok/skills
tar -xzf skill.tgz -C ~/.grok/skills
# → ~/.grok/skills/rite/SKILL.md

# Claude Code
mkdir -p ~/.claude/skills &amp;&amp; tar -xzf skill.tgz -C ~/.claude/skills

# Cursor
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

    <p class="mt-10 text-sm text-slate-500">
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
