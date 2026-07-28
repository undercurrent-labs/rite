# Compiling to Rust

`rite build` turns a script into a **native binary** that embeds the program’s intermediate representation (**Program IR**) and evaluates it with the same runtime path as the interpreter — the goal is **behavior parity**, not a separate ad-hoc backend.

## Quick build

```bash
rite build examples/hello/hello.rite --allow-all -o /tmp/rite-hello
/tmp/rite-hello
```

You should see the same stdout as:

```bash
rite run examples/hello/hello.rite --allow-all
```

## What gets produced

Typical artifacts under `.rite/build/` (paths may vary by version):

| Artifact | Role |
|----------|------|
| **Native binary** (`-o`) | Runnable program |
| **`program.ir.json`** | Embedded IR snapshot (inspectable) |
| Optional **Rust emit** | Human-readable glue when using emit flags |

Inspect IR:

```bash
# after a build, look for IR under .rite/build/
find .rite/build -name 'program.ir.json' 2>/dev/null | head
```

Or use CLI helpers when available:

```bash
rite ir examples/hello/hello.rite
rite ast examples/hello/hello.rite
```

## Permissions at build vs run

Compiled binaries still enforce the **capability model** for host calls. Build with the permissions the program needs:

```bash
rite build app.rite \
  --allow fs:read=./data \
  --allow fs:write=./out \
  -o ./bin/app
```

`--allow-all` is fine for demos; tighten for anything you distribute.

## Emit Rust (inspection)

```bash
rite build script.rite --allow-all --emit-rust -o ./bin/script
```

Use emit when you want to see how the host crate embeds IR — not as a license to hand-edit forever. The source of truth remains the `.rite` file + IR.

## Parity testing

The project’s conformance/differential suite compares interpreter and compiled execution for fixtures under `conformance/`:

```bash
cargo test -p rite-test --test conformance_gate
```

When you change the compiler or runtime, that gate is the regression net.

## When to compile

| Use `rite run` | Use `rite build` |
|----------------|------------------|
| Dev iteration | Deployable CLI tools |
| REPL / Studio | Shipping a single binary to a server |
| Quick scripts | Slightly faster startup / no `rite` install on target* |

\*Target still needs a compiled binary for the OS/arch you built.

## Limitations

- HTTP servers work as compiled programs but are still long-running processes  
- Browser WASM is a **different** artifact (`scripts/build-wasm.sh`) — not the same as `rite build`  
- Platform: build on the OS you intend to run (or cross-compile with a proper Rust target setup)

## Embedding vs build

- **`rite build`** — ship a Rite program as its own binary  
- **`RiteEngine` in Rust** — your app owns the process and calls Rite as a library ([Embedding](embedding.md))

## Next

[Text RPG](rpg.md) — a fuller `@game` example, or skip to [Embedding](embedding.md).
