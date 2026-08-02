# One-liners and the REPL

Rite suits short, self-contained scripts — the kind you reach for between larger tools. Install the CLI first:

```bash
curl -fsSL https://rite.undrc.dev/install | bash
export PATH="$HOME/.local/bin:$PATH"
```

This page is a field guide for **quick scripts**, **pipelines**, and **`rite repl`**.

## Two modes

| Mode | When |
|------|------|
| **One-shot file** | `rite run script.rite` — reproducible, shareable |
| **REPL** | `rite repl` — explore values, define helpers, iterate |

```bash
rite run job.rite --allow-all          # trusted scripts only
rite run job.rite --allow fs:read=./data
rite repl                              # default caps (console/clock/random)
rite repl --allow-all                  # full host for local exploration
```

## REPL basics

```text
$ rite repl
Rite 0.6.1 — type :help for commands
rite〉1 + 2
3
rite〉xs ← [1, 2, 3, 4, 5]
rite〉xs → keep { |n| n % 2 = 0 } → sum
6
rite〉:quit
```

### What the REPL is good at

- Arithmetic, lists, records, pipelines  
- Defining **functions** and **bindings** that stick for the rest of the session  
- Pattern matching and small pure transforms  
- Printing with `@console`  

### Session model (important)

- **Definitions** (bindings like `x ← …`, functions, imports) are kept in a **prelude** and re-applied before each new input.  
- **Expressions** and **effects** (e.g. `! @console.println(...)`) run against that prelude but are **not** stored — they will not re-fire later.  
- **`:reset`** clears the prelude and environment.  
- Each evaluation **restarts** the time/step budget, so **idle time does not count** toward the timeout — only the current expression is on the clock.

### Meta commands

| Command | Meaning |
|---------|---------|
| `:help` | Command list |
| `:bindings` | Show names in the environment after last eval |
| `:load path` | Run a file into the session (and remember path) |
| `:reload` | Re-run last loaded file |
| `:allow fs:read=./data` | Grant a permission |
| `:timeout 600` | Per-eval wall-clock limit in seconds (default 300) |
| `:reset` | Clear session |
| `:quit` | Exit |

History is saved to `~/.rite_history` when possible.

## One-liners (no REPL)

Put a tiny file in `/tmp` or pipe via a file — the CLI is **file-based**:

```bash
cat > /tmp/sum.rite <<'EOF'
! @console.println(str([1, 2, 3, 4] → sum))
EOF
rite run /tmp/sum.rite
```

### Glyph examples

```rite browser
// sum of squares of even numbers
! @console.println(str(
  [1, 2, 3, 4, 5, 6]
    → keep { |n| n % 2 = 0 }
    → map { |n| n * n }
    → sum
))
```

```rite browser
// record merge for config layers
defaults ← ⟨host: "localhost", port: 8080, debug: false⟩
overrides ← ⟨port: 9090⟩
! @console.println(defaults + overrides)
```

```rite browser
// match on a status atom
msg ← ~ #ok ⟦
  #ok → "ready"
  #error → "failed"
  _ → "unknown"
⟧
! @console.println(msg)
```

```rite browser
// JSON in memory (no FS)
data ← ⟨hello: "world", n: 1⟩
! @console.println(@json.encode(data))
```

### ASCII equivalents

```rite browser
do host.console.println(str(
  [1, 2, 3, 4]
    -> keep { |n| n % 2 = 0 }
    -> map { |n| n * n }
    -> sum
))
```

Convert either way:

```bash
rite convert /tmp/sum.rite --to ascii --stdout   # print it
rite fmt --ascii /tmp/sum.rite                   # rewrite it in place
```

## Recipes

### 1. Summarize a list of numbers

```rite browser
nums ← [12, 7, 99, 3, 40]
! @console.println("count=" + str(nums → count))
! @console.println("sum=" + str(nums → sum))
```

### 2. Filter + map text-ish tokens

```rite browser
words ← ["alpha", "beta", "gamma", "δ"]
// keep short names
short ← words → keep { |w| (w → count) <= 4 }
! @console.println(short)
// → [beta, δ]
```

`count` works on strings as well as lists, and counts **characters**, not bytes — `δ`
is one, not two.

### 3. Read JSON file (needs FS)

```rite native_only
raw ← ! @fs.read("data/input.json")?
doc ← @json.decode(raw)?
! @console.println(doc)
```

```bash
rite run job.rite --allow fs:read=./data
```

### 4. Write a report JSON

```rite native_only
report ← ⟨ok: true, total: 42⟩
! @fs.write("out/report.json", @json.encode(report))
```

```bash
rite run job.rite --allow fs:write=./out
```

### 5. Tiny HTTP health server

```rite browser
@http.listen "127.0.0.1:4040" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

```bash
rite run server.rite
# expect: rite: listening on http://127.0.0.1:4040
# other terminal:
curl -sS http://127.0.0.1:4040/health
# → {"status":"ok"}
```

Loopback is allowed under default security. The process **blocks** until Ctrl-C. Full walkthrough: [HTTP services](http.md).

### 6. REPL-driven helper, then save

In the REPL:

```text
rite〉◆ square(n) ⟦ ^ n * n ⟧
rite〉square(12)
144
```

Copy the function into a `.rite` file when it stabilizes; use `:load` / `:reload` while iterating on disk.

## Permissions cheat sheet

| Goal | Flag |
|------|------|
| Pure / console only | (defaults — no flag) |
| Read data dir | `--allow fs:read=./data` |
| Write output | `--allow fs:write=./out` |
| Outbound HTTP host | `--allow net=api.example.com` |
| Script's own arguments | *(none — `! @process.args` always works)* |
| Everything local | `--allow-all` (trusted scripts only) |

## How the session remembers things

Each input is compiled on its own, so the REPL keeps a **prelude** of your definitions
and replays it before every line. Three consequences worth knowing:

**Redefining a name replaces it, keeping its original position.** The newest value wins,
and anything defined in between sees it:

```text
> x ← 1
> ◆ get() ⟦ ^ x ⟧
> x ← 99
> get()
99
```

**An effectful binding performs its effect once.** The session stores the *result*, not
the expression, so a read or a POST does not happen again on every later line:

```text
> data ← ! @fs.read("big.json")     // reads once
> data → count                      // does not re-read
```

**A mutation is not remembered.** `↢` declares a mutable binding and is kept, but a
later `:=` is a statement, not a definition, so the prelude replays the original value:

```text
> n ↢ 0
> n := n + 5
5
> n
0                                   // the declaration replayed
```

Use `:reset` to clear the prelude, and re-declare with `↢` when you want a new starting
value.

## Common “weird” REPL moments

| Symptom | Cause | Fix |
|---------|--------|-----|
| `room` unexpected token | `room` is reserved (game DSL) | Use another name (`place`, `chamber`) |
| Nested list `[[1,2]]` parse error | `[[` is ASCII block open | Use `[ [1, 2], [3, 4] ]` with spaces |
| `ok(42)` parse error | `ok` / `err` are keywords | Use match on atoms / host results + `?` |

## Studio vs CLI

| | Studio (browser) | CLI / REPL |
|--|------------------|------------|
| Pure + console | Yes (WASM) | Yes |
| FS / process | No | Yes (with allows) |
| Real HTTP listen | Virtual / limited | Yes |
| Share snippet | `/studio#s=…` | File / gist |

**https://rite.undrc.dev/studio** is great for pure experiments; use the installed CLI for real files and servers.

## Next

- [Pipelines](pipelines.md) · [Effects](effects.md) · [Files and JSON](files-json.md) · [HTTP](http.md)  
- [Browser & Studio](browser.md)

## Pictures of code

`rite render` draws highlighted source, using the language's own lexer and the
same palette the site uses — so an image in a README cannot drift from the way
the code reads on the page.

```bash
rite render greet.rite --output greet.svg
rite render greet.rite --format png --frame window --output greet.png
cat greet.rite | rite render - --frame box > greet.svg
```

| Flag | Does |
|---|---|
| `--format svg` | Small, and uses whatever monospace font the viewer has. The default |
| `--format svg-font` | Self-contained: the face travels with the picture, ~100× larger |
| `--format png` | Rasterised, for somewhere that will not take an SVG |
| `--frame text \| box \| window` | Background only, a rounded border, or a title bar with dots |
| `--font-size` · `--scale` | Type size, and pixels per unit for PNG |

Layout is computed per column rather than measured, so plain `--format svg` still
lines up in a viewer whose monospace font is not the one you have. Reach for
`svg-font` when the picture has to look identical everywhere, and for `png` when
whatever you are pasting into refuses SVG at all.

Source that does not compile still renders — that is deliberate, so a page
explaining a mistake can show it.
