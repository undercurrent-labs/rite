# HTTP services

Rite can serve HTTP with a small Sinatra-style DSL under `@http.listen`. Handlers are ordinary Rite blocks; they return a **status** and a **JSON body**.

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
  use @http.log
  use @http.recover

  GET "/health" ⟦
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

```bash
rite run examples/07-http-service/main.rite --allow-all
```

(That example uses port `0` — watch the `listening on` line.)

### Request value (`|req|`)

| Field | Meaning |
|-------|---------|
| `req.path` | Path parameters (`:word` → `req.path.word`) |
| `req.query` | Query string as a record |
| `req.json` | Parsed JSON body as a **result** (unwrap with `?`) |
| `req.uri` | Path string |
| `req.method` | `"GET"`, `"POST"`, … |

### Middleware

```rite
@http.listen "127.0.0.1:4040" ⟦
  use @http.log
  use @http.recover

  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

- **`@http.log`** — request logging  
- **`@http.recover`** — turn panics into safer error responses  

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
