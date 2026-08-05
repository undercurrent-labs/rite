/**
 * Where the "Run" affordances go, and what they say.
 *
 * Pure on purpose: no `vscode` import, so the placement rules can be tested
 * without an editor host. `extension.ts` turns these into `vscode.CodeLens`
 * objects and nothing else decides placement.
 *
 * Rite's lenses come from `rite-lsp`, which resolves the program properly.
 * These are Cant's, which has no language server — so the rules here are
 * deliberately conservative: a Cant file is *one flow*, so there is exactly one
 * place a lens belongs, and finding it is a matter of skipping what comes
 * before rather than parsing what follows.
 */

export type Lens = {
  /** Zero-based line the lens sits above. */
  line: number;
  title: string;
  command: string;
  tooltip?: string;
};

/**
 * The first line of the flow: the first line that is not blank, not a comment,
 * and not a `use` import.
 *
 * Returns `null` for a file with no program in it — all comments, or empty —
 * where a Run lens would offer to run nothing.
 *
 * Block comments are tracked across lines. A `/*` inside a string would fool
 * this, which is the price of not running the lexer; the cost of being wrong
 * is a lens one line off, not a wrong program.
 */
export function firstFlowLine(text: string): number | null {
  const lines = text.split(/\r?\n/);
  let inBlockComment = false;

  for (let i = 0; i < lines.length; i++) {
    let line = lines[i];

    if (inBlockComment) {
      const end = line.indexOf("*/");
      if (end === -1) continue;
      line = line.slice(end + 2);
      inBlockComment = false;
    }

    // Strip block comments that open and close on this line, then a trailing
    // opener that carries to the next.
    for (;;) {
      const open = line.indexOf("/*");
      if (open === -1) break;
      const close = line.indexOf("*/", open + 2);
      if (close === -1) {
        line = line.slice(0, open);
        inBlockComment = true;
        break;
      }
      line = line.slice(0, open) + line.slice(close + 2);
    }

    const trimmed = line.replace(/\/\/.*$/, "").trim();
    if (trimmed === "") continue;
    if (/^use\s+[A-Za-z_][A-Za-z0-9_.]*\s*$/.test(trimmed)) continue;
    return i;
  }
  return null;
}

/**
 * The lenses for a Cant document.
 *
 * One row above the flow. `Run` first because it is what the affordance is for;
 * the rest are the three ways of looking at a program before trusting it, plus
 * the sigil.
 */
export function cantLenses(text: string): Lens[] {
  const line = firstFlowLine(text);
  if (line === null) return [];
  return [
    { line, title: "▶ Run", command: "cant.runFile", tooltip: "cant run this file" },
    { line, title: "Check", command: "cant.checkFile", tooltip: "cant check — parse, graph, and Rite's own resolver" },
    { line, title: "Explain", command: "cant.explainFile", tooltip: "What this program does, in prose, and what it will touch" },
    { line, title: "Rite", command: "cant.expandFile", tooltip: "The generated Rite this compiles to" },
    { line, title: "Sigil", command: "cant.showSigil", tooltip: "Render the program's topology as a sigil" },
  ];
}

/**
 * Whether a file looks like it needs permissions it will not be granted.
 *
 * A lens is one click, and `--allow-all` by default would mean reading an
 * unfamiliar program could give it the filesystem. The extension runs lenses
 * ungranted unless `rite.codeLens.allowAll` is set, so this exists to warn
 * rather than to widen: if the source names a capability, say so in the tooltip
 * before the run fails with exit 5.
 *
 * Capability-shaped text inside a string or comment produces a false positive,
 * which costs a tooltip and nothing else.
 */
export function capabilitiesNamed(text: string): string[] {
  const found = new Set<string>();
  const re = /@([a-z_][a-z0-9_]*)\b/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) found.add(m[1]);
  return [...found].sort();
}
