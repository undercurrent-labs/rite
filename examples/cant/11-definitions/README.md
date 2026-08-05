# 11 — definitions

```cant
clean:{ trim -> ?{ count($) > 0 } }
loud:{ upper -> $ + "!" }
["  hello  ", "", " cant "] -> * -> clean -> loud -> []
```

`clean:{ … }` names a flow. Definitions come before the main flow, and each one
is **spliced in** wherever its name appears as a stage, so this program is the
one you would have written out by hand:

```text
["  hello  ", "", " cant "] -> * -> trim -> ?{ count($) > 0 } -> upper -> $ + "!" -> []
```

It answers `[HELLO!, CANT!]`. The empty string is trimmed to nothing and the
ward drops it, which is the point of naming the pair: a definition is a chain,
not a function, so it can hold a ward, a scatter or a collect the way any other
run of stages can.

A stage that is nothing but the name is a use. `clean($)` is not — that is Rite
expression text, and Rite reports a name nothing defines. A definition is not a
value and takes no arguments; for either of those, `use` a Rite module and call
its functions by qualified name, as [08 — modules](../08-modules/README.md)
does.

Definitions may name each other and may be written in any order. What is refused
is a definition that reaches itself, since a splice has no end — the repetition
you want is an orbit, [05 — orbit](../05-orbit/README.md). An unused definition
is refused too: the usual reason is a typo at the use site, which quietly became
an ordinary Rite name.

Run it:

```bash
cant run examples/cant/11-definitions/main.cant
cant expand examples/cant/11-definitions/main.cant   # no definition survives
```
