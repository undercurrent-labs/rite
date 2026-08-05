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
16. [Network: HTTP services](http.md) — `@http.listen`, routes, middleware, client calls
17. [Network: sockets](sockets.md) — `@udp` datagrams, `@tcp` streams, bytes on the wire
18. [Model Context Protocol](mcp.md) — `@mcp.serve` and `@mcp.connect`: tools, resources, prompts
19. [Environment](environment.md) — `@env`, `@clock`, `@random`, `@store`
20. [Processes](processes.md) — `@process`, running commands, script arguments
21. [Modules](modules.md) — `use`, `pub`, relative paths, aliases
22. [Compiling to Rust](compiling.md) — `rite build`, IR, parity
23. [Text RPG](rpg.md) — `@game` tutorial
24. [Embedding](embedding.md) — `RiteEngine` from Rust
25. [Browser & Studio](browser.md) — hosted site, WASM limits
26. [Agents & the skill bundle](agents.md) — skill install, self-update, VS Code extension
27. [Testing](testing.md) — `◆ test`, `expect`, `rite test`
28. [Contributing tests](contributing-tests.md) — suite map, HTTP I/O capture, PR checklist

## Where each capability is covered

| Capability | Chapter | Default |
|---|---|---|
| `@console` | [First script](first-script.md) | allowed |
| `@fs` | [Files, JSON, and CSV](files-json.md) | denied |
| `@json` · `@csv` | [Files, JSON, and CSV](files-json.md) | — |
| `@crypto` | [Hashing and encoding](crypto.md) | — (`random_bytes` needs `random`) |
| `@db` | [Databases](db.md) | denied |
| `@http` | [Network: HTTP services](http.md) | denied |
| `@udp` · `@tcp` | [Network: sockets](sockets.md) | denied |
| `@mcp` | [Model Context Protocol](mcp.md) | serving on stdio allowed; HTTP bind and both `connect` forms denied |
| `@env` | [Environment](environment.md) | denied |
| `@process` | [Processes](processes.md) | denied |
| `@clock` · `@random` | [Environment](environment.md) | allowed |
| `@store` | [Environment](environment.md) | no permission |
| `@game` | [Text RPG](rpg.md) | — |

Every function of every one of them, with its arity and the permission it needs, is
in the generated [capability reference](reference/capabilities.md).

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
Hosted book + Studio: [rite.foo](https://rite.foo)
