# Model Context Protocol servers

> CLI only. `@mcp` needs the native host — the browser runtime has neither process
> streams nor a socket layer.

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
| `err(…)` | the error text, with `isError: true` |
| `none` | empty content |

A failing tool is a *successful* protocol response carrying `isError: true`. That is the
difference between "the server broke" and "the model passed the wrong thing and can try
again" — only the second is useful to the caller.

```rite native_only
tool "lookup" "Find a user" |id: int| ⟦
  ^ err(⟨kind: "not_found", message: "no user " + str(id)⟩)
⟧
```

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

## Permissions

| What | Needs |
|---|---|
| `transport: #stdio` | nothing — the process's own streams |
| `transport: #http`, loopback address | nothing |
| `transport: #http`, any other interface | `--allow net=<host>` |
| whatever the tool bodies do | their own grants, as usual |

Serving does not widen anything. A tool that reads a file still needs
`--allow fs:read=…`, and it is denied to the client exactly as it would be to the script.

## Which revision this speaks

Natively the **2026-07-28** revision, in which MCP is stateless: no `initialize`
handshake, no sessions, and `server/discover` as the one mandatory call. Clients that
still speak the older shape are answered too — send `initialize` and the connection
falls back to `2025-06-18` for its lifetime.

Not implemented, deliberately:

- `subscriptions/listen` and the `*ListChanged` notifications. A Rite server's tables
  are fixed when `@mcp.serve` starts and cannot change while it runs, so the capability
  is not advertised rather than advertised and never fired.
- `notifications/message`, deprecated upstream — see logging above.
- Multi Round-Trip Requests. Every result is `"complete"`.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| the client sees no tools | a `tool` word not followed by a string literal is an ordinary binding, not a declaration |
| `E021: @mcp.serve performs an effect` | the `!` is missing |
| the client reports a parse error | something wrote to stdout under `#stdio` — an `@http.listen` in the same script, or a native library |
| `E040` on start | a non-loopback HTTP bind without `--allow net=<host>` |
| a tool always errors on a valid-looking argument | the declared type is narrower than what the client sends; check with `@mcp.tool_schema` |

## Next

- [Network: HTTP services](http.md) — the same declaration shape, for the web
- [Effects](effects.md) — why `@mcp.serve` needs its `!`
- [Files, JSON, and CSV](files-json.md) — what a tool body usually reaches for
