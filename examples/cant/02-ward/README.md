# 02 — ward

```cant
lines("alpha\nbeta\ngamma") -> * -> ?{ starts_with($, "b") } -> upper -> []
```

A ward is a filter that emits *the unchanged input* or nothing at all. It never
transforms; that is what the stage after it is for.

- truthy predicate → the input passes through untouched;
- falsey → nothing is emitted, and the flow continues with fewer values;
- an error in the predicate propagates. It is not swallowed as "falsey", because
  a filter that hides failures reports the wrong answer confidently.

`$` is the current emission. In `upper` there is no `$`, so the emission goes
into the first argument position — the same rule Rite pipelines use.

Result: `["BETA"]`.

Effectful predicates are rejected in v0. A filter that reads a file is a
different thing from a filter, and it needs ordering rules Cant does not have
yet.
