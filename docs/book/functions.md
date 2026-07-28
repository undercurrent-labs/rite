# Functions and closures

Functions are first-class values: define them, pass them to pipelines, return them from other functions.

## Defining a function

Glyph:

```rite
◆ square(n) ⟦
  ^ n * n
⟧

! @console.println(str(square(4)))   // 16
```

ASCII:

```rite
def square(n) [[
  return n * n
]]

do host.console.println(str(square(4)))
```

| Glyph | ASCII | Role |
|-------|-------|------|
| `◆ name(params)` | `def name(params)` | Definition |
| `⟦ … ⟧` | `[[ … ]]` | Body block |
| `^ expr` | `return expr` | Return value |

## Return rules

1. **`^` / `return`** exits the function immediately with a value.
2. The **last expression** in a block can also act as the result when you structure bodies that way — explicit `^` is clearer for readers and early exits.

```rite
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
```

## Multiple parameters

```rite
◆ add(a, b) ⟦
  ^ a + b
⟧

! @console.println(str(add(2, 3)))
```

## Closures

Functions close over bindings from outer scopes:

```rite
factor ← 10
◆ scale(n) ⟦
  ^ n * factor
⟧

! @console.println(str(scale(3)))  // 30
```

Pipeline stages often use **block lambdas**:

```rite
xs ← [1, 2, 3, 4]
ys ← xs → map { |n| n * n }
! @console.println(ys)
```

The `{ |args| body }` form is the small anonymous function used by `map`, `keep`, and friends.

## Local helpers

Non-exported helpers are just functions without `pub` (modules) or nested defs:

```rite
◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧
```

## Calling

```rite
result ← square(12)
! @console.println(str(result))
```

Trailing-block call sugar exists for some forms (e.g. `keep { … }` in pipelines). **Match and if scrutinees** do not swallow a following block as a call argument — write `~ value ⟦ arms ⟧` / `match value [[ arms ]]` deliberately.

## Effects inside functions

If a function performs host I/O, mark those calls with `!` / `do`:

```rite
◆ greet(name) ⟦
  ! @console.println("hi " + name)
  ^ none
⟧

! greet("Aura")
```

Permission checks still apply at the host boundary.

## Public functions (modules)

```rite
// math.rite
pub ◆ square(value) ⟦
  ^ value * value
⟧
```

Only `pub` items are imported by `use` — see [Modules](modules.md).

## Try it

```bash
rite run examples/hello/hello.rite --allow-all
```

## Next

[Pipelines](pipelines.md): chain transforms with `→` / `->`.
