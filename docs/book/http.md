# Network: HTTP services

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
| `^ ⟨status: 200, body: …, headers: ⟨…⟩⟩` | …and response headers |
| `^ "plain text"` | 200 + text/plain |

`@http.response(status, body, headers)` builds that explicit record for you, which is
handy when the status is computed rather than written literally:

```rite browser
! @console.println(@http.response(201, ⟨id: 7⟩))
! @console.println(@http.response(404))
```

```text
⟨status: 201, body: ⟨id: 7⟩⟩
⟨status: 404, body: none⟩
```

It is not marked, because it builds a record and touches nothing. Both the body and
the headers are optional, the body defaulting to `none`.

### Content types and headers

Without a `headers` field the media type is inferred from the **type of the body**:

| Body | Content-Type |
|------|--------------|
| String | `text/plain; charset=utf-8` |
| Bytes | `application/octet-stream` |
| Record / list / anything else | `application/json` |
| `none` | no body, status only |

That inference is a guess, and for HTML it is the wrong one — a browser renders
`text/plain` markup as source text. An explicit `content-type` **replaces** it:

```rite browser
^ ⟨
  status: 200,
  body: "<h1>hello</h1>",
  headers: ⟨"content-type": "text/html; charset=utf-8"⟩
⟩
```

Header names hold hyphens, so they need **quoting** as record keys — `⟨"content-type": …⟩`,
not `⟨content-type: …⟩`, which parses as a subtraction. Names without hyphens
(`location`, `etag`) can be written bare.

A header whose value is a **list** is sent once per element. This is the only way to
set more than one cookie, since a record holds a single value per key:

```rite browser
^ ⟨
  status: 204,
  headers: ⟨"set-cookie": ["session=abc; Path=/", "theme=dark; Path=/"]⟩
⟩
```

Redirects need nothing else:

```rite browser
^ @http.response(302, none, ⟨location: "/signed-in"⟩)
```

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

### Catch-all routes

A `:name` param matches **one** segment. A final `*name` matches the whole
remainder, including slashes and including nothing at all:

```rite browser
@http.listen "127.0.0.1:4040" ⟦
  GET "/files/*rest" |req| ⟦
    ^ 200 ⟨wanted: req.path.rest⟩
  ⟧
⟧
```

| Request | `req.path.rest` |
|---------|-----------------|
| `/files/a.txt` | `"a.txt"` |
| `/files/deep/nested/a.txt` | `"deep/nested/a.txt"` |
| `/files` | `""` |

A bare `*` matches the same way without binding anything.

**Specific routes always win**, whatever the declaration order — a catch-all is only
tried once every literal and `:param` route has failed to match. That is why a
site-wide `GET "/*path"` sit at the top of the block with its API routes below it.

### Serving files

`@http.file(root, subpath)` reads a file under `root` and builds the response for it,
with a `content-type` from the extension. It is effectful and needs a read grant:

```bash
rite run site.rite --allow fs:read=./public
```

```rite native_only
@http.listen "127.0.0.1:4040" ⟦
  GET "/*path" |req| ⟦
    ^ ! @http.file("./public", req.path.path)?
  ⟧
⟧
```

Two things it does on your behalf:

- **The subpath cannot escape `root`.** `../../etc/passwd` comes back as
  `err(⟨kind: "http.forbidden", …⟩)`, checked before the file is opened. The read
  grant still applies on top of that.
- **A directory resolves to its `index.html`**, so `/` works with no special case.

It returns a result, so a missing file is a value you decide about rather than a
crash — `err(⟨kind: "http.not_found", …⟩)`.

Recognised extensions cover what a static site ships: `html`, `css`, `js`, `mjs`,
`json`, `svg`, `png`, `jpg`, `gif`, `webp`, `avif`, `ico`, `woff`, `woff2`, `ttf`,
`otf`, `wasm`, `xml`, `csv`, `txt`, `pdf`, `mp4`, `webm`, `mp3`, `wav`. Anything else
is `application/octet-stream` — a download, never a guess a browser might sniff as
script.

#### A single-page app

An SPA needs its client-routed deep links (`/settings/profile`) to return the shell
rather than a 404. Try the file, fall back to the index:

```rite native_only
@http.listen "127.0.0.1:4040" ⟦
  GET "/api/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧

  GET "/*path" |req| ⟦
    hit ← ! @http.file("./public", req.path.path)
    ^ ~ hit ⟦
      ok page → page
      err e → ! @http.file("./public", "index.html")?
    ⟧
  ⟧
⟧
```

The API route keeps answering as itself: it is specific, so it is matched first.

### Middleware

| Form | Glyph | Effect |
|------|-------|--------|
| `use @http.log` | `⊏ @http.log` | Access log on **stderr**: `rite: GET /path 200 3ms` |
| `use @http.recover` | `⊏ @http.recover` | Handler errors/panics → JSON `500` instead of raw failure |
| `use { \|req, next\| … }` | `⊏ { \|req, next\| … }` | Custom middleware: call `next(req)` to continue, or return a response to short-circuit |

Declaration order is **outer-first** (first `use` runs first). Built-ins and custom closures share the same chain.

### Sharing state with handlers

Handlers share the capability host and open handles of the script that called
`listen`. Open a `@db` connection or seed a `@store` namespace before
`@http.listen`, and every handler uses that same connection and store —
concurrent requests included, so one database connection serves the whole
server instead of one writer per request:

```rite native_only
conn ← ! @db.open("app.duckdb")?
@http.listen "127.0.0.1:4040" ⟦
  GET "/items" ⟦
    rows ← ! @db.query(conn, "SELECT * FROM items")?
    ^ 200 ⟨items: rows⟩
  ⟧
⟧
```

Requests run concurrently — including through custom middleware — and each
gets its own console buffer and budget. What persists across requests is the
module scope (top-level bindings), the capability host (`@db`, `@store`,
`@env` overlays) and the handle table.

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
| `req.path` | Path parameters (`:word` → `req.path.word`, `*rest` → `req.path.rest`) |
| `req.query` | Query string as a record |
| `req.headers` | Request headers as a record (lowercase names → string) |
| `req.json` | Parsed JSON body as a **result** (unwrap with `?`) |
| `req.form` | `application/x-www-form-urlencoded` body as a **result** |
| `req.uri` | Path string |
| `req.method` | `"GET"`, `"POST"`, … |

`req.form` is decided by the **content type**, not by whether the bytes happen to
parse — a JSON body answers `err`, so a handler can tell the two apart:

```rite browser
POST "/subscribe" |req| ⟦
  fields ← req.form?
  ^ 200 ⟨email: fields.email⟩
⟧
```

Decoding matches the query string exactly: `+` is a space, `%xx` is a byte, and a
repeated key keeps its last value.

### When nothing matches

| Situation | Answer |
|-----------|--------|
| No route for the path | `404` + `{"error":"not_found"}` |
| Path exists, method does not | `405` + `{"error":"method_not_allowed","allow":[…]}` and an `Allow` header |

Serve your own 404 page by claiming the tail with a catch-all — `GET "/*path"` is
reached only after every specific route has declined.

Note the interaction: once a catch-all is in place, *every* path matches it, so a
request with a method it does not cover answers `405`, not `404`. That is the honest
answer — a `GET "/*path"` really does serve that path, but it means a site with a
catch-all stops producing `404` for anything except methods no route declares.

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

### Several requests at once

The calls block one at a time; to issue requests concurrently, fan out with
[`parallel`](collections.md#doing-the-slow-parts-together):

```rite native_only
◆! fetch_one(url) ⟦
  ^ ! @http.get(url)?
⟧

pages ← ! parallel(urls, fetch_one)
```

## Next

[Network: sockets](sockets.md) — the raw `@udp` and `@tcp` layer under all of this ·
[Modules](modules.md) · [Browser & Studio](browser.md)
