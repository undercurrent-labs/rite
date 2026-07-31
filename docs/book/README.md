# Rite guided book

Rite is a small scripting language for tools, pipelines, and embeds: dual **glyph** / **ASCII** syntax, explicit **effects**, **capability** permissions, and a Rust-backed interpreter (with IR compile for native binaries).

This book is a linear path. You can stop after any chapter and experiment in [Studio](/studio) (pure scripts + console) or the CLI (full host I/O).

## Chapters

1. [Installation](installation.md) — install binary, PATH, verify
2. [First script](first-script.md) — hello world, run, format dialects
3. [One-liners & REPL](one-liners.md) — quick scripts, session model, recipes
4. [Values and atoms](values.md) — types, truthiness, records & lists
5. [Bindings](bindings.md) — immutable vs mutable, assignment
6. [Functions](functions.md) — definitions, return, closures
7. [Pipelines](pipelines.md) — `→` / `->`, keep/map/sum, `$`
8. [Collections](collections.md) — list & record ops in depth
9. [Pattern matching](matching.md) — `~` / `match`, destructure
10. [Results and errors](results.md) — `ok` / `err`, `?`, match
11. [Syntax sugar](sugar.md) — ranges, for/while, say, compose, op-assign, …
12. [Effects and capabilities](effects.md) — `!` / `do`, permissions
13. [Files, JSON, and CSV](files-json.md) — `@fs`, `@json`, `@csv`
14. [Hashing and encoding](crypto.md) — `@crypto`, digests, HMAC, base64/hex
15. [Databases](db.md) — `@db` (DuckDB), SQL, transactions
16. [HTTP services](http.md) — `@http.listen`, routes, middleware, `@udp` datagrams
17. [Modules](modules.md) — `use`, `pub`, relative paths, aliases
18. [Compiling to Rust](compiling.md) — `rite build`, IR, parity
19. [Text RPG](rpg.md) — `@game` tutorial
20. [Embedding](embedding.md) — `RiteEngine` from Rust
21. [Browser & Studio](browser.md) — hosted site, WASM limits
22. [Agents & the skill bundle](agents.md) — skill install, self-update, VS Code extension
23. [Testing](testing.md) — `◆ test`, `expect`, `rite test`
24. [Contributing tests](contributing-tests.md) — suite map, HTTP I/O capture, PR checklist

## Glyph ↔ ASCII at a glance

| Glyph | ASCII | Role |
|-------|-------|------|
| `◆` | `def` | Function definition |
| `◆!` | `def!` | Function that performs host effects |
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

Generated from the implementation itself, so it cannot describe a function or a
flag that is not there:

- [Capability reference](reference/capabilities.md) — every `@host` function, its
  arity, whether it is effectful, and the permission it needs
- [CLI reference](reference/cli.md) — every subcommand, argument and flag

Rebuild them, plus the agent bundle, with:

```bash
rite docs build    # → docs/generated/
rite docs agent    # agent-oriented summary
```

## Repository

Source: [github.com/undercurrent-labs/rite](https://github.com/undercurrent-labs/rite)  
Hosted book + Studio: [rite.undrc.dev](https://rite.undrc.dev)
