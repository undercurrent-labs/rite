# Model Context Protocol

> CLI only. `@mcp` needs the native host — the browser runtime has neither process
> streams nor a socket layer.

Rite speaks MCP in both directions. `@mcp.serve` publishes tools, resources and
prompts; `@mcp.connect` calls someone else's. [Jump to the client](#calling-other-servers)
if that is what you are here for.

## Serving

An MCP server exposes tools, resources and prompts to a model host. In Rite it is a
declaration table, the same shape as [an HTTP service](http.md):

```rite native_only
! @mcp.serve "calculator" ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧
⟧
```

```bash
rite run calculator.rite
```

That is a complete, launchable server. Point a local MCP client at
`rite run calculator.rite` and it will find one tool called `add`, taking two integers.

## The schema comes from the types you already wrote

You do not describe a tool twice. The `inputSchema` an MCP client sees is derived from
the parameter annotations on the declaration:

| You write | The client is told |
|---|---|
| `\|a: int\|` | `{"type": "integer"}` |
| `\|x: float\|` · `\|x: number\|` | `{"type": "number"}` |
| `\|s: string\|` | `{"type": "string"}` |
| `\|b: bool\|` | `{"type": "boolean"}` |
| `\|xs: [string]\|` | `{"type": "array", "items": {"type": "string"}}` |
| `\|who: ⟨name: string⟩\|` | an object with `name` required |
| `\|x\|` (no annotation) | `{}` — any value, still required |

`@mcp.tool_schema` shows you exactly what a function would publish, without starting
anything:

```rite browser
◆ add(a: int, b: int) ⟦ ^ a + b ⟧
! @console.println(@mcp.tool_schema(add))
```

```text
⟨properties: ⟨a: ⟨type: integer⟩, b: ⟨type: integer⟩⟩, required: [a, b], type: object⟩
```

Every parameter is required, annotated or not: a missing one is a slot the body has no
value for, so the schema declares it rather than letting it arrive as `none`.

The same annotations are enforced when the call arrives. A client that sends a string
where `int` was declared gets the tool's own type error back — the contract is checked
by the machinery that checks any typed Rite call, not by a second checker that could
disagree with it.

## What a tool returns

| Body returns | The client sees |
|---|---|
| a string | that text |
| a record or list | pretty JSON as text, **plus** `structuredContent` |
| `err(…)` | the failure, with `isError: true` — a record keeps its fields |
| `none` | empty content |

A failing tool is a *successful* protocol response carrying `isError: true`. That is the
difference between "the server broke" and "the model passed the wrong thing and can try
again" — only the second is useful to the caller.

```rite native_only
! @mcp.serve "users" ⟦
  tool "lookup" "Find a user" |id: int| ⟦
    ^ err(⟨kind: "not_found", message: "no user " + str(id)⟩)
  ⟧
⟧
```

An error record is shaped like any other return value, so its fields travel: the client
reads `message` for the sentence and `data` for the rest. A record's `message` field is
the content text, because that is the line a model reads and acts on.

## Resources and prompts

```rite native_only
! @mcp.serve "docs" ⟦
  resource "config://app" "Application config" ⟦
    ^ ! @fs.read("app.json")?
  ⟧

  prompt "review" "Review some code" |code: string| ⟦
    ^ "Please review the following:\n" + code
  ⟧
⟧
```

A `resource` is named by its URI and takes no parameters. A `prompt` takes parameters
like a tool and returns the message text.

## Transports

`#stdio` is the default and needs no permission — it is the process's own streams, and
it is what a local client launches directly.

```rite native_only
! @mcp.serve ⟨name: "calculator", transport: #http, addr: "127.0.0.1:8080"⟩ ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦ ^ a + b ⟧
⟧
```

```bash
rite run calculator.rite --allow net=127.0.0.1
```

Streamable HTTP is a single POST endpoint. It follows the same bind policy as
`@http.listen`: loopback binds by default, any other interface needs
`--allow net=<host>`.

### Under stdio, stdout is the wire

Anything else written to stdout would corrupt the JSON-RPC stream, so while an stdio
server is running, everything a tool body prints goes to **stderr** instead:

```rite native_only
tool "noisy" "Prints while working" ⟦
  ! @console.println("this appears on stderr, not on the wire")
  ^ "done"
⟧
```

You do not have to do anything for this; it is what `@console` does inside a tool body.
The one thing to avoid is running `@http.listen` and an stdio `@mcp.serve` in the same
script — the HTTP server announces itself on stdout.

## Logging and progress

```rite native_only
! @mcp.serve "calculator" ⟦
  use @mcp.log

  tool "slow" "Takes a while" |n: int| ⟦
    ! @mcp.progress(0.5, "halfway")
    ^ n * 2
  ⟧
⟧
```

`use @mcp.log` writes one structured JSON line per request to stderr:

```text
{"level":"info","method":"tools/call","ms":3,"name":"add","ok":true}
```

Stderr rather than the protocol's own logging notifications, which the specification
deprecated — its suggested migration is exactly this. `@mcp.progress` does send a real
`notifications/progress`, on the stream of the call it belongs to.

> `@mcp.progress` only means something inside a tool body. Called anywhere else it
> fails, rather than quietly reporting progress nobody could receive.

## Calling other servers

`@mcp.connect` opens a connection to a server someone else wrote and answers a handle,
the same kind of value [`@tcp.connect`](sockets.md) and `@db.open` give you:

```rite native_only
c ← ! @mcp.connect(⟨command: "npx", args: ["-y", "@modelcontextprotocol/server-memory"]⟩)?

! @console.println(! @mcp.call_tool(c, "search", ⟨query: "rite"⟩)?)

! @mcp.close(c)
```

The spec names one transport. `command` starts the server as a subprocess and speaks
JSON-RPC on its stdin and stdout, which is how a local MCP server is normally launched:

| Key | For | Meaning |
|---|---|---|
| `command` | stdio | the program to start |
| `args` | stdio | its arguments, as a list |
| `env` | stdio | extra environment variables for the child, as a record |
| `url` | HTTP | a Streamable HTTP endpoint to POST to |
| `headers` | HTTP | extra request headers, as a record |
| `timeout_ms` | both | how long any one call waits, default `30000` |

```rite native_only
c ← ! @mcp.connect(⟨url: "https://example.com/mcp",
                    headers: ⟨authorization: "Bearer " + token⟩⟩)?
```

A key the spec does not understand is an error rather than ignored, and naming both
`command` and `url` is refused — there is no sensible way to guess which was meant.

### What you can ask a connection

| Call | Answers |
|---|---|
| `! @mcp.tools(c)` | `ok([⟨name, description, input_schema⟩])` |
| `! @mcp.call_tool(c, name, args)` | `ok(result)` |
| `! @mcp.resources(c)` | `ok([⟨uri, name, description⟩])` |
| `! @mcp.read_resource(c, uri)` | `ok(text)` |
| `! @mcp.prompts(c)` | `ok([⟨name, description, arguments⟩])` |
| `! @mcp.get_prompt(c, name, args)` | `ok(⟨description, messages⟩)` |
| `! @mcp.close(c)` | `ok(none)` |

`input_schema` is the tool's JSON Schema as an ordinary record, so it can be read
field by field. Each prompt argument is `⟨name, required⟩`, and each message of a
rendered prompt is `⟨role, text⟩`.

`call_tool` answers whatever the tool returned, decoded the way the server encoded it:
a structured result comes back as a record or a list, and anything else comes back as
the text of its content blocks joined with newlines. That is the [serving table](#what-a-tool-returns)
read backwards, so a Rite tool's record arrives as a record.

```rite native_only
stats ← ! @mcp.call_tool(c, "stats", ⟨xs: [3, 1, 4]⟩)?
! @console.println(stats.total)
```

Connections close when the run ends. `@mcp.close` is for closing one earlier, and under
stdio it sends the server EOF before stopping it. Closing twice is fine.

### When a call does not work

Everything after `connect` answers `ok` or `err`, so `?` unwraps it and a `~` reads the
reason. Four kinds, told apart by `e.kind`:

| `kind` | What happened |
|---|---|
| `mcp.tool_error` | the tool ran and reported failure — `⟨kind, tool, message, data⟩` |
| `mcp.error` | the server refused in JSON-RPC terms — `⟨kind, operation, code, message⟩` |
| `mcp.transport` | the pipe or the socket — `⟨kind, operation, message⟩` |
| `mcp.timeout` | no answer within `timeout_ms` — `⟨kind, operation, timeout_ms, message⟩` |

```rite native_only
report ← ~ (! @mcp.call_tool(c, "divide", ⟨a: 1, b: 0⟩)) ⟦
  ok n → "got " + str(n)
  err e → "refused: " + e.message
⟧
```

`data` carries the failing tool's own fields when it sent a structured failure, so a
tool returning `err(⟨kind: "not_found", message: …⟩)` reaches the caller as
`e.message` for the sentence and `e.data.kind` for the reason to branch on. `e.kind`
stays `mcp.tool_error`: it says which of the four rows this is, not what the tool
called its own failure.

The first is the mirror of what a Rite tool returning `err(…)` produces. It is a value
rather than a raise for the same reason it is on the serving side: a model can read the
message and try again. Using a handle after closing it raises instead, because that is
a mistake in the script rather than something the server did.

## Permissions

| What | Needs |
|---|---|
| `transport: #stdio` | nothing — the process's own streams |
| `transport: #http`, loopback address | nothing |
| `transport: #http`, any other interface | `--allow net=<host>` |
| whatever the tool bodies do | their own grants, as usual |
| `@mcp.connect` with `command` | `--allow process` |
| `@mcp.connect` with `url` | `--allow net=<host>` |
| every other call on a connection | nothing further |

Serving does not widen anything. A tool that reads a file still needs
`--allow fs:read=…`, and it is denied to the client exactly as it would be to the script.

Connecting is checked once, at `connect`, and the two transports do not cover for each
other: `--allow process` will not reach a URL, and `--allow net=…` will not start a
subprocess. A `url` host is matched as written, before DNS, the same rule
[`@http.get`](http.md) follows. The calls that take an open handle need no grant of
their own — the handle cannot exist without the one that was already checked.

> Starting a server is running a program of the caller's choosing, which is exactly what
> `--allow process` means. A connection spec taken from untrusted input is a command
> taken from untrusted input.

## Which revision this speaks

Natively the **2026-07-28** revision, in which MCP is stateless: no `initialize`
handshake, no sessions, and `server/discover` as the one mandatory call. Clients that
still speak the older shape are answered too — send `initialize` and the connection
falls back to `2025-06-18` for its lifetime.

`@mcp.connect` does the same negotiation from the other side, and there is nothing to
configure. It probes with `server/discover`; a server that has never heard of it gets
the `initialize` handshake instead and stays on `2025-06-18` for the life of the
connection. Most servers in the field today take that path.

Not implemented, deliberately:

- `subscriptions/listen` and the `*ListChanged` notifications. A Rite server's tables
  are fixed when `@mcp.serve` starts and cannot change while it runs, so the capability
  is not advertised rather than advertised and never fired.
- `notifications/message`, deprecated upstream — see logging above.
- Multi Round-Trip Requests. Every result is `"complete"`.
- On the client side, sampling and elicitation. Those are a server asking the *client*
  to run a model or prompt a person, and a Rite script has neither to offer. A
  connected server's progress notifications are read off the wire and dropped, since
  `call_tool` answers one value.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| the client sees no tools | a `tool` word not followed by a string literal is an ordinary binding, not a declaration |
| `E021: @mcp.serve performs an effect` | the `!` is missing |
| the client reports a parse error | something wrote to stdout under `#stdio` — an `@http.listen` in the same script, or a native library |
| `E040` on start | a non-loopback HTTP bind without `--allow net=<host>` |
| a tool always errors on a valid-looking argument | the declared type is narrower than what the client sends; check with `@mcp.tool_schema` |
| `connect` answers `err(⟨kind: "mcp.transport"⟩)` naming the command | the program is not on PATH, or the arguments do not start a server |
| `connect` is denied | `command` needs `--allow process`; `url` needs `--allow net=<host>` for the host as written |
| every call answers `mcp.timeout` | the server is slow or wedged; raise `timeout_ms` on the spec, or check its stderr, which a stdio connection passes straight through |
| `@mcp.connect: unknown option …` | a key the spec does not understand — the accepted ones are `command`, `args`, `env`, `url`, `headers`, `timeout_ms` |

## Next

- [Network: HTTP services](http.md) — the same declaration shape, for the web
- [Effects](effects.md) — why `@mcp.serve` needs its `!`
- [Files, JSON, and CSV](files-json.md) — what a tool body usually reaches for
