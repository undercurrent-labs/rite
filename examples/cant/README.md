# Cant examples

One directory per construct. Every `main.cant` is a complete program; the
`README.md` beside it says what it does and what it evaluates to.

Cant is **not** a Rite dialect — it is a sibling language that lowers to
canonical ASCII Rite and runs on Rite's runtime. See
[`docs/adr/0001-cant-sibling-frontend.md`](../../docs/adr/0001-cant-sibling-frontend.md).

## Running them

```bash
cant run examples/cant/01-flow/main.cant
cant run examples/cant/06-capabilities/main.cant --allow fs:read=examples/cant/06-capabilities
```

Every example runs — the documentation gate executes each `main.cant` and
requires it to succeed, so an example that stops working stops the build.
`cant check` and `cant parse --structure` work on all of them too.

## Quoting

Cant's operators — `>`, `|`, `!`, `?`, `*` — are shell metacharacters. Quote a
`-e` expression, as you would for `awk`, `sed`, or `jq`:

```bash
cant check -e '[1, 2, 3] -> * -> ?{ $ > 1 } -> []'
```

Unquoted one-liners are not portable and are not claimed to be.

## Glyphs are optional

Every example is written in ASCII, which is the canonical form. The glyph
spellings (`→ ⋇ ⌁ ⊣⟦⟧ ⫴⟦⟧ ⟲⟦⟧`) are accepted on input and produce the same
program; `conformance/cant/dialect/` holds the pairs that prove it. You never
need to type one.
