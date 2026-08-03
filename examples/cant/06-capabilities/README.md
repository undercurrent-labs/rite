# 06 — capabilities

```cant
"data.json" -> !@fs.read? -> @json.decode -> .name
```

Reading `data.json` beside this file, it evaluates to `"cant"`.

The `?` is Rite's, and it is doing real work: `@fs.read` returns a result, so
without it `@json.decode` would receive an `ok(…)` rather than the string inside
it. Cant does not wrap or unwrap anything on your behalf — a stage is Rite
expression text, and Rite's rules apply inside it unchanged.

Cant keeps Rite’s effect discipline exactly as it is — it does not relax it and
it does not reimplement it. `!` marks a host call, `@fs.read` names the
capability, and the generated Rite this lowers to contains that same marked call
in a function body where Rite’s resolver can see it.

That last part is the whole design. Rite’s effect analysis cannot see through a
function value passed into an opaque helper, so Cant never lowers a ward, fork,
or orbit body into one — see
[`docs/adr/0002-cant-lowers-through-rite.md`](../../docs/adr/0002-cant-lowers-through-rite.md).
An effect inside an orbit is as visible, and as permission-gated, as one at the
top level.

Running it will need a grant, from the same permission model `rite run` uses:

```bash
cant run examples/cant/06-capabilities/main.cant --allow fs:read=./data
```

There is no Cant permission grammar and no `@cant` capability.
