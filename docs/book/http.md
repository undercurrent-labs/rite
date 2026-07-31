# HTTP services

Rite can serve HTTP with a small Sinatra-style DSL under `@http.listen`. Handlers are ordinary Rite blocks; they return a **status** and a **JSON body** via multi-value return: `^ 200 ⟨…⟩` (juxta status + body on one return).

> **CLI only.** Real sockets need the native binary (`rite run`), not Studio WASM. In the browser, listen is virtualized.

## Minimal server (copy-paste)

Save as `health.rite`:

```rite browser
@http.listen "127.0.0.1:4040" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

ASCII form (same program):

```rite browser
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

```rite browser
^ 200 ⟨status: #ok⟩
```

means **HTTP status `200`** and **JSON body** `{"status":"ok"}` (the atom `#ok` becomes the string `"ok"` in JSON).

Equivalent explicit form:

```rite browser
^ ⟨status: 200, body: ⟨ok: true⟩⟩
```

| Form | Meaning |
|------|---------|
| `^ 200 ⟨…⟩` | Status code + JSON object body |
| `^ ⟨status: 200, body: …⟩` | Explicit status + body fields |
| `^ "plain text"` | 200 + text/plain |

### Port `0` (ephemeral)

```rite browser
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

```rite browser
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

```rite browser
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

```rite browser
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

`@http.get`, `@http.post` and `@http.request` make outbound requests. Each needs
**net** permission for the target host — this is what `--allow net=…` grants:

```bash
rite run fetch.rite --allow net=api.example.com
```

```rite native_only
resp ← ! @http.get("https://api.example.com/v1/status")?
! @console.println(str(resp.status))

body ← resp.json?
! @console.println(body.message)
```

A record body is sent as JSON; a string is sent verbatim:

```rite native_only
resp ← ! @http.post("https://api.example.com/items", ⟨name: "aura"⟩)?
```

`@http.request` takes the whole request as a record when you need a method or headers:

```rite native_only
resp ← ! @http.request(⟨
  method: "PUT",
  url: "https://api.example.com/items/1",
  headers: ⟨authorization: "Bearer …"⟩,
  body: ⟨name: "renamed"⟩
⟩)?
```

### The response

Deliberately the same shape a handler receives, so both directions of HTTP read alike:

| Field | Meaning |
|-------|---------|
| `resp.status` | Status code as an integer |
| `resp.headers` | Response headers, lowercased names → string |
| `resp.text` | Body as a **result** — unwrap with `?` |
| `resp.json` | Parsed JSON as a **result** |
| `resp.body` | Raw bytes |

The call itself returns a result, like `@fs.read`: a refused connection, DNS failure or
timeout comes back as `err(⟨kind: "net.error", …⟩)` rather than ending the script, so
you can match on it. Requests time out after 30 seconds.

Permission is checked **per host**, and the host is parsed from the URL — a grant for
`example.com` does not open `evil.example`, and credentials or a port in the URL do not
change which host is checked.

Not available in the browser (hosted Studio): there is no socket layer there, so these
return a capability error, the same as `@db`.

## Datagrams (`@udp`)

Below HTTP is the other half of the network host: UDP. There is no connection, so there
is no lifetime to manage beyond the socket itself — `bind`, then `send_to` / `recv_from`
as often as you like, then `close`.

```rite native_only
peer ← ! @udp.bind("127.0.0.1:0")?
sock ← ! @udp.bind("127.0.0.1:0")?

! @udp.send_to(sock, ! @udp.local_addr(peer)?, "ping")?

got ← ! @udp.recv_from(peer, 1000)?
! @console.println("{got.from} said {got.text}")

! @udp.close(sock)?
! @udp.close(peer)?
```

```bash
rite run ping.rite --allow net=127.0.0.1
```

| Call | Answers |
|------|---------|
| `@udp.bind(addr)` | `ok(socket)` — an opaque handle, like a `@db` connection |
| `@udp.local_addr(sock)` | `ok("127.0.0.1:54321")` — the address actually bound |
| `@udp.send_to(sock, addr, data)` | `ok(n)`, the number of bytes sent |
| `@udp.recv_from(sock, timeout_ms)` | `ok(⟨from, data, text⟩)` or `err(…)` |
| `@udp.close(sock)` | `ok(none)`, also for an already-closed handle |

Port `0` asks the OS for a free port, so `@udp.local_addr` is how you learn where you
ended up — the same reason `@http.listen` prints its bound address.

### Waiting, and giving up

`recv_from` waits up to `timeout_ms` (default 1000). **A timeout is an `err` value, not a
raise** — waiting for a datagram that never comes is ordinary, so the script keeps going
and decides for itself:

```rite native_only
sock ← ! @udp.bind("127.0.0.1:0")?
~ ! @udp.recv_from(sock, 250) ⟦
  ok msg → ! @console.println(msg.text)
  err e → ! @console.println("nothing arrived ({e.kind})")
⟧
! @udp.close(sock)?
```

The error record is `⟨kind: "udp.timeout", operation, message, timeout_ms⟩`. Transport
failures use `kind: "udp.error"` and carry `address` instead — so `e.kind` tells the two
apart. Note the missing `?` on the `recv_from` line: `?` would hand the timeout straight
back to the caller, which is exactly what you do *not* want here.

### Bytes on the wire

Rite strings are UTF-8 and a datagram is not, so payloads use the **bytes** type —
the same one `@fs.read_bytes` returns and `@http` puts in `resp.body`.

| Direction | Representation |
|-----------|----------------|
| `send_to` payload | a **string** (sent as its UTF-8 bytes) or a **bytes** value (verbatim) |
| `recv_from` `data` | **bytes** — `len(data)` counts bytes, not characters |
| `recv_from` `text` | the same payload decoded as UTF-8, invalid sequences replaced |

Anything else — a record, an int — is an error rather than a silent stringification.

Bytes received can be sent again unchanged, which is enough to relay or echo. It is not
yet enough to *author* a binary packet: bytes are opaque in Rite today, with no builtin
that converts a hex string to bytes or back, so a DNS query cannot be built from source.
That gap is recorded in `IMPLEMENTATION.md`; when it closes it will close for `@fs` and
`@http` at the same time, because all three name the same type.

### Permissions

Two different checks, and they are the ones `@http` already makes:

| Gate | Rule |
|------|------|
| `bind` address | Loopback binds by default; `0.0.0.0`, `[::]` or a LAN IP needs `--allow net=<host>` — the same policy as `@http.listen` |
| `send_to` destination | Always per host, like an outbound `@http.get` — **including loopback** |

So a script that only talks to itself still needs `--allow net=127.0.0.1`. Binding a
socket grants nothing about where it may send, and a grant for one host never covers
another. The destination is matched as written, before any name is resolved.

Native only: the browser runtime has no socket layer, so `@udp` there is a clear
capability error rather than a stub, the same as `@process`.

## Next

[Modules](modules.md) · [One-liners & REPL](one-liners.md) · [Browser & Studio](browser.md)
