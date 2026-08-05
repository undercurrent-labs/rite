# 09 — failures

```cant
["missing.txt", "here.txt"] -> * -> !@fs.read($) -> ?{ is_ok($) } -> unwrap_or($, "") -> trim -> []
```

One of these files exists. Without a `?`, a failed capability call flows as an
ordinary `err` value, so the ward *filters* the failures, `unwrap_or` opens
the survivors, and the program answers `[present]`.

The three postures toward a failure, all Rite vocabulary rather than new
syntax:

| Posture | Spelling | When |
|---|---|---|
| Propagate and stop | `!@fs.read($)?` | a missing file makes the run meaningless |
| Drop the failures | `?{ is_ok($) } -> unwrap_or($, "")` | partial results are results |
| Replace with a fallback | `unwrap_or($, "default")` | every input deserves an answer |

The full story is in [the language page](../../../docs/cant/language.md#failures);
`conformance/cant/execution/error-dropped` and `error-replaced` pin both
run paths.
