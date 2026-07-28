# Rite guided book

Rite is a small scripting language for tools, pipelines, and embeds: dual **glyph** / **ASCII** syntax, explicit **effects**, **capability** permissions, and a Rust-backed interpreter (with IR compile for native binaries).

This book is a linear path. You can stop after any chapter and experiment in [Studio](/studio) (pure scripts + console) or the CLI (full host I/O).

## Chapters

1. [Installation](installation.md) — install binary, PATH, verify
2. [First script](first-script.md) — hello world, run, format dialects
3. [One-liners & REPL](one-liners.md) — daily scratch work, session model, recipes
4. [Values and atoms](values.md) — types, truthiness, records & lists
5. [Bindings](bindings.md) — immutable vs mutable, assignment
6. [Functions](functions.md) — definitions, return, closures
7. [Pipelines](pipelines.md) — `→` / `->`, keep/map/sum, `$`
8. [Collections](collections.md) — list & record ops in depth
9. [Pattern matching](matching.md) — `~` / `match`, destructure
10. [Results and errors](results.md) — `ok` / `err`, `?`, match
11. [Effects and capabilities](effects.md) — `!` / `do`, permissions
12. [Files and JSON](files-json.md) — `@fs`, `@json`
13. [HTTP services](http.md) — `@http.listen`, routes, middleware
14. [Modules](modules.md) — `use`, `pub`, cycles
15. [Compiling to Rust](compiling.md) — `rite build`, IR, parity
16. [Text RPG](rpg.md) — `@game` tutorial
17. [Embedding](embedding.md) — `RiteEngine` from Rust
18. [Browser & Studio](browser.md) — hosted site, WASM limits

## Glyph ↔ ASCII at a glance

| Glyph | ASCII | Role |
|-------|-------|------|
| `◆` | `def` | Function definition |
| `←` | `<-` | Immutable bind |
| `↢` | `<~` | Mutable bind |
| `→` | `->` | Pipeline |
| `^` | `return` | Early return |
| `?` | `if` | Conditional |
| `~` | `match` | Pattern match |
| `!` | `do` | Effectful statement |
| `@` | `host.` | Host capability |
| `#atom` | `:atom` | Atom / symbol |
| `⟦ ⟧` | `[[ ]]` | Block |
| `⟨ ⟩` | `<< >>` | Record |

```bash
rite fmt script.rite              # prefer glyph
rite fmt --ascii script.rite      # prefer ASCII
rite convert script.rite --to ascii --stdout
```

## API reference

Machine-generated capability and CLI docs:

```bash
rite docs build    # → docs/generated/
rite docs agent    # agent-oriented summary
```

## Repository

Source: [github.com/undercurrent-labs/rite](https://github.com/undercurrent-labs/rite)  
Hosted book + Studio: [rite.undrc.dev](https://rite.undrc.dev) (Undercurrent Labs LLC)
