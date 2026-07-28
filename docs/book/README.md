# Rite guided book

Rite is a small scripting language for tools, pipelines, and embeds: dual **glyph** / **ASCII** syntax, explicit **effects**, **capability** permissions, and a Rust-backed interpreter (with IR compile for native binaries).

This book is a linear path. You can stop after any chapter and experiment in [Studio](/studio) (pure scripts + console) or the CLI (full host I/O).

## Chapters

1. [Installation](installation.md) — build CLI, PATH, verify
2. [First script](first-script.md) — hello world, run, format dialects
3. [Values and atoms](values.md) — types, truthiness, records & lists
4. [Bindings](bindings.md) — immutable vs mutable, assignment
5. [Functions](functions.md) — definitions, return, closures
6. [Pipelines](pipelines.md) — `→` / `->`, keep/map/sum, `$`
7. [Collections](collections.md) — list & record ops in depth
8. [Pattern matching](matching.md) — `~` / `match`, destructure
9. [Results and errors](results.md) — `ok` / `err`, `?`, match
10. [Effects and capabilities](effects.md) — `!` / `do`, permissions
11. [Files and JSON](files-json.md) — `@fs`, `@json`
12. [HTTP services](http.md) — `@http.listen`, routes, middleware
13. [Modules](modules.md) — `use`, `pub`, cycles
14. [Compiling to Rust](compiling.md) — `rite build`, IR, parity
15. [Text RPG](rpg.md) — `@game` tutorial
16. [Embedding](embedding.md) — `RiteEngine` from Rust
17. [Browser & Studio](browser.md) — hosted site, WASM limits

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
