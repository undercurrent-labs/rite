# 03 — fork

```cant
5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []
```

Every branch receives the *same* input value, and their emissions are
concatenated in source order. Result: `[6, 10, 25]`.

Fork is **sequential** in v0, left to right, and so are any effects inside it.
That is a deliberate limit rather than an oversight: parallel branches need
bounded concurrency in the runtime and explicit ordering and cancellation
semantics in Cant, and shipping the keyword before either exists would fix the
wrong meaning in place.

Note `$ * $` rather than `square`: Cant v0 has no way to define a function, so
an example that called one could not run. See open question 1 in
`docs/cant/checklist.md`.
