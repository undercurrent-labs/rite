# 07 — standard input

```cant
!@stdin.lines -> * -> ?{ contains($, "500") } -> [] -> count
```

`@stdin` is what makes a one-liner a shell citizen — the program on `-e`, the
data on the pipe:

```bash
cat access.log | cant run -e '!@stdin.lines -> * -> ?{ contains($, "500") } -> [] -> count'
```

Result: how many lines mention `500`. With nothing piped in, the input is an
empty list, the flow runs zero times, and the count is `0` — which is what the
documentation gate sees when it runs this file with a closed stdin.

`!@stdin.lines` emits the input as a list of lines; `!@stdin.read` as one
string. One read, cached, so the two agree if you use both. Reading stdin is an
effect — hence the `!` — with its own permission, allowed by default and
revocable with `--deny stdin`.

More shapes in [the one-liners page](../../../docs/cant/one-liners.md).
