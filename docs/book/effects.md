# Effects and capabilities

Rite separates **pure computation** from **host effects**. Host functions live under capabilities (`@console`, `@fs`, …) and are gated by **permissions**.

## Marking effects

Statement-level host I/O uses `!` (glyph) or `do` (ASCII):

```rite browser
! @console.println("hi")
```

### Effects travel through your own functions

A function that performs a host effect declares it with `◆!` (ASCII `def!`), and
callers mark the call the same way they mark a capability:

```rite browser
◆! greet(name) ⟦
  ! @console.println("hello, {name}")
⟧

! greet("world")
```

Leave the declaration off and the compiler says so, at the declaration rather
than at every call:

```text
error[E021]: `greet` performs host effects but is not declared `◆!`
  help: declare it `◆! greet(…)`, then callers mark the call with `!`
```

This carries through the call graph: a function that calls a `◆!` function is
itself effectful and needs its own marker, however many layers deep the host call
sits. Recursion and mutual recursion are fine — the check is a fixed point, not a
walk.

Declaring a function `◆!` whose body happens to be pure is allowed. It is a
promise about the API, not a description of today's body, so a function can
reserve the right to perform effects later without breaking its callers.

### Passing an effectful function

Handing an effectful function to another one runs it, so the call takes a marker
even though the function receiving it is pure:

```rite browser
◆! shout(n) ⟦ ! @console.println(str(n)) ⟧

! ([1, 2] → each(shout))
```

A lambda written inline is different — its own `!` is already visible on the same
line, so nothing is hidden and no extra marker is asked for:

```rite browser
[1, 2] → each { |n| ! @console.println(str(n)) }
```

The rule reads what is written at the call. A closure stored in a binding and
passed along later is not tracked; that would need a type system Rite does not
have. Permissions still bound what any of it can reach.

```rite browser
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

### Not every `@` call is an effect

`@json.encode`, `@csv.encode`, `@clock.format` and the whole of
[`@crypto`](crypto.md) except `random_bytes` are functions of their arguments: same
input, same answer, nothing outside the program touched. They take no marker and no
grant. The `@` still tells you a host implements it — the missing `!` tells you it
cannot surprise you.

```rite browser
! @console.println(@crypto.sha256("abc"))
```

Naming a host function is not a way around the marker. A capability reference is not a
value you can pass around — mentioning it *calls* it, with no arguments — so it takes a
marker too:

```rite browser
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
| `@crypto` | sha256/sha512, hmac_sha256, base64/hex, constant_time_eq — all pure; `random_bytes` needs `random` |
| `@db` | DuckDB open/query/exec/prepare/transactions (native) |
| `@clock` | now, sleep, parse, … |
| `@random` | seed, int, … |
| `@env` | get/set environment (permissioned) |
| `@process` | `args` (this script's own arguments, no grant needed); `run`/`which` spawn or probe, and need `process` |
| `@http` | `listen` + middleware helpers; `get`/`post`/`request` for outbound calls (all need `net`) |
| `@udp` | `bind`/`send_to`/`recv_from`/`close` datagram sockets (native, needs `net`) |
| `@tcp` | `connect`/`send`/`recv`/`close` byte streams, and `listen` with a per-connection block (native, needs `net`) |
| `@game` | text adventure helpers |

Details and signatures: `rite docs build` → `docs/generated/`.

## Example: safe defaults

```rite browser
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

## Randomness and reproducibility

`@random` is seeded from the operating system, so two runs of the same script differ.
Call `@random.seed(n)` to pin a sequence:

```rite browser
! @random.seed(1)
! @console.println(str(! @random.int(1, 6)))   // same value on every run
```

A seed covers the whole capability — `int`, `float`, `choose`, `shuffle` and `uuid` all
draw from the one generator, so a seeded run reproduces its identifiers too. Seed at the
top of the script, before anything draws from it.

Studio pins a seed for you, so editing and re-running shows changes you made rather than
noise you didn't.

> `@random` is allowed by default and needs no `--allow`. It is not a cryptographic
> generator — don't use it for keys, tokens, or anything an attacker gets to guess at.
> Use `@crypto.random_bytes(n)` for those; it draws from the operating system and
> ignores the seed on purpose. See [Hashing and encoding](crypto.md).

## Example: files need FS

```rite browser
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

```rite native_only
argv ← ! @process.args        // ["alpha", "beta"]
```

It still takes the `!` marker, because the answer differs between runs for the same
reason `@clock.now` does. Spawning something new (`@process.run`) is a different
privilege and does need `--allow process`. A compiled binary reads its own argv, with
no `--` to strip.

## Next

[Files and JSON](files-json.md) — practical `@fs` and `@json` patterns.
