# Cant

A terminal-typeable, graph-oriented sibling to [Rite](https://rite.foo).

```bash
$ cant -e '[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []'
[2, 4, 6]
```

Cant is not another spelling of Rite. Rite's ASCII and glyph forms are two ways
of writing the same program; Cant composes differently. Every stage emits zero or
more values, and scatter, collect, ward, fork, orbit and rescue change how many
are in flight and where they go. It is a separate language that compiles to Rite and runs on Rite's
runtime, capabilities, budgets and compiler.

- [Your first program](tutorial.md) — start here; the language from one value up
- [The language](language.md) — emissions, stages, and every operator
- [One-liners](one-liners.md) — recipes short enough to type into a shell
- [Past the one-liner](projects.md) — files, modules, configuration, tests,
  binaries, and the REPL
- [Command line](cli.md) — running, formatting, converting, inspecting
- [When something goes wrong](diagnostics.md) — every diagnostic code, and what
  to do about it
- [Graph schema](graph-schema.md) — the JSON `cant graph` emits

Or run it without installing anything: <https://cant.rite.foo/studio> is the same
engine compiled to WebAssembly, and it shows the graph, the generated Rite and
the value side by side.

Cant is experimental: the operator vocabulary and the graph format can still
change between versions.

## The vocabulary

Twelve operators, all typeable, each with at most one glyph you never have to
type.

| Concept | ASCII | Glyph | Meaning |
|---|---:|---:|---|
| Flow | `->` | `→` | Send each current emission through the next stage |
| Current value | `$` | `$` | Where the emission goes in a stage |
| Effect | `!` | `!` | Rite's explicit effect boundary, unchanged |
| Capability | `@` | `@` | Host namespace, as in Rite |
| Scatter | `*` | `⋇` | Expand a list into ordered emissions |
| Collect | `[]` | `⌁` | Materialize the emissions as one list |
| Ward | `?{ p }` | `⊣⟦ p ⟧` | Pass the input only when `p` is truthy |
| Fork | `\|{ a ; b }` | `⫴⟦ a ; b ⟧` | Ordered branches from the same input |
| Orbit | `~{ b }` | `⟲⟦ b ⟧` | Bounded breadth-first fixed point |
| Rescue | `!{ h }` | `↯⟦ h ⟧` | Route a failed emission into a handler flow |
| Definition | `n:{ f }` | `n≔⟦ f ⟧` | Name a flow, spliced in wherever it is used |
| Modifier | `:name v` | same | Configure the form to its left |

Three ASCII spellings do double duty, and position decides which you meant:

- `*` is scatter only when it is a whole stage, so `$ * 2` stays multiplication;
- `:name` is a modifier only right after a block's `}`, so `= :error` stays an
  atom;
- `:{` opens a definition only after a name in the preamble, so a Rite record
  field holding a block, `<< f:{ |x| x } >>`, stays one.

## Reading a program

<!-- ignore: reads module files this page does not ship; every name in it is
     real, and the same walk runs on the site's front page bar the files. -->
```cant ignore
["main"]
  -> *
  -> ~{ !@fs.read($ + ".cant")? -> @regex.find_all($, "use [a-z_]+")? -> * -> replace($, "use ", "") }
     :by str
     :max 4096
  -> []
```

Start from `main` and walk breadth-first: read each unseen module, pull its
`use` lines out with a regex, and follow them. `:by str` means a module reached
twice is visited once; it stops when the worklist empties, or after 4096 unique
modules. Collect every module found.

Three commands show that before running it: `cant graph` gives the topology,
`cant expand` the Rite it becomes, and `cant explain` the same thing in prose.

## Determinism

A Cant program's value is deterministic. Stages run in source order, fork
branches join in branch order, scatter preserves list order, collect preserves
emission order, and an orbit is breadth-first with the first occurrence of a
value winning.

Effects happen in exactly that order too, unless a fork asks for otherwise. The
one construct that runs concurrently is `|{ a ; b }:par`, and even there the
branches' emissions join in branch order — what has no order is when their
effects reach the world.

Orbit is the only cyclic construct, and it cannot run away: `:max` bounds
accepted candidates (1024 by default), and Rite's step and time budgets apply
underneath it.

## Shell quoting

`>`, `|`, `!`, `?` and `*` are shell metacharacters. Quote the expression, as
for `awk`, `sed` or `jq`:

```bash
cant check -e '["a.txt"] -> * -> !@fs.read -> lines -> * -> ?{ $ != "" } -> []'
```

Files and `-` for standard input work too:

```bash
cant run program.cant
cat program.cant | cant run -
```

## Examples

[`examples/cant/`](../../examples/cant/) has one directory per construct, each
with a short explanation of what it does.
