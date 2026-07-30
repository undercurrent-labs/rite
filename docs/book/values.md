# Values and atoms

Rite is dynamically typed at runtime. Values are simple, JSON-adjacent, and designed for scripting — not a full object system.

## Value kinds

| Kind | Examples | Notes |
|------|----------|--------|
| **none** | `none` | Absence; falsey |
| **bool** | `true`, `false` | Only `false` is falsey among bools |
| **int** | `0`, `42`, `-3` | Arbitrary-size intent; platform int in practice |
| **float** | `3.14` | IEEE float |
| **string** | `"hello"`, `"a\nb"` | UTF-8; `+` concatenates |
| **atom** | `#ok`, `#error`, `:ok` | Symbols / tags (glyph `#`, ASCII `:`) |
| **list** | `[1, 2, 3]` | Ordered sequence |
| **record** | `⟨a: 1, b: 2⟩` | String/atom keys → values |
| **function** | `◆ f(x) ⟦ ^ x ⟧` | Closures and defs |
| **result** | `ok(v)`, `err(e)` | Success / failure (see [Results](results.md)) |

```rite browser
name ← "Aura"
score ← 42
pi ← 3.14
tags ← [#demo, #v1]
rec ← ⟨name: name, score: score, tags: tags, active: true⟩

! @console.println(rec)
! @console.println(tags)
```

ASCII records use `<< >>`:

```rite
rec <- <<name: name, score: score>>
```

### Spreading one record into another

`..other` pours an existing record into the one being built. Entries flow left to
right and **later ones win**, so a spread reads as "start from this, then override":

```rite browser
base ← ⟨host: "localhost", port: 80, tls: false⟩
prod ← ⟨..base, port: 443, tls: true⟩
// ⟨host: "localhost", port: 443, tls: true⟩
```

Spread anywhere, as often as you like — `⟨..a, ..b⟩`, `⟨k: 1, ..a⟩`, `⟨..a⟩`. A key
keeps the position where it **first** appears and the value from the **last** write, so
`⟨port: 1, ..base⟩` is `⟨port: 80⟩`: `port` stays first, but `base` wins the value.

This is exactly the `+` merge operator wearing different clothes — one merge rule in
the language, not two:

```rite
⟨..base, ..over⟩ = base + over    // true, always
```

`..` is the canonical sigil in both dialects; `...` is accepted and `rite fmt`
normalises it to `..`.

## Atoms

Atoms are lightweight symbolic values — great for statuses, enums, and match tags.

```rite
status ← #ok
# same idea in ASCII:
// status <- :ok
```

Compare with `=` / `!=`. Match on them with `~` / `match` (next chapters).

## Strings

```rite browser
a ← "hello"
b ← " " + "world"
! @console.println(a + b)
! @console.println(str(99))
```

- `+` on strings concatenates
- `str(x)` stringifies other values for embedding in messages

### Interpolation

`{name}` inside a double-quoted string is replaced by that binding's value:

```rite browser
name ← "Aura"
n ← 3
! @console.println("hi {name}, you have {n}")
```

A record field works too — `"{user.name}"`. Anything more involved is clearer built with
`+` and `str(…)`.

### Escapes

| Escape | Means |
|--------|-------|
| `\n` `\t` `\r` `\0` | Newline, tab, carriage return, NUL |
| `\\` | A backslash |
| `\"` | A double quote |
| `\u{1F600}` | A Unicode code point |
| `\{` `\}` | A **literal brace** — not an interpolation hole |

A doubled brace means the same thing: `{{` produces one `{`, so `"{{ mustache }}"` is the
text `{ mustache }`. `rite fmt` prints the doubled spelling, so a `\{` you wrote comes
back as `{{`; both mean a literal brace and both re-read identically.

```rite browser
! @console.println("literal \{name} stays as written")
```

### Three kinds of string literal

| Form | Interpolates | Notes |
|------|--------------|-------|
| `"…"` | yes | Escapes as above |
| triple-quoted | yes | Multi-line; common leading indentation is removed |
| `r"…"` | **no** | Raw: every character is literal, including `{` and `\` |

Use a raw string for anything full of braces or backslashes that should not be touched —
a regex, a Windows path, a template:

```rite browser
pattern ← r"\d+"
tpl ← r"{name} is not substituted here"
```

## Lists

```rite browser
xs ← [1, 2, 3]
ys ← ["a", "b"]
empty ← []
```

Lists are the workhorse of [pipelines](pipelines.md): `map`, `keep`, `sum`, `count`, …

## Records

```rite browser
user ← ⟨
  id: 1,
  name: "Aura",
  tags: [#admin]
⟩

! @console.println(user.name)     // field access
! @console.println(user.missing)  // missing field → none
```

- Glyph: `⟨ key: value, … ⟩`
- ASCII: `<< key: value, … >>`
- Keys may be identifiers, strings, or atoms
- **Merge** with `+` (right-biased): `defaults + overrides`
- Dot access on a missing key yields **`none`**, not an error

## Conditionals and truthiness

Glyph conditional uses `?` (ASCII `if`):

```rite browser
score ← 42
label ← ? score > 0 ⟦ #ok ⟧ : ⟦ #nope ⟧
! @console.println(label)
```

### What is falsey?

| Value | Truthiness |
|-------|------------|
| `false` | falsey |
| `none` | falsey |
| `0`, `0.0` | **truthy** |
| `""` empty string | **truthy** |
| `[]` empty list | **truthy** |
| `⟨⟩` empty record | **truthy** |

Only **`false`** and **`none`** are falsey. That differs from JavaScript — empty collections do not fail an `if`.

## Equality and operators

Common operators:

- Arithmetic: `+ - * / %`
- Compare: `= != < <= > >=` (note: equality is `=`, not `==`)
- Logic: `and` / `or` / `not` (as implemented in the language surface)
- Membership: `∈` / `in`, `∉` / `not in`
- Coalesce: `??` — use right side if left is `none` (and similar absence cases)

```rite browser
x ← none
y ← x ?? 10
! @console.println(y)   // 10
```

## Display vs structure

`@console.println` shows a human-readable form of structured values. For string building, prefer `str(...)`.

```rite browser
! @console.println(⟨a: 1⟩)
! @console.println("a=" + str(1))
```

## Try it

```bash
rite run examples/01-values/main.rite --allow-all
```

Or paste into [Studio](/studio) and click **Run**.

## Next

[Bindings](bindings.md): immutable `←`, mutable `↢`, and `:=`.
