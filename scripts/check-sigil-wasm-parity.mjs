#!/usr/bin/env node
// The wasm half of the browser parity gate (AR4/Q2).
//
// Loads the *built* wasm32 bundle — the exact bytes the site ships — executes
// it in Node, renders the ceremony example with the browser's canonical
// options, and compares the SVG and fingerprint byte-for-byte against the
// fixtures the native test pinned
// (`crates/cant-sigil-wasm/tests/browser_fixture.rs`). Together the pair
// proves what `parity.rs` alone cannot: that the wasm32 *build*, actually run,
// draws the same artifact the native renderer does.
//
// Runs from `scripts/build-sigil-site.sh`, after the wasm build, so CI and
// every release exercise it. Requires no browser: the glue exports `initSync`.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkg = path.join(root, "apps/sigil-web/public/wasm");

const glueUrl = pathToFileURL(path.join(pkg, "cant_sigil_wasm.js"));
const glue = await import(glueUrl.href);
glue.initSync({ module: fs.readFileSync(path.join(pkg, "cant_sigil_wasm_bg.wasm")) });

const source = fs.readFileSync(path.join(root, "examples/sigil/ceremony.cant"), "utf8");
// The browser's defaults, canonically oriented — must mirror browser_fixture.rs.
const options = {
  theme: "neon-ritual",
  mode: "veiled",
  metadata: "safe",
  ornament: "ritual",
  tracery: "flowing",
  seed: "canonical",
  background: "theme",
  canonical: true,
  simplify: false,
};

const result = JSON.parse(glue.renderCant("ceremony.cant", source, JSON.stringify(options)));
if (!result.ok) {
  console.error("wasm render failed:", JSON.stringify(result.diagnostics, null, 2));
  process.exit(1);
}

let failed = false;
function compare(name, actual, fixturePath) {
  const expected = fs.readFileSync(fixturePath, "utf8");
  if (actual === expected) {
    console.log(`wasm parity: ${name} matches ${path.relative(root, fixturePath)}`);
    return;
  }
  failed = true;
  const a = expected.split("\n");
  const b = actual.split("\n");
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if (a[i] !== b[i]) {
      console.error(`wasm parity: ${name} differs at line ${i + 1}:`);
      console.error(`  native: ${a[i] ?? "<absent>"}`);
      console.error(`  wasm32: ${b[i] ?? "<absent>"}`);
      break;
    }
  }
}

compare("svg", result.svg, path.join(root, "fixtures/sigil/browser/ceremony.svg"));
compare(
  "fingerprint",
  result.fingerprint,
  path.join(root, "fixtures/sigil/browser/ceremony.fingerprint")
);

if (failed) {
  console.error(
    "the executed wasm32 build disagrees with the native renderer — " +
      "if the native side changed intentionally, re-bless with " +
      "SIGIL_BLESS=1 cargo test -p cant-sigil-wasm --test browser_fixture " +
      "and rebuild the wasm"
  );
  process.exit(1);
}
