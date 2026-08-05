# 09 — failures

```cant
["missing.txt", "here.txt"] -> * -> !@fs.read($) -> ?{ is_ok($) } -> unwrap_or($, "") -> trim -> []
```

One of these files exists. Without a `?`, a failed capability call flows as an
ordinary `err` value, so the ward *filters* the failures, `unwrap_or` opens
the survivors, and the program answers `[present]`.

The four postures toward a failure. Three are Rite vocabulary rather than new
syntax; the fourth is a rescue, in [10 — rescue](../10-rescue/README.md):

| Posture | Spelling | When |
|---|---|---|
| Unwrap, or fail the run | `!@fs.read($)?` | any failure invalidates the whole flow |
| Drop the failures | `?{ is_ok($) } -> unwrap_or($, "")` | partial results are results |
| Replace with a fallback | `unwrap_or($, "default")` | every input deserves an answer |
| Route them | `!{ "missing: " + str($.kind) }` | the failure itself is worth something |

The full story is in [the language page](../../../docs/cant/language.md#failures);
`conformance/cant/execution/error-dropped` and `error-replaced` pin both
run paths.
