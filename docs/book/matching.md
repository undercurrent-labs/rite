# Pattern matching

Matching turns values into control flow and bindings. Glyph form uses `~`; ASCII uses `match`.

## Basic match

```rite browser
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

```rite browser
~ #ok ⟦
  #ok → 1
  _ → 2
⟧
```

## Atoms and literals

```rite browser
code ← 200
label ← ~ code ⟦
  200 → #ok
  404 → #missing
  _ → #other
⟧
```

## List destructuring

```rite browser
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

```rite browser
user ← ⟨name: "Aura", role: #admin⟩

title ← ~ user ⟦
  ⟨role: #admin, name: n⟩ → "admin:" + n
  ⟨name: n⟩ → n
  _ → "anonymous"
⟧
```

Prefer simple field access when you don’t need multi-way branching: `user.name`.

## Nested match

```rite browser
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

```rite browser
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

## Or-patterns

`|` joins alternatives in one arm, tried left to right:

```rite browser
◆ size(n) ⟦
  ^ ~ n ⟦
    1 | 2 | 3 → "small"
    4 | 5 | 6 → "medium"
    _ → "large"
  ⟧
⟧

! @console.println(size(2))    // small
! @console.println(size(5))    // medium
```

Alternatives may bind names, but every alternative must bind the **same**
names — the arm body runs whichever one matched:

```rite
~ outcome ⟦
  ok v | err v → v      // the payload, success or not
⟧
```

`ok v | err e → …` is an error, since `e` would be unbound when `ok v`
matched. Or-patterns join whole arms only; inside a list or record pattern,
write separate arms instead. `true | false` counts as covering the whole
boolean domain for the exhaustiveness warning.

## Guards

A guard is a condition after the pattern — glyph `?`, ASCII `if`. The arm
matches only when the pattern matches **and** the guard is truthy; otherwise
the value moves on to the next arm:

```rite browser
◆ classify(n) ⟦
  ^ ~ n ⟦
    x ? x < 0 → "negative"
    0 → "zero"
    x ? x % 2 = 0 → "even"
    _ → "odd"
  ⟧
⟧

! @console.println(classify(6))     // even
! @console.println(classify(7))     // odd
```

The guard sees the pattern's bindings (`x` above). It is parsed below pipeline
precedence, so the `→` after it always belongs to the arm; parenthesize a
pipeline if a guard needs one. A guarded arm never counts toward
exhaustiveness — the guard can refuse any value — so `x ? x > 0 → …` still
wants a `_` arm after it.

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
