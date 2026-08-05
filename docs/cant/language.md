# The Cant language

A Cant program is one flow: a chain of stages, each receiving one value and
emitting zero or more.

```cant run
[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []
```

That is the shape of the language. Everything below is what a stage can be.

## Emissions

Every stage takes one input value and produces a list of output values, and the
next stage runs once per value:

| Stage | Emits |
|---|---|
| a source expression | one value |
| an ordinary stage | its one returned value |
| a ward | the unchanged input, or nothing |
| a rescue | the input unwrapped, or its handler's emissions |
| `*` | one emission per element of a list |
| `[]` | one list holding every emission that reached it |
| a fork | its branches' emissions, concatenated in order |
| an orbit | every first-seen candidate, in breadth-first order |

At the end of the program, collection is implicit: zero emissions become `none`,
one becomes that value, and many become a list in emission order.

A list is an ordinary Rite value and does not turn itself into multiple
emissions. `*` is always written, so `xs -> count` counts the list rather than
its elements.

## Stages

### Ordinary stages

A stage is Rite expression text. The current emission goes into the first
argument position, unless the stage contains an explicit `$`:

```cant run
"cant" -> upper
```

evaluates `upper("cant")`. With an explicit `$`, the emission goes exactly where
you put it:

```cant run
"-" -> join(["a", "b"], $)
```

is `join(["a", "b"], "-")`. A projection reads as one:

```cant run
<< message: "hi" >> -> .message
```

First-argument insertion is Rite's own pipeline rule; Cant does not invent a
second one. A capability call is the one place the two differ: Rite's pipeline
does not insert into `@fs.read`, so Cant writes the argument out rather than
leaving it implicit, and `path -> !@fs.read` becomes `!@fs.read(path)`.

Cant does not re-specify Rite's expression grammar. Whatever Rite accepts inside
a stage, Cant accepts — including closures, whose braces are ordinary text to
the Cant parser:

```cant run
[ [1, -1], [-2, -3] ] -> * -> ?{ any($, { |n| n > 0 }) } -> []
```

### Flow — `->` (`→`)

Chains stages. Each stage runs once per incoming emission, in order.

### Scatter — `*` (`⋇`)

Expands a list into one emission per element, preserving order.

Every picture below is `cant graph --format dot` run through Graphviz — the same
topology the program executes, not a drawing of it.

![The flow graph for a scatter, ward and collect](graphs/flow.svg)

```cant run
[ [1, 2], [3, 4], [5] ] -> * -> sum -> []    // [3, 7, 5]
```

Note the spaces. A stage is Rite expression text, and Rite lexes `[[` as its
block opener, so `[[1, 2], [3]]` is not a valid list in Rite either.

Applied to something that is not a list it is a runtime error, reported at the
`*` rather than somewhere inside generated code.

`*` is scatter only when it is a whole stage; `$ * 2` is multiplication. The
glyph `⋇` is unambiguous, and is an error anywhere a stage is not expected.

### Collect — `[]` (`⌁`)

Consumes every emission reaching it and produces one list, in emission order.

```cant run
[<< active: true >>, << active: false >>] -> * -> ?{ $.active } -> []
```

The end of a program collects implicitly, so writing `[]` matters when the list
has to go somewhere: `… -> [] -> sum` sums the collection, `… -> sum` sums each
emission separately.

As the **first** stage of a program, `[]` is the empty list literal — nothing has
been emitted yet, so there is nothing to gather.

### Ward — `?{ p }` (`⊣⟦ p ⟧`)

A filter. The predicate is evaluated with `$` bound to the incoming emission.

- truthy → the input is emitted **unchanged**;
- falsey → nothing is emitted;
- an error → it propagates, instead of being read as false.

A ward never transforms; use the next stage for that. The predicate is one
expression, not a flow, so `?{ a -> b }` is rejected with a message telling you
to close the ward and continue after it.

Effectful predicates are rejected: Cant has no ordering rules for effects inside
a filter.

### Fork — `|{ a ; b ; c }` (`⫴⟦ a ; b ; c ⟧`)

Ordered branches from the same input value. Each branch receives the identical
immutable input; their emissions are concatenated in source order.

```cant run
5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []    // [6, 10, 25]
```

![The flow graph for a three-branch fork](graphs/fork.svg)

Each branch is a cluster. The dashed edges into them carry the branch number;
the dashed edges back are where the emissions are concatenated.

Forks nest.

#### `:par` — branches at the same time

`:par` runs the branches concurrently instead of one after another:

```cant run
5 -> |{ $ + 1 ; $ * 2 ; $ * $ } :par -> []    // [6, 10, 25]
```

The value is the same as without it. Results are joined in **branch order**
whatever order the branches finish in, so a `:par` fork emits exactly what the
sequential one would.

What changes is *when the effects happen*. Two branches reading files or calling
a host do so at the same time, in no fixed order, and that is the point: it is
worth writing where a fork waits on the world rather than on arithmetic.

<!-- ignore: reads a file this page does not ship and calls a host that is not
     there; the executed forms are conformance/cant/execution/parallel-effectful
     and parallel-with-rescue. -->
```cant ignore
"config.json" -> |{ !@fs.read($)? -> @json.decode? ; !@http.get("https://…")? } :par -> []
```

Four rules:

- **Every branch settles before the fork emits.** If one fails, the fork fails,
  after the others have finished rather than the moment the first one does.
- **At most 16 branches run at once.** More than that queues, keeping order.
- **Effects still need their grants**, and an unmarked one is still refused. A
  parallel branch is an ordinary branch to Rite's effect analysis.
- **`:par` on a single-branch fork is a warning** (`CANT-G024`): there is nothing
  to run it alongside.

Rescues, orbits and wards all work inside a parallel branch, because a branch is
an ordinary chain.

There is no cancellation: a failing branch does not stop the others, it waits for
them. The reasoning is in `docs/adr/0012-parallel-fork-is-a-modifier.md`.

### Orbit — `~{ body }` (`⟲⟦ body ⟧`)

A bounded breadth-first fixed point, and the only cyclic construct in the
language.

<!-- ignore: reads module files this page does not ship; every name in it is
     real, and the same program runs on the site's front page bar the files. -->
```cant ignore
["main"]
  -> *
  -> ~{ !@fs.read($ + ".cant")? -> @regex.find_all($, "use [a-z_]+")? -> * -> replace($, "use ", "") }
     :by str
     :max 4096
  -> []
```

A runnable one, with the same shape:

```cant run
[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 64 -> []
```

![The flow graph for an orbit, showing the feedback edge](graphs/orbit.svg)

The bold pink edge is the orbit's feedback: the body's emissions returning to the
worklist. It is the only cycle a Cant program can contain.

1. The worklist starts as a FIFO of the incoming emissions.
2. Pop the next candidate.
3. Compute its identity — `:by f` applies a pure function `f`; otherwise
   structural value identity is used.
4. An identity already seen is skipped. First occurrence wins.
5. Otherwise record the candidate and emit it as part of the orbit's result.
6. Run the body with the candidate as input.
7. Append the body's emissions to the end of the worklist.
8. Stop when the worklist is empty.
9. Fail with a structured diagnostic if `:max` candidates have been accepted, or
   if Rite's global step or time budget runs out.

Rules:

- `:max` must be a positive integer, and defaults to **1024**.
- `:by` must be pure.
- A value with no stable structural identity needs `:by`.
- Effects in the body are allowed, and run once per first-seen candidate.
- The body runs sequentially.

Reaching `:max` is a failure, not a truncated answer. A traversal larger than
expected fails rather than returning a partial result.

Orbit is the only cyclic construct in the language. There are no feedback edges
to named nodes.

### Rescue — `!{ handler }` (`↯⟦ handler ⟧`)

Where failures go. A capability call answers `ok(value)` or `err(record)`, and a
rescue splits the two:

- `ok(v)` → `v` continues along the flow, unwrapped;
- `err(e)` → the handler runs with `$` bound to `e`, and whatever it emits
  rejoins the flow in place;
- anything else → passes through unchanged.

```cant run
[ok(1), err("nope"), 3] -> * -> !{ "recovered from " + $ } -> []
// [1, recovered from nope, 3]
```

The handler is a stage like any other, so the emission it applies to — the
failure — has to appear in it, as `$` or as the first argument. There is no way
to write a stage that ignores its input, which is why a bare `!{ "" }` is a call
of a string rather than a constant. Substituting a constant with no handler at
all is `unwrap_or($, "")`.

![The flow graph for a rescue, showing the failure edge into its handler](graphs/rescue.svg)

The handler is a cluster, like a fork branch. The dashed edge into it is labelled
`err` and is the only way in.

It is also a flow rather than one expression, so it can report as well as
replace:

<!-- ignore: reads a file this page does not ship; the executed form is
     conformance/cant/execution/rescue-handles. -->
```cant ignore
["a.txt", "b.txt"] -> * -> !@fs.read($) -> !{ $.message -> "failed: " + $ } -> []
```

A handler that emits nothing drops the failure; one that emits several fans out.
Effects inside it are allowed, and are permission-gated like any other.

What a rescue catches is an `err` **arriving at it**. A `panic` — scatter applied
to a non-list, an orbit reaching its `:max` — ends the run and is not routable.
Neither is a failure a `?` has already unwrapped away, which is rejected rather
than left looking like error handling:

```text
error[CANT-G017]: this `?` removes the failure the rescue would route
```

A rescue takes no modifiers, and cannot be a program's first stage: nothing has
been emitted yet, so nothing can have failed.

### Modifiers — `:name value`

Configure the structural form immediately to their left, with no arrow between:

```cant run
[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 1024 -> []
```

The colon must touch the name. That is what keeps `:` usable as Rite's atom
prefix, so `?{ $.level = :error }` reads as the comparison it looks like.

Some take a value and some do not. An orbit's `:by` and `:max` each need one;
a fork's `:par` is the whole modifier, and writing `:par true` is an error
(`CANT-G023`) rather than a second way to spell it. There are three in the
language:

| Modifier | On | Value |
|---|---|---|
| `:by` | an orbit | a pure function computing a candidate's identity |
| `:max` | an orbit | a positive integer, default 1024 |
| `:par` | a fork | none |

## Named flows — `name:{ … }` (`name≔⟦ … ⟧`)

A definition names a flow so a program can use it more than once. Definitions
come before the main flow, which is the last thing in the file:

```cant run
clean:{ trim -> ?{ count($) > 0 } }
["  a  ", "", " b "] -> * -> clean -> []
```

A definition is a **chain**, not a function. It is spliced in wherever its name
appears as a stage, so the program above is the one you would have written out:

```cant run
["  a  ", "", " b "] -> * -> trim -> ?{ count($) > 0 } -> []
```

That is what makes `[]` inside a definition mean what it says — it collects
everything that reached it, across the whole flow — and what makes an effect
inside one need its grant at every place the name is used.

A stage that is **nothing but the name** is a use. Anything else is Rite
expression text, so `clean` is the flow and `clean($)` is a Rite call of a name
nothing defines. A definition is not a value and cannot be passed anywhere.

Definitions may appear in any order and may name each other, and the braces end
them, so a whole program still fits on one line:

```cant run
loud:{ upper -> $ + "!" }
shout:{ trim -> loud }
["  a  "] -> * -> shout -> []
```

Four things are refused:

- **a name defined twice**, since the second one would never run (`CANT-G018`);
- **a name Rite already binds** — a builtin, or a module the program imports —
  because `count` as a stage and `count($)` inside a leaf would mean different
  things (`CANT-G019`);
- **a definition that reaches itself**, directly or through others: a splice has
  no end, and an orbit is the only construct that repeats (`CANT-G020`);
- **a definition nothing uses**, which is usually a typo at the use site that
  became an ordinary Rite name (`CANT-G021`).

`cant graph` and `cant explain` describe the spliced program, because that is
what runs. A definition used twice appears twice in both.

## Modules — `use`

A Cant file is still an expression, not a module — but it can import Rite
modules, which is where named functions come from when builtins and inline
closures are not enough:

<!-- ignore: imports a module this page does not ship; the executed form is
     conformance/cant/execution/use-module. -->
```cant ignore
use mathy
[1, 2, 3] -> * -> mathy.square($) -> []
```

`use name` lines come first, one per line, before the flow. Cant does not
resolve them: the names are emitted verbatim at the top of the generated Rite,
and Rite's module system does everything else — resolution relative to the
program's directory, qualified access, collision reporting. An unknown module
or a typo in a qualified call is Rite's own diagnostic, mapped back onto the
Cant source.

Effect discipline crosses the boundary intact: a call to an effectful module
function takes the marker like any host call (`!logger.shout($)`), and an
unmarked call is rejected by Rite's analysis as it would be in Rite.

## Effects

Cant keeps Rite's effect discipline exactly as it is.

<!-- ignore: reads a file this document does not ship; the runnable version is
     examples/cant/06-capabilities. -->
```cant ignore
"data.json" -> !@fs.read? -> @json.decode? -> .name
```

![The flow graph for a program that reads a file](graphs/effects.svg)

Effectful stages are drawn in cyan, so what a program touches is visible in its
shape.

`!` marks a host call and `@` names the capability, as in Rite. The Rite this
compiles to contains that same marked call, so Rite's own effect analysis sees
it: an effect inside an orbit is as visible, and as permission-gated, as one at
the top level. Run `cant expand` to see it.

Reads are effects, in Cant as in Rite: `@fs.read`, `@env.get` and `@db.query`
need `!` for the same reason `@clock.now` does.

### Standard input

`@stdin` takes the data on the pipe, with the program on `-e`:

```bash
cat access.log | cant run -e '!@stdin.lines -> * -> ?{ contains($, "500") } -> []'
```

`!@stdin.lines` emits the input as a list of lines and `!@stdin.read` as one
string. An empty pipe is an empty list, so the flow runs zero times instead of
once over `""`. Reading stdin is an effect with its own permission, allowed by
default and revocable with `--deny stdin`.

## Failures

A capability call answers a result, `ok(value)` or `err(record)`. A flow has
four postures toward the `err`. Three are Rite's own vocabulary rather than new
syntax; the fourth is the rescue.

**Unwrap, or fail the run.** Postfix `?` unwraps the `ok`. An `err` ends the
whole run with `CANT-R004`, naming the stage and carrying the failure record —
a `?` says the flow only makes sense when this call worked:

<!-- ignore: reads a file this document does not ship; the runnable form is
     examples/cant/06-capabilities. -->
```cant ignore
"config.json" -> !@fs.read? -> @json.decode? -> .name
```

**Drop the failures.** Without `?`, the `err` flows as an ordinary value —
so a ward can filter on it, and `unwrap_or` opens the survivors:

<!-- ignore: reads files this document does not ship; the executed form is
     conformance/cant/execution/error-dropped. -->
```cant ignore
["a.txt", "b.txt"] -> * -> !@fs.read($) -> ?{ is_ok($) } -> unwrap_or($, "") -> []
```

**Replace with a fallback.** `unwrap_or` alone keeps every emission, failed
reads becoming the default:

<!-- ignore: reads files this document does not ship; the executed form is
     conformance/cant/execution/error-replaced. -->
```cant ignore
["a.txt", "b.txt"] -> * -> !@fs.read($) -> unwrap_or($, "") -> []
```

**Route them.** A rescue sends the `err` into a handler flow and unwraps the
`ok`. It is the only posture that hands the failure record to something that can
report on it:

<!-- ignore: reads files this document does not ship; the executed form is
     conformance/cant/execution/rescue-handles. -->
```cant ignore
["a.txt", "b.txt"] -> * -> !@fs.read($) -> !{ "failed: " + $.message } -> []
```

All four are pinned by `conformance/cant/execution/error-dropped`,
`error-replaced`, `error-try-fails-the-run` and `rescue-handles`, interpreted
and compiled alike.

## Determinism

**A Cant program's value is deterministic.** Run it twice on the same input and
it produces the same thing, always:

- stages execute in source order;
- fork branches join in branch order, `:par` or not;
- scatter preserves list order;
- collect preserves emission order;
- orbit uses breadth-first worklist order;
- duplicate detection keeps the first occurrence.

**The order effects happen in is deterministic too, with one exception.** Without
a `:par` fork, effects execute sequentially in exactly the order above. Inside
one, the branches run at the same time, and Cant does not say which of two
branches writes its file or sends its request first. That is the only place in
the language where two effects have no order between them, and it is what `:par`
is asking for.

Console output is the exception to the exception: each branch's output is
buffered and spliced back in branch order, so two branches printing do not
interleave, and a `:par` program prints the same thing every run.

## Comments and strings

```cant run
// A line comment. -> ?{ and ⋇ in here are just text.
/* Block comments too. */
"a -> b" -> replace($, "->", "|{")
```

Operator characters inside a string or a comment are never operators.
`cant fmt` and `cant convert` work from the parse rather than the text, so
neither reaches inside one, and neither mistakes the `[]` in `f([])` for a
collect or the `}` closing a Rite closure for the end of a Cant block.

## What Cant does not have

Each of these is deferred, not overlooked:

- **Functions.** A definition names a flow, not a function: it takes no
  arguments, is not a value, and cannot be exported or imported, because a
  `.cant` file is still an expression rather than a module. For anything that
  needs a parameter or a second file, use Rite's builtins, a closure inside a
  stage, or `use` a Rite module and call its functions by qualified name.
- **Cancellation** and **retries**. A `:par` fork waits for every branch even
  when one has already failed, and a rescue routes a failure but never re-runs
  the stage that produced it.
- **Named anchors and arbitrary feedback edges.** Orbit is the only cycle.
- **Lazy or infinite streams.** Every emission set is finite.
- **A second value model, permission system or host runtime.** All three are
  Rite's, unchanged.

---

This page is the reference. [Your first program](tutorial.md) is the
introduction, [one-liners](one-liners.md) a set of recipes, and
[past the one-liner](projects.md) covers files, modules and tests. Diagnostics
are indexed in [when something goes wrong](diagnostics.md).
