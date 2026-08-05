# 10 — rescue

```cant
["missing.txt", "here.txt"] -> * -> !@fs.read($) -> !{ "missing: " + str($.kind) } -> trim -> []
```

One of these files exists. `!@fs.read($)` answers `ok(text)` for `here.txt` and
`err(record)` for the one that is not there, and the rescue splits them: the
`ok` continues unwrapped, and the `err` goes into the handler with `$` bound to
the failure record. The program answers
`[missing: io.not_found, present]` — both emissions, in the order the paths
were scattered.

The handler is a flow, so it can do more than substitute:

```text
-> !{ $.path -> "could not read " + $ }
```

It is a stage like any other, which means the failure has to appear in it — as
`$`, or in the first argument position. `!{ "" }` calls a string. To substitute
a constant with no handler, `unwrap_or($, "")` is still the shorter answer, and
[09 — failures](../09-failures/README.md) shows it beside the other postures.

What a rescue catches is an `err` reaching it. A `panic` is not routable, and
neither is a failure a `?` has already unwrapped away — that is `CANT-G017`.

Run it:

```bash
cant run examples/cant/10-rescue/main.cant --allow fs:read=examples/cant/10-rescue
```
