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

Payloads are built with the byte builtins — `from_hex`, `bytes`, `to_hex`, `to_text`,
`byte_at`, and `concat` / `slice` / `count`, which understand bytes as well as lists and
strings. A DNS query header, which is not valid UTF-8 anywhere:

```rite browser
header ← from_hex("abcd01000001000000000000")?
question ← bytes([0, 1, 0, 1])
packet ← concat(header, question)

! @console.println(to_hex(packet))
! @console.println("id " + to_hex(slice(packet, 0, 2)))
```

They name the same type as `@fs.read_bytes` and `@http`'s `resp.body`, so a payload read
from one can be sent by another without conversion. See
[Values and atoms](values.md) for the full set.

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

## Streams (`@tcp`)

`@udp` has no connection; `@tcp` is all connection. A connection is an opaque handle,
like a `@udp` socket or a `@db` connection, and the client half reads the same way:

```rite native_only
conn ← ! @tcp.connect("127.0.0.1:9000")?

! @tcp.send(conn, "ping\n")?
reply ← ! @tcp.recv(conn, 1024, 5000)?
! @console.println(to_text(reply)?)

! @tcp.close(conn)?
```

```bash
rite run ping.rite --allow net=127.0.0.1
```

| Call | Answers |
|------|---------|
| `@tcp.connect(addr)` | `ok(conn)` — an opaque handle |
| `@tcp.send(conn, data)` | `ok(n)`, the number of bytes written — always the whole payload |
| `@tcp.recv(conn, max_bytes, timeout_ms)` | `ok(bytes)` or `err(…)` — see below |
| `@tcp.peer_addr(conn)` | `ok("host:port")` — the other end |
| `@tcp.local_addr(conn)` | `ok("host:port")` — this end |
| `@tcp.close(conn)` | `ok(none)`, also for an already-closed handle |
| `@tcp.listen addr ⟦ \|conn\| … ⟧` | Blocks until shutdown; runs the block per connection |

`connect` gives up after 30 seconds, the ceiling `@http` puts on a request. A refused
connection, like a timed-out one, is an `err` value rather than a raise.

### Reading: two ways to get no bytes

TCP is a stream, so a `recv` returns *up to* `max_bytes` (default `65536`) — a short
read is normal, not a truncation. What it cannot do is conflate the two reasons you
might get nothing back, so it does not:

| Answer | Means |
|--------|-------|
| `ok(bytes)`, `len(bytes) > 0` | Data arrived |
| `ok(bytes)`, `len(bytes) = 0` | **The peer closed the stream.** End of input; reading again will say the same |
| `err(⟨kind: "tcp.timeout", …⟩)` | Nothing arrived within `timeout_ms`. **The connection is still open** — ask again, or give up |

```rite native_only
conn ← ! @tcp.connect("127.0.0.1:9000")?
~ ! @tcp.recv(conn, 1024, 250) ⟦
  ok data → ? len(data) = 0 ⟦
    ! @console.println("peer hung up")
  ⟧ : ⟦
    ! @console.println(to_hex(data))
  ⟧
  err e → ! @console.println("nothing yet ({e.kind})")
⟧
! @tcp.close(conn)?
```

Neither is a raise, which is why neither line ends in `?`: `?` would hand the timeout
straight back to the caller, and that is exactly what you do not want here. Transport
failures use `kind: "tcp.error"` and carry `address`, so `e.kind` tells them apart.

Payloads are the **bytes** type and the byte builtins, exactly as in `@udp` — `send`
takes a string (sent as UTF-8) or a bytes value (sent verbatim), and `recv` answers
bytes. A binary frame is built with `from_hex` / `bytes` / `concat`, not with a
`@tcp`-only spelling:

```rite browser
frame ← concat(from_hex("0102")?, bytes([0, 255]))
! @console.println(to_hex(frame))
```

### Serving

The server is callback-shaped. There is no `accept`:

```rite native_only
! @tcp.listen "127.0.0.1:9000" ⟦ |conn|
  got ← ! @tcp.recv(conn, 1024, 5000)?
  ! @tcp.send(conn, "echo: " + to_text(got)?)?
⟧
```

```bash
rite run echo.rite
# in another terminal:  printf 'hello\n' | nc 127.0.0.1 9000
```

The block runs **once per accepted connection**, in its own task — a slow connection
does not hold up the next one — with the connection bound to its parameter. It sees
the top-level bindings and functions it was written next to, and **the connection is
closed when the block returns**. That is the whole lifetime rule, and it is why there
is no `accept`: a connection handed back to the script would have a lifetime the
language cannot express, since Rite has no destructors and no scope-bound resources.

`@tcp.listen` **blocks until shutdown** (Ctrl-C), as `@http.listen` does. Port `0`
picks a free port and the bound address is printed, which is the only way to learn it:

```text
rite: listening on tcp://127.0.0.1:54321
```

> Unlike `@http.listen`, `@tcp.listen` takes the effect marker: it is an ordinary
> capability call wearing a nicer shape, so `!` (or `do`) is required, like every
> other `@tcp` call.

### Who is on the other end

A server usually wants to log the client it just accepted:

```rite native_only
! @tcp.listen "127.0.0.1:9000" ⟦ |conn|
  ! println("connection from " + ! @tcp.peer_addr(conn)?)
  ! @tcp.send(conn, "hello\n")?
⟧
```

`@tcp.peer_addr` is the far end and `@tcp.local_addr` is the near end, so the two swap
meaning depending on which side you ask — a client's `peer_addr` is the server it
dialled, and its `local_addr` is the ephemeral source port the operating system chose
for it.

Both are fixed for the life of a connection and are read once, when it is accepted or
opened. That matters in practice: asking is never blocked by a `recv` that is still
waiting, which is exactly when a server wants to know who has gone quiet.

They need an open connection. After `@tcp.close` the handle refers to nothing, and
asking raises rather than answering `err` — using a closed handle is a mistake in the
script, not something the network did.

### Permissions

The two gates are the ones `@http` and `@udp` already apply, reached through the same
code:

| Gate | Rule |
|------|------|
| `listen` address | Loopback binds by default; `0.0.0.0`, `[::]` or a LAN IP needs `--allow net=<host>` |
| `connect` destination | Always per host, like an outbound `@http.get` — **including loopback** |

So a client that dials its own machine still needs `--allow net=127.0.0.1`, while a
loopback server needs no grant at all. Listening grants nothing about where you may
connect, and a grant for one host never covers another.

Native only: the browser runtime has no socket layer, so `@tcp` there is a clear
capability error rather than a stub, the same as `@udp` and `@process`.

## Next

[Modules](modules.md) · [One-liners & REPL](one-liners.md) · [Browser & Studio](browser.md)
