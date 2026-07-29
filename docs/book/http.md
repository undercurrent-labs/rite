# HTTP services

Rite can serve HTTP with a small Sinatra-style DSL under `@http.listen`. Handlers are ordinary Rite blocks; they return a **status** and a **JSON body** via multi-value return: `^ 200 ⟨…⟩` (juxta status + body on one return).

> **CLI only.** Real sockets need the native binary (`rite run`), not Studio WASM. In the browser, listen is virtualized.

## Minimal server (copy-paste)

Save as `health.rite`:

```rite
@http.listen "127.0.0.1:4040" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

ASCII form (same program):

```rite
host.http.listen "127.0.0.1:4040" [[
  GET "/health" [[
    return 200 <<status: :ok>>
  ]]
]]
```

Run (loopback does not need `--allow-all`, but using it is fine):

```bash
rite run health.rite
# or: rite run health.rite --allow-all
```

You should see:

```text
rite: listening on http://127.0.0.1:4040
```

In another terminal:

```bash
curl -sS http://127.0.0.1:4040/health
# → {"status":"ok"}
```

Stop the server with **Ctrl-C**. The process **blocks** until then (that is expected).

### Handler return shape

The juxta form used above:

```rite
^ 200 ⟨status: #ok⟩
```

means **HTTP status `200`** and **JSON body** `{"status":"ok"}` (the atom `#ok` becomes the string `"ok"` in JSON).

Equivalent explicit form:

```rite
^ ⟨status: 200, body: ⟨ok: true⟩⟩
```

| Form | Meaning |
|------|---------|
| `^ 200 ⟨…⟩` | Status code + JSON object body |
| `^ ⟨status: 200, body: …⟩` | Explicit status + body fields |
| `^ "plain text"` | 200 + text/plain |

### Port `0` (ephemeral)

```rite
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

Rite prints the real port:

```text
rite: listening on http://127.0.0.1:54321
```

Use that URL with `curl`. Prefer a fixed port while learning.

## Routes and params

```rite
@http.listen "127.0.0.1:4040" ⟦
  use @http.log          // or glyph: ⊏ @http.log
  use @http.recover      // or glyph: ⊏ @http.recover

  GET "/health" ⟦
    ! @console.println("health check")   // prints to the server process stdout
    ^ 200 ⟨status: #ok⟩
  ⟧

  GET "/echo/:word" |req| ⟦
    ^ 200 ⟨echo: req.path.word, query: req.query⟩
  ⟧

  POST "/sum" |req| ⟦
    payload ← req.json?
    numbers ← payload.numbers ?? []
    ^ 200 ⟨
      total: numbers → sum,
      count: numbers → count
    ⟩
  ⟧
⟧
```

### Middleware

| Form | Glyph | Effect |
|------|-------|--------|
| `use @http.log` | `⊏ @http.log` | Access log on **stderr**: `rite: GET /path 200 3ms` |
| `use @http.recover` | `⊏ @http.recover` | Handler errors/panics → JSON `500` instead of raw failure |
| `use { \|req, next\| … }` | `⊏ { \|req, next\| … }` | Custom middleware: call `next(req)` to continue, or return a response to short-circuit |

Declaration order is **outer-first** (first `use` runs first). Built-ins and custom closures share the same chain.

Handler `! @console.println(...)` writes to the **server process stdout** (flushed after each request). That is separate from the access log on stderr.

```rite
@http.listen "127.0.0.1:4040" ⟦
  ⊏ @http.log
  ⊏ @http.recover
  GET "/health" ⟦
    ! @console.println("hit")
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

```bash
rite run examples/07-http-service/main.rite --allow-all
```

(That example uses port `0` — watch the `listening on` line.)

### Custom middleware (HTTP auth)

Closures receive the request and a **`next`** callable. Call `next(req)` to run the rest of the chain (more middleware + the route). Return a status/body **without** calling `next` to short-circuit (typical for auth failures).

```rite
@http.listen "127.0.0.1:4040" ⟦
  use @http.log
  use @http.recover

  use { |req, next|
    token ← req.headers.authorization ?? ""
    ? token = "Bearer secret" ⟦
      next(req)
    ⟧ else ⟦
      ^ 401 ⟨error: #unauthorized⟩
    ⟧
  }

  GET "/health" ⟦ ^ 200 ⟨status: #ok⟩ ⟧
  GET "/secret" ⟦ ^ 200 ⟨ok: true⟩ ⟧
⟧
```

```bash
rite run examples/08-middleware/main.rite --allow-all
# curl without token → 401; with -H 'Authorization: Bearer secret' → 200
```

Header names on `req.headers` are **lowercase** (`authorization`, `content-type`, …).

### Request value (`|req|`)

| Field | Meaning |
|-------|---------|
| `req.path` | Path parameters (`:word` → `req.path.word`) |
| `req.query` | Query string as a record |
| `req.headers` | Request headers as a record (lowercase names → string) |
| `req.json` | Parsed JSON body as a **result** (unwrap with `?`) |
| `req.uri` | Path string |
| `req.method` | `"GET"`, `"POST"`, … |

## Permissions

| Bind address | Default secure |
|--------------|----------------|
| `127.0.0.1` / `localhost` | Allowed |
| Other interfaces (`0.0.0.0`, LAN IPs) | Needs `--allow net=…` or `--allow-all` |

```bash
rite run server.rite --allow-all
rite run server.rite --allow net=0.0.0.0
```

## Troubleshooting

| Symptom | Likely cause |
|---------|----------------|
| Process sits with no output | Older CLI without listen logging — upgrade, or use a fixed port and try curl anyway |
| `Address already in use` | Something else on that port — pick another (`4041`, …) or kill the old process |
| `bind failed` / permission | Non-loopback without net allow |
| Studio shows virtual routes only | Expected in WASM — use `rite run` for real sockets |
| Parse errors on `⟦` / `⟨` | Encoding / font — use the ASCII form (`[[ ]]`, `<< >>`) |
| `curl` connection refused | Server not running, wrong port, or already exited with an error |
| Lots of red diagnostics in Studio | Run via CLI; Studio is not a full native host |

## Blocking listen

`@http.listen` is **effectful and blocks** until shutdown (Ctrl-C). Automated tests set `RITE_HTTP_TEST=1` so servers auto-stop; normal runs do not.

## Client calls

Outbound HTTP (if used) needs **net** permission to the target host:

```bash
rite run fetch.rite --allow net=api.example.com
```

## Next

[Modules](modules.md) · [One-liners & REPL](one-liners.md) · [Browser & Studio](browser.md)
