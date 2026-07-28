# Rite quick reference

## Dialects

| Glyph | ASCII |
|-------|-------|
| ◆ | def |
| ← | <- |
| ↢ | <~ |
| → | -> |
| ^ | return |
| ? | if |
| ~ | match |
| ! | do |
| @ | host. |
| #name | :name |
| ⟦⟧ | [[]] |
| ⟨⟩ | <<>> |
| ∈ | in |
| ∉ | not in |

## Commands

```bash
rite run FILE --allow-all
rite check FILE
rite fmt FILE --dialect glyph|ascii
rite convert FILE --to glyph|ascii
rite build FILE --allow-all
rite lsp / rite-lsp
rite studio --port 4041
rite describe language --json
```

## Effects

```rite
! @console.println("hi")   // required for effectful host calls
data ← @json.decode(text)? // pure-ish ops; ? for results
```

## Truthiness

Only `false` and `none` are falsey.
