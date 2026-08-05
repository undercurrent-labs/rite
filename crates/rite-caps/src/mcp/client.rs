//! MCP client: calling *other* servers, over stdio and Streamable HTTP.
//!
//! The parent module publishes a declaration table; this half consumes someone else's.
//! A connection is an opaque `Value::Handle`, the representation `@db`, `@tcp` and
//! `@udp` already use, and the shape is theirs too: `connect` → calls → `close`, with
//! `close` on an already-closed handle a no-op.
//!
//! # Which revision it speaks
//!
//! [`connect`] probes with `server/discover`, the one mandatory RPC of the 2026-07-28
//! revision. A server that answers it is spoken to statelessly, with each request
//! carrying its own version in `_meta`. A server that answers `method not found` gets
//! the older `initialize` handshake and stays on [`super::MCP_LEGACY_PROTOCOL_VERSION`]
//! for the life of the connection. That is the server half's compatibility layer in
//! reverse, and the same one revision back.
//!
//! # Where the permission is checked
//!
//! At `connect`, and only there. A stdio connection spawns a subprocess, so it needs
//! `--allow process`; an HTTP one reaches a host, so it needs `--allow net=<host>`,
//! matched by [`crate::http::host_of`] exactly as an outbound `@http.get` is. Calls on
//! an open handle check nothing further: the handle cannot be obtained without the
//! grant, and re-checking would only ask the same question again.
//!
//! # What a failure looks like
//!
//! Everything after `connect` answers `ok(…)` or `err(⟨kind, …⟩)`, so `?` unwraps it.
//! Four kinds, told apart by `e.kind`: `mcp.error` is the server refusing in JSON-RPC
//! terms, `mcp.transport` is the pipe or socket, `mcp.timeout` is a server that never
//! answered, and `mcp.tool_error` is a tool that ran and reported failure in band.
//!
//! Using a closed or foreign handle raises instead: that is a bug in the script, not a
//! condition the network produced. `@tcp` draws the line in the same place.

use super::{McpRevision, MCP_LEGACY_PROTOCOL_VERSION, MCP_PROTOCOL_VERSION};
use crate::permissions::PermissionSet;
use rite_runtime::{EvalError, HostHandle, Key, RuntimeContext, Value};
use serde_json::{json, Map, Value as Json};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex as AsyncMutex;

/// Handle kind, so a `@tcp` connection handed to `@mcp.call_tool` is caught by name.
const HANDLE_KIND: &str = "mcp.client";

/// How long one request waits for its answer when the spec does not say.
///
/// The ceiling `@http` puts on an outbound request and `@tcp` on a connect. A server
/// that hangs must not take the script with it, and under stdio there is no socket
/// timeout underneath to fall back on.
const DEFAULT_TIMEOUT_MS: i64 = 30_000;

/// Refuse a timeout long enough to be indistinguishable from none.
const MAX_TIMEOUT_MS: i64 = 10 * 60 * 1_000;

// ---------------------------------------------------------------------------
// The connection
// ---------------------------------------------------------------------------

/// A subprocess speaking JSON-RPC on its own stdin and stdout.
///
/// `stdin` is an `Option` so `close` can drop it while keeping the `Child`: dropping
/// the writer is EOF, which is how an stdio server is asked to shut down cleanly.
struct StdioPipe {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Lines<BufReader<ChildStdout>>,
}

impl StdioPipe {
    /// EOF first, then a kill if the server does not take the hint.
    async fn shutdown(&mut self) {
        drop(self.stdin.take());
        let waited = tokio::time::timeout(Duration::from_secs(2), self.child.wait()).await;
        if waited.is_err() {
            let _ = self.child.kill().await;
        }
    }
}

#[derive(Clone)]
enum Transport {
    /// One ordered byte stream, so one request at a time — the mutex is the queue.
    Stdio(Arc<AsyncMutex<StdioPipe>>),
    Http {
        url: String,
        headers: Vec<(String, String)>,
        http: reqwest::Client,
    },
}

/// Cloned out of the handle table before every call.
///
/// Holding the table's lock across an await would serialize every other handle in the
/// run, and trips `clippy::await_holding_lock`. Everything expensive here is behind an
/// `Arc`, so the clone is a few refcount bumps.
#[derive(Clone)]
struct Client {
    transport: Transport,
    revision: McpRevision,
    next_id: Arc<AtomicI64>,
    timeout: Duration,
}

/// Why a round trip did not produce a result.
enum RequestError {
    /// The server answered, in JSON-RPC terms, that it would not.
    Rpc {
        code: i64,
        message: String,
    },
    /// The pipe or the socket.
    Transport(String),
    Timeout(i64),
}

impl RequestError {
    fn into_value(self, operation: &str) -> Value {
        let fields = match self {
            RequestError::Rpc { code, message } => vec![
                (Key::String("kind".into()), Value::string("mcp.error")),
                (Key::String("operation".into()), Value::string(operation)),
                (Key::String("code".into()), Value::Int(code)),
                (Key::String("message".into()), Value::string(message)),
            ],
            RequestError::Transport(message) => vec![
                (Key::String("kind".into()), Value::string("mcp.transport")),
                (Key::String("operation".into()), Value::string(operation)),
                (Key::String("message".into()), Value::string(message)),
            ],
            RequestError::Timeout(ms) => vec![
                (Key::String("kind".into()), Value::string("mcp.timeout")),
                (Key::String("operation".into()), Value::string(operation)),
                (Key::String("timeout_ms".into()), Value::Int(ms)),
                (
                    Key::String("message".into()),
                    Value::string(format!("no answer to {operation} within {ms}ms")),
                ),
            ],
        };
        Value::err(Value::record(fields))
    }
}

impl Client {
    /// The protocol version header value for whichever revision was negotiated.
    fn version(&self) -> &'static str {
        match self.revision {
            McpRevision::Current => MCP_PROTOCOL_VERSION,
            McpRevision::Legacy => MCP_LEGACY_PROTOCOL_VERSION,
        }
    }

    /// Stamp the negotiated version into `_meta`, which is where the current revision
    /// carries it. The legacy revision has no such field and pinned the version once,
    /// at `initialize`, so nothing is added for it.
    fn prepare(&self, mut params: Map<String, Json>) -> Json {
        if self.revision == McpRevision::Current {
            let meta = params
                .entry("_meta")
                .or_insert_with(|| json!({}))
                .as_object_mut();
            if let Some(meta) = meta {
                meta.insert(
                    super::meta_keys::PROTOCOL_VERSION.into(),
                    json!(MCP_PROTOCOL_VERSION),
                );
            }
        }
        Json::Object(params)
    }

    async fn request(&self, method: &str, params: Map<String, Json>) -> Result<Json, RequestError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": self.prepare(params),
        });

        let reply = match tokio::time::timeout(self.timeout, self.round_trip(method, &body, id))
            .await
        {
            Err(_elapsed) => return Err(RequestError::Timeout(self.timeout.as_millis() as i64)),
            Ok(Err(e)) => return Err(RequestError::Transport(e)),
            Ok(Ok(v)) => v,
        };

        if let Some(err) = reply.get("error") {
            return Err(RequestError::Rpc {
                code: err.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                message: err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("(no message)")
                    .to_string(),
            });
        }
        Ok(reply.get("result").cloned().unwrap_or_else(|| json!({})))
    }

    /// A message with no id. Nothing answers one, so nothing is read back.
    async fn notify(&self, method: &str, params: Map<String, Json>) {
        let body = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": self.prepare(params),
        });
        let _ = tokio::time::timeout(self.timeout, self.send_notification(method, &body)).await;
    }

    async fn round_trip(&self, method: &str, body: &Json, id: i64) -> Result<Json, String> {
        match &self.transport {
            Transport::Stdio(pipe) => stdio_round_trip(pipe, body, id).await,
            Transport::Http { url, headers, http } => {
                let text = self.http_post(http, url, headers, method, body).await?;
                serde_json::from_str(&text)
                    .map_err(|e| format!("invalid JSON from server: {e} (in {text:?})"))
            }
        }
    }

    async fn send_notification(&self, method: &str, body: &Json) -> Result<(), String> {
        match &self.transport {
            Transport::Stdio(pipe) => {
                let mut p = pipe.lock().await;
                write_line(&mut p, body).await
            }
            Transport::Http { url, headers, http } => {
                self.http_post(http, url, headers, method, body).await?;
                Ok(())
            }
        }
    }

    /// One POST to the Streamable HTTP endpoint.
    ///
    /// `Mcp-Method` and `Mcp-Name` let an intermediary route without parsing the body,
    /// and the server half rejects them when they disagree with it — so they are
    /// derived from the body rather than passed in alongside it.
    async fn http_post(
        &self,
        http: &reqwest::Client,
        url: &str,
        headers: &[(String, String)],
        method: &str,
        body: &Json,
    ) -> Result<String, String> {
        let mut req = http
            .post(url)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("mcp-method", method)
            .header("mcp-protocol-version", self.version());
        if let Some(name) = body
            .get("params")
            .and_then(|p| p.get("name").or_else(|| p.get("uri")))
            .and_then(|v| v.as_str())
        {
            req = req.header("mcp-name", name);
        }
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        // 202 with no body is how a notification is acknowledged; every other empty
        // answer is a server that failed without saying so in JSON-RPC.
        if text.trim().is_empty() && status != 202 {
            return Err(format!("empty response (HTTP {status})"));
        }
        Ok(text)
    }
}

async fn write_line(pipe: &mut StdioPipe, body: &Json) -> Result<(), String> {
    let mut line = serde_json::to_string(body).map_err(|e| e.to_string())?;
    line.push('\n');
    let stdin = pipe
        .stdin
        .as_mut()
        .ok_or_else(|| "connection closed".to_string())?;
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

/// Write one request and read until the matching id comes back.
///
/// Lines without that id are the server's notifications and are dropped.
/// `notifications/progress` arrives on this stream ahead of the result it belongs to,
/// and has nowhere to go in a call that answers a single value.
async fn stdio_round_trip(
    pipe: &AsyncMutex<StdioPipe>,
    body: &Json,
    id: i64,
) -> Result<Json, String> {
    let mut p = pipe.lock().await;
    write_line(&mut p, body).await?;
    loop {
        match p.lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => continue,
            Ok(Some(line)) => {
                let parsed: Json = serde_json::from_str(&line)
                    .map_err(|e| format!("invalid JSON from server: {e} (in {line:?})"))?;
                if parsed.get("id").and_then(|i| i.as_i64()) == Some(id) {
                    return Ok(parsed);
                }
            }
            Ok(None) => return Err("server closed the connection".into()),
            Err(e) => return Err(e.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

/// The connection spec, after validation.
#[derive(Debug)]
struct Spec {
    command: Option<String>,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    url: Option<String>,
    headers: Vec<(String, String)>,
    timeout_ms: i64,
}

const SPEC_FORM: &str = "@mcp.connect expects ⟨command: \"npx\", args: [...]⟩ for a stdio \
                         server, or ⟨url: \"https://example.com/mcp\"⟩ for an HTTP one";

fn string_pairs(v: &Value, what: &str) -> Result<Vec<(String, String)>, EvalError> {
    let Value::Record(r) = v else {
        return Err(EvalError::Message(format!(
            "@mcp.connect: `{what}` must be a record, got {}",
            v.type_name()
        )));
    };
    let mut out = Vec::with_capacity(r.len());
    for (k, val) in r {
        let name = match k {
            Key::String(s) => s.clone(),
            Key::Atom(a) => a.clone(),
            other => {
                return Err(EvalError::Message(format!(
                    "@mcp.connect: `{what}` names must be strings, got `{other:?}`"
                )))
            }
        };
        match val.as_str() {
            Some(s) => out.push((name, s.to_string())),
            None => out.push((name, format!("{val}"))),
        }
    }
    Ok(out)
}

/// Read the spec, refusing anything it does not understand.
///
/// An unknown key is an error rather than ignored, for the reason `@process.run`
/// settled: `⟨cmd: "npx"⟩` was accepted and thrown away, so a typo looked like it
/// worked. Two transports in one record is refused for the same reason — there is no
/// sensible way to guess which one was meant.
fn parse_spec(v: Option<&Value>) -> Result<Spec, EvalError> {
    let Some(Value::Record(fields)) = v else {
        return Err(EvalError::Message(SPEC_FORM.into()));
    };
    let mut spec = Spec {
        command: None,
        argv: Vec::new(),
        env: Vec::new(),
        url: None,
        headers: Vec::new(),
        timeout_ms: DEFAULT_TIMEOUT_MS,
    };
    for (key, value) in fields {
        let name = match key {
            Key::String(s) => s.as_str(),
            Key::Atom(a) => a.as_str(),
            other => {
                return Err(EvalError::Message(format!(
                    "@mcp.connect: unknown option `{other:?}` — {SPEC_FORM}"
                )))
            }
        };
        match name {
            "command" => {
                spec.command = Some(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            EvalError::Message("@mcp.connect: `command` must be a string".into())
                        })?
                        .to_string(),
                )
            }
            "args" => {
                let Value::List(xs) = value else {
                    return Err(EvalError::Message(
                        "@mcp.connect: `args` must be a list of strings".into(),
                    ));
                };
                spec.argv = xs
                    .iter()
                    .map(|x| match x.as_str() {
                        Some(s) => s.to_string(),
                        None => format!("{x}"),
                    })
                    .collect();
            }
            "env" => spec.env = string_pairs(value, "env")?,
            "url" => {
                spec.url = Some(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            EvalError::Message("@mcp.connect: `url` must be a string".into())
                        })?
                        .to_string(),
                )
            }
            "headers" => spec.headers = string_pairs(value, "headers")?,
            "timeout_ms" => {
                spec.timeout_ms = value.as_int().ok_or_else(|| {
                    EvalError::Message("@mcp.connect: `timeout_ms` must be an integer".into())
                })?
            }
            other => {
                return Err(EvalError::Message(format!(
                    "@mcp.connect: unknown option `{other}` — expected `command`, `args`, \
                     `env`, `url`, `headers` or `timeout_ms`"
                )))
            }
        }
    }
    match (&spec.command, &spec.url) {
        (None, None) => return Err(EvalError::Message(SPEC_FORM.into())),
        (Some(_), Some(_)) => {
            return Err(EvalError::Message(
                "@mcp.connect: a spec names one transport — `command` for stdio or `url` \
                 for HTTP, not both"
                    .into(),
            ))
        }
        _ => {}
    }
    if spec.timeout_ms < 1 || spec.timeout_ms > MAX_TIMEOUT_MS {
        return Err(EvalError::Message(format!(
            "@mcp.connect: `timeout_ms` must be between 1 and {MAX_TIMEOUT_MS}"
        )));
    }
    Ok(spec)
}

/// Start the subprocess and take its two streams.
///
/// Stderr is inherited rather than piped. An stdio server logs there by convention:
/// `@mcp.serve` announces itself and `use @mcp.log` writes its lines there. Piping it
/// without a reader would fill the pipe buffer and block the server mid-session.
fn spawn_stdio(spec: &Spec) -> Result<StdioPipe, String> {
    let command = spec.command.as_deref().unwrap_or_default();
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(&spec.argv)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        // The handle table drops its entries when the run ends, and a dropped `Child`
        // otherwise leaves the server running with nothing on the other end of its
        // pipes. Under `RiteEngine`, where the host process keeps going, that would
        // be a subprocess leaked for the life of the host.
        .kill_on_drop(true);
    for (name, value) in &spec.env {
        cmd.env(name, value);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("cannot start `{command}`: {e}"))?;
    let stdin = child.stdin.take().ok_or("no stdin on the spawned server")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("no stdout on the spawned server")?;
    Ok(StdioPipe {
        child,
        stdin: Some(stdin),
        lines: BufReader::new(stdout).lines(),
    })
}

/// `! @mcp.connect(spec)` — open a connection and finish the handshake.
async fn connect(
    args: Vec<Value>,
    perms: &PermissionSet,
    ctx: &RuntimeContext,
) -> Result<Value, EvalError> {
    let spec = parse_spec(args.first())?;
    let timeout = Duration::from_millis(spec.timeout_ms as u64);

    let transport = if let Some(url) = &spec.url {
        // Matched as written, before DNS, so `--allow net=example.com` does not become
        // a grant for wherever that name points next. The rule `@http.get` follows.
        let host = crate::http::host_of(url)
            .ok_or_else(|| EvalError::Message(format!("@mcp.connect: cannot parse url `{url}`")))?;
        perms.check_net(&host).map_err(EvalError::Permission)?;
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| EvalError::Capability(format!("mcp: cannot build http client: {e}")))?;
        Transport::Http {
            url: url.clone(),
            headers: spec.headers.clone(),
            http,
        }
    } else {
        // Starting a server is running a program of the caller's choosing, which is
        // exactly what `--allow process` grants.
        perms.check_process().map_err(EvalError::Permission)?;
        match spawn_stdio(&spec) {
            Ok(pipe) => Transport::Stdio(Arc::new(AsyncMutex::new(pipe))),
            Err(e) => return Ok(connect_err(&e)),
        }
    };

    let mut client = Client {
        transport,
        revision: McpRevision::Current,
        next_id: Arc::new(AtomicI64::new(1)),
        timeout,
    };

    if let Err(e) = handshake(&mut client).await {
        // The subprocess is still running and now has nobody to talk to.
        if let Transport::Stdio(pipe) = &client.transport {
            pipe.lock().await.shutdown().await;
        }
        return Ok(e.into_value("mcp.connect"));
    }

    let id = ctx
        .handles
        .insert(HANDLE_KIND, Box::new(client))
        .map_err(|limit| {
            EvalError::Message(format!(
                "mcp: too many open connections ({limit}). A connection is closed with \
                 @mcp.close, or when the run ends"
            ))
        })?;
    Ok(Value::ok(Value::Handle(HostHandle {
        kind: HANDLE_KIND.into(),
        id,
    })))
}

/// `server/discover` first, `initialize` only if the server has never heard of it.
///
/// The 2026-07-28 revision dropped `initialize` and made `server/discover` the one
/// mandatory RPC, so probing with it is both the modern call and the version check.
/// Anything other than "method not found" is a real failure and is reported: a server
/// that refuses `server/discover` for its own reasons is not a legacy server.
async fn handshake(client: &mut Client) -> Result<(), RequestError> {
    match client.request("server/discover", Map::new()).await {
        Ok(_) => Ok(()),
        Err(RequestError::Rpc { code, .. }) if code == METHOD_NOT_FOUND => {
            client.revision = McpRevision::Legacy;
            let mut params = Map::new();
            params.insert("protocolVersion".into(), json!(MCP_LEGACY_PROTOCOL_VERSION));
            params.insert("capabilities".into(), json!({}));
            params.insert(
                "clientInfo".into(),
                json!({"name": "rite", "version": env!("CARGO_PKG_VERSION")}),
            );
            client.request("initialize", params).await?;
            // The older revision requires this before any other request; it carries no
            // id and the server answers nothing.
            client.notify("notifications/initialized", Map::new()).await;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// JSON-RPC's own code for a method the server does not implement.
const METHOD_NOT_FOUND: i64 = -32601;

fn connect_err(message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("mcp.transport")),
        (
            Key::String("operation".into()),
            Value::string("mcp.connect"),
        ),
        (Key::String("message".into()), Value::string(message)),
    ]))
}

// ---------------------------------------------------------------------------
// Decoding what a server answers
// ---------------------------------------------------------------------------

fn field_string(v: &Json, key: &str) -> Value {
    Value::string(v.get(key).and_then(|s| s.as_str()).unwrap_or_default())
}

/// The text of a list of content blocks, or the blocks themselves.
///
/// Text is what a body almost always is, and a string is what a script wants to
/// interpolate or print. Anything else in the list — an image, an embedded resource —
/// has no text to flatten to, so the whole list comes back as records and the script
/// reads `type` for itself.
fn content_value(blocks: &[Json]) -> Value {
    if blocks
        .iter()
        .all(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
    {
        let text: Vec<&str> = blocks
            .iter()
            .map(|b| b.get("text").and_then(|t| t.as_str()).unwrap_or_default())
            .collect();
        Value::string(text.join("\n"))
    } else {
        Value::list(blocks.iter().map(Value::from_json))
    }
}

fn blocks_of<'a>(result: &'a Json, key: &str) -> &'a [Json] {
    result
        .get(key)
        .and_then(|c| c.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn decode_tools(result: &Json) -> Value {
    Value::list(blocks_of(result, "tools").iter().map(|t| {
        Value::record(vec![
            (Key::String("name".into()), field_string(t, "name")),
            (
                Key::String("description".into()),
                field_string(t, "description"),
            ),
            (
                Key::String("input_schema".into()),
                Value::from_json(t.get("inputSchema").unwrap_or(&Json::Null)),
            ),
        ])
    }))
}

fn decode_resources(result: &Json) -> Value {
    Value::list(blocks_of(result, "resources").iter().map(|r| {
        Value::record(vec![
            (Key::String("uri".into()), field_string(r, "uri")),
            (Key::String("name".into()), field_string(r, "name")),
            (
                Key::String("description".into()),
                field_string(r, "description"),
            ),
        ])
    }))
}

fn decode_prompts(result: &Json) -> Value {
    Value::list(blocks_of(result, "prompts").iter().map(|p| {
        let arguments = blocks_of(p, "arguments").iter().map(|a| {
            Value::record(vec![
                (Key::String("name".into()), field_string(a, "name")),
                (
                    Key::String("required".into()),
                    Value::Bool(a.get("required").and_then(|r| r.as_bool()).unwrap_or(false)),
                ),
            ])
        });
        Value::record(vec![
            (Key::String("name".into()), field_string(p, "name")),
            (
                Key::String("description".into()),
                field_string(p, "description"),
            ),
            (
                Key::String("arguments".into()),
                Value::list(arguments.collect::<Vec<_>>()),
            ),
        ])
    }))
}

/// A prompt's messages, flattened to `⟨role, text⟩`.
///
/// The wire nests the text one level deeper, in a content block per message. Every
/// message a server can send through this client is text (`prompts/get` has no other
/// block type), so the nesting carries nothing a script would read.
fn decode_prompt(result: &Json) -> Value {
    let messages = blocks_of(result, "messages").iter().map(|m| {
        let text = match m.get("content") {
            Some(Json::Array(blocks)) => content_value(blocks),
            Some(one) => {
                Value::string(one.get("text").and_then(|t| t.as_str()).unwrap_or_default())
            }
            None => Value::string(""),
        };
        Value::record(vec![
            (Key::String("role".into()), field_string(m, "role")),
            (Key::String("text".into()), text),
        ])
    });
    Value::record(vec![
        (
            Key::String("description".into()),
            field_string(result, "description"),
        ),
        (
            Key::String("messages".into()),
            Value::list(messages.collect::<Vec<_>>()),
        ),
    ])
}

/// A tool result.
///
/// `structuredContent` wins when the server sent it: it is the same data the text
/// block holds, already shaped, and a script that wants the text can ask for it with
/// `@json.encode`. `isError` is a successful response carrying a failure, so it
/// becomes an `err` value rather than a raise — the distinction the server half draws
/// between "the server broke" and "the call was wrong".
fn decode_tool_result(name: &str, result: &Json) -> Value {
    let blocks = blocks_of(result, "content");
    if result.get("isError").and_then(|e| e.as_bool()) == Some(true) {
        return Value::err(Value::record(vec![
            (Key::String("kind".into()), Value::string("mcp.tool_error")),
            (Key::String("tool".into()), Value::string(name)),
            (Key::String("message".into()), content_value(blocks)),
        ]));
    }
    match result.get("structuredContent") {
        Some(s) if !s.is_null() => Value::ok(Value::from_json(s)),
        _ => Value::ok(content_value(blocks)),
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

fn handle_id(v: Option<&Value>) -> Result<u64, EvalError> {
    match v {
        Some(Value::Handle(h)) if h.kind == HANDLE_KIND => Ok(h.id),
        Some(Value::Handle(h)) => Err(EvalError::Message(format!(
            "expected handle {}, got {}",
            HANDLE_KIND, h.kind
        ))),
        _ => Err(EvalError::Message(format!(
            "expected handle {HANDLE_KIND} from @mcp.connect"
        ))),
    }
}

fn client_of(v: Option<&Value>, ctx: &RuntimeContext) -> Result<Client, EvalError> {
    let id = handle_id(v)?;
    ctx.handles
        .with::<Client, _>(id, |c| c.clone())
        .ok_or_else(|| EvalError::Message("mcp connection closed or invalid".into()))
}

fn string_arg(args: &[Value], i: usize, message: &str) -> Result<String, EvalError> {
    args.get(i)
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| EvalError::Message(message.into()))
}

/// A record of named arguments, as the protocol carries them.
fn arguments_json(v: Option<&Value>, ctx: &RuntimeContext) -> Result<Json, EvalError> {
    match v {
        None | Some(Value::None) => Ok(json!({})),
        Some(v @ Value::Record(_)) => Ok(v.to_json(&ctx.atoms)),
        Some(other) => Err(EvalError::Message(format!(
            "arguments must be a record, got {}",
            other.type_name()
        ))),
    }
}

/// One list call: send, decode, or report the failure as a value.
async fn listing(
    client: &Client,
    method: &'static str,
    decode: fn(&Json) -> Value,
) -> Result<Value, EvalError> {
    match client.request(method, Map::new()).await {
        Ok(result) => Ok(Value::ok(decode(&result))),
        Err(e) => Ok(e.into_value(method)),
    }
}

pub(super) async fn call(
    method: &str,
    args: Vec<Value>,
    perms: &PermissionSet,
    ctx: &RuntimeContext,
) -> Result<Value, EvalError> {
    match method {
        "connect" => connect(args, perms, ctx).await,
        "tools" => {
            let client = client_of(args.first(), ctx)?;
            listing(&client, "tools/list", decode_tools).await
        }
        "resources" => {
            let client = client_of(args.first(), ctx)?;
            listing(&client, "resources/list", decode_resources).await
        }
        "prompts" => {
            let client = client_of(args.first(), ctx)?;
            listing(&client, "prompts/list", decode_prompts).await
        }
        "call_tool" => {
            let client = client_of(args.first(), ctx)?;
            let name = string_arg(&args, 1, "@mcp.call_tool expects a tool name")?;
            let arguments = arguments_json(args.get(2), ctx)?;
            let mut params = Map::new();
            params.insert("name".into(), json!(name));
            params.insert("arguments".into(), arguments);
            match client.request("tools/call", params).await {
                Ok(result) => Ok(decode_tool_result(&name, &result)),
                Err(e) => Ok(e.into_value("tools/call")),
            }
        }
        "read_resource" => {
            let client = client_of(args.first(), ctx)?;
            let uri = string_arg(&args, 1, "@mcp.read_resource expects a uri")?;
            let mut params = Map::new();
            params.insert("uri".into(), json!(uri));
            match client.request("resources/read", params).await {
                // `contents` blocks carry `text` without a `type`, so they are read for
                // their text directly rather than through `content_value`.
                Ok(result) => {
                    let text: Vec<&str> = blocks_of(&result, "contents")
                        .iter()
                        .map(|c| c.get("text").and_then(|t| t.as_str()).unwrap_or_default())
                        .collect();
                    Ok(Value::ok(Value::string(text.join("\n"))))
                }
                Err(e) => Ok(e.into_value("resources/read")),
            }
        }
        "get_prompt" => {
            let client = client_of(args.first(), ctx)?;
            let name = string_arg(&args, 1, "@mcp.get_prompt expects a prompt name")?;
            let arguments = arguments_json(args.get(2), ctx)?;
            let mut params = Map::new();
            params.insert("name".into(), json!(name));
            params.insert("arguments".into(), arguments);
            match client.request("prompts/get", params).await {
                Ok(result) => Ok(Value::ok(decode_prompt(&result))),
                Err(e) => Ok(e.into_value("prompts/get")),
            }
        }
        "close" => {
            let id = handle_id(args.first())?;
            let client = ctx.handles.with::<Client, _>(id, |c| c.clone());
            ctx.handles.close(id);
            if let Some(Client {
                transport: Transport::Stdio(pipe),
                ..
            }) = client
            {
                pipe.lock().await.shutdown().await;
            }
            Ok(Value::ok(Value::None))
        }
        other => Err(EvalError::Capability(format!(
            "unknown @mcp function `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pairs: Vec<(&str, Value)>) -> Value {
        Value::record(pairs.into_iter().map(|(k, v)| (Key::String(k.into()), v)))
    }

    #[test]
    fn a_spec_names_exactly_one_transport() {
        let both = record(vec![
            ("command", Value::string("npx")),
            ("url", Value::string("http://example.com/mcp")),
        ]);
        assert!(parse_spec(Some(&both)).is_err());
        assert!(parse_spec(Some(&record(vec![]))).is_err());
        assert!(parse_spec(None).is_err());
    }

    /// A typo must not be indistinguishable from the default.
    #[test]
    fn an_unknown_option_is_refused() {
        let spec = record(vec![
            ("command", Value::string("npx")),
            ("arguments", Value::list([Value::string("-y")])),
        ]);
        let err = parse_spec(Some(&spec)).unwrap_err().to_string();
        assert!(err.contains("arguments"), "unhelpful message: {err}");
    }

    #[test]
    fn the_timeout_is_bounded() {
        let too_long = record(vec![
            ("url", Value::string("http://example.com/mcp")),
            ("timeout_ms", Value::Int(MAX_TIMEOUT_MS + 1)),
        ]);
        assert!(parse_spec(Some(&too_long)).is_err());
        let zero = record(vec![
            ("url", Value::string("http://example.com/mcp")),
            ("timeout_ms", Value::Int(0)),
        ]);
        assert!(parse_spec(Some(&zero)).is_err());
    }

    #[test]
    fn the_current_revision_stamps_its_version_and_the_legacy_one_does_not() {
        let client = |revision| Client {
            transport: Transport::Http {
                url: String::new(),
                headers: Vec::new(),
                http: reqwest::Client::new(),
            },
            revision,
            next_id: Arc::new(AtomicI64::new(1)),
            timeout: Duration::from_secs(1),
        };
        let current = client(McpRevision::Current).prepare(Map::new());
        assert_eq!(
            current["_meta"][super::super::meta_keys::PROTOCOL_VERSION],
            json!(MCP_PROTOCOL_VERSION)
        );
        let legacy = client(McpRevision::Legacy).prepare(Map::new());
        assert!(legacy.get("_meta").is_none(), "legacy carried a _meta");
    }

    #[test]
    fn text_blocks_flatten_and_mixed_ones_do_not() {
        let text = vec![
            json!({"type": "text", "text": "one"}),
            json!({"type": "text", "text": "two"}),
        ];
        assert_eq!(content_value(&text).as_str(), Some("one\ntwo"));

        let mixed = vec![
            json!({"type": "text", "text": "one"}),
            json!({"type": "image", "data": "…"}),
        ];
        assert!(matches!(content_value(&mixed), Value::List(_)));
    }

    #[test]
    fn an_in_band_tool_failure_is_an_err_value() {
        let result = json!({
            "content": [{"type": "text", "text": "not today"}],
            "isError": true,
        });
        let v = decode_tool_result("failing", &result);
        let Value::Result(rite_runtime::value::ResultValue::Err(e)) = v else {
            panic!("expected err, got {v}");
        };
        assert_eq!(e.get_field("kind").as_str(), Some("mcp.tool_error"));
        assert_eq!(e.get_field("message").as_str(), Some("not today"));
    }

    #[test]
    fn structured_content_wins_over_the_text_that_repeats_it() {
        let result = json!({
            "content": [{"type": "text", "text": "{\n  \"n\": 1\n}"}],
            "structuredContent": {"n": 1},
            "isError": false,
        });
        let Value::Result(rite_runtime::value::ResultValue::Ok(v)) =
            decode_tool_result("stats", &result)
        else {
            panic!("expected ok");
        };
        assert_eq!(v.get_field("n").as_int(), Some(1));
    }
}
