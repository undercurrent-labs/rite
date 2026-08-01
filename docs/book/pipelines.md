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
| `sort` / related | Ordering helpers where implemented |
| `flatten` | Nested lists → flat (lists only) |

```rite browser
words ← ["alpha", "beta", "gamma"]
! @console.println(words → count)
```

Exact builtin set evolves with the runtime; `rite docs build` and capability/docs output list host functions. Pure list helpers live in the evaluator builtins.

## Multi-line style

Prefer one stage per line for readability:

```rite
summary ← rows
  → keep { |r| r.active }
  → map { |r| r.score }
  → sum
```

## The `$` placeholder

When the piped value should **not** be the first argument, use `$`:

```rite browser
// Conceptual: pass piped value as a later argument
// value → some_fn(fixed, $, other)
```

Use `$` when a helper’s primary parameter is not in first position (e.g. “replace needle in haystack” styles). If everything is written first-arg-friendly, you rarely need `$`.

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

The reason is worth knowing, because it is the one thing about `→` you have to
remember. An infix operator cannot be looser than `+` on its left and tighter than
`+` on its right — reaching the input side costs the result side. Rite spends the
parentheses on the result and keeps the input honest, and says so at the point of
the mistake instead of quietly parsing something else. `|>` in F#, Elixir and Elm
makes the same trade; there the equivalent shows up as a type error.

## Next

[Collections](collections.md) — list/record operations and merge semantics in more detail.
