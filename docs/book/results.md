# Results and errors

Fallible work returns a **result**: success as `ok(value)`, failure as `err(...)`. That includes many host operations (`@fs`, `@json.decode`, `@game.save`, …).

## Why results?

Exceptions are not the primary control model. You either:

1. **Propagate** with postfix `?`, or  
2. **Branch** with `~` / `match` on `ok` / `err`.

## Creating results

```rite browser
// Fallible host ops produce results (ok/err). Prefer matching tags or `?`:
status ← #ok
label ← ~ status ⟦
  #ok → "good"
  #error → "bad"
  _ → "other"
⟧
! @console.println(label)
```

Error payloads from the host are often **records** with machine-readable fields (`kind`, `message`, …).

## Postfix `?` (unwrap or early return)

```rite native_only
// If left is ok(v), the expression yields v
// If left is err(e), the current function/script returns that err
text ← ! @fs.read("config.json")?
data ← @json.decode(text)?
```

**`?` requires a result.** `42?` is an error, not `42`. The operator says "this can
fail", so writing it over something that cannot is a claim about the code that is
not true — and the case that costs is a call which *used* to answer a result and
stopped, where a pass-through `?` would go on doing nothing.

`ok(none)` is a result and unwraps to `none`, which is how `@fs.read_line` reports
the end of a file.

### A function that can fail answers a result on both paths

`?` returns `err` from the enclosing function, so any function containing one can
answer a failure — and then it has to answer `ok(…)` on the way out too, or a
caller cannot tell the two apart:

```rite native_only
◆! describe(path) ⟦
  m ← ! @fs.metadata(path)?     // may return err from describe
  ^ ok(⟨path: path, len: m.len⟩)  // …so success is wrapped to match
⟧

info ← ! describe("notes.txt")?
```

Returning the bare record would make `describe` answer a record sometimes and an
`err` other times — a half-result, which `?` at the call site cannot unwrap.

Parser note: postfix `?` on a value is **not** the same as the conditional `?` (`if`). Position decides:

```rite
// conditional (glyph if)
label ← ? x = none ⟦ "empty" ⟧ : ⟦ "full" ⟧

// unwrap result
text ← ! @fs.read("f.txt")?
```

## Match on results

```rite
outcome ← @json.decode("{\"n\":1}")

msg ← ~ outcome ⟦
  ok data → "n=" + str(data.n)
  err e → "decode failed"
⟧

! @console.println(msg)
```

Use match when both success and failure need custom handling (logging, defaults, alternate paths).

## Coalesce and defaults

For **`none`** (missing fields), use `??`:

```rite
port ← cfg.port ?? 8080
```

For **results**, prefer `?` or match — don’t confuse `none` with `err`.

## Result helpers

| Call | Answers |
|---|---|
| `is_ok(r)` / `is_err(r)` | `true` / `false` |
| `unwrap_or(r, fallback)` | the value, or `fallback` when `err` |
| `or_else(r, fallback)` | the same, spelled the other way round |
| `require(r)` | the result unchanged, for asserting intent |

```rite browser
! @console.println(unwrap_or(err("bad"), 99))
! @console.println(is_ok(ok(1)))
```

```text
99
true
```

Each takes a **value** as its fallback, not a function — there is no lazy variant, so
the fallback is evaluated whether or not it is needed. Keep it cheap, and reach for
a match when the alternative involves real work.

## Chaining with `and_then`

`and_then(result, f)` calls `f` with the value of an `ok`, and passes an `err`
straight through without calling anything:

```rite browser
! @console.println(and_then(ok(2), { |n| ok(n * 10) }))
! @console.println(and_then(err("bad"), { |n| ok(n * 10) }))
! @console.println(and_then(ok(2), { |n| err("rejected") }))
```

```text
ok(20)
err(bad)
err(rejected)
```

The function is expected to answer a result of its own, so steps chain: each one may fail, and the first failure is the one that comes out.

Inside a function that can propagate, `?` says the same thing more directly —
`b ← step_one(a)?` then `step_two(b)?`. Reach for `and_then` where `?` is not
available: building a chain as a value, or composing steps a caller supplies.

## HTTP handlers

Request JSON is often fallible:

```rite browser
POST "/sum" |req| ⟦
  payload ← req.json?
  numbers ← payload.numbers ?? []
  ^ 200 ⟨total: numbers → sum⟩
⟧
```

If `req.json` is `err`, `?` propagates failure out of the handler according to runtime rules (typically an error response path via middleware like `@http.recover`).

## Permissions vs results

A **permission denial** is not always a soft `err` you match in-script — the CLI may exit with a permission code before or as a hard failure. Grant what you need:

```bash
rite run app.rite --allow fs:read=./data
```

See [Effects and capabilities](effects.md).

## Style

| Situation | Prefer |
|-----------|--------|
| Linear “happy path” | `?` chain |
| Dual handling | `match` / `~` |
| Missing optional field | `??` |
| Programmer bug | fail loudly; don’t hide with `_` everywhere |

## Example

```bash
rite run examples/03-files-and-json/main.rite --allow-all
```

## Next

[Effects and capabilities](effects.md) — marking I/O and configuring permissions.
