# Embedding Rite in a Rust program

**You will build** a Rust program whose pricing rules live in a Rite file — so the
rules can change without recompiling the program, and a rules file cannot read
anything you did not let it.

**You need** a Rust toolchain and this repository, because the `rite` crate is not
on crates.io (see [The dependency](#the-dependency)).

> **The Rust half of this page is not run in CI.** The rules script at the end is,
> against a fixture, like every other tutorial. The host program is compiled and
> run by hand — every output below came from a real `cargo run`, but nothing stops
> it drifting except someone doing that again.

## Why embed rather than shell out

`rite run` is a fine way to use Rite from a program: spawn it, read its output.
Embedding buys you three things that spawning does not.

You choose the permissions in code rather than on a command line, so the policy
lives with the program that depends on it. You get the script's **value** back as a
Rust value instead of parsing text. And you can check a script for errors *before*
running it, which matters when the script came from a user.

The example here is pricing rules: the kind of logic that changes weekly, is
written by someone who is not the person maintaining the service, and should not
require a deploy.

## The dependency

The `rite` crate is **not published to crates.io**, and not by oversight — the
`rite-runtime` name there belongs to an unrelated project, so a version dependency
would pull in someone else's code. Depend on a checkout or on git:

```toml
[dependencies]
rite = { path = "../rite/crates/rite" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

Tokio is not optional. The evaluator is async because host calls are — `@http`,
`@clock.sleep`, a socket read — so the host needs a runtime.

## The smallest host that works

```rust
use rite::RiteEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = RiteEngine::builder().build()?;
    let value = engine.run_source("demo.rite", "^ 6 * 7").await?;
    println!("host received: {value}");
    Ok(())
}
```

```text
host received: 42
```

`run_source` takes a **name** as well as the source. The name is what appears in
diagnostics and stack traces, so give it the file name or something equally
findable — `"<generated>"` will be exactly as unhelpful as it sounds when a rules
file starts failing at 3am.

`builder().build()` returns `anyhow::Result`, which is why the error type above is
a boxed `std::error::Error` — `anyhow::Error` converts into one, so a host that
does not use anyhow still gets `?`.

## The guest is not trusted

The engine you just built is **default-secure**: the same posture as `rite run`
with no flags. Console, clock and random are available; the filesystem, network,
environment, subprocesses and database are not.

That matters immediately, because our rules script wants to read the order:

```rust
let engine = RiteEngine::builder().build()?;
// rules.rite does: raw ← ! @fs.read("order.json")?
let priced = engine.run_source("rules.rite", &rules).await?;
```

```text
Error: Permission("fs:read permission denied for `order.json`")
```

Grant exactly what the rules need, scoped as narrowly as you can stand:

```rust
let engine = RiteEngine::builder()
    .allow("fs:read=.")?
    .build()?;
```

`allow` takes the same spec strings as the CLI's `--allow`, and returns a `Result`
because a misspelled capability should fail where you wrote it rather than
silently granting nothing. There is `allow_all()` too. Reach for it in tests and
regret it in production: a rules file is exactly the kind of input that arrives
from somewhere else.

## Checking before running

A rules file written by someone else can be wrong in two quite different ways: it
can fail to compile, or it can compile and then misbehave. Separate them.

```rust
let diags = engine.check_source("rules.rite", &rules);
if diags.has_errors() {
    eprintln!("rules.rite has {} problem(s)", diags.len());
    std::process::exit(1);
}
```

```text
rules.rite has 1 problem(s)
```

`check_source` parses and resolves without running anything, so it catches a typo'd
name or a missing `!` before a single side effect has happened. It is the same
check `rite check` runs.

## What the script prints, and where

A guest's `@console.println` goes to the host's own stdout by default, as it would
under `rite run`. When you would rather have it in a log, or a UI pane, or a test
assertion, hand the engine a sink:

```rust
let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
let sink = seen.clone();
let engine = RiteEngine::builder()
    .with_output(move |_stream, text| sink.lock().unwrap().push(text.to_string()))
    .build()?;

engine.run_source("chatty.rite", "! @console.println(\"from the guest\")").await?;
println!("captured: {:?}", seen.lock().unwrap());
```

```text
captured: ["from the guest\n"]
```

The sink is called as the script writes, not collected at the end, so a
long-running guest streams rather than holding everything until it finishes.

## Stopping a runaway script

A rules file with an accidental long loop should not take the service with it.

```rust
let budget = rite::runtime::ExecutionBudget::new().with_max_steps(1000);
let engine = RiteEngine::builder().with_budget(budget).build()?;
```

```text
stopped: execution step budget exceeded
```

There is `with_timeout` for wall-clock as well. Both produce an ordinary `Err` the
host can handle — the guest cannot catch either one, which is the point.

## The host, in full

```rust
use rite::RiteEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Default-secure: the guest gets console, clock and random. Reading the order
    // file is a grant this host chooses to make, scoped to this directory.
    let engine = RiteEngine::builder().allow("fs:read=.")?.build()?;

    let rules = std::fs::read_to_string("rules.rite")?;

    // A syntax or name error in the rules is worth catching before anything runs,
    // and separately from a failure while running.
    let diags = engine.check_source("rules.rite", &rules);
    if diags.has_errors() {
        eprintln!("rules.rite has {} problem(s)", diags.len());
        std::process::exit(1);
    }

    let priced = engine.run_source("rules.rite", &rules).await?;
    println!("host received: {priced}");
    Ok(())
}
```

```text
order A-1042 is #silver
host received: ⟨subtotal: 300, discount: 15, shipping: 25, total: 310⟩
```

The first line is the guest printing; the second is the host printing the record
the script returned. A whole record comes back, not a string to parse — the
script's final `^` is the interface between the two languages.

## What the host cannot do yet

**There is no way to call a function inside the guest from Rust.** The engine runs
a script and gives you its value. That is why the rules below read their input
from a file the host controls rather than taking an argument: passing data in
means putting it somewhere the guest can reach, and a path grant is the honest
version of that.

If you need per-item evaluation today, run the script once per item — it is a tree
walk over an already-parsed program, not a process spawn — or have the script
return a record of parameters and apply them in Rust.

## The whole script

The order the rules price. Save this as `order.json`:

```json
{
  "id": "A-1042",
  "customer": "orbital-supply",
  "express": true,
  "items": [
    { "sku": "bolt-m6", "qty": 400, "unit": 0.30 },
    { "sku": "flange-90", "qty": 12, "unit": 8.00 },
    { "sku": "gasket-xl", "qty": 4, "unit": 21.00 }
  ]
}
```

And the rules beside it, as `rules.rite`:

```rite native_only
// Pricing rules. The host program does not change when these do.

◆ line_total(item) ⟦
  ^ item.qty * item.unit
⟧

◆ tier(subtotal) ⟦
  ^ ? subtotal >= 500 ⟦ #gold ⟧ : ⟦ ? subtotal >= 100 ⟦ #silver ⟧ : ⟦ #bronze ⟧ ⟧
⟧

◆ discount_rate(t) ⟦
  ^ ~ t ⟦
    #gold → 0.10
    #silver → 0.05
    _ → 0.0
  ⟧
⟧

◆! main() ⟦
  raw ← ! @fs.read("order.json")?
  order ← @json.decode(raw)?

  subtotal ← order.items → map({ |i| line_total(i) }) → sum
  t ← tier(subtotal)
  discount ← subtotal * discount_rate(t)
  shipping ← ? order.express ⟦ 25.0 ⟧ : ⟦ 0.0 ⟧
  total ← subtotal - discount + shipping

  ! @console.println("order " + order.id + " is " + str(t))
  ^ ⟨subtotal: subtotal, discount: discount, shipping: shipping, total: total⟩
⟧
```

```bash
rite run rules.rite --allow fs:read=.
```

```text
order A-1042 is #silver
⟨subtotal: 300, discount: 15, shipping: 25, total: 310⟩
```

That is the same script the host runs, and it runs standalone under the CLI with
the same grant the host makes in code. That is worth keeping true: rules you can
run by hand are rules you can debug by hand.

## Next

[Embedding Rite in Rust](../book/embedding.md) for the rest of the API surface —
`run_path`, `compile_ir`, `parse` — and [Effects and
capabilities](../book/effects.md) for what each grant actually opens up.
