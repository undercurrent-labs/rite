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
11. [**Sugar pack**](sugar.md) — ranges, for/while, say, compose, op-assign, …
12. [Effects and capabilities](effects.md) — `!` / `do`, permissions
13. [Files, JSON, and CSV](files-json.md) — `@fs`, `@json`, `@csv`
14. [Databases](db.md) — `@db` (DuckDB), SQL, transactions
15. [HTTP services](http.md) — `@http.listen`, routes, middleware
16. [Modules](modules.md) — `use`, `pub`, relative paths, aliases
17. [Compiling to Rust](compiling.md) — `rite build`, IR, parity
18. [Text RPG](rpg.md) — `@game` tutorial
19. [Embedding](embedding.md) — `RiteEngine` from Rust
20. [Browser & Studio](browser.md) — hosted site, WASM limits
21. [Agents & skill](agents.md) — skill install, self-update, VS Code extension
22. [Testing (contributors)](testing.md) — suite map, HTTP I/O capture, PR checklist

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
| `⊏` | `use` | Import / middleware plug-in |
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
