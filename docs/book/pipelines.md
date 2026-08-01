# Pipelines

Pipelines are the main way to transform collections and values without nested calls. The operator is `→` (glyph) or `->` (ASCII).

## Core rule

**The value on the left becomes the first argument** of the call on the right.

```rite browser
xs ← [1, 2, 3, 4, 5, 6]

result ← xs
  → keep { |n| n % 2 = 0 }
  → map { |n| n * n }
  → sum

! @console.println(result)   // 2²+4²+6² = 4+16+36 = 56
```

Read top to bottom: start with `xs`, keep even numbers, square them, sum.

ASCII:

```rite
result <- xs
  -> keep { |n| n % 2 = 0 }
  -> map { |n| n * n }
  -> sum
```

## Common stages

These names are available as pipeline-friendly builtins (collection-oriented):

| Stage | Role |
|-------|------|
| `map { \|x\| … }` | Transform each element |
| `keep { \|x\| … }` | Filter (predicate true → keep) |
| `sum` | Sum numbers |
| `count` | Length |
| `first` / `last` | Ends of a list, string or bytes |
| `sort` | Order a list, string or bytes |
| `unique` | Drop repeats, keeping first appearance |
| `flatten` | Nested lists → flat (lists only) |

```rite browser
words ← ["alpha", "beta", "gamma"]
! @console.println(words → count)
```

That table is a starting set, not the whole one. `rite docs build` writes the
complete list of builtins and host functions to `docs/generated/`, generated from the
same tables the runtime dispatches on, so it cannot drift from what is actually there.

## Multi-line style

Prefer one stage per line for readability:

```rite
summary ← rows
  → keep { |r| r.active }
  → map { |r| r.score }
  → sum
```

## Your own functions are stages

A stage is any callable — a builtin, a function you defined, a module function, or
a binding holding a closure. A bare name resolves the way it would in a call, so a
definition of your own shadowing a builtin wins in both places:

```rite browser
◆ shout(s) ⟦ ^ upper(s) + "!" ⟧

! @console.println("hello" → shout)        // HELLO!
```

```rite browser
◆ square(n) ⟦ ^ n * n ⟧

! @console.println(str([1, 2, 3] → map(square) → sum))   // 14
```

## The `$` placeholder

When the piped value should **not** be the first argument, use `$`:

```rite browser
! @console.println("-" → join(["a", "b"], $))   // a-b
```

Without it the value goes first, as `["a", "b"] → join("-")` does.
Use `$` when a helper's primary parameter is not in first position (a "replace
needle in haystack" shape). If everything is written first-arg-friendly, you rarely
need it.

## Results do not short-circuit

A stage receives whatever the previous one produced, and a `Result` is an ordinary
value. Nothing unwraps it and nothing stops on `err`:

```rite browser
◆ tag(v) ⟦ ^ ⟨seen: v⟩ ⟧

! @console.println(err("boom") → tag)   // ⟨seen: err(boom)⟩ — tag still ran
```

That is deliberate: a stage that silently skipped itself would make a pipeline's
behaviour depend on a value you cannot see in the source. Short-circuiting is
opt-in, and it is spelled `and_then`:

```rite browser
! @console.println(err("boom") → and_then { |n| ok(n * 10) })   // err(boom)
! @console.println(ok(3) → and_then { |n| ok(n * 10) })         // ok(30)
```

To unwrap and propagate, put `?` on the pipeline's **result** — `?` on a stage is
rejected (`E016`), because it would apply to the stage rather than to the value
flowing through it:

```rite
total ← (rows → map { |r| r.n } → sum)?
```

## Evaluation is eager

Every stage runs to completion and hands the next one a finished value, so
`xs → map f → keep p → sum` builds two intermediate lists. There are no lazy or
streaming values in Rite today. For a large file, read it in pieces with
`@fs.open` and its handle rather than piping `@fs.lines` over the whole thing.

## Pipelines vs nested calls

```rite
// Nested (harder to extend)
sum(map(keep(xs, { |n| n > 0 }), { |n| n * 2 }))

// Pipeline (easy to insert stages)
xs
  → keep { |n| n > 0 }
  → map { |n| n * 2 }
  → sum
```

## Binding intermediate results

```rite
evens ← xs → keep { |n| n % 2 = 0 }
squares ← evens → map { |n| n * n }
! @console.println(squares)
```

Useful for debugging: print an intermediate list before the next stage.

## Pipelines on non-lists

Some stages accept scalars or records depending on the function. When in doubt, keep pipelines on **lists** and use ordinary calls for scalars:

```rite browser
n ← 12
! @console.println(str(n * n))
```

## Example from the repo

```bash
rite run examples/02-pipelines/main.rite --allow-all
```

## Studio

Pipelines work offline in [Studio](/studio) for pure data (no FS). Paste the even-square-sum example and **Run**.

## Precedence

`→` binds **looser than every operator**, so whatever is on the left is finished
before it is piped:

```rite browser
a ← 2
b ← 3
! @console.println(a + b → str)   // (a + b) → str   → "5"
```

Each stage is a name, a call, or a trailing-block call — never a bare operator
expression. `xs → count + 1` would have to mean `xs → (count + 1)`, and `count + 1`
is not something you can apply to a list.

### Using a pipeline's result

Parenthesise it. An operator directly after a pipeline is a **parse error**, not a
regrouping:

```rite browser
xs ← [1, 2, 3]
! @console.println(str((xs → count) > 2))
```

Without them:

```rite compile_fail
xs ← [1, 2, 3]
xs → count > 2
```

The reason: an infix operator cannot be looser than `+` on its left and tighter
than `+` on its right, so reaching the input side costs the result side. Rite spends
the parentheses on the result, and reports the mistake at the point of use instead
of parsing something else. `|>` in F#, Elixir and Elm makes the same trade, where
the equivalent shows up as a type error.

## Next

[Collections](collections.md) — list/record operations and merge semantics in more detail.
