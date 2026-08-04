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

`..` is the canonical glyph in both dialects; `...` is accepted and `rite fmt`
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

### Working on strings

```rite browser
parts ← "name, age , city" → split(",") → map { |p| trim(p) }
! @console.println(str(parts))

! @console.println(upper("héllo") + " / " + lower("HÉLLO"))
! @console.println(replace("a-b-c", "-", "+"))
! @console.println(pad_start("7", 3, "0"))
```

| Call | Answers |
|------|---------|
| `split(s, sep)` | a list; an empty or missing separator splits into characters |
| `trim(s)` · `trim_start` · `trim_end` | whitespace removed |
| `replace(s, from, to)` | every occurrence replaced |
| `starts_with(s, part)` · `ends_with` | `true` / `false` |
| `upper(s)` · `lower(s)` | case changed |
| `pad_start(s, width, fill)` · `pad_end` | padded to `width`, unchanged if already longer |
| `slice(s, start)` · `slice(s, start, end)` | a substring, end exclusive |
| `index_of(s, part)` | the position, or **`none`** when absent |
| `count(s)` | how many characters |
| `lines(s)` · `words(s)` · `join(xs, sep)` | split and rejoin |
| `take(s, n)` · `drop(s, n)` · `first(s)` · `last(s)` · `rest` · `init` | as for lists, answering a string |
| `reverse(s)` · `sort(s)` · `unique(s)` · `chunk(s, n)` | the sequence family, by character |

**Everything counts characters, not bytes.** `count("δ")` is `1`, and `slice`,
`index_of`, `pad_*`, `take` and `drop` agree with it — an API that counted
characters in one place and bytes in another would only go wrong on non-ASCII
input, which is the worst time to find out.

Indices may be negative to count from the end, and out-of-range values clamp
rather than fail, so `slice` is safe on input you did not choose:

```rite browser
! @console.println(slice("abcdef", -2))
! @console.println("[" + slice("abc", 5, 9) + "]")
```

`index_of` answers `none` rather than `-1` — a sentinel that is also a valid
index is how off-by-one bugs get written. Pair it with `??`:

```rite browser
at ← index_of("hello", "z") ?? (0 - 1)
! @console.println(str(at))
```

### Numbers

```rite browser
! @console.println(str(round(2.5)) + " " + str(floor(2.9)) + " " + str(ceil(2.1)))
! @console.println(str(sqrt(16)))
```

`round`, `floor` and `ceil` answer with an **int**, since that is what you wanted
one for; `round` goes half away from zero, so `round(-0.5)` is `-1`. Also
available: `abs`, `clamp`, `pow`, `idiv`, `min`, `max`, `sum`.

Parsing untrusted text answers with a **Result**, so `?` handles it like anything
else that can fail:

```rite browser
n ← parse_int("41")?
! @console.println(str(n + 1))
```

```rite browser
! @console.println(str(parse_int("nope")))
```

### Bytes

Some things are not text: a datagram, a file read with `@fs.read_bytes`, an HTTP
body. Those are **bytes**, and they are built and inspected with their own set
rather than by pretending they are strings.

```rite browser
packet ← concat(from_hex("abcd0100")?, bytes([0, 1, 255]))
! @console.println(to_hex(packet))
! @console.println("first byte " + str(byte_at(packet, 0)))
! @console.println(str(count(packet)) + " bytes")
```

| Call | Answers |
|------|---------|
| `from_hex(s)` | a **Result** of bytes — any byte, not only text-safe ones |
| `bytes(x)` | bytes from a list of `0`–`255`, from a string's UTF-8, or bytes unchanged |
| `to_hex(b)` | the hex spelling |
| `to_text(b)` | a **Result** of a string — bytes are not always text |
| `byte_at(b, i)` | the byte as a number, or `none` past the end; negative counts from the end |
| `concat` · `slice` · `count` | as for lists and strings, staying bytes |
| `take` · `drop` · `rest` · `init` · `reverse` · `chunk` | the sequence family, staying bytes |
| `first` · `last` · `index_of` | a single byte as a number; `index_of` takes one too |

`count` measures **bytes** here, not characters — `count(bytes("é"))` is `2` while
`count("é")` is `1`. That is the distinction the type exists to make.

Both conversions admit when they cannot: `from_hex` rejects odd lengths and
non-hex digits, and `to_text` rejects bytes that are not valid UTF-8, rather than
substituting replacement characters. Out-of-range numbers are refused rather than
truncated, since a silently wrapped `0x1ff` is a packet that goes out wrong and
gets debugged at the far end.

**`@crypto.hex_decode` is not this.** It answers a *string* and rejects anything
that is not valid UTF-8, which is right for decoding text that happens to be hex
encoded, and useless for a DNS header. Use `from_hex` for bytes.

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
