# Effects and capabilities

Rite separates **pure computation** from **host effects**. Host functions live under capabilities (`@console`, `@fs`, …) and are gated by **permissions**.

## Marking effects

Statement-level host I/O uses `!` (glyph) or `do` (ASCII):

```rite
! @console.println("hi")
```

```rite
do host.console.println("hi")
```

The marker is required wherever the call appears, including when you bind its result:
`text ← ! @fs.read(path)?`. Two things are always explicit — the **capability** (you see
`@fs` / `host.fs` in the source) and the **effect** (you see the `!` / `do`).

A call is effectful if it observes *or* changes state outside the program: the filesystem,
environment, subprocesses, sockets, the terminal, the clock, the entropy source, a
database. Note that **reads count** — `@fs.read` and `@db.query` need `!` just as
`@fs.write` does, for the same reason `@clock.now` does: the answer can differ between
runs. The exception is state a capability owns in-process (`@game`'s world, `@store`'s
map), where only writes are marked; reading it is like reading a local binding.

Naming a host function is not a way around the marker. A capability reference is not a
value you can pass around — mentioning it *calls* it, with no arguments — so it takes a
marker too:

```rite
now ← ! @clock.now        // reads the clock
```

Without the `!` that is `E021`. There is no form that captures `@clock.now` as a
function to call later.

## Capability prefix

| Glyph | ASCII |
|-------|-------|
| `@console.println` | `host.console.println` |
| `@fs.read` | `host.fs.read` |
| `@json.encode` | `host.json.encode` |

## Default permission policy

| Capability | Default |
|------------|---------|
| **console** | allowed (deny with `--deny console`) |
| **clock** | allowed |
| **random** | allowed |
| **fs** | denied |
| **net** / HTTP | denied |
| **env** | denied |
| **process** | denied |
| **db** (DuckDB) | denied |

So hello-world console scripts run without flags; reading files does not.

## Granting permissions (CLI)

### Allow everything (demos only)

```bash
rite run script.rite --allow-all
```

### Scoped grants

```bash
rite run app.rite \
  --allow fs:read=./data \
  --allow fs:write=./output \
  --allow net=api.example.com \
  --allow env=APP_MODE \
  --allow db \
  --allow db=./data
```

- `--allow db` — in-memory DuckDB only  
- `--allow db=./data` — file-backed DBs under that path prefix

Patterns are path/host oriented — tighten to the minimum your script needs.

### Inspect

```bash
rite capabilities
```

Lists registered host modules and related metadata.

## Built-in capability map (overview)

| Module | Typical use |
|--------|-------------|
| `@console` | print, println, warn, error, inspect |
| `@fs` | read, write, list, … (permissioned paths) |
| `@json` | encode, decode |
| `@csv` | encode, decode, read, write |
| `@db` | DuckDB open/query/exec/prepare/transactions (native) |
| `@clock` | now, sleep, parse, … |
| `@random` | seed, int, … |
| `@env` | get/set environment (permissioned) |
| `@process` | `args` (this script's own arguments, no grant needed); `run`/`which` spawn or probe, and need `process` |
| `@http` | `listen` + middleware helpers; `get`/`post`/`request` for outbound calls (all need `net`) |
| `@game` | text adventure helpers |

Details and signatures: `rite docs build` → `docs/generated/`.

## Example: safe defaults

```rite
! @console.println("console ok")
now ← ! @clock.now()
! @console.println(now)
! @random.seed(1)
n ← ! @random.int(1, 6)
! @console.println(n)
```

```bash
rite run examples/05-capabilities/main.rite
# usually works without --allow-all
```

## Example: files need FS

```rite
data ← ⟨hello: "world"⟩
text ← @json.encode(data)
// writing would need fs:write
! @console.println(text)
```

```bash
rite run examples/03-files-and-json/main.rite --allow-all
```

## Browser / Studio

Hosted [Studio](/studio) runs a **WASM** runtime:

- **Pure scripts + `@console`** (and similar pure-ish paths) work  
- **`@process`**, unrestricted FS, real HTTP listen → **native** `rite studio` / CLI only  
- Virtual HTTP routes may appear without a real socket  

See [Browser & Studio](browser.md).

## Design intent

1. **Readable effects** — scan for `!` / `do` and `@` / `host.`  
2. **Least privilege** — default deny for powerful caps  
3. **Same script, different hosts** — CLI, embedder, and browser can install different permission sets  

## Embedding

Rust hosts configure permissions via `RiteEngine::builder()` (see [Embedding](embedding.md)).

## Reading the script's own arguments

Arguments after `--` are the invoker's input to *this* program, so reading them needs
no grant — refusing would be like refusing to let a script read its own source:

```bash
rite run tool.rite -- alpha beta
```

```rite
argv ← ! @process.args        // ["alpha", "beta"]
```

It still takes the `!` marker, because the answer differs between runs for the same
reason `@clock.now` does. Spawning something new (`@process.run`) is a different
privilege and does need `--allow process`. A compiled binary reads its own argv, with
no `--` to strip.

## Next

[Files and JSON](files-json.md) — practical `@fs` and `@json` patterns.
