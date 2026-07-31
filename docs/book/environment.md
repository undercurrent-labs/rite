# Environment

A script does not only read files and open sockets. It also sits inside an
*environment*: variables it was configured with, a clock, a source of randomness,
and a scratch pad that lasts as long as the run. Four small capabilities cover it,
and they are grouped here because they share a shape — a handful of functions each,
[its own permission](effects.md) each, and all of them things you reach for while
writing a tool rather than while learning the language.

For running *other* programs, see [Processes](processes.md).

## Configuration (`@env`)

```rite native_only
home ← ! @env.get("HOME")
! @console.println(home)
```

`@env.get` answers the value **or `none`** — not a result. A missing variable is an
ordinary absence, not a failure, so it matches with `?` against `none` rather than
needing `ok`/`err`:

```rite native_only
~ ! @env.get("PORT") ⟦
  none → ! @console.println("defaulting to 8080")
  p → ! @console.println("port " + p)
⟧
```

When a variable is genuinely required, say so and get a result instead:

```rite native_only
◆! main() ⟦
  ~ ! @env.require("NOPE_NOT_SET") ⟦
    ok v → ! @console.println("got " + v)
    err e → ! @console.println("kind " + e.kind + " / " + e.message)
  ⟧
⟧
```

```text
kind env.missing / missing env `NOPE_NOT_SET`
```

The two exist so the *script* declares which variables are optional, rather than
every caller having to remember.

### Grants are per variable

```bash
rite run app.rite --allow env=HOME          # just HOME
rite run app.rite --allow env=HOME,PORT     # two
rite run app.rite --allow env               # the whole environment
```

Environment variables are where secrets live, so the default is to deny and the
useful grant is the narrow one. A script that reads `HOME` should not be handed
`AWS_SECRET_ACCESS_KEY` as well.

`@env.all` answers a record of **everything the script may read** — which is the whole
environment under `--allow env`, and exactly the names you listed under a scoped
grant:

```rite native_only
! @console.println(keys(! @env.all()?))
```

```bash
rite run dump.rite --allow env=PATH,HOME
# [HOME, PATH]
```

It reveals nothing `@env.get` would not answer one name at a time, so the scoped
grant stays honest: the record has as many entries as you granted, and no more. With
nothing granted it is a permission error rather than an empty record — a script
asking for the environment when it may not have one should hear so.

## Time (`@clock`)

```rite native_only
! @console.println(! @clock.now())
```

```text
2026-07-31T16:31:13.680626617+00:00
```

RFC3339, always UTC. That format is fixed rather than configurable for a reason
worth knowing: **RFC3339 in UTC sorts lexicographically**, so `<` and `>` on the
plain strings really are time comparisons, with no parsing step. `@fs.metadata`
reports `mtime` in the same spelling precisely so the two can be compared directly —
see [Auditing a directory](../tutorials/fs-audit.md).

| Call | Answers | Marked? |
|---|---|---|
| `@clock.now()` | RFC3339 UTC string | **yes** |
| `@clock.sleep(ms)` | `none`, after waiting | **yes** |
| `@clock.parse(s)` | `ok(normalized)` or `err(message)` | no |
| `@clock.format(t, pattern)` | `ok(string)` or `err` | no |
| `@clock.duration(v)` | `ok(milliseconds)` or `err` | no |

`parse` normalizes to UTC and validates, which is the way to check that a string you
were given is a timestamp at all:

```rite native_only
! @console.println(@clock.parse("2026-01-01T00:00:00+00:00"))
```

```text
ok(2026-01-01T00:00:00+00:00)
```

`sleep` is the honest way to pace a loop — a retry backoff, a poll interval:

```rite native_only
! @clock.sleep(250)
```

### Formatting

`@clock.format` takes a strftime pattern:

```rite native_only
t ← "2026-07-31T16:31:13.680626617+00:00"
! @console.println(@clock.format(t, "%Y-%m-%d")?)
! @console.println(@clock.format(t, "%A, %d %B %Y")?)
```

```text
2026-07-31
Friday, 31 July 2026
```

It answers a **result**, because both of its arguments can be wrong: a string that is
not a timestamp gives `err(⟨kind: "clock.parse", …⟩)`, and an unknown specifier gives
`err(⟨kind: "clock.pattern", …⟩)` rather than taking the process down with it.

Formatting is one-way and for people. Anything a program will read back should stay
in the RFC3339 form, which is the only one that still sorts.

### Saying how long

`@clock.duration` normalizes a duration to whole milliseconds, so a timeout can be
written the way it is said out loud:

```rite native_only
! @clock.sleep(@clock.duration("1.5s")?)
```

| Written | Milliseconds |
|---|---|
| `1500` or `"1500"` or `"1500ms"` | 1500 |
| `"2s"` | 2000 |
| `"5m"` | 300000 |
| `"1h"` | 3600000 |
| `"1d"` | 86400000 |

A bare number is milliseconds, so the integer and string forms agree. An unknown unit
is an `err` naming the ones that exist.

**There is no date arithmetic.** No "thirty days ago", no adding an hour. A cutoff
has to be a timestamp you already hold — a literal, or one a previous run wrote
down. Comparison is the whole toolkit.

## Randomness (`@random`)

| Call | Answers |
|---|---|
| `@random.int(min, max)` | integer in `[min, max]`, both ends included |
| `@random.float()` | float in `[0, 1)` |
| `@random.choose(list)` | one element |
| `@random.shuffle(list)` | a shuffled **copy** — the original is untouched |
| `@random.uuid()` | a UUID v4 string |
| `@random.seed(n)` | reseeds; answers `none` |

All of them are marked, and all ride the `random` permission — which is **allowed by
default**, along with `console` and `clock`. Revoke it like any other default:

```bash
rite run t.rite --deny random
```

### Seeding makes a run reproducible

```rite native_only
◆! main() ⟦
  ! @random.seed(42)
  ! @console.println(! @random.int(1, 6))
  ! @console.println(! @random.choose(["a", "b", "c"]))
  ! @console.println(! @random.shuffle([1, 2, 3, 4, 5]))
⟧
```

```text
4
b
[2, 3, 5, 4, 1]
```

Run that twice and it prints the same three lines, which is what makes a test over
random input worth writing.

> **Seeding covers `uuid` too.** After `@random.seed(42)`, `@random.uuid()` returns
> the *same* UUID on every run. That is exactly what you want in a test and exactly
> what you must not ship: a seeded UUID is not unique and not unguessable, so never
> use one as a session token, a password reset link, or anything else that is
> supposed to be a secret. For those, reach for
> [`@crypto.random_bytes`](crypto.md), which reads the machine's entropy pool and
> ignores the seed.

## Scratch state (`@store`)

`@store` is a namespaced key/value map that lives in the interpreter for the
lifetime of the run:

```rite browser
! @store.set("cfg", "retries", 3)
! @console.println(@store.get("cfg", "retries"))
! @store.delete("cfg", "retries")
! @console.println(@store.get("cfg", "retries"))
```

```text
ok(3)
ok(none)
```

Three things to notice:

- **Every call answers a result.** A missing key is `ok(none)`, not `err` — asking
  for something that is not there is not a failure.
- **`get` is not marked.** Reading a map the program itself filled observes nothing
  outside the program, so it needs no `!`. `set` and `delete` are marked, because
  they change state something else can see.
- **It needs no permission**, because it reaches nothing outside the process. There
  is nothing to grant and nothing to deny.

The first argument is a namespace, which is what keeps two unrelated bits of a
program from colliding on the key `"id"`:

```rite browser
! @store.set("user", "id", 7)
! @store.set("order", "id", 99)
! @console.println(@store.get("user", "id"))
```

```text
ok(7)
```

**It is not persistence.** Nothing is written to disk and nothing survives the
process — a second `rite run` starts empty. For state that outlives the run, write a
file ([Files, JSON, and CSV](files-json.md)) or use [a database](db.md).

## Permissions at a glance

| Capability | Default | Grant |
|---|---|---|
| `@env` | denied | `--allow env=NAME[,NAME…]` or `--allow env` |
| `@clock` | **allowed** | `--deny clock` to revoke |
| `@random` | **allowed** | `--deny random` to revoke |
| `@store` | always | — |

Except for `@store`, none of these exist in the browser: Studio has no environment
and no entropy pool of its own. See [Browser & Studio](browser.md).

## Next

[Processes](processes.md) · [Effects and capabilities](effects.md) · [Modules](modules.md)
