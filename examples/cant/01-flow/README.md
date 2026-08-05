# 01 — flow

```cant
[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []
```

Reads left to right:

1. the list is one emission;
2. `*` scatters it into six;
3. the ward passes the three even ones and emits nothing for the rest;
4. `[]` gathers what survived into one list.

Result: `[2, 4, 6]`.

Each stage runs once per incoming emission. A list does not scatter itself:
`*` is always explicit, which is what keeps `[[1, 2], [3]] -> count` meaning
what it looks like.
