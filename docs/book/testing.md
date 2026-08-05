# Testing

Rite has a test runner built in. Tests live beside the code they cover, in
ordinary `.rite` files, and run with `rite test`.

> Looking for how the Rite project itself is tested? That is
> [Contributing tests](contributing-tests.md).

## A first test

A test is a declaration, so it starts with `◆` (`def` in ASCII) like a function
does — the name is a string rather than an identifier:

```rite
◆ double(n) ⟦ ^ n * 2 ⟧

◆ test "doubles a number" ⟦
  expect(double(4), 8)
⟧
```

ASCII form:

```rite
def double(n) [[ return n * 2 ]]

def test "doubles a number" [[
  expect(double(4), 8)
]]
```

Run it:

```bash
rite test math.rite
# tests: 1 passed, 0 failed, 1 total
```

With no paths, `rite test` looks in `tests` and `examples`:

```bash
rite test
rite test src tests            # or name the directories yourself
rite test --filter doubles     # only tests whose name contains this
```

## `expect`

`expect` takes either one value or two.

| Form | Passes when |
|------|-------------|
| `expect(value)` | `value` is truthy |
| `expect(actual, expected)` | the two are **structurally equal** |

```rite
◆ test "both forms" ⟦
  expect(2 + 2 = 4)
  expect(2 + 2, 4)
⟧
```

Two values is usually the better choice, because the failure message prints both
sides. `expect(double(4), 9)` reports

```text
expectation failed: 8 != 9
```

where the single-argument form can only say `expectation failed`.

Structural equality goes all the way down, so whole records and lists compare in
one step:

```rite
◆ test "a record round-trips" ⟦
  expect(⟨ok: true, items: [1, 2]⟩, ⟨ok: true, items: [1, 2]⟩)
⟧
```

**`expect` is not "assert with a message".** A second argument is the *expected
value*, never a description — `expect(x = 1, "x should be one")` compares `true`
against a string and fails.

## `fail`

`fail` ends a test immediately with a message of your choosing. It is the right
tool when the condition is not a simple comparison:

```rite
◆ test "rejects an unknown status" ⟦
  result ← classify(#unknown)
  ~ result ⟦
    #error → expect(true)
    _ → fail("unknown status should not classify")
  ⟧
⟧
```

## Interpreted and compiled

By default tests run through the interpreter. `rite build` claims the compiled
path behaves identically, and `--both` is how you hold it to that: every test
runs twice, once each way, and both must pass.

```bash
rite test --interpreted    # the default
rite test --compiled       # the path a built binary takes
rite test --both           # both, and the totals count each run
```

`--both` doubles the reported total: three tests become `6 total`. See
[Compiling to Rust](compiling.md) for what the two paths share.

## In a script or a pipeline

`rite test` exits `0` when everything passes and `7` when anything fails, so it
drops straight into CI:

```bash
rite test || echo "suite failed with $?"
```

`--json` prints a machine-readable summary instead of the rendered report:

```bash
rite test --json
```

```text
{
  "passed": 2,
  "failed": 1,
  "total": 3,
  "failures": [ … ]
}
```

## Tests run with full host access

This is the one surprise. `rite run` starts locked down and makes
you grant permissions; **`rite test` grants everything**. A test can read and
write files, open sockets and start processes with no `--allow` flag — and there
is no flag to restrict it.

```rite
◆ test "writes a real file" ⟦
  ! @fs.write("out.txt", "written by a test")
  expect(! @fs.read("out.txt")?, "written by a test")
⟧
```

That makes fixtures easy, and it means **a test file is as trusted as the CLI
itself**. Read tests before you run them, the same way you would a script you
were about to `--allow-all`. Prefer a temporary directory for anything a test
writes, and clean up after yourself — nothing does it for you.

## Next

- [Compiling to Rust](compiling.md) — the compiled path `--both` checks against
- [Effects and capabilities](effects.md) — the permission model tests bypass
