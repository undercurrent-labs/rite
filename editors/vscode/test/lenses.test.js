// Placement rules for the Cant lenses. Pure logic, so no editor host needed.
const { test } = require("node:test");
const assert = require("node:assert");
const { firstFlowLine, cantLenses, capabilitiesNamed } = require("../out/lenses.js");

test("the lens sits on the flow, not on the preamble", () => {
  assert.equal(firstFlowLine('[1] -> * -> []'), 0);
  assert.equal(firstFlowLine('// a comment\n[1] -> * -> []'), 1);
  assert.equal(firstFlowLine('use helpers\n[1] -> * -> []'), 1);
  assert.equal(firstFlowLine('// why\nuse a\nuse b\n\n[1] -> []'), 4);
});

test("block comments are skipped, including multi-line ones", () => {
  assert.equal(firstFlowLine('/* one line */\n[1] -> []'), 1);
  assert.equal(firstFlowLine('/* two\n   lines */\n[1] -> []'), 2);
  assert.equal(firstFlowLine('/* a */ [1] -> []'), 0, "code after a closed comment is the flow");
});

test("a file with no program gets no lens", () => {
  assert.equal(firstFlowLine(''), null);
  assert.equal(firstFlowLine('// just a comment'), null);
  assert.equal(firstFlowLine('use only\n'), null);
  assert.equal(firstFlowLine('/* unterminated'), null);
  assert.deepEqual(cantLenses('// nothing here'), []);
});

test("the lens row leads with Run", () => {
  const ls = cantLenses('[1] -> * -> []');
  assert.equal(ls[0].command, "cant.runFile");
  assert.ok(ls.every((l) => l.line === 0));
  assert.deepEqual(ls.map((l) => l.command), [
    "cant.runFile", "cant.checkFile", "cant.explainFile", "cant.expandFile", "cant.showSigil",
  ]);
});

test("capabilities are named so an ungranted run is not a surprise", () => {
  assert.deepEqual(capabilitiesNamed('"x" -> !@fs.read? -> @json.decode?'), ["fs", "json"]);
  assert.deepEqual(capabilitiesNamed('[1] -> * -> []'), []);
  assert.deepEqual(capabilitiesNamed('!@fs.read -> !@fs.write'), ["fs"], "deduplicated");
});
