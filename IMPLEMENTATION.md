# Rite Implementation Status

Tracks implementation status for the Rite language and V1 tooling. Detailed design specs are kept private and are not part of this public tree.

## Thorough gap review (post-V1 pass)

### Critical language bugs fixed in this pass

| Issue | Fix |
|---|---|
| `~ status ⟦ arms ⟧` failed to parse | Match/if scrutinee disables trailing-block call sugar |
| Example ladder 04/05 broken | Updated examples; match + nested effect rules clarified |
| LSP diagnostics always at 0:0 | Analysis snapshot now emits `start_line` / UTF-16 columns |

### P0 items executed (this pass)

| Item | Status |
|---|---|
| WASM package + Studio hosted path | **Done** — pure eval in WASM; `scripts/build-wasm.sh` → studio + web `public/wasm` |
| Product site (home · docs · studio) | **Done** — `apps/rite-web`, `pnpm site:build` / `site:deploy`, routes `/` `/docs/*` `/studio` |
| Binary install (no clone) | **Done** — `scripts/install.sh` → `/install`, Release workflow tags `v*`, checksummed assets |
| Doctest runner | **Done** — `rite docs check` + `rite-doc` doctest module/tests |
| Format/convert source maps | **Done** — `LineSourceMap` + Studio/VS Code cursor restore |
| Multi-file analysis | **Done** — `WorkspaceIndex` (imports on disk, workspace symbols, references); LSP wired |
| CI release workflow | **Done** — `.github/workflows/ci.yml` (rust, wasm, studio, vsix, manifest) |

### Remaining gaps (after this pass)

1. **WASM host I/O** — browser run evaluates pure scripts + `@console`; FS/HTTP listen/process need native `rite studio`.
2. **Virtual HTTP request replay in hosted mode** — UI panel exists; full handler re-entry is native-local.
3. **Scope-aware multi-file rename** — references work; rename still textual.
4. **Semantic tokens** — still TextMate-primary.
5. **macOS/Windows CI matrix** — Ubuntu primary; others commented in workflow.

#### P1 — Quality / polish

6. **Semantic tokens** — LSP returns empty; TextMate is the highlighter.  
7. **Scope-aware rename** — currently whole-document identifier replace.  
8. **Aliased imports** — `use m as x` injects `x__f` names, not `x.f`.  
9. **Game free-form sugar** — still prefer `@game.register_*`.  
10. **CI matrix** — no GitHub Actions workflow for multi-OS, VSIX, Studio e2e, Cloudflare preview.  
11. **Incremental relexing / CST** — no rowan green tree; recovery is best-effort parse.  
12. **Performance benchmarks** — not automated against v1 targets (100–300 ms LSP).  
13. **VS Code VSIX in CI** — package.json ready; not produced by a release job.  
14. **Example 07/08 HTTP** — blocks until shutdown (correct for servers); e2e ladder skips them.

#### P2 — Explicitly V2

DAP, package registry, JetBrains, collaborative Studio, cloud compile, bytecode VM.

---

## Architecture (current)

| Layer | Crates / apps |
|---|---|
| Language | `rite-syntax`, `rite-sem`, `rite-runtime`, `rite-caps`, `rite-compiler` |
| Tooling | `rite-fmt`, `rite-analysis`, `rite-lsp`, `rite-doc`, `rite-wasm`, `rite-cli` |
| Editors | `editors/vscode` |
| Studio | `apps/rite-studio` (playground) + `rite studio` Axum API |
| Product site | `apps/rite-web` (home, docs book, studio shell) → Cloudflare |
| Agent | `skills/rite` |

### Key decisions

- Consolidated crates vs a fuller tooling split (boundaries documented).  
- Compiler embeds **ProgramIr** JSON, evaluates via `run_ir` (parity).  
- Dialects via parse→print (`grammar/aliases.json`).  
- Trailing blocks for `keep {…}`; **disabled** for match/if scrutinees.

---

## Acceptance snapshot

| Area | Status |
|---|---|
| MVP language + caps + HTTP + modules | Working |
| Conformance + differential | Working (+ match fixture) |
| Formatter / convert dialects | Working + property tests |
| LSP core features | Working (ranges improved) |
| VS Code baseline + full commands | Scaffold complete |
| WASM library API | Working (native host); pack optional |
| Studio local mode | Working |
| Docs book (17 chapters) | Filled |
| Diagnostic encyclopedia | Starter pages E020/E021/E024/E040 |
| Agent skill + machine manifests | Working |
| Example ladder e2e tests | Working (non-server scripts) |
| CONTRIBUTING | Added |

---

## Commands

```bash
cargo test --workspace
cargo build -p rite-cli -p rite-lsp --release

rite run examples/04-pattern-matching/main.rite --allow-all
rite convert file.rite --to ascii --stdout
rite studio --port 4041 --no-open
rite docs build && rite docs agent
rite describe language --json
```

---

## Testing summary

- Unit: lexer/parser (incl. match trailing-block), runtime, fmt dialects, analysis, wasm, permissions, HTTP handlers.  
- Conformance: arithmetic, pipeline, function, interpolation, match.  
- Differential: interpreter vs IR.  
- E2E ladder: `rite-test/tests/example_ladder.rs`.  

---

## Compatibility

V1 tooling does not change core language semantics. Public docs live under `docs/book/`; internal design notes stay out of this tree.
