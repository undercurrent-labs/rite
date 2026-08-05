# ADR 0011 — Named flows are spliced, not called

- **Status:** Accepted
- **Date:** 2026-08-05
- **Supersedes:** nothing
- **Related:** [ADR 0002 — Generated canonical Rite is the v0 execution boundary](0002-cant-lowers-through-rite.md) ·
  [ADR 0010 — Error routing is a rescue stage](0010-error-routing-is-a-rescue-stage.md)

## Context

Cant has one form of reuse: `use` of a Rite module. A flow written twice is
written twice, and `docs/cant/language.md` lists function definitions under what
the language does not have, answering them with builtins, an inline closure, or a
Rite module.

That answer costs a second file for anything a program wants to name for itself,
and it fixes the shape of the reuse as a Rite function: one value in, one value
out. What repeats in a Cant program is a *chain*. `trim -> ?{ count($) > 0 }` is
two stages, one of which emits zero or one value, and there is no function that
is those two stages.

Four facts about the implementation bear on the design.

- **The parser never sees a newline.** Trivia is dropped before parsing. The
  `use` preamble is unambiguous without line structure because `use` takes
  exactly one identifier and stops. A construct whose body is a *flow* has no
  such stopping point: a leaf run consumes tokens until `->`, `;` or `}`, so
  `clean := trim` followed by `[1, 2] -> …` on the next line reads `trim [1, 2]`
  as one leaf.
- **A leaf is Rite expression text, passed through unchanged.** Every new lexeme
  is one more thing that cannot appear inside one.
- **Lowering is total and runs before validation** (`crates/cant-sem/src/lower.rs`).
  A construct that can name itself can make lowering recurse forever, and a stack
  overflow is not a diagnostic anyone can point at.
- **The graph schema is frozen at version 2** by
  `crates/cant-sem/tests/schema_freeze.rs`, in lockstep with
  `docs/cant/graph-schema.md`. Any key added, removed or renamed is a version
  bump, and a bump re-fingerprints every stored Sigil scene.

## Decision

**A definition is `name:{ flow }` (glyph `name≔⟦ flow ⟧`), written before the
main flow. Where its name appears as a whole stage, the definition's stages are
spliced in.**

```cant
clean:{ trim -> ?{ count($) > 0 } }
!@stdin.lines -> * -> clean -> []
```

### Syntax

`:{` joins `?{`, `|{`, `~{` and `!{` as a block opener and closes with the same
`}`. The braces are what make the construct work at all: they end the definition
without a line break, so the grammar stays newline-blind and a definition and its
flow fit on one `-e` line.

`:{` is genuinely ambiguous, unlike `!{`. A Rite record whose field holds a block
is `<< f:{ |x| x } >>`, and that is valid inside a leaf. So `:{` is resolved by
position, the way `*`, `[]` and `:` already are: the lexer emits one
`DefineOpen` token wherever it sees the lexeme, and only the parser calls it a
definition, and only when it follows an identifier in the preamble. Everywhere
else it is leaf material. Two consequences hold it in place:

- `DefineOpen` counts toward leaf depth, like `{`, and is **not** a block opener
  for the purposes of "a block opener can only start a stage". A leaf containing
  `<< f:{ |x| x } >>` therefore lexes and parses exactly as it did before.
- The glyph `≔⟦` has no second meaning, so one inside a leaf is reported
  (`CANT-P007`), as `⋇` and `⌁` already are.

The name is an identifier. `use` lines and definitions may be interleaved, and
both come before the main flow, which is the last thing in the file. `cant fmt`
prints one definition per line with the name against the brace.

### Semantics

**A definition is a chain, not a leaf.** It takes the emissions reaching it and
produces whatever its last stage emits. A definition used as a stage is replaced
by its stages, in place:

```cant
loud:{ upper -> $ + "!" }
["a", "b"] -> * -> loud -> []          // ["A!", "B!"]
```

is the same program as `["a", "b"] -> * -> upper -> $ + "!" -> []`. That is what
makes `[]` inside a definition mean what it says: it collects everything that
reached it, across the whole flow, not once per emission.

**A bare name is a definition when this program defines it, and a leaf
otherwise.** `-> trim ->` is a Rite function today and stays one; `-> clean ->`
is the flow when the program says `clean:{ … }`. Only a stage that is *nothing
but* the name is a use. `clean($)` is leaf text and Rite reports the name it does
not know: a definition is a stage, not a value, and not callable.

**Order does not matter, and a cycle is refused.** A definition may name one
written below it. What is refused is a definition that reaches itself, directly
or through others: every cycle in Cant is orbit-owned, and that is a tested
invariant (`crates/cant-sem/src/validate.rs`), not a preference. Splicing a
recursive definition would also not terminate. Ordering the definitions instead,
so that a forward reference is the error, would have meant `a:{ b }` silently
meaning a Rite function or a Cant flow depending on where `b` sits in the file.

**An unused definition is an error.** The usual way to leave one unused is a typo
at the use site, which becomes an ordinary leaf and is reported as an undefined
Rite name — a message that names the wrong problem. Reporting the definition too
makes the pair readable. It also keeps Cant's posture: an unknown modifier and a
`?` before a rescue are both errors already.

**A definition may not shadow a Rite builtin or an imported module.** `count:{ … }`
would mean the flow as a stage and the builtin inside `?{ count($) > 0 }`, in the
same program, with nothing to tell them apart.

**Effects are recomputed at every splice.** A definition holding `!@fs.read` puts
an effectful node at each place it is used, so each carries its `!` and each is
permission-gated. Nothing is memoized, because there is nothing to memoize: the
splice produces fresh nodes with fresh hygienic names (`cant_<hash>_nN`), and
effect-ness is computed over the graph that results.

### The graph, still at version 2

**Splicing happens during lowering, before the graph exists.** A definition is
not a node, not a subgraph, and not a key. The graph a consumer reads is the
inlined program, which is exactly what executes, and `cant graph` draws that.

This is the reason the schema does not move. Naming a subgraph and recording
splice points would be two new keys and a version 3, and the freeze test does not
permit either silently. It would also make the differential harness compare two
things that are no longer the same shape. Sigil needs none of it for v1 of this
feature: a spliced graph draws as the flow it is.

The cost is that a diagnostic inside a definition used twice points at the
definition, once per use, rather than at the use site. That is the ordinary
inlining trade, and the definition is where the code is.

### Expansion

Nothing in `crates/cant-sem/src/expand.rs` changes. It receives an ordinary
graph, and every rule it already applies — one function per node, `def!` where a
host call is in the body, hygiene over the source hash — applies to spliced nodes
without knowing they were spliced.

### Diagnostics

Four codes, all in the graph group, all checked against the **AST** rather than
the graph for the same reason modifier validation is: lowering consumes the
definitions, so by the time there is a graph there is nothing left to point at.

| Code | Meaning |
|---|---|
| `CANT-G018` | Two definitions share a name. |
| `CANT-G019` | A definition's name is a Rite builtin or an imported module. |
| `CANT-G020` | A definition reaches itself. |
| `CANT-G021` | A definition the program never uses. |

Lowering still refuses to reject anything, so it has to survive a program
`CANT-G020` will refuse. A name already being spliced is left as an ordinary
leaf rather than followed, which terminates and keeps the node the diagnostic
points at.

### Tooling

- `cant fmt` and `cant convert` round-trip the form, and `:{` ↔ `≔⟦` is a
  manifest entry like every other spelling.
- `cant graph` renders the **inlined** program. So does `cant explain`, which
  reads the graph; it names the definitions and says they are spliced, so a
  definition used twice appearing twice in the steps is not a surprise.
- `cant expand` shows the inlined Rite, in which no definition name occurs.
- The REPL is unaffected. `:let` binds a *value* produced by a whole program;
  a definition is part of one program's text. Both can appear on a line, and
  they do not interact.

### Out of scope

- **Parameters.** A definition takes the emissions reaching it and nothing else.
  Adding arguments would mean a call syntax, an arity rule and a scope for the
  names, which is a function, and Rite already has functions and a way to import
  them.
- **Exporting a definition, or `use`ing one.** A `.cant` file is still an
  expression rather than a module. Sharing a flow between two files is the same
  problem as sharing one between two programs, and it needs a module system,
  which is what `use` reaches into Rite for.
- **A definition inside a leaf or a ward predicate.** Both are Rite expression
  text, and a definition is not an expression.
- **Recursion**, above.

## Consequences

**Good.** A flow used twice is written once, without a second file. The most
common shape — a few stages of cleanup applied in more than one place — no longer
has to become a Rite module to be named.

**Good.** No schema change, no expansion change, no runtime change. The
differential harness compares the same three executions of the same graph it
always did, and every existing stored graph and Sigil scene is untouched.

**Cost.** A fifth `X{` opener, and the first one whose ASCII spelling can
legitimately occur inside a leaf. The mitigation is that `DefineOpen` does not
break a leaf run, so the collision is invisible unless a definition is what was
meant; the failure mode of the alternatives was a *truncated leaf*, which is
worse.

**Cost.** Two ways to spell reuse: a definition and a `use`d Rite module. They
answer different questions (a chain against a function) and the docs say which
is which, but a reader now has to choose.

**Cost.** Inlining is visible in `cant graph` and `cant explain`: a definition
used three times is three copies of its stages. That is honest about what runs,
and it makes a large definition used often produce a large graph.

## Alternatives rejected

**`name := flow`, ended by the line.** Rejected: it makes Cant's grammar
line-sensitive for the first time, and only in one construct. The rule would have
had to be "a newline at block depth zero ends the definition unless the next line
begins with `->`", so that `cant fmt`'s broken layout still parsed — a rule the
formatter and the parser would have to keep agreeing about, forever, for a
language whose lexer deliberately hands the parser no lines at all. It also makes
`-e` unusable for anything with a definition in it.

**`name := flow ;`.** Rejected: it works and is newline-blind, but `;` already
separates fork branches, so the terminator would mean one thing at top level and
another inside a `|{ }` two characters away. The braces do the same job with a
delimiter that already reads as "a flow lives here".

**`name: flow`, the plain colon.** Rejected: `:` is Rite's ASCII atom prefix and
Cant's modifier prefix, and the two are already told apart by whether the colon
touches the name after it. A third reading told apart by whether it touches the
name *before* it is a rule nobody can hold in their head, and `clean:trim` would
have been a definition while `~{ f } :max 4` stayed a modifier.

**Name the subgraph in the graph and splice at expansion.** Rejected: two new
keys and a version 3, which re-fingerprints every stored scene and SVG for a
feature that changes no geometry. The graph would also stop being the thing that
executes, since the executed program is the inlined one either way. Worth
revisiting only if Sigil grows a reason to draw a definition as a region.

**A definition as a per-emission leaf, that is, a function.** Rejected: it cannot
express a ward, a scatter or a collect, which is most of what anyone would want
to name. It is also what `use` of a Rite module already provides, better.
