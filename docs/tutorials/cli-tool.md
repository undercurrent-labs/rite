# Building a CLI

**You will build** a command-line tool that takes names and flags, greets each
name, and fails properly when it is called wrong.

**You need** nothing but a Rite install. No files, no network — which makes this
the smallest useful thing you can ship.

Every block below was run to produce the output shown next to it.

## Where the arguments come from

```rite native_only
argv ← ! @process.args()
! println(str(argv))
```

```bash
rite run greet.rite -- --upper ada grace
```

```text
[--upper, ada, grace]
```

Everything after `--` belongs to your script; everything before it belongs to
`rite`. The separator is what keeps `--upper` from being read as a flag to the
interpreter, and it is why the two never have to agree on a flag namespace.

`@process.args` is the one call in the capability set that **needs no permission**.
The arguments are what the invoker chose to hand this program — they are its input,
not ambient state it went looking for. Compare `@env.get`, which reads something the
caller may not have meant to share and is denied by default.

The call is still marked `!`. It reads something from outside the program, and the
marker says so; needing no *permission* and performing no *effect* are different
questions.

## Splitting flags from arguments

There is no argument-parsing library. There does not need to be — a flag is a
string that starts with `--`, and Rite already has the two functions that says:

```rite browser
argv ← ["--upper", "ada", "grace"]
! println(str(reject(argv, { |a| starts_with(a, "--") })))
! println(str(keep(argv, { |a| starts_with(a, "--") })))
```

```text
[ada, grace]
[--upper]
```

`keep` and `reject` are the same predicate read both ways, which is exactly the
split a CLI wants: positionals are what is left when you take the flags out.

## Two kinds of flag

A **switch** is present or absent:

```rite
◆ flag(argv, name) ⟦
  ^ contains(argv, "--" + name)
⟧
```

An **option** carries a value, and needs the value cut off the front:

```rite
◆ option(argv, name, fallback) ⟦
  prefix ← "--" + name + "="
  hit ← find(argv, { |a| starts_with(a, prefix) })
  ^ ? hit = none ⟦ fallback ⟧ : ⟦ slice(hit, len(prefix), len(hit)) ⟧
⟧
```

`find` answers the first match **or `none`** — not a result, because "no such flag"
is an ordinary outcome and the fallback is right there. That is the whole reason
`option` takes a `fallback` argument rather than answering a result for the caller
to unwrap.

Note `slice` rather than `split(hit, "=")`. Splitting looks tidier until someone
passes `--greeting=a=b`, at which point taking the last piece gives `b` and taking
the first gives `a`. Cutting at a known offset is right for any value:

```bash
rite run greet.rite -- --greeting=a=b ada
```

```text
a=b, ada
```

> **`drop` also works here.** `drop(hit, len(prefix))` gives the same answer —
> `take`, `drop`, `first` and the rest of that family read strings by character,
> like `slice` and `count` always have. `slice` is used above because it says
> "from here to there" outright, which is what this line means; `drop` says it as
> "not the first n". Either is fine.
>
> This was not always true: those builtins used to count list elements only, and
> answered an empty *list* for a string. The error then surfaced somewhere else
> entirely, as `upper expects a string, got list`.

## Failing properly

```rite
? count(names) = 0 ⟦
  ! @console.error("usage: greet [--upper] [--greeting=WORD] NAME...")
  ^ fail("no names given")
⟧
```

Two separate jobs. `@console.error` writes the usage line to **stderr**, so it stays
on the terminal when someone pipes your output somewhere. `fail` ends the run with a
non-zero status, which is the part a shell script or a CI job actually checks:

```bash
rite run greet.rite --
echo $?
```

```text
usage: greet [--upper] [--greeting=WORD] NAME...
runtime error: no names given
1
```

That works, but it says two things at once: `runtime error: no names given` reads
like the program broke, when in fact it worked correctly and the *caller* got it
wrong. And `fail` is blunt — it always means **1**. `@process.exit` lets you say
which:

```rite
? count(names) = 0 ⟦
  ! @console.error("usage: greet [--upper] [--greeting=WORD] NAME...")
  ! @process.exit(2)      // 2 is the shell's conventional "you used it wrong"
⟧
```

```bash
rite run greet.rite --
echo $?
```

```text
usage: greet [--upper] [--greeting=WORD] NAME...
2
```

The usage line is the whole message now — no runtime-error noise on top of it —
and `2` tells a wrapper this was a misuse rather than a crash. Nothing after the
call runs, and the status cannot be caught. It needs no permission: saying how your
own program ended is not the same privilege as running another one. This is what
the finished script below uses.

The [full exit-code table](../book/effects.md#exit-codes) is part of Rite's
contract — `5` for a permission denial, `8` for a blown budget — so a wrapper can
tell "your script was wrong" from "your script said no". Those codes are not
off-limits to you, though. `@process.exit` accepts anything from 0 to 255, because
the most common thing to do with an exit status is pass on the one you were handed:

```rite
◆! main() ⟦
  r ← ! @process.run("sh", ["-c", "exit 17"], ⟨⟩)?
  ! @process.exit(r.status)
⟧
```

```bash
rite run forward.rite --allow process
echo $?
```

```text
17
```

A `@process.exit` that refused the runtime's own codes would fail on exactly this —
and only for the runs where the child happened to return one.

## The whole script

Save it as `greet.rite`:

```rite
// greet.rite — greet each name given, with flags.

◆ flag(argv, name) ⟦
  ^ contains(argv, "--" + name)
⟧

◆ option(argv, name, fallback) ⟦
  prefix ← "--" + name + "="
  hit ← find(argv, { |a| starts_with(a, prefix) })
  ^ ? hit = none ⟦ fallback ⟧ : ⟦ slice(hit, len(prefix), len(hit)) ⟧
⟧

◆! main() ⟦
  argv ← ! @process.args()
  names ← reject(argv, { |a| starts_with(a, "--") })

  ? count(names) = 0 ⟦
    ! @console.error("usage: greet [--upper] [--greeting=WORD] NAME...")
    ! @process.exit(2)
  ⟧

  word ← option(argv, "greeting", "hello")
  shout ← flag(argv, "upper")

  each(names, { |n|
    line ← word + ", " + n
    ! println(? shout ⟦ upper(line) ⟧ : ⟦ line ⟧)
  })
⟧
```

```bash
rite run greet.rite -- --upper --greeting=welcome ada grace
```

```text
WELCOME, ADA
WELCOME, GRACE
```

Without the flags it is the plain form:

```bash
rite run greet.rite -- ada grace
```

```text
hello, ada
hello, grace
```

Nothing here needs a single `--allow`. A tool that only reads its own arguments and
writes to stdout touches nothing it must ask for, which is worth noticing: the
default-secure posture costs you nothing until you actually reach outside.

## Making it feel like a program

Add a shebang and the `--` disappears from the caller's view:

```rite
#!/usr/bin/env -S rite run --
```

```bash
chmod +x greet.rite
./greet.rite --upper ada
```

See [First script](../book/first-script.md) for the shebang forms and their
trade-offs.

## Next

- [Processes](../book/processes.md) — running *other* commands from your tool
- [Environment](../book/environment.md) — configuration, when arguments are not enough
- [Reshaping JSON](json-pipeline.md) — give the tool something to chew on
