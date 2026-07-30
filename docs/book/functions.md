# Functions and closures

Functions are first-class values: define them, pass them to pipelines, return them from other functions.

## Defining a function

Glyph:

```rite browser
◆ square(n) ⟦
  ^ n * n
⟧

! @console.println(str(square(4)))   // 16
```

ASCII:

```rite browser
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

1. **`^` / `return`** exits the function immediately with a value — including from nested `if` / match / bare blocks.
2. The **last expression** in a block can also act as the result when you structure bodies that way — explicit `^` is clearer for readers and early exits.
3. **Multi-value return** (HTTP handlers): juxta expressions after `^` become a list, e.g. `^ 200 ⟨status: #ok⟩` → `[200, ⟨status: #ok⟩]`. Keep status/body on the same return statement; don’t put unrelated following statements on the juxta list.

```rite browser
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
```

## Multiple parameters

```rite browser
◆ add(a, b) ⟦
  ^ a + b
⟧

! @console.println(str(add(2, 3)))
```

## Closures

Functions close over bindings from outer scopes:

```rite browser
factor ← 10
◆ scale(n) ⟦
  ^ n * factor
⟧

! @console.println(str(scale(3)))  // 30
```

Pipeline stages often use **block lambdas**:

```rite browser
xs ← [1, 2, 3, 4]
ys ← xs → map { |n| n * n }
! @console.println(ys)
```

The `{ |args| body }` form is the small anonymous function used by `map`, `keep`, and friends.

## Local helpers

Non-exported helpers are just functions without `pub` (modules) or **nested defs** inside a function body. Nested `◆` / `def` bind in the enclosing block and close over outer parameters:

```rite browser
◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧

! @console.println(str(area(3, 4)))   // 12
! @console.println(str(area(-1, 4)))  // 0
```

ASCII:

```rite browser
def area(w, h) [[
  def clamp(n) [[
    if n < 0 [[
      return 0
    ]]
    return n
  ]]
  return clamp(w) * clamp(h)
]]
```

You can also **return** a nested function (it keeps its capture):

```rite browser
◆ make_adder(n) ⟦
  ◆ add(x) ⟦ ^ x + n ⟧
  ^ add
⟧
plus3 ← make_adder(3)
! @console.println(str(plus3(10)))  // 13
```

## Conditionals (`if` / `?`)

Both dialects use **`:`** between the then-block and else-block. The keyword `else` is **not** a separator (it would be read as a name).

```rite
// Glyph
label ← ? x = none ⟦ "empty" ⟧ : ⟦ "full" ⟧

// ASCII — same colon for else
label <- if x = none [[ "empty" ]] : [[ "full" ]]
```

## Calling

```rite
result ← square(12)
! @console.println(str(result))
```

Trailing-block call sugar exists for some forms (e.g. `keep { … }` in pipelines). **Match and if scrutinees** do not swallow a following block as a call argument — write `~ value ⟦ arms ⟧` / `match value [[ arms ]]` deliberately.

## Effects inside functions

If a function performs host I/O, mark those calls with `!` / `do`:

```rite browser
◆! greet(name) ⟦
  ! @console.println("hi " + name)
  ^ none
⟧

! greet("Aura")
```

Permission checks still apply at the host boundary.

## Public functions (modules)

```rite browser
// math.rite
pub ◆ square(value) ⟦
  ^ value * value
⟧
```

Only `pub` items are imported by `use` — see [Modules](modules.md).

## Documenting a function

A `///` comment directly above a declaration is its documentation. A `//!` comment at the
top of the file documents the file itself.

```rite browser
//! Geometry helpers.

/// Area of a circle.
/// @param radius Distance from the centre to the edge.
/// @returns The area, as a float.
pub ◆ circle_area(radius) ⟦
  ^ 3.14159 * radius * radius
⟧
```

An ordinary `//` comment is *not* documentation — it stays in the source and never
reaches the generated page.

### Tags

| Tag | Means |
|-----|-------|
| `@param <name> <text>` | Describes one parameter — repeat per parameter |
| `@returns <text>` | Describes the return value |
| `@effects <perm>` | Notes an effect the function performs, e.g. `fs:read` |
| `@permission <grant>` | Notes the grant it needs, e.g. `fs:read=./data` |

Anything untagged is prose. A fenced block inside the comment becomes a rendered example:

````rite
/// Loads configuration.
/// ```
/// load_config("app.toml")
/// ```
◆ load_config(path) ⟦
  ^ ! @fs.read(path)?
⟧
````

### Generating the pages

Point `rite doc` at a file or a directory:

```bash
rite doc src/                 # → docs/generated/scripts.md, index.json, html/
rite doc src/geo.rite --out build/docs
rite docs build --scripts src/    # language reference *and* your scripts
```

With no path you get the language reference alone. Undocumented declarations are left
out entirely, so the page lists what you chose to describe rather than every name in the
file. One unparseable script does not fail the build — the rest are still documented.

## Try it

```bash
rite run examples/hello/hello.rite --allow-all
```

## Next

[Pipelines](pipelines.md): chain transforms with `→` / `->`.
