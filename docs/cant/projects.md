# Past the one-liner

Cant is built for the shell and most Cant stays there. A flow you reach for
repeatedly wants a file; one that repeats itself wants a name; one you rely on
wants a test.

None of that changes the language. A `.cant` file is still one flow: no
declarations, no statements, no `main`. What changes is what surrounds it.

## A flow in a file

Write the same text you would have quoted:

```bash
$ cat > evens.cant <<'EOF'
[1, 2, 3, 4, 5, 6]
  -> *
  -> ?{ $ % 2 = 0 }
  -> []
EOF
$ cant run evens.cant
[2, 4, 6]
```

A file can span as many lines as it likes. `cant fmt` lays one out and
`cant fmt --compact` folds it back onto a single line, for moving a program
between a file and a shell command:

```bash
cant fmt evens.cant --write      # tidy it in place
cant fmt evens.cant --compact    # print it as a one-liner
cant fmt evens.cant --check      # exit 1 if it would change — for CI
```

`cant convert --to glyph` rewrites the operators as `→ ⋇ ⌁`, and `--to ascii`
puts them back. Both work from the parse rather than the text, so neither
reaches inside a string or a comment.

## Named functions

Cant names *flows*, not functions. `clean:{ trim -> ?{ count($) > 0 } }` is a
chain, spliced in wherever it is used; it takes no argument and cannot leave the
file it is written in — see [the language reference](language.md).

Anything that needs a parameter, or needs the same name in two programs, is a
Rite function, and `use` imports an ordinary **Rite** module:

`report.rite`:

```rite
pub def significant(line) [[ return count(line) > 3 ]]
pub def label(word) [[ return upper(word) + " (" + str(count(word)) + ")" ]]
```

`words.cant`:

<!-- ignore: imports a module this page describes but does not ship; the
     executed form is examples/cant/08-modules. -->
```cant ignore
use report
!@stdin.read
  -> words
  -> *
  -> ?{ report.significant($) }
  -> report.label($)
  -> []
```

```bash
$ echo 'the quick brown fox' | cant run words.cant
[QUICK (5), BROWN (5)]
```

`use name` lines come first, one per line, before the flow. Cant does not
resolve them: the names go out verbatim at the top of the generated Rite, and
Rite's module system does the rest, including resolution relative to the
program's directory, qualified access and collision reporting. An unknown
module is Rite's own diagnostic, mapped back onto your Cant source.

Effect discipline crosses the boundary intact. An effectful module function
takes the marker like any host call (`!report.announce($)`), and an unmarked
call to one is rejected as it would be in Rite.

## Modules without a `use` line

`cant -e '…'` and the REPL have no file to put a `use` line at the top of,
which is most of what `--use` is for:

```bash
cant --use report -e '["fox"] -> * -> report.label($) -> []'
```

Three layers supply modules, and they **compose** rather than replace: `--use b`
adds to what `CANT_USE=a` asked for. When the same module comes from two, the
more specific one wins.

| | |
|---|---|
| `--use NAME` | this invocation |
| `CANT_USE=a,b` | this shell |
| `use = ["a"]` in `cant.toml` | this directory tree |

A `cant.toml` is found by walking up from the working directory — or from the
program's own directory for `cant run file.cant` — and carries two keys:

```toml
use = ["report"]
module-roots = ["./lib"]
```

With that beside it, a program needs no import line at all:

```bash
$ echo 'the quick brown fox' | cant run flow.cant
2
```

`--module-root DIR` (and `CANT_MODULE_PATH`, and `module-roots`) adds a place to
look; the program's own directory is always searched first. `--no-default-use`
ignores the environment and the config file, for a run that has to be
reproducible regardless of where it starts.

A module that cannot be found is reported **before anything runs**, naming the
layer that asked for it, so you know which of the three to change:

```text
cant: no module `report`, asked for by `use` in /work/cant.toml
  searched: /work, /work/lib
```

Paths in a config file are relative to the file, not to wherever you ran the
command. An unknown key is an error: a typo in `module-roots` that was silently
dropped would surface much later as "module not found".

> **A config file cannot grant permissions.** It is discovered by walking up
> from the working directory, so an `allow` key would let `cd` into a directory
> widen what a program may do, and cloning a repository would be enough to
> arrange that. Permissions come from the command line. `cant.toml` refuses
> `allow`, `allow-all` and `deny` with a message saying so.

## Configuration

Reading a variable is an effect with a scoped grant, so a program says which
ones it needs and you decide:

```bash
cant --allow env=PORT -e '"PORT" -> !@env.get ?? "8080"'
```

Naming every variable gets old, and a `.env` file is the usual answer:

```bash
$ printf 'API_KEY=secret\n' > .env
$ cant --env-file .env -e '"API_KEY" -> !@env.get'
secret
```

`--env-file` grants reading **exactly the names the file defines** and nothing
else, so no `--allow` is needed. The reasoning is the one `@process.args`
already uses: a file you named on this command line is your own input to the
program, not ambient state it is asking you to expose. `#` starts a comment, a
leading `export ` is accepted, and values may be quoted. There is no
interpolation: `$FOO` is four literal characters.

`@env.set` writes a variable for the rest of the run under its own grant,
`--allow env:write`. Reading and writing are separate: a program allowed to read
`PATH` is not thereby allowed to change what the commands it starts find on it.
What it writes is an overlay the run owns, visible to `@env.get` and inherited
by `@process.run`. It is not the operating system's environment for this
process, because writing that is unsafe while other threads are running.

`@sys` covers where you are, rather than what you were configured with.

```bash
cant --allow sys -e '!@sys.cwd'
```

`cwd`, `home`, `temp_dir`, `os`, `arch`, `pid` and `hostname`, all under
`--allow sys`, all effectful — none of them is constant for the life of a run.

## Tests

`cant test` runs a program and compares its value against an expectation:

```bash
cant test evens.cant --expect '[2, 4, 6]'
```

Or put the expectation in a sidecar beside the program, which is what a suite
wants — `evens.cant` is checked against `evens.expect`:

```bash
$ printf '[2, 4, 6]\n' > evens.expect
$ cant test evens.cant
ok
```

A mismatch exits **7**, the status the contract reserves for a test failure,
and shows both values:

```text
test failed: evens.cant
  expected: [2, 4]
  actual:   [2, 4, 6]
```

The comparison is over the *printed* value, exactly the text `cant run` shows,
trimmed of trailing whitespace on both sides. `none` is spelled `none`, so a
program expected to answer nothing can say so. That case is worth testing: a
flow with one `?` missing answers `none` and still exits 0, so a check on the
exit code alone passes. Every directory under
[`examples/cant/`](../../examples/cant/) carries a `main.expect` for that
reason.

For CI, `cant check` is the cheap gate — parse, graph, and everything Rite's own
resolver says about the generated code, without executing anything:

```bash
cant check words.cant && cant test evens.cant && cant fmt --check evens.cant
```

## A binary

`cant build` compiles through Rite's compiler to a native executable:

```bash
$ cant build evens.cant -o evens
built evens
generated Rite: .rite/cant/evens.rite
$ ./evens
[2, 4, 6]
```

Permissions are baked in at build time with the same `--allow` flags, so a
compiled program carries the grants it was built with and asks for nothing at
run time.

The generated Rite is kept rather than deleted. It is the same artifact
`cant expand` prints, so a compiled program stays auditable: you can read what
was compiled, and `rite build` it yourself.

## The REPL as a workbench

`cant repl` builds a flow one stage at a time. The language still has nothing
for a line to leave behind, but the *session* keeps values:

```text
cant> words <- "the quick brown fox" -> words
[the, quick, brown, fox]
cant> words -> * -> ?{ count($) > 3 } -> []
[quick, brown]
cant> it -> count
2
```

`x <- <program>` runs a program and keeps its **answer**, not the program, so
nothing re-runs and no effect repeats. `it` is always the last answer.
`~> <program>` runs one and shows per-node emission counts beside the value,
which is the quickest way to find a stage emitting more than you expected.

A session carries the permissions, modules and budget it started with, and can
change them without restarting: `:permissions`, `:allow`, `:deny`, `:use`,
`:uses`, `:timeout`, `:steps`. `:expand`, `:graph` and `:explain` take a program
and show that view of it instead of running it.

A session has **no wall clock** unless `--timeout` asks for one, and Ctrl-C
interrupts the running line rather than ending the session. See
[the command line](cli.md#cant-repl) for the whole surface.

## Where to go next

- [When something goes wrong](diagnostics.md) — what each diagnostic means
- [The command line](cli.md) — every flag, exit code and permission
- [The language](language.md) — the complete operator reference
- [Graph schema](graph-schema.md) — the JSON `cant graph` emits, for tooling
