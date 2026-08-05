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

### No arguments is still a function

The `|…|` marks a block as a function, not the names inside it. Write `||`
for a function that takes nothing — a thunk, for deferring work rather than doing
it now:

```rite browser
answer ← ⟦ 6 * 7 ⟧          // a block: runs now, `answer` is 42
later ← { || 6 * 7 }        // a function: runs when called
! @console.println(str(answer))
! @console.println(str(later()))
! @console.println(type_of(later))
```

```text
42
42
function
```

The two look almost the same and mean quite different things, so write `||`
deliberately rather than let it be inferred from an empty list of
names.

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

## Type annotations

A parameter or a return can declare a type. Rite has no type *checker* — nothing is
verified before the program runs — but a declared type is enforced **at runtime**,
on entry and on exit:

```rite browser
◆ double(x: int) → int ⟦
  ^ x * 2
⟧

! @console.println(str(double(21)))
```

Break the contract and the call fails where the mistake is, rather than somewhere
downstream:

```rite
◆ double(x: int) → int ⟦
  ^ "twenty-one"
⟧

double(21)
```

> `double: declared to return int, but returned string`

The types are the value kinds — `int`, `float`, `number` (either), `string`, `bool`,
`atom`, `list`, `record`, `bytes`, `function`, `none` — plus `any`, and three
composites:

| Written | Means |
|---|---|
| `[int]` | a list whose every element is an `int` |
| `result<int>` | an `ok` carrying an `int` (an `err` carries a failure, so its payload is unconstrained) |
| `⟨name: string, age: int⟩` | a record with at least those fields, of those types |

Checking is **structural**, not nominal: a value fits when its shape does. An empty
list satisfies `[int]` because there is nothing in it that does not, and a record
may carry fields the annotation never mentions.

Annotations are optional and independent — annotate one parameter and leave the
rest, or declare a return type and no parameters. What is not annotated is not
constrained, and `any` says so explicitly:

```rite browser
◆ label(x: any) → string ⟦
  ^ "value: " + str(x)
⟧

! @console.println(label(#ok))
```

They cost a check per call, so they earn their place at the edges of a program —
where data arrives from a file, a request or a caller you do not control — more
than in a helper called in a loop.

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
