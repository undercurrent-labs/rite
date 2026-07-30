# Bindings and immutability

Rite distinguishes **immutable** bindings, **mutable** bindings, and **assignment** to existing mutables. That keeps most script data stable while still allowing loops and counters.

## Forms

| Glyph | ASCII | Meaning |
|-------|-------|---------|
| `name ← expr` | `name <- expr` | Bind **immutable** name |
| `name ↢ expr` | `name <~ expr` | Bind **mutable** name |
| `name := expr` | `name := expr` | Assign to an existing **mutable** |

```rite
// Immutable — default choice
x ← 1
// x := 2          // error: x is not mutable

// Mutable counter
c ↢ 0
c := c + 1
c := c + 1
! @console.println(c)   // 2
```

## Prefer immutable

Use `←` / `<-` unless you need to update a name in place:

```rite
name ← "Aura"
greeting ← "hi, " + name
// shadowing a new immutable is fine:
name ← name + "!"
```

Re-binding with `←` in an inner block is normal **shadowing** (inner scope), not mutation of the outer binding.

## Mutable patterns

### Running totals

```rite
total ↢ 0
xs ← [1, 2, 3, 4]
// pipelines often avoid mutation entirely — see next chapters
// but imperative style is available:
i ↢ 0
// ... use := in loops when you add them to scripts
```

For list processing, prefer [pipelines](pipelines.md) (`→ sum`, `→ map`) over manual mutation.

### Accumulating in a function

```rite
◆ countdown(n) ⟦
  c ↢ n
  // body uses c := c - 1 in a loop-like structure when needed
  ^ c
⟧
```

## Scope

- Bindings are **block-scoped** (`⟦ ⟧` / `[[ ]]`, function bodies, match arms).
- Function parameters are immutable bindings for the body.
- Host results are often bound immutably: `text ← ! @fs.read(path)?`

## Shadowing vs assign

```rite
x ← 1
◆ demo() ⟦
  x ← 2              // shadows outer x inside demo
  ! @console.println(x)
⟧
! demo()
! @console.println(x)  // still 1
```

```rite
x ↢ 1
◆ bump() ⟦
  // cannot assign outer x unless you design for shared mutables;
  // prefer returning new values
  ^ x + 1
⟧
```

## Destructuring (preview)

Pattern matching can bind names inside arms:

```rite
pair ← [10, 20]
head ← ~ pair ⟦
  [h, ..rest] → h
  _ → 0
⟧
! @console.println(head)  // 10
```

Full detail in [Pattern matching](matching.md).

## Style guide

1. **Immutable by default** (`←`).
2. **Mutable** (`↢` + `:=`) for counters, state machines, game loops.
3. **Return new data** from functions instead of mutating caller state when possible.
4. Use **pipelines** for collection transforms instead of hand-rolled indexes.

## Next

[Functions](functions.md): `◆` / `def`, `^` / `return`, and closures.
