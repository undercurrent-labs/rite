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

Naming a function first changes nothing. A binding that holds an effectful
function is checked exactly as the function is, so neither of these gets past the
compiler:

```rite
◆! shout(n) ⟦ ! @console.println(str(n)) ⟧

◆ run(xs) ⟦
  g ← shout          // a rename, not a disguise
  each(xs, g)        // error[E021]: passing `g` here requires `!` on the call
⟧
```

```rite
◆! shout(n) ⟦ ! @console.println(str(n)) ⟧

◆ run() ⟦
  f ← shout
  f(1)               // error[E021]: calling `f` requires `!`
⟧
```

A lambda bound to a name carries the property the same way, from its body:

```rite
f ← ⟦ |n| ! @console.println(str(n)) ⟧   // performs an effect when called

◆ run(xs) ⟦ each(xs, f) ⟧                // error[E021]
```

#### What is not tracked

The property follows a **name**. It does not follow a function that arrives some
other way, because there is no name to attach it to:

- a function held in a record field or a list element — `each(xs, r.go)`
- a function received as a parameter — `◆ run(xs, f) ⟦ each(xs, f) ⟧`
- a function returned by a call — `each(xs, pick())`

In each of those, `rite check` is silent whether or not the function performs
effects. Closing this needs effect polymorphism — a way to say "this combinator is
effectful exactly when its argument is" — and that needs a type system Rite does
not have. Marking every function that takes a function would be the alternative,
and it would make the marker mean less rather than more: `map` itself would carry
one.

**Permissions still bound what any of it can reach.** The marker is a legibility
device — it says where to look. The grant is the boundary, and it is checked at
the capability, not at the call site that reaches it.

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

## Exit codes

The status a run ends with is part of Rite's contract, so a shell script or a CI job
can tell what happened without parsing any output. Every code below was checked
against the binary, not against intent:

| Code | Means | Comes from |
|---|---|---|
| 0 | Success | The script finished |
| 1 | Runtime error | `fail`, `panic`, an unhandled capability error |
| 2 | Usage error | The `rite` command line itself was wrong |
| 3 | Would not parse | A syntax or lexical error — `E00x`, `E01x` |
| 4 | Would not resolve | It parsed; a name, effect marker or import was wrong — `E02x` |
| 5 | Permission denied | A capability call without the grant it needs |
| 6 | Build failed | `rite build` |
| 7 | A test failed | `rite test` |
| 8 | Budget exceeded | `--max-steps` or the execution timeout |

Codes 3 and 4 describe **what was wrong with the source**, not which command
noticed: `rite run`, `rite check` and `rite semantic-ir` all answer 3 for a file
that will not parse and 4 for one that parses but does not resolve. A wrapper can
act on that — 3 means the text is not Rite, 4 means it is Rite that refers to
something that is not there.

### Choosing your own status

`@process.exit(code)` ends the run with any status from 0 to 255:

```rite native_only
◆! main() ⟦
  ! @console.error("usage: greet NAME...")
  ! @process.exit(2)
⟧
```

Nothing after the call runs, no `^` or middleware can catch it, and buffered output
is still flushed. It needs **no permission**, for the same reason `@process.args`
does not: what status you end with is your own business, not ambient authority. A
status outside 0–255 is an error at the call rather than a silent truncation.

Most often the status is not one you chose but one you are passing on:

```rite native_only
◆! main() ⟦
  r ← ! @process.run("git", ["push"], ⟨⟩)?
  ! @process.exit(r.status)
⟧
```

That is why the range is not restricted to codes the runtime does not use. A
subprocess can hand you any status, and a `@process.exit` that rejected some of them
would fail only for the ones a child happened to return — long after your tests
passed. The cost of allowing it is that **1–8 mean two things**: the table above when
the runtime ended the run, and whatever you decided when your script did. If a
wrapper has to tell those apart, read stderr — the runtime always says `permission
denied:` or `budget exceeded:` when it is the one stopping you, and says nothing at
all about a status you chose.

Inside an `@http` or `@tcp` handler, an exit ends the **process**, not the request:
the server stops accepting, the request in flight gets a `503`, and `@http.listen`
ends the script with the status. `use @http.recover` does not intercept it — it turns
handler *failures* into described 500s, and an exit is not a failure.

## Next

[Files and JSON](files-json.md) — practical `@fs` and `@json` patterns.
