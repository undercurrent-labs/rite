# One-liners

Cant is meant to be typed. This page is a field guide: recipes short enough to
put in a shell, and the two or three things that surprise people the first time.

Every program here runs as written. Try any of them in
[Studio](https://cant.rite.foo/studio) without installing anything.

## The shape

```bash
cant -e '<program>'
```

`-e` runs the expression and prints the result. Quote it — `>`, `|`, `!`, `?`
and `*` are shell metacharacters, and Cant is not bent out of shape to avoid
them. Single quotes on bash, zsh and PowerShell alike.

Files and standard input work the same way:

```bash
cant run report.cant
cat report.cant | cant run -
```

## Lists

Scatter, do something per item, collect:

```cant
[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []
```

```text
[2, 4, 6]
```

Square each element:

```cant
[1, 2, 3] -> * -> $ * $ -> []
```

```text
[1, 4, 9]
```

A whole-list function needs no scatter, because a list is one emission:

```cant
[1, 2, 3] -> sum
```

```text
6
```

## Text

Split, transform, collect:

```cant
"a,b,c" -> split($, ",") -> * -> upper -> []
```

```text
[A, B, C]
```

Keep the lines longer than four characters:

```cant
lines("alpha\nbeta") -> * -> ?{ count($) > 4 } -> []
```

```text
[alpha]
```

Count the words in a string:

```cant
"hello world" -> words -> count
```

```text
2
```

## Files

Reading is an effect, so it carries `!` and needs a grant:

```bash
cant -e '"notes.txt" -> !@fs.read -> lines -> count' --allow fs:read=.
```

Non-empty lines:

```cant
"notes.txt" -> !@fs.read -> lines -> * -> ?{ $ != "" } -> []
```

A field out of a JSON file:

```cant
"data.json" -> !@fs.read -> @json.decode -> .name
```

The permission model is Rite's, unchanged — `--allow`, `--deny`, and the same
grammar for both. There is no second thing to learn.

## Around the system

```cant
"HOME" -> !@env.get
```

```cant
"https://example.com" -> !@http.get -> .status
```

Both need a grant: `--allow env` and `--allow net=example.com`.

## Several answers at once

A fork runs each branch from the same input and concatenates what they emit, in
order:

```cant
[1, 2, 3] -> |{ sum ; count } -> []
```

```text
[6, 3]
```

## Repeating until nothing is new

An orbit walks breadth-first and stops when the worklist empties — or when
`:max` is reached, whichever comes first:

```cant
[1] -> * -> ~{ ?{ $ < 100 } -> $ * 2 } :max 32 -> []
```

```text
[1, 2, 4, 8, 16, 32, 64, 128]
```

The ward inside decides what goes back on the worklist: 128 is emitted, and then
`128 < 100` is false, so nothing follows it. Orbit is the only cyclic construct
in the language and it is always bounded.

## Three things that surprise people

**A list is one emission.** `*` is always written, so this counts the list, not
its elements:

```cant
[1, 2, 3] -> count
```

```text
3
```

**Collect wraps whatever is in flight.** `sort` takes a list and returns a list —
one emission — so collecting after it gives a list containing one list:

```cant
[3, 1, 2] -> sort -> []
```

```text
[[1, 2, 3]]
```

Drop the `[]` if you wanted the sorted list itself. `[]` is for gathering
*several* emissions back into one value.

**`*` is scatter only when it is the whole stage.** Inside an expression it is
still multiplication, which is why `$ * 2` means what it looks like:

```cant
[1, 2, 3] -> * -> $ * 2 -> []
```

```text
[2, 4, 6]
```

## Seeing what a one-liner does

Three ways to look at a program before trusting it:

```bash
cant graph -e '[1, 2] -> * -> []'       # the topology, as JSON or DOT
cant expand -e '[1, 2] -> * -> []'      # the Rite it becomes, which is what runs
cant explain -e '[1, 2] -> * -> []'     # the same thing in prose
```

`cant explain` also lists the capabilities a program needs, which is the quickest
way to answer "what will this touch?" before granting anything.

## Exit codes

Cant uses Rite's, so `$?` means the same thing it does after `rite run`:

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error |
| 2 | usage error |
| 3 | parse error |
| 4 | resolve error |
| 5 | permission denied |
| 8 | budget exhausted |

Useful in a script:

```bash
if cant check -e "$PROGRAM" >/dev/null 2>&1; then
  cant -e "$PROGRAM"
fi
```

See [the command line](cli.md) for the full flag list and
[the language](language.md) for what the operators mean.
