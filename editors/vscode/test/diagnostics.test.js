// Byte offsets from `cant check --json-errors` → editor positions.
// Cant is written in `→ ⋇ ⌁ ⊣⟦⟧`, so bytes and UTF-16 units differ on exactly
// the programs most likely to carry an error.
const { test } = require("node:test");
const assert = require("node:assert");
const { OffsetMap, toDiagnostics, fullMessage } = require("../out/diagnostics.js");

test("ascii offsets map straight through", () => {
  const m = new OffsetMap("abc\ndef\n");
  assert.deepEqual(m.positionAt(0), { line: 0, character: 0 });
  assert.deepEqual(m.positionAt(2), { line: 0, character: 2 });
  assert.deepEqual(m.positionAt(4), { line: 1, character: 0 });
  assert.deepEqual(m.positionAt(6), { line: 1, character: 2 });
});

test("multi-byte glyphs count as characters, not bytes", () => {
  // "→" is 3 bytes, 1 UTF-16 unit.
  const src = "[1] → ⋇ → ⌁";
  const m = new OffsetMap(src);
  const byteOfSecondArrow = Buffer.from("[1] → ⋇ ", "utf8").length;
  const pos = m.positionAt(byteOfSecondArrow);
  assert.equal(pos.line, 0);
  assert.equal(pos.character, "[1] → ⋇ ".length, "character is UTF-16 units, not bytes");
});

test("a glyph on an earlier line does not shift later lines", () => {
  const m = new OffsetMap("→→→\nx\n");
  const byteOfX = Buffer.from("→→→\n", "utf8").length;
  assert.deepEqual(m.positionAt(byteOfX), { line: 1, character: 0 });
});

test("the primary label is what gets underlined", () => {
  const json = JSON.stringify([{
    code: "CANT-P003", severity: "error", title: "unclosed block", help: "close it with `}`",
    notes: [],
    labels: [
      { primary: false, message: "reached this", span: { file: 0, span: { start: 14, end: 14 } } },
      { primary: true,  message: "opened here", span: { file: 0, span: { start: 7, end: 9 } } },
    ],
  }]);
  const [d] = toDiagnostics(json, "[1] -> ~{ deps");
  assert.deepEqual(d.range, { start: { line: 0, character: 7 }, end: { line: 0, character: 9 } });
  assert.equal(d.code, "CANT-P003");
  assert.equal(d.severity, "error");
  assert.match(d.message, /unclosed block/);
  assert.match(d.message, /close it with/, "the help is kept — the terminal shows it");
});

test("the headline comes from `title`, which is what Cant emits", () => {
  // Reading this as `message` gave an empty hover — caught only by running the
  // real CLI, since a hand-written fixture agreed with the wrong field.
  const json = JSON.stringify([{
    code: "CANT-P003", severity: "error", title: "unclosed `~{`",
    help: "close it with `}`", notes: [], labels: [],
  }]);
  const [d] = toDiagnostics(json, "x");
  assert.match(d.message, /unclosed/);
  assert.ok(!/undefined/.test(d.message), "the headline must not be undefined");
});

test("malformed output produces no squiggles rather than one at line 1", () => {
  assert.deepEqual(toDiagnostics("not json at all", "x"), []);
  assert.deepEqual(toDiagnostics("", "x"), []);
  assert.deepEqual(toDiagnostics('{"not":"an array"}', "x"), []);
});

test("a diagnostic with no labels still lands somewhere valid", () => {
  const json = JSON.stringify([{ code: "CANT-R001", severity: "error", title: "boom", labels: [] }]);
  const [d] = toDiagnostics(json, "x");
  assert.deepEqual(d.range.start, { line: 0, character: 0 });
});
