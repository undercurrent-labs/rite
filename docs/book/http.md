# HTTP services

Rite can serve HTTP with a small Sinatra-style DSL under `@http.listen`. Handlers are ordinary Rite blocks with access to a request value.

## Minimal server

```rite
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

- Address `"127.0.0.1:0"` binds an ephemeral port (useful in tests/examples)  
- Handler return shape: **`status_code record_or_body`** (as implemented — status + payload)  
- `^` returns the response from the handler  

```bash
rite run examples/07-http-service/main.rite --allow-all
```

Network listen requires appropriate **net** permissions (`--allow-all` or a net grant).

## Routes and params

```rite
@http.listen "127.0.0.1:0" ⟦
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

### Request value

Handlers can take `|req|` and use:

| Field / form | Meaning |
|--------------|---------|
| `req.path` | Path parameters (e.g. `:word` → `req.path.word`) |
| `req.query` | Query string fields |
| `req.json` | Parsed JSON body as a **result** (use `?`) |
| method/path | Determined by the route line (`GET`, `POST`, …) |

### Middleware

```rite
@http.listen "127.0.0.1:0" ⟦
  use @http.log
  use @http.recover

  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```

- **`@http.log`** — request logging  
- **`@http.recover`** — convert panics/errors into safer responses  

Order matters: put recovery outside so handler failures become HTTP errors instead of killing the process.

## Response patterns

```rite
// JSON object body with 200
^ 200 ⟨ok: true⟩

// Explicit empty-ish success (conceptual)
// ^ @http.response(status: 204)
```

Prefer records for JSON APIs — they encode cleanly.

## Blocking listen

`@http.listen` is **effectful and blocks** until shutdown. That is correct for servers; automated test ladders often skip long-running HTTP examples or run them with special harnesses.

## Client calls

Outbound HTTP (if enabled in your build) is a separate `@http` client surface and needs **net** permission to the target host. Prefer explicit allows:

```bash
rite run fetch.rite --allow net=api.example.com
```

## Studio / browser

In hosted Studio:

- Scripts with `@http.listen` may produce a **virtual** route list  
- Full accept-loop and real sockets need **native** `rite run` / `rite studio`  
- The HTTP panel in Studio is for inspection/simulation, not production hosting  

## Middleware example

See `examples/08-middleware/main.rite` — same shape as the sum/echo service with log + recover.

## Design tips

1. Keep handlers **thin**: parse → pure transform → respond  
2. Use **`?`** on `req.json` and validate with `??` defaults  
3. Put **auth / recover / log** in middleware, not every route  
4. Bind `"127.0.0.1"` in dev; be explicit about `0.0.0.0` only when you mean it  

## Next

[Modules](modules.md) — split servers and helpers across files.
