/**
 * A small syntax highlighter for the languages the book actually uses.
 *
 * Deliberately not Shiki: that means shipping Oniguruma's WASM to every reader
 * to colour a handful of code blocks, which costs more than the entire site
 * bundle. Rite's grammar is small and regular enough to tokenise directly, and
 * the tables come from grammar/ at build time (see vite.config.ts) so keywords
 * and host functions cannot drift from the language.
 *
 * Token kinds mirror the scopes in editors/vscode/syntaxes/rite.tmLanguage.json
 * so the site and the editor classify the same things the same way.
 */

export type TokenKind =
  | "comment"
  | "string"
  | "number"
  | "atom"
  | "capability"
  | "capability-fn"
  | "keyword"
  | "sigil"
  | "operator"
  | "http"
  | "punctuation"
  | "plain";

export type Token = { kind: TokenKind; text: string };

const GRAMMAR = __RITE_GRAMMAR__;
const KEYWORDS = new Set(GRAMMAR.keywords);
const SOFT_KEYWORDS = new Set(GRAMMAR.softKeywords);
const GLYPHS = new Set(GRAMMAR.glyphs);
const CAPABILITIES = new Set(GRAMMAR.capabilities);
const CAPABILITY_FNS = new Set(GRAMMAR.capabilityFns);

const HTTP_METHODS = new Set(["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]);

/** Multi-character operators, longest first so `<=` never matches as `<`. */
const OPERATORS = [
  "not in",
  "...",
  "<-",
  "<~",
  "->",
  ":=",
  "??",
  "!=",
  "<=",
  ">=",
  "==",
  "[[",
  "]]",
  "<<",
  ">>",
  "..",
  "+",
  "-",
  "*",
  "/",
  "%",
  "=",
  "<",
  ">",
];

const IDENT_START = /[A-Za-z_]/;
const IDENT_PART = /[A-Za-z0-9_]/;

function isDigit(c: string): boolean {
  return c >= "0" && c <= "9";
}

/**
 * Rite, both dialects at once. A document is glyph or ASCII, but a highlighter
 * that only understands one would mangle the other, and the book shows both —
 * often the same program twice, side by side.
 */
function tokenizeRite(src: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  const push = (kind: TokenKind, text: string) => {
    if (!text) return;
    const last = out[out.length - 1];
    if (last && last.kind === kind) last.text += text;
    else out.push({ kind, text });
  };

  while (i < src.length) {
    const c = src[i];

    // Comments — `///` doc, `//` line, `/* */` block.
    //
    // Not after a colon: Studio highlights run output with this tokeniser, and a
    // printed `http://…` would otherwise comment out the rest of the line. In
    // real source a URL only appears inside a string, which is consumed above.
    if (c === "/" && src[i + 1] === "/" && src[i - 1] !== ":") {
      const end = src.indexOf("\n", i);
      const stop = end === -1 ? src.length : end;
      push("comment", src.slice(i, stop));
      i = stop;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      const stop = end === -1 ? src.length : end + 2;
      push("comment", src.slice(i, stop));
      i = stop;
      continue;
    }

    // Strings — `r"…"` is raw (no escapes, and no interpolation)
    if (c === '"' || (c === "r" && src[i + 1] === '"')) {
      const raw = c === "r";
      let j = raw ? i + 2 : i + 1;
      while (j < src.length) {
        if (!raw && src[j] === "\\") {
          j += 2;
          continue;
        }
        if (src[j] === '"') {
          j += 1;
          break;
        }
        j += 1;
      }
      push("string", src.slice(i, j));
      i = j;
      continue;
    }

    // Numbers, including 0x / 0b and exponents
    if (isDigit(c)) {
      let j = i;
      if (c === "0" && (src[i + 1] === "x" || src[i + 1] === "b")) {
        j = i + 2;
        while (j < src.length && /[0-9a-fA-F_]/.test(src[j])) j += 1;
      } else {
        while (j < src.length && /[0-9_]/.test(src[j])) j += 1;
        if (src[j] === "." && isDigit(src[j + 1])) {
          j += 1;
          while (j < src.length && /[0-9_]/.test(src[j])) j += 1;
        }
        if (src[j] === "e" || src[j] === "E") {
          let k = j + 1;
          if (src[k] === "+" || src[k] === "-") k += 1;
          if (isDigit(src[k])) {
            j = k;
            while (j < src.length && /[0-9_]/.test(src[j])) j += 1;
          }
        }
      }
      push("number", src.slice(i, j));
      i = j;
      continue;
    }

    // Atoms — `#name` (glyph) and `:name` (ASCII). `:=` is an operator, not an atom.
    if ((c === "#" || (c === ":" && src[i + 1] !== "=")) && IDENT_START.test(src[i + 1] ?? "")) {
      let j = i + 1;
      while (j < src.length && (IDENT_PART.test(src[j]) || src[j] === ".")) j += 1;
      push("atom", src.slice(i, j));
      i = j;
      continue;
    }

    // Host capabilities — `@fs.read` and its ASCII twin `host.fs.read`
    const hostAscii = src.startsWith("host.", i);
    if (c === "@" || hostAscii) {
      const prefixLen = hostAscii ? "host.".length : 1;
      let j = i + prefixLen;
      let nameStart = j;
      while (j < src.length && IDENT_PART.test(src[j])) j += 1;
      const capName = src.slice(nameStart, j);
      if (CAPABILITIES.has(capName)) {
        push("capability", src.slice(i, j));
        // The function after the dot, when it is one we know about
        if (src[j] === ".") {
          let k = j + 1;
          while (k < src.length && IDENT_PART.test(src[k])) k += 1;
          const fnName = src.slice(j + 1, k);
          if (CAPABILITY_FNS.has(fnName)) {
            push("punctuation", ".");
            push("capability-fn", fnName);
            i = k;
            continue;
          }
        }
        i = j;
        continue;
      }
      // `@` on something unknown still reads as the host sigil
      if (!hostAscii) {
        push("sigil", "@");
        i += 1;
        continue;
      }
    }

    // Identifiers and words
    if (IDENT_START.test(c)) {
      let j = i;
      while (j < src.length && IDENT_PART.test(src[j])) j += 1;
      const word = src.slice(i, j);
      // `not in` is one operator when the words are adjacent
      if (word === "not" && /^\s+in\b/.test(src.slice(j))) {
        const m = src.slice(j).match(/^\s+in\b/)!;
        push("operator", word + m[0]);
        i = j + m[0].length;
        continue;
      }
      if (HTTP_METHODS.has(word)) push("http", word);
      else if (KEYWORDS.has(word)) push("keyword", word);
      else if (SOFT_KEYWORDS.has(word)) push("keyword", word);
      else push("plain", word);
      i = j;
      continue;
    }

    // Single-character glyph sigils, from grammar/aliases.json
    if (GLYPHS.has(c)) {
      push("sigil", c);
      i += 1;
      continue;
    }

    // Multi-character operators before single ones
    const op = OPERATORS.find((o) => src.startsWith(o, i));
    if (op) {
      push("operator", op);
      i += op.length;
      continue;
    }

    if ("()[]{}⟦⟧⟨⟩,;.|".includes(c)) {
      push("punctuation", c);
      i += 1;
      continue;
    }

    push("plain", c);
    i += 1;
  }

  return out;
}

const SHELL_BUILTINS = new Set([
  "cd", "curl", "export", "echo", "cat", "mkdir", "tar", "cp", "mv", "rm", "ls", "set",
  "bash", "sh", "sudo", "git", "cargo", "pnpm", "npm", "npx", "rite", "rite-lsp", "python3",
  "grep", "less", "install", "test", "source", "printf", "wrangler", "code", "vsce",
]);

/** Shell: comments, strings, the leading command word, flags and variables. */
function tokenizeShell(src: string): Token[] {
  const out: Token[] = [];
  const push = (kind: TokenKind, text: string) => text && out.push({ kind, text });

  for (const line of src.split("\n")) {
    const hash = line.indexOf("#");
    // A `#` inside quotes is not a comment; good enough for shell snippets in prose
    const quoteBefore = hash > -1 && (line.slice(0, hash).match(/["']/g)?.length ?? 0) % 2 === 1;
    const commentAt = hash > -1 && !quoteBefore ? hash : -1;
    const code = commentAt > -1 ? line.slice(0, commentAt) : line;

    let first = true;
    for (const part of code.split(/(\s+|"[^"]*"|'[^']*')/).filter((p) => p !== "")) {
      if (/^\s+$/.test(part)) push("plain", part);
      else if (/^["']/.test(part)) push("string", part);
      else if (part.startsWith("-")) push("operator", part);
      else if (part.startsWith("$")) push("atom", part);
      else if (part === "|" || part === "&&" || part === ">" || part === ">>") push("sigil", part);
      else if (first && SHELL_BUILTINS.has(part)) push("keyword", part);
      else if (!first && /^[a-z-]+$/.test(part) && first) push("plain", part);
      else push("plain", part);
      if (!/^\s+$/.test(part)) first = false;
    }
    if (commentAt > -1) push("comment", line.slice(commentAt));
    push("plain", "\n");
  }
  if (out.length && out[out.length - 1].text === "\n") out.pop();
  return out;
}

const RUST_KEYWORDS = new Set([
  "use", "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "async", "await",
  "match", "if", "else", "for", "while", "loop", "return", "self", "Self", "crate", "mod",
  "const", "static", "type", "where", "move", "ref", "in", "as", "dyn", "true", "false",
]);

function tokenizeRust(src: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  const push = (kind: TokenKind, text: string) => text && out.push({ kind, text });

  while (i < src.length) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      const end = src.indexOf("\n", i);
      const stop = end === -1 ? src.length : end;
      push("comment", src.slice(i, stop));
      i = stop;
      continue;
    }
    if (c === '"') {
      let j = i + 1;
      while (j < src.length && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      push("string", src.slice(i, j + 1));
      i = j + 1;
      continue;
    }
    if (c === "#" && src[i + 1] === "[") {
      const end = src.indexOf("]", i);
      const stop = end === -1 ? src.length : end + 1;
      push("atom", src.slice(i, stop));
      i = stop;
      continue;
    }
    if (isDigit(c)) {
      let j = i;
      while (j < src.length && /[0-9_.]/.test(src[j])) j += 1;
      push("number", src.slice(i, j));
      i = j;
      continue;
    }
    if (IDENT_START.test(c)) {
      let j = i;
      while (j < src.length && IDENT_PART.test(src[j])) j += 1;
      const word = src.slice(i, j);
      if (RUST_KEYWORDS.has(word)) push("keyword", word);
      else if (/^[A-Z]/.test(word)) push("capability", word);
      else push("plain", word);
      i = j;
      continue;
    }
    if (c === "!" || c === "?" || c === "&") {
      push("sigil", c);
      i += 1;
      continue;
    }
    const op = OPERATORS.find((o) => src.startsWith(o, i));
    if (op) {
      push("operator", op);
      i += op.length;
      continue;
    }
    push("plain", c);
    i += 1;
  }
  return out;
}

function tokenizeJson(src: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  const push = (kind: TokenKind, text: string) => text && out.push({ kind, text });
  while (i < src.length) {
    const c = src[i];
    if (c === '"') {
      let j = i + 1;
      while (j < src.length && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      const text = src.slice(i, j + 1);
      // A string followed by `:` is a key
      const after = src.slice(j + 1).match(/^\s*:/);
      push(after ? "atom" : "string", text);
      i = j + 1;
      continue;
    }
    if (isDigit(c) || (c === "-" && isDigit(src[i + 1] ?? ""))) {
      let j = i + 1;
      while (j < src.length && /[0-9._eE+-]/.test(src[j])) j += 1;
      push("number", src.slice(i, j));
      i = j;
      continue;
    }
    if (IDENT_START.test(c)) {
      let j = i;
      while (j < src.length && IDENT_PART.test(src[j])) j += 1;
      push("keyword", src.slice(i, j));
      i = j;
      continue;
    }
    push("punctuation", c);
    i += 1;
  }
  return out;
}

function tokenizeToml(src: string): Token[] {
  const out: Token[] = [];
  for (const line of src.split("\n")) {
    if (/^\s*#/.test(line)) out.push({ kind: "comment", text: line });
    else if (/^\s*\[/.test(line)) out.push({ kind: "capability", text: line });
    else {
      const eq = line.indexOf("=");
      if (eq > -1) {
        out.push({ kind: "atom", text: line.slice(0, eq) });
        out.push({ kind: "operator", text: "=" });
        out.push(...tokenizeJson(line.slice(eq + 1)));
      } else out.push({ kind: "plain", text: line });
    }
    out.push({ kind: "plain", text: "\n" });
  }
  if (out.length && out[out.length - 1].text === "\n") out.pop();
  return out;
}

/** Console transcripts: highlight the prompt and any leading `#` comment only. */
function tokenizeText(src: string): Token[] {
  const out: Token[] = [];
  for (const line of src.split("\n")) {
    const prompt = line.match(/^(rite〉|\$ |> |# )/);
    if (prompt) {
      out.push({ kind: "sigil", text: prompt[0] });
      out.push({ kind: "plain", text: line.slice(prompt[0].length) });
    } else if (line.startsWith("→") || line.startsWith("#")) {
      out.push({ kind: "comment", text: line });
    } else {
      out.push({ kind: "plain", text: line });
    }
    out.push({ kind: "plain", text: "\n" });
  }
  if (out.length && out[out.length - 1].text === "\n") out.pop();
  return out;
}

const TOKENIZERS: Record<string, (src: string) => Token[]> = {
  rite: tokenizeRite,
  bash: tokenizeShell,
  sh: tokenizeShell,
  shell: tokenizeShell,
  console: tokenizeShell,
  rust: tokenizeRust,
  json: tokenizeJson,
  toml: tokenizeToml,
  text: tokenizeText,
};

/** Unknown languages fall back to a single plain token rather than guessing. */
export function tokenize(source: string, lang: string): Token[] {
  const fn = TOKENIZERS[lang.toLowerCase()];
  return fn ? fn(source) : [{ kind: "plain", text: source }];
}

export function isHighlighted(lang: string): boolean {
  return lang.toLowerCase() in TOKENIZERS;
}

/** True when the source uses glyph sigils, so Studio opens in the right dialect. */
export function detectDialect(source: string): "glyph" | "ascii" {
  for (const ch of source) if (GLYPHS.has(ch)) return "glyph";
  return "ascii";
}
