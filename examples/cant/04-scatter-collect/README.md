# 04 — scatter and collect

```cant
[ [1, 2], [3, 4], [5] ] -> * -> sum -> []
```

`*` and `[]` are inverses, and they are the only two stages that change how many
values are in flight:

- `*` turns one list into one emission per element, in order. Applied to
  anything that is not a list it is a runtime error, reported at the span of the
  `*`.
- `[]` consumes every emission reaching it and produces one list, in emission
  order.

Result: `[3, 7, 5]`.

Note the spaces in `[ [1, 2], … ]`. A stage is Rite expression text, and Rite
lexes `[[` as its block opener — so `[[1, 2], [3]]` is not a valid list in Rite
either, and Cant inherits that. `cant check` catches it and reports it against
the stage, as `CANT-S004`.

At the end of a program collection is implicit — zero emissions become `none`,
one becomes that value, many become a list. Writing `[]` explicitly matters when
the list has to go somewhere: `... -> [] -> sum` sums the collection, while
`... -> sum` sums each emission separately.
