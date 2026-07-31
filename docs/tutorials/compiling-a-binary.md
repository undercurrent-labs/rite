# Compiling to a binary

**You will build** a word-count tool, compile it to a native executable, and see
what happens to its permissions when you do.

**You need** a Rust toolchain — `rite build` writes a Rust crate and hands it to
cargo. `rite run` needs nothing but the `rite` binary; this is the one thing that
does.

<!-- ci: local-only -->

> **This tutorial's script is not run in CI**, unlike most. A cold `rite build`
> compiles the whole runtime and costs minutes — the same reason the compiler's own
> end-to-end tests are `#[ignore]`d. It is run locally with
> `cargo test -p rite-cli --test tutorial_scripts -- --ignored`, and every output
> below came from a real run.

## The script

```rite
◆ tally(text) ⟦
  ^ ⟨lines: count(lines(text)), words: count(words(text)), chars: len(text)⟩
⟧

◆! main() ⟦
  argv ← ! @process.args()
  path ← ? count(argv) = 0 ⟦ "sample.txt" ⟧ : ⟦ first(argv) ⟧
  text ← ! @fs.read(path)?
  t ← tally(text)
  ! println(str(t.lines) + " lines, " + str(t.words) + " words, " + str(t.chars) + " chars")
⟧
```

`lines` and `words` split text the way you would expect; `len` on a string counts
characters. `tally` is pure, so it can be tested with a literal string — see
[Testing what you built](testing-what-you-built.md).

Interpreted, with a two-line `sample.txt`:

```bash
rite run wc.rite --allow fs:read=.
```

```text
2 lines, 5 words, 24 chars
```

## Compiling it

```bash
rite build wc.rite --allow fs:read=. --output ./wc
```

```text
built ./wc
```

The first build is slow — it compiles the Rite runtime itself — and later builds
reuse that work from a shared cache. Artifacts go to `.rite/build/<hash>/` next to
your script, not into your source tree.

Now it is an ordinary executable:

```bash
./wc
./wc other.txt
```

```text
2 lines, 5 words, 24 chars
1 lines, 3 words, 6 chars
```

Note there is no `--` before the filename. `rite run wc.rite -- other.txt` needs one
to separate your arguments from the interpreter's; a compiled binary has no
interpreter in front of it, so its own argv *is* the script's.

## The permissions came with it

This is the part worth understanding, because it is not what most people assume.

**The `--allow` flags are baked in at build time.** The binary is not a script that
reads flags at startup; it carries the exact `PermissionSet` those flags produced,
and it enforces it:

```bash
./wc /etc/hostname
```

```text
permission denied: fs:read permission denied for `/etc/hostname`
```

You cannot widen it from outside. There is no `--allow` on the compiled binary,
which is the point: the person who built it decided what it may touch, and shipping
it does not hand that decision to whoever runs it.

**A relative grant stays relative.** `--allow fs:read=.` means "the directory the
binary runs in", not "the directory it was built in" — so the tool works when copied
somewhere else:

```bash
cd /tmp/elsewhere && /path/to/wc
```

```text
2 lines, 5 words, 24 chars
```

That is deliberate: `--allow fs:write=./out` on a distributable tool means `./out`
wherever it lands. A grant naming a path *outside* the build directory is baked
absolute instead, because an absolute path is what you asked for. The one ambiguous
case is an explicitly absolute grant that happens to point inside the build
directory — it is indistinguishable from the relative form and is treated as
relative.

## What actually got compiled

The generated crate reports what the backend managed:

```text
// backend: 0 of 0 top-level statements and 2 of 2 functions lowered to Rust
```

Both functions became Rust. Anything the backend cannot express yet falls back to
the interpreter **per statement**, inside the same binary — so a script always
compiles, and the parts that lower are simply faster. `rite build --emit-rust`
prints where the crate was written if you want to read it.

Compiled statements must produce identical results to interpreted ones. That is not
aspirational: the interpreter is normative, and a differential suite runs
conformance cases both ways to hold the two together.

> **The two paths have disagreed.** Until recently a compiled binary ran the
> top-level statements and stopped, never calling `main` — so a script written the
> way this one is compiled to a binary that printed nothing and exited `0`. The
> conformance fixtures are written as top-level statements, where both paths agreed,
> so nothing caught it. If a compiled binary ever behaves differently from
> `rite run`, that is a bug worth reporting rather than a subtlety to work around.

## The whole script

Save as `wc.rite`, beside a `sample.txt`:

```rite
// wc.rite — count lines, words and characters.

◆ tally(text) ⟦
  ^ ⟨lines: count(lines(text)), words: count(words(text)), chars: len(text)⟩
⟧

◆! main() ⟦
  argv ← ! @process.args()
  path ← ? count(argv) = 0 ⟦ "sample.txt" ⟧ : ⟦ first(argv) ⟧
  text ← ! @fs.read(path)?
  t ← tally(text)
  ! println(str(t.lines) + " lines, " + str(t.words) + " words, " + str(t.chars) + " chars")
⟧
```

```bash
rite run wc.rite --allow fs:read=.
```

```text
2 lines, 5 words, 24 chars
```

The gate runs the interpreted form, because that is the part whose output is worth
pinning; the build is exercised by the compiler's own `--ignored` tests, including
one that asserts a compiled binary calls `main` exactly as the interpreter does.

## When to compile

Not by default. `rite run` starts fast, needs no toolchain, and is what you want
while writing. Reach for `rite build` when you are **shipping** — a tool for someone
without Rite installed, a container that should hold one binary, or a hot loop where
the compiled functions earn their build time.

## Next

- [Compiling to Rust](../book/compiling.md) — the IR, the backend, and what lowers
- [Building a CLI](cli-tool.md) — the tool this compiles
- [Testing what you built](testing-what-you-built.md) — test it before you ship it
