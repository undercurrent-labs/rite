# When something goes wrong

Every Cant diagnostic carries a stable code. This page is the index: what each
one means, and what to do about it.

```text
error[CANT-P003]: unclosed block

  --> report.cant:1:8
   |
 1 | [1] -> ~{ deps
   |        ^^
   |         this orbit is never closed
```

The code does not change between versions, so it is safe to search for and to
match on in a script.

## Reading one

A diagnostic points at **your** source. Some of these originate in Rite's
analysis of the generated code, but the span is mapped back onto the `.cant` you
wrote, and the generated text is attached as related metadata. You should not
have to read generated code to understand a failure.

Three commands show a program without running it:

```bash
cant explain -e '<program>'   # what it does, in prose, plus what it will touch
cant graph   -e '<program>'   # the topology, as DOT or JSON
cant expand  -e '<program>'   # the Rite it becomes, which is what actually runs
```

Start with `cant explain`. It is read from the same graph that executes, so it
cannot describe a different program from the one you wrote.

For a wrong *answer* rather than an error, the REPL's trace arrow shows how many
values each stage emitted:

```text
cant> ~> [1, 2, 3] -> * -> ?{ $ > 1 } -> []
trace  n0:1  n1:3  n2:2  n3:1
[2, 3]
```

## Exit codes

Cant uses Rite's, so `$?` means the same thing after either tool:

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error |
| 2 | usage error |
| 3 | parse error |
| 4 | resolve error |
| 5 | permission denied |
| 7 | test expectation not met |
| 8 | budget exhausted |

## Lexical — `CANT-Lxxx`

The source could not be turned into tokens.

| Code | Means | Do |
|---|---|---|
| `CANT-L001` | A character that cannot begin any Cant token. | Usually a stray glyph, or a smart quote pasted from a document. |
| `CANT-L002` | A string literal reached the end of the source unclosed. | Close the `"`. Watch for a `\` before it. |
| `CANT-L003` | A `/* … */` comment reached the end of the source unclosed. | Close it with `*/`. Block comments do not nest. |

## Parse — `CANT-Pxxx`

The tokens are fine; the shape is not.

| Code | Means | Do |
|---|---|---|
| `CANT-P001` | The source contains no stages. | An empty program, or a file that is only comments. |
| `CANT-P002` | A flow arrow, separator, or block is not followed by a stage. | Something like `-> f` at the very start: a program begins with a value, not with an arrow. |
| `CANT-P003` | A ward, fork, or orbit was never closed. | Add the `}`. |
| `CANT-P004` | A block close with no matching ward, fork, or orbit. | A `}` too many. Note that the braces of a Rite closure inside a stage do not confuse the parser. |
| `CANT-P005` | A flow arrow with nothing after it. | A trailing `->`, often left behind while editing a multi-line flow. |
| `CANT-P006` | A `;` outside a fork. | `;` separates fork branches and nothing else. |
| `CANT-P007` | A glyph-only operator (`⋇`, `⌁`) used inside a leaf expression. | The glyphs are always stages, never expression text. Use `*` or `[]` for multiplication or a list. |
| `CANT-P008` | A `:name value` modifier that follows no structural form. | A modifier attaches to the ward, fork or orbit immediately on its left, with no arrow between. |
| `CANT-P009` | A modifier `:` not followed by a name. | The colon must touch the name, which is what keeps `:` usable as Rite's atom prefix. |
| `CANT-P010` | A modifier name not followed by a value. | `:max` needs its number. |
| `CANT-P011` | A fork branch with no stages. | An extra `;`, or a leading one. |
| `CANT-P012` | A ward predicate is one expression, not a flow. | `?{ a -> b }` is rejected. Close the ward and continue after it: `?{ a } -> b`. |
| `CANT-P013` | Structural blocks nested past the supported depth. | Pull the inner part into a Rite module function and `use` it. |

## Graph validation — `CANT-Gxxx`

The program parsed, but the flow it describes does not hold together.

| Code | Means | Do |
|---|---|---|
| `CANT-G001` | The graph has no entry node. | Cant's own invariant; a report-worthy bug if you see it from source. |
| `CANT-G002` | An edge names a node that is not in the graph. | Only reachable by feeding `cant` a hand-edited graph document. |
| `CANT-G003` | An edge attaches to a port the node does not have. | As above; see [the graph schema](graph-schema.md). |
| `CANT-G004` | A fork branch does not rejoin the fork that opened it. | As above. |
| `CANT-G005` | Scatter used where nothing has been emitted yet. | `*` as the first stage. Scatter needs something to scatter. |
| `CANT-G006` | Collect used where nothing has been emitted yet. | `[]` as the first stage is the empty list *literal*, not a collect. Anywhere else it needs emissions to gather. |
| `CANT-G007` | An orbit `:max` that is not a positive integer. | `:max 0` and `:max -1` bound nothing. |
| `CANT-G008` | An orbit `:by` function that performs an effect. | Identity has to be reproducible for "already seen" to mean anything. Compute the key before the orbit. |
| `CANT-G009` | A cycle that is not owned by an orbit. | Orbit is the only cyclic construct; there are no feedback edges to named nodes. |
| `CANT-G010` | A `:name` the form it is attached to does not accept. | Check the spelling: a ward takes no modifiers, an orbit takes `:by` and `:max`. |
| `CANT-G011` | The same modifier given twice on one form. | Keep the one you meant. |
| `CANT-G012` | Two nodes in a deserialized graph share an identifier. | From a graph document, not from source. |
| `CANT-G013` | A node no edge can reach from the entry. | From a graph document, not from source. |
| `CANT-G014` | A ward predicate that performs an effect. | Cant has no ordering rules for effects inside a filter. Do the read in a stage before the ward. |
| `CANT-G015` | A fork branch or orbit body with no nodes. | An empty `~{ }` or a branch that lowered to nothing. |

## Semantic — `CANT-Sxxx`

The flow holds together; Rite disagrees with what is inside a stage.

| Code | Means | Do |
|---|---|---|
| `CANT-S001` | A host call without a `!` marker, as Rite requires. | Write `!@fs.read`. Reads are effects too: `@env.get` and `@db.query` need the marker as much as `@clock.now`. |
| `CANT-S002` | A name that does not resolve. | A typo, or a function that lives in a Rite module you have not imported. See [Past the one-liner](projects.md#named-functions). |
| `CANT-S003` | A Rite semantic error, remapped onto Cant source. | Rite's message is the real one; the span is yours. |
| `CANT-S004` | A leaf expression that Cant accepted but Rite cannot parse. | Cant does not re-specify Rite's grammar, so a stage that is not valid Rite gets here. The classic is `[[1, 2], [3]]`: Rite lexes `[[` as its block opener, so write `[ [1, 2], [3] ]`. |

## Orbit and budget — `CANT-Oxxx`

| Code | Means | Do |
|---|---|---|
| `CANT-O001` | One of Rite's global budgets (steps, time, collection or string size) was exhausted. | Raise it with `--max-steps` or `--timeout`, or use the REPL's `~>` to find the stage emitting more than you expected. |
| `CANT-O002` | An orbit accepted its `:max` candidates and stopped. | **Not** a truncated answer: the run fails rather than returning a partial result. Raise `:max` if the traversal really is that large, or tighten the ward inside the body. |

## Runtime — `CANT-Rxxx`

| Code | Means | Do |
|---|---|---|
| `CANT-R001` | A Rite runtime failure, remapped onto Cant source. | Rite's message says what happened. The most common cause is a missing `?`: a capability answers a *result*, so `!@fs.read -> lines` hands `lines` an `ok(…)` rather than a string. |
| `CANT-R002` | A capability the run was not granted. | Grant it with `--allow`, or `--allow-all` for a program you trust. `cant explain` lists what a program needs before you run it. |
| `CANT-R003` | Scatter applied to something that is not a list. | Reported at the `*`, not somewhere inside generated code. Check what the stage before it actually emits. |

## Expansion — `CANT-Xxxx`

| Code | Means | Do |
|---|---|---|
| `CANT-X001` | Generated Rite that Rite's own parser rejected. | A bug in Cant, not in your program. The generated file is named in the diagnostic; please report it with the `.cant` that produced it. |

## Version — `CANT-Vxxx`

Reserved for version negotiation on graph documents. No code in this group is
emitted today; the prefix is allotted so a consumer matching on the letter does
not need rewriting when one appears.

## The quiet failure

Not everything that goes wrong produces a diagnostic:

```text
$ cant -e '"data.json" -> !@fs.read? -> @json.decode -> .name' --allow fs:read=.
$ echo $?
0
```

No output, no error, exit 0. `@json.decode` also answers a result, so `.name`
projected a field out of an `ok(…)`, found nothing, and answered `none`, which
is not printed. One `?` per capability:

```bash
cant -e '"data.json" -> !@fs.read? -> @json.decode? -> .name' --allow fs:read=.
```

`cant test` catches this. Comparing against an expectation notices a program
that answers `none`; checking the exit code does not.

## Machine-readable diagnostics

`--json-errors` emits the whole structure on stdout, for an editor or a script:

```bash
cant check -e '[1] -> ~{ deps' --json-errors
```

Each entry carries `code`, `severity`, `message`, and labelled spans. Match on
the code: the prose is not stable between versions.
