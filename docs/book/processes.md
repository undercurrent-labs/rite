# Processes

`@process` is how a Rite script runs another program — and how it reads the
arguments it was handed itself.

It is the sharpest capability in the set: a script that may run a subprocess can run
*any* binary on the machine, with the permissions of the user running it. `process`
is denied by default and there is no narrower grant than "yes", so the question to
ask before granting it is not which command you had in mind but what the whole
machine would allow.

```rite native_only
◆! main() ⟦
  r ← ! @process.run("echo", ["hello", "world"], ⟨⟩)?
  ! @console.println(r.stdout)
⟧
```

```text
hello world
```

The result record is `⟨status, stdout, stderr⟩`. The third argument is an options
record; pass `⟨⟩` when you have nothing to say.

| Option | Type | Effect |
|---|---|---|
| `cwd` | string | The directory the child runs in |
| `env` | record | Variables **added to** the inherited environment |

```rite native_only
◆! main() ⟦
  a ← ! @process.run("pwd", [], ⟨cwd: "subdir"⟩)?
  b ← ! @process.run("sh", ["-c", "echo $GREETING"], ⟨env: ⟨GREETING: "hello"⟩⟩)?
  ! @console.println(trim(b.stdout))
⟧
```

```text
hello
```

`env` extends rather than replaces, because a child that loses `PATH` usually cannot
start. Neither option needs a permission of its own: `--allow process` already lets
the script run any binary, and setting a child's directory or environment tells the
script nothing back.

**An unrecognised key is an error**, not a default — `⟨cdw: "…"⟩` says so rather
than silently running in the wrong directory.

**There is no shell.** The command and its arguments go straight to `exec`, which is
why the arguments are a *list* rather than one string. Nothing expands `*`, nothing
splits on spaces, and nothing interprets `|` or `>`. That removes the entire class of
bug where a filename with a space in it becomes two arguments — and it means if you
genuinely want a pipeline you have to ask for a shell yourself:

```rite native_only
! @process.run("sh", ["-c", "ls | wc -l"], ⟨⟩)?
```

Doing that hands the shell a string to parse, so it is back on you to make sure
nothing untrusted is interpolated into it.

### A command that fails is not an error

```rite native_only
◆! main() ⟦
  r ← ! @process.run("sh", ["-c", "exit 3"], ⟨⟩)?
  ! @console.println("status " + str(r.status))
⟧
```

```text
status 3
```

`grep` finding nothing exits 1, and `diff` finding a difference exits 1. Those are
answers, not failures, so a non-zero `status` still comes back as `ok` and it is your
job to decide what it means.

A command that could not be *started* is different — it raises rather than
answering `err`, and it will end the script even inside a match:

```text
runtime error: No such file or directory (os error 2)
```

So check first if the binary might be absent, which is what `which` is for.

### Finding a binary

```rite native_only
! @console.println(! @process.which("echo"))
```

```text
ok(/usr/bin/echo)
```

Missing gives `err(not found: …)`. `which` needs **two** permissions, because
locating a binary means reading `PATH`:

```bash
rite run t.rite --allow process --allow env=PATH
```

Ask for only one and the error says which half is missing:

```text
permission denied: process.which reads the PATH environment variable: also needs `--allow env=PATH` (or --allow env / --allow-all)
```

`@process.run` needs no `env` grant, because it never reports the environment back
to the script.

### Your own arguments

```rite native_only
! @console.println(! @process.args())
```

```bash
rite run tool.rite -- a b
```

```text
[a, b]
```

Everything after `--` is yours; everything before it belongs to `rite`. This is the
one call in the capability set that **needs no permission at all** — the arguments
are what the invoker chose to hand this program, not ambient state it went looking
for.

## Permissions

| Call | Needs |
|---|---|
| `@process.run` | `--allow process` |
| `@process.which` | `--allow process` **and** `--allow env=PATH` |
| `@process.args` | nothing |

Native only: the browser has no subprocesses, so every call here is a clear
capability error in Studio. See [Browser & Studio](browser.md).

## Next

[Environment](environment.md) · [Compiling to Rust](compiling.md) · [Effects and capabilities](effects.md)
