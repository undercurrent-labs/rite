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

Cant's operators (`>`, `|`, `!`, `?`, `*`) are shell metacharacters, and the
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
cant 0.3.0
cant_language_version: 1
cant_graph_schema_version: 3
rite: 0.11.0
```

Four numbers because they move independently: the tool, the language it
implements, the graph JSON schema a consumer may have stored, and the Rite that
expansion targets. `--json` emits the same as an object.

**Cant's version is not Rite's.** `cant` ships inside the Rite release archive,
but it is a v0 language on its own number; the release tag you downloaded is
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
Cant code, severity, spans, notes and help, plus, once expansion lands, the
underlying Rite code and generated span as related metadata.

It checks **syntax and the flow graph**: an unknown modifier, a `:max` that is
not a positive integer, an effectful ward predicate or orbit `:by`, a fork branch
that does not rejoin, and any cycle an orbit does not own are all rejected here, before a capability is granted or a byte is read.

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

The canonical one-liner form. `cant -e '…'` is `cant run -e '…'`. The shorthand
exists because a one-liner should be as short to type as `awk '…'`.

```bash
$ cant -e '[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []'
[2, 4, 6]

$ cant run pipeline.cant --allow fs:read=./data
```

The program compiles to Rite and runs on Rite's runtime. `cant expand` prints
exactly what runs: `cant run`, running the expansion with `rite`, and the
compiled binary all produce the same value, output and exit status.

The value is printed after whatever the program itself wrote, and only when it is
not `none`, the same rule `rite run`
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
$ cant fmt --width 40 -e '["main"] -> * -> ~{ !@fs.read($ + ".cant")? -> @regex.find_all($, "use [a-z_]+")? -> * -> replace($, "use ", "") } :by str :max 4096 -> []'
["main"]
  -> *
  -> ~{
       !@fs.read($ + ".cant")?
       -> @regex.find_all($, "use [a-z_]+")?
       -> *
       -> replace($, "use ", "")
     }
     :by str
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

Two things it does not do. It does not reformat the inside of a stage:
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
inside the string is still `->`. And the `[]` inside `f([])` is still `[]`: an argument, not a collect. Conversion works from the parse, so it only ever
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
loop, so a dense one-liner expands to a screenful. It is written to be read
rather than to be short.

### `cant graph [source] [--format json|dot]`

Prints the flow graph: the normalized semantic form of the program, and what
lowering to Rite will read.

```bash
$ cant graph --format dot -e '["m"] -> * -> ~{ !@fs.read($)? -> lines -> * } :max 4096' | dot -Tsvg > graph.svg
```

JSON is the machine format; DOT is for looking at, and clusters each fork branch
and orbit body so containment is visible. Both are deterministic, so a diff of
two graphs reads as a diff of the program.

The [graph schema](graph-schema.md) has the full shape. The graph is printed even
when the program has errors, with diagnostics on stderr, so
`cant graph … | jq` and `| dot` stay clean either way.

### `cant explain [source]`

What the program does, in prose: a semantic reading, not a syntax-tree dump.

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
the world, anything to be aware of before running it, and the ordering guarantees.
Those sections appear only when they have something to say.

`--verbose` adds a pointer to the other two views of the same program,
`cant expand` and `cant graph`.

It is read from the graph, which is what executes, so the explanation and the
program cannot drift apart.

### `cant repl`

An interactive session. **Each line is a whole program.** The *language* has
nothing for a line to leave behind; the *session* has a workbench of values.

```text
$ cant repl
cant — each line is a whole program. Values can persist; programs cannot.
  :help                what you can type
  x <- <program>       run it, keep the answer as `x` (`it` is the last answer)
  :expand <program>    the Rite it becomes
  :graph <program>     its topology, as DOT
  :explain <program>   what it does, in prose
  ~> <program>         run it, with per-node emission counts
  :quit                leave

cant> [1, 2, 3] -> * -> ?{ $ > 1 } -> []
[2, 3]
cant> :explain 5 -> |{ $ + 1 ; $ * 2 }
…
```

A Cant program is one flow: no declarations, no bindings, no statements. The
*language* has nothing for a line to leave behind, but the **session** has a
workbench:

```text
cant> evens <- [1, 2, 3, 4] -> * -> ?{ $ % 2 = 0 } -> []
[2, 4]
cant> evens -> * -> $ * 10 -> []
[20, 40]
cant> it -> count
2
cant> :bindings
evens <- [ 2, 4 ]
it <- 2
```

The binding arrow is Rite's own, so what `:bindings` prints back is exactly what
you type. It is sugar for `:let evens = …`, which also works. `←` is its
glyph twin. To *compare* against a negative number rather than bind, space the
operator: `x < -3`.

`:let` (and its arrow) is a meta-command, not syntax: a `.cant` file containing it does not
parse. What persists is the **value**, not the program: re-using `evens`
re-runs nothing and repeats no effects. Bindings reach the next line as a
generated-Rite preamble (`:expand` says so when any are live), which is why a
bound name works inside any stage. Only data values can be bound; a handle or
a function has no literal to write, and the refusal says so. `it` is always
the last successful answer.

`~> <program>` (glyph `⟿`, longhand `:trace`) runs a line and prints per-node
emission counts beside the value, the same counts `cant run --trace` reports:

```text
cant> ~> evens -> * -> []
trace  n0:1  n1:2  n2:1
[2, 4]
```

The session also carries the permissions, modules and budget it started with:

```bash
cant repl --allow fs:read=./data --use helpers --timeout 5s
```

#### The session's own settings

`:permissions` shows what the session may reach; `:allow <spec>` and
`:deny <spec>` change it without restarting, taking the same spellings as the
command line. Both need a terminal: a REPL's input *is* the program, so in
`cat untrusted | cant repl` a self-granting command would let a program widen its
own permissions. The refusal says to pass `--allow` instead.

`:use NAME` makes another module available from that line on, and `:uses` lists
what is loaded and where it is searched for. `:fmt <program>` runs the formatter
on one line.

#### The budget bounds a line, not the session

**A session has no wall clock unless `--timeout` asks for one.** When it is
given it applies per line: the budget restarts before every evaluation, so time
spent at the prompt is not charged to the next program. `:timeout <30s|off>` and
`:steps <n|off>` change both without restarting.

**Ctrl-C interrupts the running line** and leaves the session open, reporting
`interrupted` rather than a budget failure. A program stuck inside a host call
that cancellation cannot reach takes a second Ctrl-C, which exits with status
130.

Ctrl-D or `:quit` leaves; Ctrl-C at an empty prompt abandons the line. Line
history is kept in `~/.cant_history`.

#### Colour

The prompt highlights as you type, using the same table
(`grammar/palette.json`) the site and `rite render` use. It is driven by Cant's
lexer, so a `->` inside a string stays a string. `--color auto|always|never`
chooses; `NO_COLOR` and `CLICOLOR_FORCE` apply under `auto`, and a piped session
gets plain text. The palette is built for a dark background, so use
`--color never` on a light terminal.

### `cant test`

Run a program and compare its final value against an expectation:

```bash
cant test -e '[1, 2] -> * -> $ * 2 -> []' --expect '[2, 4]'
cant test pipeline.cant          # compares against pipeline.expect beside it
```

The comparison is over the printed value, exactly the text `cant run` shows,
with trailing whitespace trimmed on both sides, so a sidecar file ending in a
newline compares equal. A match prints `ok` and exits 0. A mismatch shows both
values and exits **7**, the code the contract reserves for test failures. A
program that fails before producing a value keeps its own exit code: a parse
error is not a wrong answer.

Permissions and budgets apply as they do to `cant run`, so a test that reads a
file still says `--allow fs:read=.`.

### Tracing a run

`cant run --trace` counts how many emissions left every node and reports a
`cant.trace` document on stderr. `--trace-out PATH` writes it to a file
instead, and implies `--trace`:

```bash
cant run --trace-out p.trace.json p.cant
cant sigil p.cant --weights p.trace.json
```

```json
{
  "schema": "cant.trace",
  "version": "1",
  "source": "p.cant",
  "nodes": { "n0": 1, "n1": 3, "n2": 2 }
}
```

Node ids are the graph's (`cant graph --format json` names the same ones), so
the trace joins the sigil: `--weights` draws hot paths bright and thick and a
branch that never ran faint. The program's *value* is untouched: it prints on
stdout exactly as an untraced run would, so a traced run still pipes.

Counts accumulate: an orbit body that ran five times over two candidates
reports the sum. The instrumentation lives in the generated Rite (run
`cant expand` on nothing; the traced variant differs only by `@store`
counting), and a run that fails produces no trace: half a measurement of a
crashed run would read as a measurement.

## In an editor

The VS Code extension covers Cant as well as Rite: highlighting, inline errors
from `cant check`, and a lens row above the flow.

```text
▶ Run   Check   Explain   Rite   Sigil
[1, 2, 3] -> * -> ?{ $ > 1 } -> []
```

**Run grants nothing.** A lens is one click, so a program you opened to read
does not get the filesystem because you clicked it. A program naming
capabilities reads `▶ Run (ungranted)`, with the tooltip listing them, and
running it fails with exit 5 exactly as it would in a shell. Set
`rite.codeLens.allowAll` to change that.

**Sigil** renders the program's topology beside the editor and keeps it current
on save — `Cant: Open Sigil Preview`, or the lens.

Install it with `rite vscode install`.

## Exit codes

Cant uses Rite's contract rather than inventing one: a source rejected for a
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
| 8 | budget exhausted | `CANT-O001`, Rite's step, time, collection or string budget |

When several errors are reported the status comes from the **first** one, which
is the earliest thing that went wrong. Anything raised by Rite after expansion
keeps the code Rite gives it, including at run time, so that `cant run` and
`rite run <cant expand>` cannot disagree about the same execution.

An orbit reaching its `:max` exits **1**, and is identified by its code,
`CANT-O002`. Rite's own budgets (steps, time, collection and string size) exit
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
your `.cant` source. The Rite code and the generated span are attached as
related metadata, shown when you ask for them.

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

### Reading and writing the environment

`--allow env` is a **read** grant: it permits `@env.get`, `@env.require` and
`@env.all`, scoped with `--allow env=NAME,NAME`. Changing a variable with
`@env.set` is a separate class, `--allow env:write` (or `env:write=NAME`).
Reusing the read grant would have widened every grant that already exists. `env:read` is the explicit spelling of the bare form.

`@env.set` writes an overlay this run owns, not the operating-system
environment: writing that races every C library reading it while other threads
are running. `@env.get` and `@env.all` read the overlay first, and
`@process.run` passes it to the command it starts. A program started any other
way does not see it.

`--allow sys` covers `@sys`: `cwd`, `home`, `temp_dir`, `os`, `arch`, `pid`,
`hostname`. Denied by default like `fs` and `env`: small facts, but facts about
the machine rather than about the program.

### `--env-file`

```bash
printf 'API_KEY=secret\n' > .env
cant --env-file .env -e '"API_KEY" -> !@env.get'
```

Loads `KEY=VALUE` pairs into the run's environment overlay, and grants reading
**exactly the names the file defines**, so the line above needs no `--allow`.
The argument is the one `@process.args` already makes: a file you named on this
command line is your own input to the program, not ambient state it is asking
you to expose. Nothing else becomes readable, writing is still a separate grant,
and an explicit `--deny env` still takes it away.

`#` starts a comment, a leading `export ` is accepted, and a value may be
single- or double-quoted. **There is no interpolation**: `$FOO` is those four
characters. Repeatable; later files win, and a file's value wins over an
inherited variable of the same name.

## Modules

A Cant program's only import form is a leading `use NAME`, and `cant -e '…'` has
no file to put one at the top of. Three layers supply them, in precedence order
**flag > environment > config file**. They compose rather than replace, so
`--use b` adds to what `CANT_USE=a` asked for:

```bash
cant --use helpers -e '["a"] -> * -> helpers.emphasize($) -> []'
CANT_USE=helpers cant -e '…'
printf 'use = ["helpers"]\n' > cant.toml && cant -e '…'
```

`--module-root DIR` (and `CANT_MODULE_PATH`, and `module-roots = [...]`) adds a
place to look; the program's own directory is always searched first.
`--no-default-use` ignores the environment and the config file, for a run that
has to be reproducible.

A `cant.toml` is found by walking up from the working directory, or from the
program's own directory for `cant run file.cant`. It carries two keys and
nothing else:

```toml
use = ["helpers", "math"]
module-roots = ["./lib"]
```

Paths in it are relative to the file, not to wherever the command was run. An
unknown key is an error: a typo in `module-roots` that was silently dropped would
present as "module not found" pointing at generated Rite.

**A config file cannot grant permissions.** It is discovered by walking up from
the working directory, so an `allow = [...]` would let `cd` into a directory
widen what a program may do, and cloning a repository would be enough to arrange
that. Permissions come from the command line.

A module named by `--use` that cannot be found is reported before anything runs,
naming the layer that asked for it:

```text
cant: no module `helpers`, asked for by `use` in /work/cant.toml
  searched: /work, /work/lib
```
