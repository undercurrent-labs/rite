# Embedding Rite in Rust

Rite is designed to sit inside larger Rust programs: game tools, ops CLIs, policy
engines, or product backends that need a scriptable edge.

This chapter is the API surface. For a worked example — a host with real rules, a
grant, and a budget — see the tutorial [Embedding Rite in a Rust
program](../tutorials/embedding-rite.md).

## Dependency

The `rite` crate is **not on crates.io**, and will not be: the `rite-runtime` name
there belongs to an unrelated project, so a version dependency would pull in
someone else's code. Depend on a checkout or on git.

```toml
[dependencies]
rite = { path = "../rite/crates/rite" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Tokio is not optional: the evaluator is async, because host calls are.

The crate re-exports the lower-level pieces — `rite::caps`, `rite::core`,
`rite::runtime`, `rite::sem`, `rite::syntax` — so a host that needs a custom
capability can reach them without adding more dependencies. Start with the engine.

## Minimal embed

```rust
use rite::RiteEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = RiteEngine::builder().build()?;
    let value = engine.run_source("demo.rite", "^ 6 * 7").await?;
    println!("{value}");
    Ok(())
}
```

`build()` returns `anyhow::Result`. `anyhow::Error` converts into a boxed
`std::error::Error`, so a host that does not use anyhow still gets `?`.

Pass a real **name** as the first argument to `run_source`: it is what diagnostics
and stack traces show.

## The builder

| Method | Effect |
|---|---|
| `allow(spec)` | Grant one capability, in the CLI's `--allow` spelling. `Result`, because a misspelled spec should fail where you wrote it |
| `allow_all()` | Everything. Tests and trusted sources only |
| `with_permissions(set)` | Replace the whole `PermissionSet` — build one with `rite::caps` |
| `with_budget(budget)` | Step and time limits, from `rite::runtime::ExecutionBudget` |
| `with_output(sink)` | Send guest `@console` output somewhere other than the host's streams |
| `build()` | The engine |

A builder with no calls is **default-secure**: console, clock and random; no
filesystem, network, environment, subprocess or database. The same posture as
`rite run` with no flags.

```rust
let engine = RiteEngine::builder()
    .allow("fs:read=./data")?
    .allow("net=api.example.com")?
    .build()?;
```

There is no `deny` on the builder. Start from nothing and add, rather than
starting from everything and subtracting — a `deny` list is a promise you have
thought of every case.

## Running

| Method | Takes | Gives |
|---|---|---|
| `run_source(name, src)` | source text | the script's value |
| `run_path(path)` | a file | the script's value |
| `check_source(name, src)` | source text | `Diagnostics` — no execution |
| `compile_ir(name, src)` | source text | `ProgramIr`, for caching a parse |
| `parse(name, src)` | source text | the AST, for tooling |

`check_source` is the same check `rite check` runs. Use it on anything a user
supplied: it separates "this file is wrong" from "this file did something wrong",
and the first should never reach the second.

The value that comes back is a `rite::runtime::Value` — a record stays a record.
The script's final `^` is the interface between the two languages.

## Output

Guest output goes to the host's own stdout and stderr by default, as under
`rite run`. To capture it instead:

```rust
let engine = RiteEngine::builder()
    .with_output(|_stream, text| eprint!("[guest] {text}"))
    .build()?;
```

The sink is called as the script writes, so a long-running guest streams rather
than holding its output until it finishes.

## Budgets

```rust
let budget = rite::runtime::ExecutionBudget::new()
    .with_max_steps(1_000_000)
    .with_timeout(std::time::Duration::from_secs(5));

let engine = RiteEngine::builder().with_budget(budget).build()?;
```

Exceeding either is an ordinary `Err` for the host. The guest cannot catch it.

## Error handling

```rust
match engine.run_source("rules.rite", src).await {
    Ok(v) => println!("ok: {v}"),
    Err(e) => eprintln!("rite error: {e}"),
}
```

`EvalError` distinguishes the cases a host usually wants to treat differently —
`Permission`, `Budget`, `Compile` — and `EvalError::exit_code()` gives the status
`rite run` would have exited with, if you are wrapping Rite in a CLI of your own.

## What is not there yet

**A host cannot call a function inside a guest script.** The engine runs a script
and hands back its value. To evaluate per item, run the script once per item — a
tree walk over an already-parsed program, not a process spawn — or have the script
return parameters the host applies.

**`with_default_builtins()` does nothing.** Builtins are always installed. It is
deprecated and kept only so existing hosts still compile.

## Isolation

1. Treat guest scripts as **untrusted** by default — the engine already does
2. Grant **path-scoped** filesystem and **host-scoped** network access
3. Leave `@process` alone unless you trust the script's author as much as your own
4. Set a **budget**; a runaway loop in a rules file should not take the service down
5. Do not share one engine across mutually distrusting tenants without thinking
   about what a script can leave behind

## When not to embed

- Simple one-off automation → `rite run` or `rite build` is enough
- Browser → use the WASM package or Studio, not a Tokio embed

## Next

[Browser & Studio](browser.md) — web playground and hosted limits.
