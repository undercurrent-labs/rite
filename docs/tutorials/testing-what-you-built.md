# Testing what you built

**You will build** a test file for the CLI from [Building a CLI](cli-tool.md), and
learn the one thing about `rite test` that will bite you if nobody says it.

**You need** that tutorial's `flag` and `option` functions, reproduced here so this
page stands alone.

Every block below was run to produce the output shown next to it.

## Start with the surprising part

**`rite test` grants every permission.** Not the default-secure set that `rite run`
gives you — everything. This test needs no `--allow` at all and still writes to the
disk:

```rite native_only
◆! test "reads a file with no --allow" ⟦
  ! @fs.write("scratch.txt", "x")?
  expect(! @fs.read("scratch.txt")?, "x")
⟧
```

```bash
rite test perm.rite
```

```text
tests: 1 passed, 0 failed, 1 total
```

That is a deliberate convenience — a test that had to be handed grants would mostly
be a test of your grant flags — but it means **a test file is as trusted as the CLI
itself**. Running someone else's test file is running their code with your
permissions. Read it first, the same way you would read an install script.

It also means a test cannot prove your script asks for the right permissions. That
is what the [conformance fixtures](../book/contributing-tests.md) are for, and it is
why "it passed the tests" and "it runs under the grants I ship" are two claims.

## A test is a function with a string for a name

```rite browser
◆ double(n) ⟦ ^ n * 2 ⟧

◆ test "doubles a number" ⟦
  expect(double(4), 8)
⟧
```

The name is a **string**, not an identifier — `◆ test doubles_a_number()` does not
parse. That is on purpose: a test name is prose, read by whoever sees the failure,
and prose does not fit in an identifier.

`expect(a, b)` compares two values. It is **not** assert-with-a-message: the second
argument is the value you expected, not an explanation. `expect(x)` with one
argument checks truthiness instead, so both of these say the same thing:

```rite browser
◆ test "both forms" ⟦
  expect(2 + 2 = 4)
  expect(2 + 2, 4)
⟧
```

Prefer the two-argument form where you can, because it can tell you what it *did*
get.

## Test the parts that are pure

The CLI's argument helpers are pure functions — argv in, answer out — which is
exactly what makes them testable without a shell, a subprocess, or a temporary
directory:

```rite
◆ test "a switch is present or it is not" ⟦
  expect(flag(["--upper", "ada"], "upper"), true)
  expect(flag(["ada"], "upper"), false)
⟧
```

This is the practical argument for keeping `main` thin. `main` reads
`@process.args` and prints; everything it hands off is pure and can be called with
a literal list. A function that reached for `@process.args` itself could only be
tested by running the whole program.

## When one fails

```rite native_only
◆ test "this one is wrong" ⟦
  expect(1 + 1, 3)
⟧
```

```bash
rite test fail.rite
echo $?
```

```text
tests: 0 passed, 1 failed, 1 total
FAIL fail.rite::this one is wrong (interpreted): expectation failed: 2 != 3
7
```

Three things worth reading in that output. The test is named by **file and test
name**, so a failure in a suite tells you where to look. The message shows both
values — `2 != 3`, got before expected. And the exit code is **7**, the code Rite
reserves for test failure; `1` would be a runtime error and `5` a permission
denial, so a CI job can tell a broken test from a broken environment.

`(interpreted)` marks which execution path ran it. Rite has two that must agree —
the tree-walking interpreter and the compiled path — and the label is there so a
failure that appears in only one is obvious rather than mysterious.

## The whole script

Save it as `greet_test.rite`:

```rite
// greet_test.rite — the pure parts of the CLI, tested without a shell.

◆ flag(argv, name) ⟦
  ^ contains(argv, "--" + name)
⟧

◆ option(argv, name, fallback) ⟦
  prefix ← "--" + name + "="
  hit ← find(argv, { |a| starts_with(a, prefix) })
  ^ ? hit = none ⟦ fallback ⟧ : ⟦ slice(hit, len(prefix), len(hit)) ⟧
⟧

◆ test "a switch is present or it is not" ⟦
  expect(flag(["--upper", "ada"], "upper"), true)
  expect(flag(["ada"], "upper"), false)
⟧

◆ test "an option falls back when absent" ⟦
  expect(option(["ada"], "greeting", "hello"), "hello")
⟧

◆ test "an option keeps everything after the first =" ⟦
  expect(option(["--greeting=a=b"], "greeting", "hello"), "a=b")
⟧
```

```bash
rite test greet_test.rite
```

```text
tests: 3 passed, 0 failed, 3 total
```

That third test is the one worth having. `--greeting=a=b` is the case where
splitting on `=` and taking a piece gives the wrong answer, and it is the reason
the implementation cuts at a known offset instead. A test that only covers the easy
input would not have told you which of the two spellings to keep.

## Next

- [Testing](../book/testing.md) — the rest of `rite test`, including directories
- [Building a CLI](cli-tool.md) — the tool these tests belong to
- [Contributing tests](../book/contributing-tests.md) — testing Rite itself
