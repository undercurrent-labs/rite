# Embedding Rite in Rust

Rite is designed to sit inside larger Rust programs: game tools, ops CLIs, policy engines, or product backends that need a scriptable edge.

## Dependency

In your `Cargo.toml` (path or crates.io when published):

```toml
[dependencies]
rite = { path = "../rite/crates/rite" }  # adjust to workspace layout
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
anyhow = "1"
```

The workspace crate graph also exposes lower-level pieces (`rite-runtime`, `rite-caps`, …) if you need custom hosts. Start with the high-level engine.

## Minimal embed

```rust
use rite::RiteEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = RiteEngine::builder()
        .allow_all()
        .build()?;

    let value = engine
        .run_source("demo.rite", r#"1 + 2"#)
        .await?;

    println!("{value}");
    Ok(())
}
```

```bash
# from this repo's tiny example script
rite run examples/10-embedded-rust/main.rite --allow-all
```

## Permissions in hosts

Never use `allow_all` in production embeds unless the script source is fully trusted.

```rust
let engine = RiteEngine::builder()
    // grant only what guest scripts need
    // e.g. console + read-only data dir
    .build()?;
```

Exact builder methods follow the crate API in your tree (`allow`, `deny`, capability install). Mirror the CLI model: default secure, opt-in power.

## Running files vs strings

```rust
// string (tests, generated snippets)
engine.run_source("inline.rite", source).await?;

// file path helpers if exposed
// engine.run_file("scripts/job.rite").await?;
```

Pass a stable **name** as the first argument so stack traces and diagnostics show useful locations.

## Async

The evaluator is async (HTTP and timers need a runtime). Use Tokio (or compatible) in the host:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ...
    Ok(())
}
```

## Capturing output

Depending on API surface:

- return **value** of the script  
- read **stdout/stderr** buffers from the runtime context if the engine exposes them  
- inject a custom **console** capability that writes to your logs  

For product embeds, custom console/FS is often better than inheriting the process stdout.

## IR and parity

You can compile or load **Program IR** and evaluate with `run_ir` paths used by `rite build`. That keeps embedded execution aligned with CLI semantics. Prefer `run_source` until you need caching of compiled IR.

## Error handling

```rust
match engine.run_source("x.rite", src).await {
    Ok(v) => println!("ok: {v}"),
    Err(e) => eprintln!("rite error: {e}"),
}
```

Map Rite failures to your domain errors; include script name and, when available, stack traces from the runtime.

## Isolation tips

1. Treat guest scripts as **untrusted** by default  
2. Grant **path-scoped** FS and **host-scoped** net  
3. Disable **`@process`** unless you fully trust the script author  
4. Set **budgets** (steps/time) if the engine exposes them — stop runaway scripts  
5. Don’t share one mutable engine across mutually distrusting tenants without care  

## When not to embed

- Simple one-off automation → `rite run` / `rite build` is enough  
- Browser → use the **WASM** package / Studio, not Tokio embed  

## Next

[Browser & Studio](browser.md) — web playground and hosted limits.
