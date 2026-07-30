# One-liners and the REPL

Rite is handy for **scratch work** on a machine where you already installed the CLI:

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
rite run job.rite --allow-all          # demos / trusted personal scripts
rite run job.rite --allow fs:read=./data
rite repl                              # default caps (console/clock/random)
rite repl --allow-all                  # full host for local exploration
```

## REPL basics

```text
$ rite repl
Rite 0.1.8 — type :help for commands
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
- Each evaluation **restarts** the time/step budget, so **idle time does not count** toward the timeout (this used to cause `execution wall-clock timeout exceeded` after ~60s of sitting in the REPL).

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

```rite
// sum of squares of even numbers
! @console.println(str(
  [1, 2, 3, 4, 5, 6]
    → keep { |n| n % 2 = 0 }
    → map { |n| n * n }
    → sum
))
```

```rite
// record merge for config layers
defaults ← ⟨host: "localhost", port: 8080, debug: false⟩
overrides ← ⟨port: 9090⟩
! @console.println(defaults + overrides)
```

```rite
// match on a status atom
msg ← ~ #ok ⟦
  #ok → "ready"
  #error → "failed"
  _ → "unknown"
⟧
! @console.println(msg)
```

```rite
// JSON in memory (no FS)
data ← ⟨hello: "world", n: 1⟩
! @console.println(@json.encode(data))
```

### ASCII equivalents

```rite
do host.console.println(str(
  [1, 2, 3, 4]
    -> keep { |n| n % 2 = 0 }
    -> map { |n| n * n }
    -> sum
))
```

Convert either way:

```bash
rite convert /tmp/sum.rite --to ascii --stdout
rite fmt --ascii /tmp/sum.rite --stdout
```

## Work-machine recipes

### 1. Summarize a list of numbers

```rite
nums ← [12, 7, 99, 3, 40]
! @console.println("count=" + str(nums → count))
! @console.println("sum=" + str(nums → sum))
```

### 2. Filter + map text-ish tokens

```rite
words ← ["alpha", "beta", "gamma", "δ"]
// keep short names
short ← words → keep { |w| (w → count) <= 5 }
! @console.println(short)
```

*(String length via pipeline `count` depends on builtins treating strings as countable; if a form fails, use list length on `words` only.)*

### 3. Read JSON file (needs FS)

```rite
raw ← ! @fs.read("data/input.json")?
doc ← @json.decode(raw)?
! @console.println(doc)
```

```bash
rite run job.rite --allow fs:read=./data
```

### 4. Write a report JSON

```rite
report ← ⟨ok: true, total: 42⟩
! @fs.write("out/report.json", @json.encode(report))
```

```bash
rite run job.rite --allow fs:write=./out
```

### 5. Tiny HTTP health server

```rite
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
| Everything local | `--allow-all` (trusted scripts only) |

## Common “weird” REPL moments

| Symptom | Cause | Fix |
|---------|--------|-----|
| `execution wall-clock timeout exceeded` after sitting idle | Old builds started a 60s clock at REPL open | Upgrade CLI (`curl … \| bash`); new builds restart the budget every eval |
| Name not found for `x` after binding | Old builds did not keep a session prelude | Upgrade; definitions now accumulate in-session |
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
