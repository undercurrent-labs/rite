# An HTTP service with real routes

**You will build** a JSON service with path parameters, query parameters, a POST
body, and an error that comes back as a proper `500` instead of taking the server
down — and a client, in the same file, that proves each route works.

**You need** nothing but a Rite install. Everything talks to loopback.

<!-- ci: local-only -->

> **This tutorial's script is not run in CI**, unlike the others. A server blocks
> until it is stopped, so running it unattended needs environment variables that a
> reader would never set — and a page whose tested command differs from its printed
> command is worse than an untested one. It is run locally instead, with
> `cargo test -p rite-cli --test tutorial_scripts -- --ignored`, and every output
> below came from a real run.

## The smallest server

```rite native_only
@http.listen "127.0.0.1:4045" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

```text
rite: listening on http://127.0.0.1:4045
```

`@http.listen` **blocks until you stop it** — Ctrl-C, normally. That is not a
limitation to work around; it is what a server is. The line it prints goes to
stdout, and if you bind port `0` it tells you which port you actually got.

Notice the handler has no `!`. `@http.listen` is not an ordinary call — it is a
declaration of routes, and the handler bodies run later, when a request arrives. The
effect marker goes on whatever a handler *does*.

## Returning a status and a body

```rite native_only
^ 200 ⟨status: #ok⟩
```

Two values, juxtaposed: the status and the JSON body. The atom `#ok` encodes as the
string `"ok"`, so that route answers `{"status":"ok"}`.

An early return is the readable way to branch, because the juxtaposed form does not
survive being buried in a conditional expression:

```rite native_only
GET "/greet/:name" |req| ⟦
  line ← "hello, " + req.path.name
  ? req.query.shout = "1" ⟦
    ^ 200 ⟨greeting: upper(line)⟩
  ⟧
  ^ 200 ⟨greeting: line⟩
⟧
```

`:name` in the path becomes `req.path.name`; `?shout=1` becomes `req.query.shout`.
Both are plain records, so a missing query parameter is `none` and comparing it to
`"1"` is simply false — there is no separate "was it provided" to check.

## Reading a body

```rite native_only
POST "/sum" |req| ⟦
  body ← req.json?
  numbers ← body.numbers ?? []
  ^ 200 ⟨total: numbers → sum, count: numbers → count⟩
⟧
```

`req.json` is a **result**, because a client can send anything, and `?` propagates a
malformed body rather than letting it reach your arithmetic. `?? []` then covers the
case where the JSON was fine but the field was absent — two different failures, two
different tools.

## Failing without falling over

One line — `⊏ @http.recover`, or `use @http.recover` in ASCII — and a handler that
raises answers `500` instead of killing the server:

```rite native_only
@http.listen "127.0.0.1:4045" ⟦
  ⊏ @http.recover

  GET "/boom" ⟦
    ^ 200 ⟨oops: fail("deliberate")⟩
  ⟧
⟧
```

```text
boom    500
```

Without `@http.recover` a raising handler takes the process with it, which is the
wrong trade for a service. An unknown route is `404` with no work from you.

## Handlers do not share memory

This one will cost you an afternoon if nobody says it:

```rite native_only
GET "/set" ⟦
  ! @store.set("t", "k", "value")
  ^ 200 ⟨set: true⟩
⟧
GET "/get" ⟦
  ^ 200 ⟨got: @store.get("t", "k")?⟩
⟧
```

```text
⟨set: true⟩
⟨got: none⟩
```

The write succeeded and the read found nothing. **`@store` does not persist across
requests** — each one gets fresh state, so an in-memory map is not a cache, a
session store, or a counter. For anything that has to outlive a request, use
[a database](../book/db.md) or [the filesystem](../book/files-json.md).

## The whole script

The client lives in the same file. `parallel` runs the two branches together, which
is what lets one script both serve and prove it serves:

```rite
// api.rite — a small JSON service, and a client that exercises it.

◆! serve() ⟦
  @http.listen "127.0.0.1:4045" ⟦
    ⊏ @http.recover

    GET "/health" ⟦
      ^ 200 ⟨status: #ok⟩
    ⟧

    GET "/greet/:name" |req| ⟦
      line ← "hello, " + req.path.name
      ? req.query.shout = "1" ⟦
        ^ 200 ⟨greeting: upper(line)⟩
      ⟧
      ^ 200 ⟨greeting: line⟩
    ⟧

    POST "/sum" |req| ⟦
      body ← req.json?
      numbers ← body.numbers ?? []
      ^ 200 ⟨total: numbers → sum, count: numbers → count⟩
    ⟧

    GET "/boom" ⟦
      ^ 200 ⟨oops: fail("deliberate")⟩
    ⟧
  ⟧
  ^ #served
⟧

◆! exercise() ⟦
  ! @clock.sleep(300)
  base ← "http://127.0.0.1:4045"

  ! println("health  " + str((! @http.get(base + "/health")?).status))
  ! println("greet   " + ((! @http.get(base + "/greet/ada")?).json?).greeting)
  ! println("shout   " + ((! @http.get(base + "/greet/ada?shout=1")?).json?).greeting)
  ! println("sum     " + str(((! @http.post(base + "/sum", ⟨numbers: [1, 2, 3]⟩)?).json?).total))
  ! println("boom    " + str((! @http.get(base + "/boom")?).status))
  ! println("missing " + str((! @http.get(base + "/nope")?).status))
  ^ #exercised
⟧

◆! branch(which) ⟦
  ^ ? which = #serve ⟦ ! serve() ⟧ : ⟦ ! exercise() ⟧
⟧

! parallel([#serve, #exercise], branch)
```

```bash
rite run api.rite --allow net=127.0.0.1
```

```text
rite: listening on http://127.0.0.1:4045
health  200
greet   hello, ada
shout   HELLO, ADA
sum     6
boom    500
missing 404
[#served, #exercised]
```

Run it as written and it serves until you press Ctrl-C — the client output appears
after about a third of a second and the server keeps going. The final
`[#served, #exercised]` is what `parallel` answered, one value per branch in input
order.

`parallel` is concurrency, not threads: the branches interleave whenever one waits,
which is exactly what a sleeping client and a listening server do. Its output is
spliced in input order however the branches finish, so this prints the same lines
every run.

**The grant is `net=127.0.0.1`, and it is needed for the client, not the server.**
Binding loopback is allowed by default; *calling* a host — even your own — is
checked per host. Two different questions, which is why binding a socket grants
nothing about where you may send.

## Next

- [Network: HTTP services](../book/http.md) — middleware chains, custom auth, the
  full request record
- [Network: sockets](../book/sockets.md) — the raw `@udp` and `@tcp` layer beneath
- [Reshaping JSON](json-pipeline.md) — what to do with a body once you have it
