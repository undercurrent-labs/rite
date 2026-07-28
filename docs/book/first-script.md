# First script

This chapter gets you from empty file to printed output, then shows glyph vs ASCII and basic tooling.

## Hello

Create `hello.rite`:

```rite
! @console.println("hello, rite")
```

| Piece | Meaning |
|-------|---------|
| `!` | This statement is **effectful** (performs I/O). ASCII: `do` |
| `@console` | Host **capability** for stdout/stderr. ASCII: `host.console` |
| `println(...)` | Print a line |

Run:

```bash
rite run hello.rite
```

Console is allowed by default, so you usually do **not** need `--allow-all` for this script.

ASCII form of the same program:

```rite
do host.console.println("hello, rite")
```

```bash
rite fmt --ascii hello.rite --stdout
```

## A slightly longer script

```rite
// hello.rite
! @console.println("hello, rite")

name ← "Aura"
greeting ← "Welcome, " + name
! @console.println(greeting)

nums ← [1, 2, 3, 4, 5]
total ← nums → sum
! @console.println("sum = " + str(total))
```

What you just used:

- **Immutable bindings** with `←` (ASCII `<-`)
- **String concatenation** with `+`
- A **list** and a **pipeline** into `sum`
- **`str(...)`** to turn a number into a string for printing

Run:

```bash
rite run hello.rite
```

Example output:

```text
hello, rite
Welcome, Aura
sum = 15
```

The same script lives in the repo as `examples/hello/hello.rite` (glyph) and `examples/hello/hello.ascii.rite`.

## Check before you run

```bash
rite check hello.rite
```

Reports parse and resolve problems without executing host calls. Useful in CI and editors.

## Format and dialects

Rite has one AST and two surface skins. Formatting **normalizes** layout; convert **changes dialect**.

```bash
# Pretty-print, keeping/preferring glyph forms
rite fmt hello.rite

# Emit ASCII keywords and operators
rite fmt --ascii hello.rite --stdout

# Convert explicitly
rite convert hello.rite --to ascii --stdout
rite convert hello.rite --to glyph --stdout
```

| Glyph | ASCII |
|-------|-------|
| `←` | `<-` |
| `→` | `->` |
| `!` | `do` |
| `@console` | `host.console` |
| `◆ name(args) ⟦ … ⟧` | `def name(args) [[ … ]]` |

Studio (browser) can format and convert without installing — open [Studio](/studio), paste a snippet, click **Format**.

## Comments and files

- Line comments: `// …`
- File extension: **`.rite`** by convention
- Scripts are free-standing files; no mandatory `main` wrapper for simple top-level statements

## REPL

```bash
rite repl
```

Type expressions and statements interactively. Good for trying pipelines and match arms without saving a file.

## Common mistakes

1. **Forgetting `!` / `do` on host calls** — pure calls may be allowed in some positions, but statement-level I/O is marked effectful.
2. **Printing a non-string without `str`** — use `str(value)` or rely on `println`’s display of structured values (records/lists print inspect-style).
3. **Expecting FS/network without permissions** — see [Effects](effects.md).

## Next

Learn the [value model](values.md): numbers, atoms, lists, records, and truthiness.
