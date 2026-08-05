/**
 * Inline errors for Cant.
 *
 * Rite gets these from `rite-lsp`. Cant has no language server, so this runs
 * `cant check --json-errors` and publishes what comes back. It is a poor
 * substitute for a server — no incremental parse, no cross-file view, and it
 * only runs on open and save — but the alternative was a `.cant` file where a
 * syntax error is invisible until you go looking for it, which is not editor
 * support in any useful sense.
 *
 * The conversion is the interesting part: Cant reports **byte** offsets into
 * the source, and VS Code positions are UTF-16 code units within a line. Cant
 * is a language people write `→ ⋇ ⌁ ⊣⟦⟧` in, so those are not the same number
 * and getting it wrong puts the squiggle in the wrong place on exactly the
 * programs most likely to have one.
 */

export type CantSpan = { start: number; end: number };
export type CantLabel = { primary: boolean; message: string; span: { file: number; span: CantSpan } };
export type CantDiagnostic = {
  code: string;
  severity: string;
  /** Cant calls the headline `title`. Confirmed against real `--json-errors`
   *  output: reading it as `message` gave an empty hover. */
  title: string;
  help?: string | null;
  notes?: string[];
  labels: CantLabel[];
};

export type Range = {
  start: { line: number; character: number };
  end: { line: number; character: number };
};

/**
 * Byte offset → line and UTF-16 character, the way an editor counts.
 *
 * Built once per document rather than per diagnostic: a file with twenty errors
 * would otherwise rescan the source twenty times.
 */
export class OffsetMap {
  /** Byte offset at which each line starts. */
  private lineStarts: number[] = [0];
  private text: string;

  constructor(text: string) {
    this.text = text;
    const bytes = Buffer.from(text, "utf8");
    for (let i = 0; i < bytes.length; i++) {
      if (bytes[i] === 0x0a) this.lineStarts.push(i + 1);
    }
  }

  positionAt(byteOffset: number): { line: number; character: number } {
    // The last line whose start is at or before the offset.
    let lo = 0;
    let hi = this.lineStarts.length - 1;
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (this.lineStarts[mid] <= byteOffset) lo = mid;
      else hi = mid - 1;
    }
    const lineStart = this.lineStarts[lo];
    // Decode the bytes from the line start to the offset, then measure that
    // prefix in UTF-16 units — which is what `character` counts.
    const all = Buffer.from(this.text, "utf8");
    const prefix = all.subarray(lineStart, Math.max(lineStart, byteOffset)).toString("utf8");
    return { line: lo, character: prefix.length };
  }

  rangeOf(span: CantSpan): Range {
    return { start: this.positionAt(span.start), end: this.positionAt(span.end) };
  }
}

/** The label an editor should underline: the primary one, or the first. */
export function primaryLabel(d: CantDiagnostic): CantLabel | undefined {
  return d.labels.find((l) => l.primary) ?? d.labels[0];
}

/**
 * The text shown on hover: the message, then the help, then any notes.
 *
 * Cant puts real guidance in `help` — "close it with `}`" — and dropping it
 * would leave the editor showing less than the terminal does.
 */
export function fullMessage(d: CantDiagnostic): string {
  const parts = [d.title];
  const label = primaryLabel(d);
  if (label?.message && label.message !== d.title) parts.push(label.message);
  if (d.help) parts.push(d.help);
  for (const n of d.notes ?? []) parts.push(n);
  return parts.join("\n\n");
}

/** `cant check --json-errors` output → editor-ready diagnostics. */
export function toDiagnostics(
  json: string,
  text: string,
): { range: Range; message: string; code: string; severity: "error" | "warning" }[] {
  let parsed: CantDiagnostic[];
  try {
    parsed = JSON.parse(json);
  } catch {
    // A non-JSON answer means the CLI failed before it could report, which is
    // not something to surface as a squiggle at line 1.
    return [];
  }
  if (!Array.isArray(parsed)) return [];

  const map = new OffsetMap(text);
  return parsed.map((d) => {
    const label = primaryLabel(d);
    const range = label
      ? map.rangeOf(label.span.span)
      : { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } };
    return {
      range,
      message: fullMessage(d),
      code: d.code,
      severity: d.severity === "warning" ? "warning" : "error",
    };
  });
}
