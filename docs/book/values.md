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

```rite
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

```rite
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

```rite
a ← "hello"
b ← " " + "world"
! @console.println(a + b)
! @console.println(str(99))
```

- `+` on strings concatenates
- `str(x)` stringifies other values for embedding in messages

## Lists

```rite
xs ← [1, 2, 3]
ys ← ["a", "b"]
empty ← []
```

Lists are the workhorse of [pipelines](pipelines.md): `map`, `keep`, `sum`, `count`, …

## Records

```rite
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

```rite
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

```rite
x ← none
y ← x ?? 10
! @console.println(y)   // 10
```

## Display vs structure

`@console.println` shows a human-readable form of structured values. For string building, prefer `str(...)`.

```rite
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
