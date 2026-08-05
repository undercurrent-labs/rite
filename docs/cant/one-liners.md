# One-liners

Recipes short enough to put in a shell, plus the two or three things that catch
people out the first time.

Every program here runs as written. Try any of them in
[Studio](https://cant.rite.foo/studio) without installing anything.

## The shape

```bash
cant -e '<program>'
```

`-e` runs the expression and prints the result. Quote it: `>`, `|`, `!`, `?`
and `*` are shell metacharacters. Single quotes work on bash, zsh and PowerShell
alike.

Files and standard input work the same way:

```bash
cant run report.cant
cat report.cant | cant run -
```

## Lists

Scatter, do something per item, collect:

```cant run
[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []
```

```text
[2, 4, 6]
```

Square each element:

```cant run
[1, 2, 3] -> * -> $ * $ -> []
```

```text
[1, 4, 9]
```

A whole-list function needs no scatter, because a list is one emission:

```cant run
[1, 2, 3] -> sum
```

```text
6
```

## Text

Split, transform, collect:

```cant run
"a,b,c" -> split($, ",") -> * -> upper -> []
```

```text
[A, B, C]
```

Keep the lines longer than four characters:

```cant run
lines("alpha\nbeta") -> * -> ?{ count($) > 4 } -> []
```

```text
[alpha]
```

Count the words in a string:

```cant run
"hello world" -> words -> count
```

```text
2
```

## Files

Reading is an effect, so it carries `!` and needs a grant:

```bash
cant -e '"notes.txt" -> !@fs.read? -> lines -> count' --allow fs:read=.
```

The `?` matters. `@fs.read` answers a *result* and `lines` wants a string, so
the program fails without it.

Non-empty lines:

```cant
"notes.txt" -> !@fs.read? -> lines -> * -> ?{ $ != "" } -> []
```

A field out of a JSON file:

```cant
"data.json" -> !@fs.read? -> @json.decode? -> .name
```

Two capabilities, two results, two `?`s. Drop the second and `.name` projects a
field out of an `ok(…)`, finds nothing, and answers `none`.

The permission model is Rite's, unchanged: `--allow`, `--deny`, and the same
grammar for both.

## Around the system

```cant
"HOME" -> !@env.get
```

```cant
"https://example.com" -> !@http.get -> .status
```

Both need a grant: `--allow env` and `--allow net=example.com`.

A `.env` file is easier than naming every variable — `--env-file` grants reading
exactly the names the file defines, and nothing else:

```bash
cant --env-file .env -e '"API_KEY" -> !@env.get'
```

`@sys` is where you are rather than what you were configured with:

```cant
!@sys.cwd
```

```bash
cant --allow sys -e '!@sys.cwd'
```

`cwd`, `home`, `temp_dir`, `os`, `arch`, `pid` and `hostname`, all under
`--allow sys`.

## Pipes

The data on the pipe, the program on `-e`, in the shape `awk`, `sed` and `jq`
use. `!@stdin.lines` is the input as a list of lines; `!@stdin.read` is the whole
of it as one string.

Count the lines that mention an error:

```bash
cat access.log | cant -e '!@stdin.lines -> * -> ?{ contains($, "500") } -> [] -> count'
```

Sum the first column:

```bash
cat sizes.txt | cant -e '!@stdin.lines -> * -> parse_int(first(words($)))? -> [] -> sum'
```

Pluck a field out of JSON, `jq`-style:

```bash
curl -s https://api.example.com/things | cant -e '!@stdin.read -> @json.decode? -> .items -> * -> .name -> []' --allow net=api.example.com
```

(The `curl` is doing the fetching there; drop it and let Cant fetch with
`!@http.get` if you prefer one process.)

The two most frequent words:

```bash
cat speech.txt | cant -e '!@stdin.read -> words -> frequencies -> take($, 2)'
```

```text
[[the, 3], [and, 2]]
```

`frequencies` answers `[value, count]` pairs, most frequent first. The long
spelling still works: `group`, a stage rewriting each bucket to `[count, word]`,
structural `sort`, then `reverse`. When the key is not the value itself,
`sort_by`, `min_by` and `max_by` take a key function or a field name.

Pull a number out of every line that has one, and sum them:

```bash
cat timings.log | cant -e '!@stdin.lines -> * -> nth(@regex.captures($, "took ([0-9]+)ms")? ?? [], 1) -> ?{ $ != none } -> parse_int($)? -> [] -> sum'
```

`@regex` is pure: no `!`, no permission. A pattern that does not compile is an
`err` value, so the usual postures apply. `captures` answers the whole match
first and each group after it; `nth` reaches into the pair, `?? []` covers the
lines that did not match, and the ward drops them.

Columns from a CSV:

```bash
cat report.csv | cant -e '!@stdin.read -> @csv.decode? -> * -> .total -> []'
```

An empty pipe is an empty list, so every recipe above answers its zero (`0`,
`none`, `[]`) rather than hanging or failing. The value goes to stdout with
nothing else mixed in, so a Cant one-liner composes with the next tool in the
pipeline, including `cant` itself.

## Several answers at once

A fork runs each branch from the same input and concatenates what they emit, in
order:

```cant run
[1, 2, 3] -> |{ sum ; count } -> []
```

```text
[6, 3]
```

## Repeating until nothing is new

An orbit walks breadth-first and stops when the worklist empties — or when
`:max` is reached, whichever comes first:

```cant run
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

```cant run
[1, 2, 3] -> count
```

```text
3
```

**Collect wraps whatever is in flight.** `sort` takes a list and returns a list —
one emission — so collecting after it gives a list containing one list:

```cant run
[3, 1, 2] -> sort -> []
```

```text
[[1, 2, 3]]
```

Drop the `[]` if you wanted the sorted list itself; `[]` gathers *several*
emissions back into one value.

**`*` is scatter only when it is the whole stage.** Inside an expression it is
still multiplication, which is why `$ * 2` means what it looks like:

```cant run
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

[Your first program](tutorial.md) is the introduction, and
[past the one-liner](projects.md) covers files, modules, tests and binaries. See
[the command line](cli.md) for the full flag list, [the language](language.md)
for what the operators mean, and [when something goes wrong](diagnostics.md) for
the diagnostics.
