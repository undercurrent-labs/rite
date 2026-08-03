# Cant examples

One directory per construct. Every `main.cant` is a complete program; the
`README.md` beside it says what it does and what it evaluates to.

Cant is **not** a Rite dialect — it is a sibling language that lowers to
canonical ASCII Rite and runs on Rite's runtime. See
[`docs/adr/0001-cant-sibling-frontend.md`](../../docs/adr/0001-cant-sibling-frontend.md).

## Running them

Not yet. Phase 1 implements the parser; `cant run` arrives in Phase 5 (see
[`docs/cant/checklist.md`](../../docs/cant/checklist.md)). Until then:

```bash
cant check examples/cant/01-flow/main.cant
cant parse --structure examples/cant/01-flow/main.cant
```

Every example here parses cleanly, and a test asserts it.

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
