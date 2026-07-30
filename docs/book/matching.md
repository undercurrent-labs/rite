# Pattern matching

Matching turns values into control flow and bindings. Glyph form uses `~`; ASCII uses `match`.

## Basic match

```rite
status ← #ok

msg ← ~ status ⟦
  #ok → "ready"
  #error → "failed"
  _ → "unknown"
⟧

! @console.println(msg)   // ready
```

ASCII:

```rite
msg <- match status [[
  :ok -> "ready"
  :error -> "failed"
  _ -> "unknown"
]]
```

- Arms are tried **top to bottom**
- `_` matches anything (wildcard)
- The match expression **returns** the arm’s result (here a string)

### One arm per line

Arms are separated by **newlines**, not commas. A comma between arms is a syntax
error, not a style choice:

```text
~ status ⟦ #ok → 1, _ → 2 ⟧      // error[E013]: expected pattern
```

Write it across lines instead:

```rite
~ #ok ⟦
  #ok → 1
  _ → 2
⟧
```

## Atoms and literals

```rite
code ← 200
label ← ~ code ⟦
  200 → #ok
  404 → #missing
  _ → #other
⟧
```

## List destructuring

```rite
pair ← [1, 2, 3]

head ← ~ pair ⟦
  [h, ..rest] → h
  _ → 0
⟧

! @console.println(head)  // 1
```

- `[h, ..rest]` binds the head and the remainder list  
- Use `_` (or other patterns) for empty / non-list fallbacks

## Record patterns

Match structure and pull fields (syntax as supported by the matcher — field patterns and nested forms):

```rite
user ← ⟨name: "Aura", role: #admin⟩

title ← ~ user ⟦
  ⟨role: #admin, name: n⟩ → "admin:" + n
  ⟨name: n⟩ → n
  _ → "anonymous"
⟧
```

Prefer simple field access when you don’t need multi-way branching: `user.name`.

## Nested match

```rite
event ← ⟨kind: #click, x: 10⟩

~ event ⟦
  ⟨kind: #click, x: x⟩ → ! @console.println("click@" + str(x))
  ⟨kind: #key⟩ → ! @console.println("key")
  _ → ! @console.println("other")
⟧
```

## Results (`ok` / `err`)

Host and fallible ops return results. The patterns are **juxtaposed, not called** —
`ok value`, never `ok(value)`, even though `ok(value)` is how you *construct* one:

```rite
outcome ← ok(⟨n: 7⟩)

text ← ~ outcome ⟦
  ok data → "n=" + str(data.n)
  err e   → "failed"
⟧
```

The bound name is ordinary — `ok data` binds the success value to `data`, `err e` binds
the error to `e` — and either arm may be `_` if you do not need the payload:

```rite
~ outcome ⟦
  ok _  → #worked
  err _ → #failed
⟧
```

Unwrap fallible host calls with postfix `?` when you want early-return style error propagation ([Results](results.md)).

## Match is an expression

You can bind the whole match:

```rite
label ← ~ status ⟦
  #ok → "ready"
  _ → "other"
⟧
```

Or use it as a pipeline stage input by binding first (match has lower “chain” priority than a simple call chain — bind intermediates when unclear).

## Scrutinee and blocks

Write the value to match, then the arm block:

```rite
~ value ⟦
  pat → body
⟧
```

Do **not** rely on trailing-block call sugar for match/if scrutinees; the block belongs to the match.

## Exhaustiveness

Rite does not require compile-time exhaustiveness like Rust. Include a `_` arm when you want a default. Without a match, failed matches surface as runtime errors depending on path — prefer an explicit `_`.

## Example

```bash
rite run examples/04-pattern-matching/main.rite --allow-all
```

## Next

[Results and errors](results.md) — `ok` / `err`, `?`, and error records.
