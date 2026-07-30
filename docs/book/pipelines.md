# Pipelines

Pipelines are the main way to transform collections and values without nested calls. The operator is `→` (glyph) or `->` (ASCII).

## Core rule

**The value on the left becomes the first argument** of the call on the right.

```rite
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
| `first` / `last` | Ends of a list |
| `sort` / related | Ordering helpers where implemented |
| `flatten` | Nested lists → flat |

```rite
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

```rite
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

```rite
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

`→` binds **tighter than the operators**, so a pipeline's result is an ordinary operand:

```rite
xs ← [1, 2, 3]
xs → count > 2        // (xs → count) > 2   → true
xs → sum + 1          // (xs → sum) + 1     → 7
xs → sum = 6          // (xs → sum) = 6     → true
```

Each stage is a name, a call, or a trailing-block call — never a bare operator
expression. That is what lets the operator after a pipeline attach to the *result*.

The trade-off is on the input side: a bare binary expression before `→` groups to the
right, so parenthesise when you mean to pipe the whole thing.

```rite
a + b → str           // a + (b → str)
(a + b) → str         // the sum, piped
```

## Next

[Collections](collections.md) — list/record operations and merge semantics in more detail.
