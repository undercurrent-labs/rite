# Contributing to Rite

## Specs

| File | Role |
|------|------|
| `IMPLEMENTATION.md` | Decisions, deviations, status (public) |
| `docs/book/` | Guided user documentation |

Internal language/tooling design notes (if present locally) are **not** published in this repository.

## Development

```bash
cargo build -p rite-cli -p rite-lsp
cargo test --workspace
cargo run -p rite-cli -- run examples/01-values/main.rite --allow-all
```

### Layout

- `crates/rite-syntax` — lexer, parser, AST  
- `crates/rite-sem` — resolve, modules, IR, desugar  
- `crates/rite-runtime` — values, evaluator  
- `crates/rite-caps` — host capabilities  
- `crates/rite-fmt` — format + dialect convert  
- `crates/rite-analysis` — snapshots for LSP/Studio  
- `crates/rite-lsp` — language server  
- `crates/rite-wasm` — browser-facing API  
- `editors/vscode` — VS Code extension  
- `apps/rite-studio` — Vue Studio SPA  

## Adding syntax

1. Update `grammar/rite.ebnf` and `grammar/aliases.json` if needed.  
2. Extend lexer tokens and parser productions.  
3. Lower in `rite-sem` desugar/IR.  
4. Evaluate in `rite-runtime`.  
5. Add conformance fixture under `conformance/` and a parser test.  
6. Document in `docs/book/` and regenerate agent bundle (`rite docs agent`).

## Adding diagnostics

1. Add a stable code in `rite-core/src/error_codes.rs`.  
2. Emit via `simple_error` / `Diagnostic` with spans.  
3. Add `docs/diagnostics/E0xx.md`.  
4. Ensure LSP publishes the code (via analysis snapshot).

## Adding capabilities

1. Implement in `rite-caps` with `NativeFunctionDescriptor` metadata.  
2. Register in `HostCapabilities`.  
3. Document effectfulness and permissions.  
4. Unit/permission tests; browser safety notes in `rite-wasm` if native-only.

## Extending the LSP

Edit `crates/rite-lsp`. Prefer putting analysis logic in `rite-analysis` so Studio/WASM share it.

## Executable docs

- Fenced `rite` examples under `docs/` should stay parseable.  
- Prefer running via `rite run` / conformance rather than untested snippets.  
- Agent skill: `rite docs agent --output skills/rite` (users install with `rite skill install`).  
- Package skill for site/release: `bash scripts/package-skill.sh dist/skill`.  
- Package VS Code VSIX: `bash scripts/package-vsix.sh dist/vscode/rite.vsix`.  
- **Before push / release:** `bash scripts/check-packaging.sh` or  
  `cargo test -p rite-cli --test packaging` (skill always; VSIX if node/npm present).

## Studio examples

Add entries in `apps/rite-studio/src/examples.ts` and mirror under `examples/0N-*/`.

## VS Code grammar

`editors/vscode/syntaxes/rite.tmLanguage.json` — keep glyph and ASCII keywords in sync with `grammar/aliases.json`.

## Release checklist

```bash
cargo test --workspace
cargo build -p rite-cli -p rite-lsp --release
rite docs build && rite docs agent
# optional: cd apps/rite-studio && pnpm build
# optional: cd editors/vscode && npm run compile && npx vsce package
```

### Publishing a release

```bash
# on main, green CI
git tag v0.5.0
git push origin v0.5.0
# → GitHub Actions "Release" builds assets + publishes the Release
```

Or **Actions → Release → Run workflow** and enter the tag.

Local package for the current machine only:

```bash
bash scripts/package-release.sh
# → dist/release/rite-$TARGET.tar.gz + SHA256SUMS
```

The site serves the installer from `scripts/install.sh`, copied into
`apps/rite-web/public/` by `pnpm site:build`.
