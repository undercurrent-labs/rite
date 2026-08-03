# 05 — orbit

```cant
[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :max 64 -> []
```

An orbit is a bounded breadth-first fixed point — the only cyclic construct in
Cant, and the only one there will be until named anchors are designed.

1. the worklist starts with the incoming emissions;
2. a candidate is popped; its identity is computed (`:by f`, or structural value
   identity when there is none);
3. an identity already seen is skipped, first occurrence winning;
4. otherwise the candidate is recorded, emitted, and run through the body;
5. the body’s emissions go on the end of the worklist;
6. it stops when the worklist empties.

Result: `[1, 2, 4, 8, 16]` — every value reached, in first-seen order,
deduplicated.

## It cannot run away

`:max` bounds the number of *accepted* candidates and defaults to 1024. Reaching
it is a structured failure, not a truncated answer, so a traversal that grew
past what you expected says so instead of quietly returning half a result. Rite’s
global step and time budgets apply underneath, so an orbit whose body is slow is
bounded even before `:max` is.

`:by` must be pure in v0. Effects in the *body* are fine and run once per
first-seen candidate.
