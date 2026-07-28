# Rite

**Rite** is a Rust-backed scripting language with glyphic + ASCII syntax, explicit effects, capabilities, HTTP/game DSLs, interpreter + IR compilation, **LSP**, **VS Code extension**, **WASM API**, and **Rite Studio**.

Status and architecture notes: [`IMPLEMENTATION.md`](IMPLEMENTATION.md). Language docs: [docs/book](docs/book/README.md).

> A language can be visually strange while remaining semantically disciplined.

## Quick start

### Install CLI (no clone)

```bash
curl -fsSL https://rite.undrc.dev/install | sh
export PATH="$HOME/.local/bin:$PATH"
rite version
```

Requires a [GitHub Release](https://github.com/undercurrent-labs/rite/releases) with platform binaries. Pin with `RITE_VERSION=v0.1.0`. Details: [docs/book/installation.md](docs/book/installation.md).

### From source

```bash
cargo build -p rite-cli -p rite-lsp --release
export PATH="$PWD/target/release:$PATH"

rite run examples/01-values/main.rite --allow-all
rite fmt --dialect glyph examples/hello/hello.rite
rite convert examples/hello/hello.ascii.rite --to glyph --stdout
rite check examples/hello/hello.rite
rite build examples/hello/hello.rite --allow-all -o /tmp/rite-hello

rite-lsp                          # language server (stdio)
rite studio --port 4041           # local Studio API + UI
rite docs build && rite docs agent
rite describe language --json

cargo test --workspace
```

### Product site (home · docs · studio)

```bash
pnpm install
pnpm site:dev            # http://127.0.0.1:5173  →  /  /docs  /studio
pnpm site:build          # WASM + apps/rite-web/dist
pnpm site:deploy         # Cloudflare (apps/rite-web/wrangler.toml)
```

### Local Studio API (full capabilities)

```bash
# terminal 1 — native host API
rite studio --port 4041 --no-open
# terminal 2 — product site or studio-only SPA
pnpm site:dev
# or: pnpm --dir apps/rite-studio dev
```

### VS Code extension

```bash
cd editors/vscode && npm install && npm run compile
# Launch Extension Development Host (F5), or package with vsce
```

## Demo checklist (v1 acceptance)

| # | Capability | Command |
|---|------------|---------|
| 1 | Interpret | `rite run examples/hello/hello.rite --allow-all` |
| 2 | Format / validate | `rite fmt examples/hello/hello.rite` · `rite check examples/hello/hello.rite` |
| 3 | Compile native | `rite build examples/hello/hello.rite --allow-all -o /tmp/rite-hello` |
| 4 | Parity | Same stdout from `rite run` and the built binary |
| 5 | HTTP | `rite run examples/http-service/server.rite --allow-all` |
| 6 | RPG | `rite run examples/text-rpg/game.rite --allow-all` |
| 7 | Permissions | `rite run tests/e2e/permission_denied.rite` (denied without `--allow`) |
| 8 | Docs | `rite doc` → `docs/generated/{reference.md,html/index.html,index.json}` |
| 9 | Test suite | `cargo test --workspace` |
| 10 | Local build | documented above |

## Language taste

```rite
◆ square(value) ⟦
  ^ value * value
⟧

name ← "Aura"
nums ← [1, 2, 3, 4, 5]
! @console.println("hi " + name + " sum=" + str(nums → sum))
```

ASCII equivalent uses `def`, `<-`, `->`, `return`, `[[ ]]`, `host.console`, etc. Format either way with:

```bash
rite fmt script.rite          # glyphic
rite fmt --ascii script.rite  # ASCII
```

## Permissions

Default: console, clock, and random allowed; filesystem, network, env, and process denied.

```bash
rite run script.rite \
  --allow fs:read=./data \
  --allow fs:write=./output \
  --allow net=api.example.com \
  --allow env=APP_MODE

rite run --allow-all script.rite
```

Effectful host calls must be marked with `!` (ASCII: `do`).

## Modules

```rite
// math.rite
pub ◆ square(value) ⟦ ^ value * value ⟧

// main.rite
use math
! @console.println(str(square(12)))
```

Circular imports produce `E024` with the import chain. Only `pub` declarations are exported.

## Conformance / differential

```bash
cargo test -p rite-test --test conformance_gate
# Fixtures live under conformance/**/case.rite with expected.* sidecars
```

Compiled builds embed `ProgramIr` (see `.rite/build/*/program.ir.json`) and evaluate via `run_ir` for interpreter parity.

## Repository layout

```text
crates/          # workspace crates (syntax, sem, runtime, caps, compiler, fmt, doc, cli, …)
examples/        # hello, data-pipeline, http-service, text-rpg, automation, modules
conformance/     # versioned language fixtures + differential gate
grammar/         # EBNF + sigil/keyword tables
docs/generated/  # output of `rite doc`
IMPLEMENTATION.md
```


## Embedding

```rust
use rite::RiteEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = RiteEngine::builder().allow_all().build()?;
    let value = engine.run_source("demo.rite", r#"1 + 2"#).await?;
    println!("{value}");
    Ok(())
}
```

## CLI

```text
rite run | build | check | fmt | repl | test | doc | explain | ast | ir | capabilities | version
```

Exit codes: 0 success, 1 runtime, 2 usage, 3 parse, 4 resolve, 5 permission, 6 compile, 7 test, 8 budget.

## Specification

See the [guided book](docs/book/README.md) and [`IMPLEMENTATION.md`](IMPLEMENTATION.md) for architecture decisions, deviations, and status.

## License

MIT
