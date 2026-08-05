# 08 — modules

```cant
use helpers
["ward", "orbit"] -> * -> upper -> helpers.emphasize($) -> !helpers.announce($) -> []
```

`use helpers` imports [`helpers.rite`](helpers.rite), a Rite module resolved
by Rite, relative to this program. Named functions come from Rite; Cant does
not grow a definition syntax of its own.

Two calls, two disciplines:

- `helpers.emphasize($)` is pure, and is called like any stage.
- `helpers.announce($)` is declared `def!`, because it prints, so the call takes the
  marker: `!helpers.announce($)`. Leaving the `!` off is rejected by Rite's
  effect analysis, pointing at this file.

Output:

```text
-> **WARD**
-> **ORBIT**
[**WARD**, **ORBIT**]
```

A typo in a qualified name, such as `helpers.emphasise`, is caught at check time and
names the module and the nearest export, because module resolution is Rite's
own machinery end to end.
