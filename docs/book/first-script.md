# First script

This chapter gets you from empty file to printed output, then shows glyph vs ASCII and basic tooling.

## Hello

Create `hello.rite`:

```rite browser
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
# same thing (implicit run when the first arg is not a subcommand):
rite hello.rite
```

Console is allowed by default, so you usually do **not** need `--allow-all` for this script.

### The rest of `@console`

| Call | Goes to | For |
|---|---|---|
| `@console.println(v)` | stdout | Output, with a newline |
| `@console.print(v)` | stdout | Output, without one |
| `@console.warn(v)` | **stderr** | A note that is not the result |
| `@console.error(v)` | **stderr** | A problem that is not the result |
| `@console.inspect(v)` | stdout | Debugging, see below |
| `@console.read_line(prompt)` | reads stdin | Asking the user something |

The split matters the moment a script is used in a pipeline. `warn` and `error` go
to **stderr**, so they stay visible on the terminal while `stdout` is being
redirected into a file or piped into the next program:

```rite browser
! @console.println("the answer")
! @console.warn("using default config")
```

```bash
rite run t.rite > answer.txt      # the warning still reaches the terminal
```

Neither adds a prefix, a colour, or a severity label — they are the two streams,
nothing more. All six ride the same `console` permission, so `--deny console`
silences the lot.

`inspect` prints the runtime's **internal debug form**, not a value you would show
anyone:

```text
Some(Record({String("a"): Int(1), String("b"): List([Int(2), Int(3)])}))
```

That is the same record `println` would render as `⟨a: 1, b: [2, 3]⟩`. `inspect`
exists for the moment you need to know what the runtime thinks it is holding — the
`Some(…)` wrapper and the `String(…)` keys are the point, not noise. Reach for it
while debugging and take it out afterwards; the shape it prints is not a stable
format and is not meant to be parsed.

### Asking for input

```rite native_only
name ← ! @console.read_line("name? ")
! @console.println("hello, " + name)
```

```bash
printf 'aura\n' | rite run ask.rite
# name? hello, aura
```

The prompt is written without a trailing newline and flushed, so the cursor waits on
the same line. The line comes back **without its terminator** — `\n` or `\r\n`, so a
script does not behave differently depending on which terminal typed into it — and
end of input answers the empty string rather than failing. Reading is an ordinary
console effect, so `--deny console` stops it like the rest.

For arguments rather than answers, see [`@process.args`](processes.md).

### Executable scripts (shebang)

Put a shebang on line 1, then `chmod +x`:

```rite browser
#!/usr/bin/env rite
! @console.println("direct exec")
```

```bash
chmod +x hello.rite
./hello.rite
```

The kernel runs `rite /path/to/hello.rite`; Rite treats a non-subcommand first argument as `run`. The lexer ignores the `#!` line.

| Shebang | Notes |
|---------|--------|
| `#!/usr/bin/env rite` | **Recommended** — uses `rite` from your `PATH` (`~/.local/bin`, etc.) |
| `#!/usr/bin/env -S rite run --allow-all` | Explicit `run` + permissions (portable multi-arg form) |
| `#!/bin/rite` | Only works if the binary is literally at `/bin/rite` (unusual; the installer uses `~/.local/bin`) |

Permissions match `rite run`: grant host access on the shebang with `env -S` when needed.

ASCII form of the same program:

```rite browser
do host.console.println("hello, rite")
```

```bash
rite convert hello.rite --to ascii --stdout
```

## A slightly longer script

```rite browser
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

# Rewrite the file in ASCII keywords and operators
rite fmt --ascii hello.rite

# Print the ASCII form without touching the file
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

- Daily scratch work: [One-liners & REPL](one-liners.md)  
- Or dive into the [value model](values.md): numbers, atoms, lists, records, and truthiness.
