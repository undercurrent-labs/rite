# `cant` — command line

## Sources

Every command that takes a source accepts exactly one of three forms:

```bash
cant check program.cant           # a file
cant check -                      # standard input
cant check -e '[1, 2] -> * -> []'  # an expression
```

Passing a file *and* `-e` is a usage error (exit 2) rather than a silent
precedence rule: which one won would be invisible in a script until it mattered.

### Quoting

Cant's operators — `>`, `|`, `!`, `?`, `*` — are shell metacharacters, and the
language is not deformed to avoid them. Quote the expression:

```bash
cant check -e '["a.txt"] -> * -> !@fs.read -> lines -> * -> ?{ $ != "" } -> []'
```

Single quotes on bash and zsh. On PowerShell, single quotes also work; double
quotes do not, because PowerShell expands `$` inside them and `$` is Cant's
current-value operator.

```powershell
cant check -e '[1, 2] -> * -> ?{ $ > 0 } -> []'
```

Unquoted one-liners are not portable and are not claimed to be. This is the same
trade `awk`, `sed` and `jq` make.

## Commands

### `cant version`

```bash
$ cant version
cant 0.1.0
cant_language_version: 0
cant_graph_schema_version: 0
rite: 0.7.0
```

Four numbers because they move independently: the tool, the language it
implements, the graph JSON schema a consumer may have stored, and the Rite that
expansion targets. `--json` emits the same as an object.

**Cant's version is not Rite's.** `cant` ships inside the Rite release archive,
but it is a v0 language on its own number — the release tag you downloaded is
Rite's. Both are in the release's `version-manifest.json` if you need to know
what an archive contains without unpacking it.

### `cant update`

There isn't one. `cant` comes with `rite`, and

```bash
rite update
```

replaces every binary in the archive, so the pair cannot drift on your machine.
Running `cant update` prints that and exits 2.

### `cant check [source]`

Parses and reports diagnostics. Exit 0 and `ok` when the source is clean.

```bash
$ cant check -e '[1, 2] -> ~{ deps'
error[CANT-P003]: unclosed `~{`

  --> <expr>:1:10
   |
   1 | [1, 2] -> ~{ deps
   |           ^^
   |           opened here, never closed
   |              ---- reached this

help: close it with `}`, or `⟧` if the block was opened with a glyph
```

`--json-errors` writes the diagnostics to stdout as JSON instead, carrying the
Cant code, severity, spans, notes and help — and, once expansion lands, the
underlying Rite code and generated span as related metadata.

It checks **syntax and the flow graph**: an unknown modifier, a `:max` that is
not a positive integer, an effectful ward predicate or orbit `:by`, a fork branch
that does not rejoin, and any cycle an orbit does not own are all rejected here —
before a capability is granted or a byte is read.

It also checks **names, arity and the effect discipline**, by compiling the
program and handing the result to Rite. An undefined name, a host call without
`!`, or a stage that is not valid Rite comes back pointing at your Cant, with the
Rite code carried alongside:

```bash
$ cant check -e '"data.json" -> @fs.read'
error[CANT-S001]: effectful capability call requires `!`

  --> <expr>:1:16
   |
   1 | "data.json" -> @fs.read
   |                ^^^^^^^^
   |                 this operation performs an external effect

help: mark the operation as an explicit effect: ! @fs.read
```

You see the one you wrote, not the generated code it came from.

### `cant parse [source]`

Prints the syntax tree. `--json` for the full tree with spans; `--structure` for
the span-free form used to compare two spellings of one program:

```bash
$ cant parse --structure -e '[1, 2] -> * -> []'
[
  {
    "leaf": "[1, 2]"
  },
  "scatter",
  "collect"
]

$ cant parse --structure -e '[1, 2] → ⋇ → ⌁'
# identical
```

### `cant -e '<expression>'` and `cant run [source]`

The canonical one-liner form. `cant -e '…'` is `cant run -e '…'` — the shorthand
exists because a one-liner should be as short to type as `awk '…'`.

```bash
$ cant -e '[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []'
[2, 4, 6]

$ cant run pipeline.cant --allow fs:read=./data
```

The program compiles to Rite and runs on Rite's runtime. `cant expand` prints
exactly what runs — `cant run`, running the expansion with `rite`, and the
compiled binary all produce the same value, output and exit status.

The value is printed after whatever the program itself wrote, and only when it is
not `none` — the same rule `rite run`
follows. Arguments after `--` are readable with `! @process.args`.

### `cant build <file>`

Compiles to a native binary through Rite's compiler.

```bash
$ cant build pipeline.cant --allow fs:read=./data
built /home/you/.cache/rite/build-target/debug/rite_script_…
generated Rite: pipeline/.rite/cant/pipeline.rite
```

The generated Rite is written to `.rite/cant/` beside the source and kept, so a
compiled program is still auditable afterwards. Permissions are baked in at build
time, as they are for `rite build`.

`--release`, `--emit-rust` and `--output` behave as they do for `rite build`.

### `cant fmt [source]`

Reprints the program with canonical layout. Prints to stdout by default;
`--write` rewrites the file.

```bash
$ cant fmt --width 40 -e 'roots -> * -> ~{ !@fs.read -> imports -> * -> resolve } :by canonical_path :max 4096 -> []'
roots
  -> *
  -> ~{
       !@fs.read
       -> imports
       -> *
       -> resolve
     }
     :by canonical_path
     :max 4096
  -> []
```

A flow that fits on one line stays on one line. When it does not, it breaks at
its arrows; a *stage* breaks only if it is itself too wide, so a short block does
not explode because the flow around it was long. A broken block hangs its body
from the opener, and puts the closing brace and any modifiers underneath it.

| Flag | Effect |
|---|---|
| `--ascii` | Format to ASCII. The default, and the canonical spelling. |
| `--glyph` | Format to glyphs. |
| `--preserve` | Keep whichever spelling the source predominantly uses. |
| `--compact` | One line, whatever the width. For `-e` and for pasting into a shell. |
| `--width N` | Break lines longer than this. Default 88. |
| `--check` | Exit 1 if the source is not already formatted; write nothing. |
| `--write` | Rewrite the file in place. |

Two things it does not do. It does not reformat the inside of a stage —
`f( 1,2 , 3 )` comes through exactly as written, because a stage is Rite
expression text and Cant does not parse it. And it does not format a source with
syntax errors, since the tree after a parse failure is a guess.

Comments stay where you wrote them. The formatter checks its own output and
refuses to write anything if a comment went missing.

### `cant convert [source] --to ascii|glyph`

Respells structural operators and changes **nothing else**: whitespace, line
breaks, comments, strings and leaf text come through byte for byte.

```bash
$ cant convert --to glyph -e '// a -> comment
"a -> string" -> f([]) -> ?{ $ > 0 } -> []'
// a -> comment
"a -> string" → f([]) → ⊣⟦ $ > 0 ⟧ → ⌁
```

Three things in that output. The `->` inside the comment is still `->`. The `->`
inside the string is still `->`. And the `[]` inside `f([])` is still `[]` — it
is an argument, not a collect. Conversion works from the parse, so it only ever
touches real operators.

`--check` exits 1 if the source is not already in the target spelling. `--stdout`
prints instead of rewriting a file. It also works on a source that does not
parse, using whatever was recognised, so toggling spellings mid-edit keeps
working.

### `cant expand [source]`

Prints the canonical Rite the program becomes. This is a permanent, public
command, not a debugging aid: it is exactly what `cant run` will execute, and it
is how you audit a Cant program without trusting Cant.

```bash
$ cant expand -e '[1, 2] -> * -> ?{ $ > 1 } -> []'
```

The output is ordinary Rite: one function per node, chained, with a header naming
the source. Generated names carry a prefix built from a hash of your source
(`cant_1f4a9c2b_n3`), so two Cant programs never collide.

`--source-map` prints the Cant ↔ Rite span pairs on **stderr**, so
`cant expand --source-map > out.rite` still writes nothing but Rite. `--output`
writes to a file. A program with errors is not expanded.

The output is not minimal. An orbit becomes a worklist, a seen-set and a bounded
loop, so a dense one-liner expands to a screenful — it is written to be read
rather than to be short.

### `cant graph [source] [--format json|dot]`

Prints the flow graph — the normalized semantic form of the program, and what
lowering to Rite will read.

```bash
$ cant graph --format dot -e 'roots -> ~{ !@fs.read -> imports -> * } :max 4096' | dot -Tsvg > graph.svg
```

JSON is the machine format; DOT is for looking at, and clusters each fork branch
and orbit body so containment is visible. Both are deterministic, so a diff of
two graphs reads as a diff of the program.

The [graph schema](graph-schema.md) has the full shape. The graph is printed even
when the program has errors, with diagnostics on stderr, so
`cant graph … | jq` and `| dot` stay clean either way.

### `cant explain [source]`

What the program does, in prose — a semantic reading, not a syntax-tree dump.

```bash
$ cant explain -e '[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 4096 -> []'
What this program does

1. Evaluate `[1, 2]`.
2. Scatter the list into one emission per element, in order.
3. Enter a breadth-first orbit, seeded with the current emissions:
   a. For each candidate not seen before:
      - Keep only the emissions where `$ < 8` holds; the rest emit nothing.
      - Evaluate `$ * 2`, with `$` bound to each emission.
   b. Whatever that emits joins the back of the worklist, identified by `str`,
      first occurrence winning.
   c. Stop when the worklist is empty, or fail after 4096 accepted candidates.
4. Collect every emission so far into one list, in emission order.
```

Below the steps it reports the capabilities the program needs, where it touches
the world, anything worth knowing before running it, and the ordering guarantees.
Those sections appear only when they have something to say.

`--verbose` adds a pointer to the other two views of the same program,
`cant expand` and `cant graph`.

It is read from the graph, which is what executes, so the explanation and the
program cannot drift apart.

### `cant repl`

An interactive session. **Each line is a whole program, and nothing persists
between them.**

```text
$ cant repl
cant — each line is a whole program, and nothing persists between them.
  :help                what you can type
  :expand <program>    the Rite it becomes
  :graph <program>     its topology, as DOT
  :explain <program>   what it does, in prose
  :quit                leave

cant> [1, 2, 3] -> * -> ?{ $ > 1 } -> []
[2, 3]
cant> :explain 5 -> |{ $ + 1 ; $ * 2 }
…
```

A Cant program is one flow: no declarations, no bindings, no statements. There is
nothing for a line to leave behind.

The session does carry the permissions and budget it started with:

```bash
cant repl --allow fs:read=./data --timeout 5s
```

Ctrl-D or `:quit` leaves; Ctrl-C abandons the line.

## Exit codes

Cant uses Rite's contract rather than inventing one — a source rejected for a
syntax error should exit 3 whichever language wrote it.

| Code | Meaning | Cant categories |
|---:|---|---|
| 0 | success | |
| 1 | runtime failure, or `--check` found a difference | `CANT-R`, `CANT-O002`; `cant fmt --check`, `cant convert --check` |
| 2 | invalid CLI usage | `CANT-V`, and argument errors |
| 3 | could not be parsed | `CANT-L`, `CANT-P` |
| 4 | parsed but did not resolve | `CANT-G`, `CANT-S`, `CANT-X` |
| 5 | permission denied | from Rite |
| 6 | compilation failed | from Rite |
| 8 | budget exhausted | `CANT-O001` — Rite's step, time, collection or string budget |

When several errors are reported the status comes from the **first** one, which
is the earliest thing that went wrong. Anything raised by Rite after expansion
keeps the code Rite gives it — including at run time, so that `cant run` and
`rite run <cant expand>` cannot disagree about the same execution.

An orbit reaching its `:max` exits **1**, and is identified by its code,
`CANT-O002`. Rite's own budgets — steps, time, collection and string size — exit
8 as `CANT-O001`.

## Diagnostic codes

| Prefix | Group |
|---|---|
| `CANT-Lxxx` | lexical |
| `CANT-Pxxx` | parser |
| `CANT-Gxxx` | graph validation |
| `CANT-Sxxx` | semantic |
| `CANT-Oxxx` | orbit |
| `CANT-Xxxx` | expansion / lowering |
| `CANT-Rxxx` | runtime, including diagnostics remapped from Rite |
| `CANT-Vxxx` | version |

A diagnostic that originated in generated Rite still points its primary label at
your `.cant` source. The Rite code and the generated span travel with it as
related metadata — visible when you ask for them, never the headline.

## Permissions and budgets

There is no Cant permission grammar. `cant run` and `cant build` take Rite's
flags unchanged and enforce them through `rite-caps` and `ExecutionBudget`:

```text
--allow PERM          --max-steps N
--deny PERM           --max-call-depth N
--allow-all           --max-collection-size N
--timeout 30s         --max-string-size N
```

They are accepted before *or* after the subcommand, so
`cant run p.cant --allow fs:read=.` works the way anyone would write it.

`cant` and `rite` share the code that reads these, so they cannot disagree about
what `--allow fs:read=./data` means.

A capability call inside an orbit body is gated exactly as one at the top level.

Scoped permissions need their scope: `net` and `fs` alone are not permissions, so
`--deny net` is an error rather than a silent no-op.
