<script setup lang="ts">
import CodeBlock from "../components/CodeBlock.vue";
import {
  VOCABULARY,
  asciiUsage,
  glyphUsage,
  hasDistinctGlyph,
  displayName,
  renderDescription,
  RITE_URL,
} from "../lib/operators";

// Every name in this program is real — it runs as written in a directory of
// modules, and \`cant check\` holds it to that. The old hero used four
// undefined names and could never run; a homepage must not open with a
// program the language rejects.
const hero = `["main"]
  -> *
  -> ~{ !@fs.read($ + ".cant")?
        -> @regex.find_all($, "use [a-z_]+")?
        -> *
        -> replace($, "use ", "") }
     :by str
     :max 4096
  -> []`;

const filter = `[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []`;
const fork = `5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []`;

const shell = `$ cant -e '[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []'
[2, 4, 6]`;

const views = [
  { command: "cant graph", body: "The topology, as JSON or Graphviz DOT." },
  {
    command: "cant expand",
    body: "The Rite it compiles to — ordinary, readable, and exactly what runs.",
  },
  {
    command: "cant explain",
    body: "The same program in prose, with the capabilities it needs.",
  },
];
</script>

<template>
  <div>
    <!-- Hero -->
    <section class="border-b border-cant-border">
      <div class="mx-auto max-w-6xl px-4 py-16 sm:py-24">
        <h1 class="max-w-3xl text-4xl font-semibold tracking-tight text-white sm:text-5xl">
          A graph-oriented language you can
          <span class="text-cant-accent">type into a terminal</span>.
        </h1>

        <p class="mt-5 max-w-prose text-lg leading-relaxed text-slate-400">
          Cant is a sibling to
          <a :href="RITE_URL" class="text-cant-cyan hover:underline">Rite</a>. Every
          stage emits zero or more values, so scatter, collect, ward, fork and orbit
          change how many are in flight. It compiles to Rite and runs on Rite's
          runtime, capabilities and compiler.
        </p>

        <div class="mt-10 grid gap-6 lg:grid-cols-[1.15fr_1fr]">
          <CodeBlock :code="hero" label="walking a dependency tree" />
          <div class="space-y-3 text-sm leading-relaxed text-slate-400">
            <p>
              Start from <code class="font-mono text-cant-accent">main</code>. Walk
              breadth-first: read each unseen module, pull its
              <code class="font-mono text-slate-300">use</code> lines out with a
              regex, and follow them. <code class="font-mono text-slate-300">:by
              str</code> means a module reached twice is visited once;
              <code class="font-mono text-slate-300">:max 4096</code> is a hard
              bound on how long the walk can run. Collect every module found.
            </p>
            <p>
              Every name in it is real — this runs, as written, in a directory of
              modules.
            </p>
          </div>
        </div>
      </div>
    </section>

    <!-- Reading a flow -->
    <section class="border-b border-cant-border bg-cant-panel/30">
      <div class="mx-auto max-w-6xl px-4 py-14">
        <h2 class="text-2xl font-semibold text-white">Reading a flow</h2>
        <div class="mt-8 grid gap-8 lg:grid-cols-2">
          <div class="space-y-3">
            <CodeBlock :code="filter" label="filter" />
            <p class="text-sm leading-relaxed text-slate-400">
              The list is one emission.
              <code class="font-mono text-cant-accent">*</code> scatters it into six.
              The ward passes the three even ones and emits nothing for the rest.
              <code class="font-mono text-cant-accent">[]</code> gathers what survived
              into <code class="font-mono text-cant-green">[2, 4, 6]</code>.
            </p>
          </div>
          <div class="space-y-3">
            <CodeBlock :code="fork" label="fork" />
            <p class="text-sm leading-relaxed text-slate-400">
              Every branch sees the same input, and their emissions are concatenated in
              order: <code class="font-mono text-cant-green">[6, 10, 25]</code>.
              Branches run left to right, one after another — or concurrently, with
              <code class="font-mono text-cant-accent">:par</code>, which joins their
              results in the same order and so produces the same list.
            </p>
          </div>
        </div>
      </div>
    </section>

    <!-- Vocabulary -->
    <section class="border-b border-cant-border">
      <div class="mx-auto max-w-6xl px-4 py-14">
        <h2 class="text-2xl font-semibold text-white">The whole vocabulary</h2>
        <p class="mt-3 max-w-prose text-slate-400">
          Ten operators. Each has one ASCII spelling you can type on any keyboard and,
          for some, a glyph you never have to enter — paste it, or don't.
        </p>

        <div class="mt-8 overflow-x-auto">
          <table class="w-full border-collapse text-sm">
            <thead>
              <tr class="border-b border-slate-700 text-left text-slate-400">
                <th class="py-2 pr-4 font-medium">Concept</th>
                <th class="py-2 pr-4 font-medium">ASCII</th>
                <th class="py-2 pr-4 font-medium">Glyph</th>
                <th class="py-2 font-medium">Meaning</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="op in VOCABULARY"
                :key="op.concept"
                class="border-b border-slate-800/70"
              >
                <td class="whitespace-nowrap py-2.5 pr-4 text-slate-200">
                  {{ displayName(op.concept) }}
                </td>
                <td class="whitespace-nowrap py-2.5 pr-4 font-mono text-cant-accent">
                  {{ asciiUsage(op) }}
                </td>
                <td class="whitespace-nowrap py-2.5 pr-4 font-mono">
                  <span v-if="hasDistinctGlyph(op)" class="text-slate-300">{{
                    glyphUsage(op)
                  }}</span>
                  <span v-else class="text-slate-600">same</span>
                </td>
                <td
                  class="py-2.5 text-slate-400"
                  v-html="renderDescription(op.description)"
                ></td>
              </tr>
            </tbody>
          </table>
        </div>

        <p class="mt-6 max-w-prose text-sm text-slate-500">
          Two spellings do double duty, and position decides which you meant:
          <code class="font-mono text-slate-400">*</code> is scatter only when it is a
          whole stage, so <code class="font-mono text-slate-400">$ * 2</code> stays
          multiplication; <code class="font-mono text-slate-400">:name</code> is a
          modifier only right after a block, so
          <code class="font-mono text-slate-400">= :error</code> stays an atom.
        </p>
      </div>
    </section>

    <!-- Three views -->
    <section class="border-b border-cant-border bg-cant-panel/30">
      <div class="mx-auto max-w-6xl px-4 py-14">
        <h2 class="text-2xl font-semibold text-white">Nothing is hidden</h2>
        <p class="mt-3 max-w-prose text-slate-400">
          Cant compiles to Rite you can read, and there are three ways to look at a
          program before you trust it.
        </p>
        <div class="mt-8 grid gap-6 sm:grid-cols-3">
          <div
            v-for="v in views"
            :key="v.command"
            class="rounded-lg border border-cant-border bg-cant-panel/60 p-5"
          >
            <div class="font-mono text-sm text-cant-accent">{{ v.command }}</div>
            <p class="mt-2 text-sm leading-relaxed text-slate-400">{{ v.body }}</p>
          </div>
        </div>
        <p class="mt-6 max-w-prose text-sm text-slate-500">
          All three run in
          <RouterLink to="/studio" class="text-cant-accent hover:underline"
            >Studio</RouterLink
          >
          too — the same engine, compiled to WebAssembly, with nothing leaving the
          browser.
        </p>
        <p class="mt-3 max-w-prose text-sm text-slate-500">
          Effects work as they do in Rite:
          <code class="font-mono text-slate-400">!@fs.read</code> is a marked host call,
          and it needs the same grant whether it sits at the top level or inside an
          orbit. There is no second permission system.
        </p>
      </div>
    </section>

    <!-- Running it -->
    <section>
      <div class="mx-auto max-w-6xl px-4 py-14">
        <h2 class="text-2xl font-semibold text-white">Running it</h2>

        <div class="mt-6 max-w-2xl space-y-4">
          <CodeBlock :code="shell" lang="rite" />
          <p class="text-sm leading-relaxed text-slate-500">
            Quote the expression. Cant's operators —
            <code class="font-mono">&gt;</code>, <code class="font-mono">|</code>,
            <code class="font-mono">!</code>, <code class="font-mono">?</code>,
            <code class="font-mono">*</code> — are shell metacharacters, the same trade
            <code class="font-mono">awk</code>, <code class="font-mono">sed</code> and
            <code class="font-mono">jq</code> make. Files and standard input work too,
            and <code class="font-mono">cant build</code> compiles to a native binary.
          </p>
          <p class="text-sm leading-relaxed text-slate-500">
            <code class="font-mono text-slate-400">cant</code> ships in the Rite release
            archives, beside <code class="font-mono text-slate-400">rite</code>.
          </p>
        </div>

        <div class="mt-10 flex flex-wrap gap-3">
          <RouterLink
            to="/studio"
            class="rounded-md border border-cant-accent/40 bg-cant-accent/10 px-4 py-2 text-sm font-medium text-cant-accent hover:bg-cant-accent/20"
          >
            Try Studio
          </RouterLink>
          <RouterLink
            to="/docs/language"
            class="rounded-md border border-cant-border px-4 py-2 text-sm text-slate-300 hover:bg-slate-800/50"
          >
            Read the language
          </RouterLink>
          <RouterLink
            to="/docs/cli"
            class="rounded-md border border-cant-border px-4 py-2 text-sm text-slate-300 hover:bg-slate-800/50"
          >
            Command reference
          </RouterLink>
        </div>

        <p class="mt-10 max-w-prose text-sm text-slate-500">
          Cant is experimental. The operator vocabulary and the graph format can still
          change between versions.
        </p>
      </div>
    </section>
  </div>
</template>
