# Your first Cant program

Cant is a small language built on one idea. This page starts from a single
value and works up to it.

Every program here runs as written. Type them into a shell, or into
[Studio](https://cant.rite.foo/studio) if you would rather not install
anything.

## A program is a chain of stages

```bash
$ cant -e '"cant" -> upper'
CANT
```

`->` is the flow arrow. The value on the left goes into the stage on the right,
and the answer is printed. A stage is ordinary [Rite](https://rite.foo)
expression text, so `upper` is Rite's `upper`, and the incoming value lands in
its first argument.

Chain as many as you like:

```cant run
"  hello  " -> trim -> upper -> count
```

```text
5
```

Each stage runs on what the one before it produced.

## The one idea: emissions

**A stage does not return a value. It emits zero or more.**

Most stages emit exactly one, which is why the programs above read like a
pipeline. Some emit none and some emit many, and when a stage emits three
values the next stage runs three times.

That is the model. Five operators change how many values are in flight; the
rest of the language is Rite.

## Scatter: one becomes many

`*` takes a list and emits its elements, one at a time:

```cant run
[1, 2, 3] -> * -> $ * $
```

```text
[1, 4, 9]
```

Read it as three separate runs of `$ * $`, with `1`, then `2`, then `3`. It is
not a map over a list. `$` is the current emission, and it is how you say where
the value goes when it does not belong in the first argument.

Without the `*`, a list is one value like any other:

```cant run
[1, 2, 3] -> count
```

```text
3
```

That counts the list. `*` is always written, so a missing one is the first
thing to check when a program does something unexpected.

## Collect: many become one

At the end of a program, whatever is in flight is gathered for you: nothing
becomes `none`, one value stays itself, several become a list. That is why
`[1, 2, 3] -> * -> $ * $` printed a list.

`[]` does that gathering **in the middle**, so the rest of the program sees one
list again:

```cant run
[1, 2, 3, 4] -> * -> ?{ $ % 2 = 0 } -> $ * 10 -> [] -> sum
```

```text
60
```

Four emissions in, two survive the filter, each is multiplied, `[]` makes them
`[20, 40]`, and `sum` sees one list. Drop the `[]` and `sum` runs twice, once
per emission, summing a single number each time.

`*` and `[]` are inverses. Most Cant programs are a sandwich of them.

## Ward: many become fewer

`?{ … }` is a filter. The predicate runs with `$` bound to the incoming value;
a truthy answer emits the input **unchanged**, and a falsey one emits nothing:

```cant run
["alpha", "be", "gamma"] -> * -> ?{ count($) > 3 } -> upper -> []
```

```text
[ALPHA, GAMMA]
```

A ward never transforms; use the next stage for that. Filtering and
transforming stay separate so you can tell at a glance which stages do which.

## Look before you run

Three commands show you a program without executing it:

```bash
cant explain -e '[1, 2] -> * -> ?{ $ > 1 } -> []'   # what it does, in prose
cant graph   -e '[1, 2] -> * -> ?{ $ > 1 } -> []'   # its topology, as DOT or JSON
cant expand  -e '[1, 2] -> * -> ?{ $ > 1 } -> []'   # the Rite it becomes
```

Reach for `cant explain` first. It is read from the same graph that executes,
so it cannot describe a different program from the one you wrote, and it lists
the capabilities the program needs before you grant anything.

`cant expand` shows the generated Rite. Cant compiles to ordinary Rite and runs
on Rite's runtime; there is no second interpreter.

## Fork: several answers from one value

`|{ a ; b }` runs each branch on the *same* input and concatenates what they
emit, left to right:

```cant run
5 -> |{ $ + 1 ; $ * 2 }
```

```text
[6, 10]
```

Branches are ordinary flows, so each can be several stages long. They run in
order, and so does anything effectful inside them.

## Orbit: keep going until nothing is new

`~{ … }` is the only loop in the language, and it is always bounded. It keeps a
worklist, runs the body on each candidate it has not seen before, and puts what
the body emits back on the end of the queue:

```cant run
[1] -> * -> ~{ ?{ $ < 20 } -> $ * 3 } -> []
```

```text
[1, 3, 9, 27]
```

Follow it: `1` is new, so it is emitted and the body runs. `1 < 20`, so `3`
goes on the queue. Same for `3` → `9` and `9` → `27`. Then `27` is emitted, but
`27 < 20` is false, so the body emits nothing, the queue empties, and the orbit
stops.

Note that `27` **is** in the answer. The ward controls what goes back on the
worklist, not what comes out.

An orbit stops for one of two reasons: the worklist emptied, or it hit `:max`
(1024 by default). Hitting `:max` is a **failure**, not a short answer, so a
traversal larger than you expected does not return half a result. Raise it with
a modifier when the traversal really is that large:

```cant run
[1] -> * -> ~{ ?{ $ < 20 } -> $ * 3 } :max 4096 -> []
```

A modifier attaches to the form immediately on its left, with no arrow between.

## Touching the world

Reading a file is an effect, and Cant keeps Rite's rules for those exactly as
they are: `!` marks the call, `@` names the capability, and the run needs a
grant.

```bash
$ echo 'alpha
beta' > notes.txt
$ cant -e '"notes.txt" -> !@fs.read? -> lines -> count' --allow fs:read=.
2
```

Without `--allow fs:read=.` that program stops with a permission error and
exit code 5. Only console, clock, randomness and standard input are allowed by
default; everything else has to be granted. `cant explain` lists what a program
will ask for.

The `?` matters. A capability answers a **result**, `ok(v)` or `err(…)`, and
`lines` wants a string, so the program fails without it. Cant does not unwrap
anything for you: a stage is Rite expression text, and Rite's rules apply
inside it.

One `?` per capability, then:

```bash
cant -e '"data.json" -> !@fs.read? -> @json.decode? -> .name' --allow fs:read=.
```

Miss one and the failure is quiet: `.name` projects a field out of an `ok(…)`,
finds nothing, and answers `none`. `?` stops the run on an `err`, which is the
posture to want when a missing file means the program cannot proceed.
[The language reference](language.md#failures) has the other two: dropping
failures, and replacing them with a fallback.

## Standard input

`@stdin` takes the data on the pipe, with the program on `-e`.

```bash
cat access.log | cant -e '!@stdin.lines -> * -> ?{ contains($, "500") } -> [] -> count'
```

`!@stdin.lines` is the input as a list of lines, `!@stdin.read` the whole of it
as one string. An empty pipe is an empty list, so the flow runs zero times
instead of once over `""`, and the program answers `0`.

## From a one-liner to a file

Put the same text in a file and run it:

```bash
$ cat > evens.cant <<'EOF'
// Even numbers, ten times bigger.
[1, 2, 3, 4, 5, 6]
  -> *
  -> ?{ $ % 2 = 0 }
  -> $ * 10
  -> []
EOF
$ cant run evens.cant
[20, 40, 60]
```

A `.cant` file is one flow: no declarations, no statements, no `main`. It can
span as many lines as it likes. `cant fmt` lays it out, and `cant fmt --compact`
folds it back onto one line for pasting into a shell.

That covers the language. [Past the one-liner](projects.md) picks up from here:
named functions, configuration, tests, and compiling to a binary.

## Where to go next

- [Past the one-liner](projects.md) — modules, configuration, tests, binaries,
  and the REPL
- [One-liners](one-liners.md) — a field guide of recipes, and the three things
  that surprise everybody
- [The language](language.md) — the complete reference for every operator
- [When something goes wrong](diagnostics.md) — what each diagnostic means
- [`examples/cant/`](../../examples/cant/) — one directory per construct, each
  with a short explanation

## The whole vocabulary, on one screen

| | | |
|---|---|---|
| `->` | flow | send each emission to the next stage |
| `$` | current value | where the emission goes in a stage |
| `*` | scatter | a list becomes one emission per element |
| `[]` | collect | the emissions so far become one list |
| `?{ p }` | ward | emit the input unchanged, or nothing |
| `\|{ a ; b }` | fork | ordered branches from the same input |
| `~{ b }` | orbit | bounded breadth-first fixed point |
| `:name v` | modifier | configure the form on its left |
| `!` | effect | a host call, as in Rite |
| `@` | capability | a host namespace, as in Rite |

Each has a glyph twin you never have to type (`→ ⋇ ⌁ ⊣⟦⟧ ⫴⟦⟧ ⟲⟦⟧`).
`cant convert --to glyph` writes them for you.
