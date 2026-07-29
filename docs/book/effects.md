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

Some contexts bind host results in expressions (`text ← @fs.read(path)?`). The important part: **capabilities are explicit** — you see `@fs` / `host.fs` in the source.

## Capability prefix

| Glyph | ASCII |
|-------|-------|
| `@console.println` | `host.console.println` |
| `@fs.read` | `host.fs.read` |
| `@json.encode` | `host.json.encode` |

## Default permission policy

| Capability | Default |
|------------|---------|
| **console** | allowed |
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
| `@process` | run subprocesses (dangerous; native only) |
| `@http` | listen, client, middleware helpers |
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

## Next

[Files and JSON](files-json.md) — practical `@fs` and `@json` patterns.
