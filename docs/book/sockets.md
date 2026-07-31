# Sockets

`@http` is the polite layer. Underneath it are the two raw ones: **`@udp`**, which
sends datagrams and forgets them, and **`@tcp`**, which opens a connection and keeps
it. Reach for these when you are speaking a protocol that is not HTTP — a line
protocol, a game tick, a health probe, a DNS query.

Both are native-only, both are gated by the same `net` permission as
[HTTP services](http.md), and both hand you an **opaque handle** the same way `@db`
does: you get a value back, you pass it to the next call, and you close it when done.

> **CLI only.** The browser runtime has no socket layer, so every call here is a
> clear capability error in Studio rather than a stub. See [Browser & Studio](browser.md).

## Datagrams (`@udp`)

UDP has no connection, so there is no lifetime to manage beyond the socket itself —
`bind`, then `send_to` / `recv_from` as often as you like, then `close`. Nothing
guarantees a datagram arrives, arrives once, or arrives in order; in exchange you get
to send one without a handshake.

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

[Modules](modules.md) · [HTTP services](http.md) · [Effects and capabilities](effects.md)
