# Text patterns

`@regex` matches, extracts, replaces and splits with regular expressions. Two
things about it are worth knowing before the function list.

**Write patterns as raw strings.** An ordinary `"…"` literal interprets `{` as
an interpolation hole and `\d` as an invalid escape, so the quantifier
`"{2,3}"` fails at runtime with ``undefined name `2,3` `` and `"\d"` fails at
check time with `error[E006]`. A raw string `r"…"` takes every character
literally — it is the form regexes are meant to be written in:

```rite browser
hit ← @regex.is_match("aaa", r"^a{2,3}$")?
! @console.println(str(hit))
```

```text
true
```

**Every call answers a result.** A pattern that does not compile is an `err`
value, not a crash, so `?` and the other postures from
[Results and errors](results.md) apply. Matching uses a guaranteed-linear-time
engine — no backtracking, so no pattern can hang a script — which is why
`@regex` needs no permission and no `!`: like `@json`, it computes and touches
nothing.

## The surface

| Function | Answers |
|---|---|
| `@regex.is_match(text, pattern)` | `ok(bool)` |
| `@regex.find(text, pattern)` | `ok(string)` — first match; `ok(none)` when nothing matches |
| `@regex.find_all(text, pattern)` | `ok(list)` — every non-overlapping match, in order |
| `@regex.captures(text, pattern)` | `ok(list)` — whole match first, then each group; `ok(none)` when nothing matches |
| `@regex.replace(text, pattern, replacement)` | `ok(string)` — every match replaced |
| `@regex.split(text, pattern)` | `ok(list)` |

Text first, pattern second, everywhere.

## Finding

```rite browser
found ← @regex.find("ERROR 42: disk full", r"\d+")?
! @console.println(str(found))

all ← @regex.find_all("a1 b22 c333", r"\d+")?
! @console.println(str(all))

missing ← @regex.find("no digits here", r"\d+")?
! @console.println(str(missing))
```

```text
42
[1, 22, 333]
none
```

A miss is `ok(none)`, not an error — the pattern was fine, it just did not
match. Reserve `is_err` for the pattern itself being malformed.

## Capturing

`captures` answers the whole match followed by each parenthesised group, with
`none` for a group that did not participate:

```rite browser
caps ← @regex.captures("2026-08-10", r"(\d{4})-(\d{2})-(\d{2})")?
! @console.println(str(caps))
```

```text
[2026-08-10, 2026, 08, 10]
```

## Replacing and splitting

`$1`, `$2`, `${name}` in the replacement refer to capture groups:

```rite browser
swapped ← @regex.replace("2026-08-10", r"(\d+)-(\d+)-(\d+)", "$3/$2/$1")?
! @console.println(swapped)

parts ← @regex.split("a,  b,c", r",\s*")?
! @console.println(str(parts))
```

```text
10/08/2026
[a, b, c]
```

## When the pattern is wrong

```rite browser
bad ← @regex.is_match("x", "(")
! @console.println(str(is_err(bad)))
```

```text
true
```

The `err` payload carries `kind: "regex.pattern"` and the engine's message, so
a pattern arriving from config or user input can be validated by calling any
`@regex` function with it and checking `is_err`.
