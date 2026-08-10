//! Model Context Protocol, over stdio and Streamable HTTP.
//!
//! This file is the server half. The client half — `@mcp.connect` and the calls that
//! take its handle — is in [`client`], and shares this module's revision constants and
//! `_meta` keys so the two cannot drift apart on what they speak.
//!
//! `@mcp.serve` is the `@http.listen` of this module: a declaration table rather than a
//! call graph, with each `tool` / `resource` / `prompt` body run per request in its own
//! `RuntimeContext`. The declarations arrive on `ctx.pending_mcp`, staged by the
//! evaluator, for the same reason routes do — a `BlockIr` cannot travel as a `Value`
//! through `CapabilityHost::call`.
//!
//! # The revision this speaks
//!
//! Natively, [`MCP_PROTOCOL_VERSION`] — the 2026-07-28 revision, in which MCP is
//! *stateless*: there is no `initialize` handshake, no session, and no `Mcp-Session-Id`.
//! Each request carries its own version in `_meta`, `server/discover` is the one
//! mandatory RPC, and list results carry cache hints.
//!
//! Clients in the field still speak the older shape, so a compatibility layer answers
//! [`MCP_LEGACY_PROTOCOL_VERSION`] as well. It is **one** legacy revision, deliberately,
//! and it is not a second implementation: every handler produces the modern result body
//! and [`encode_result`] is the only place that knows what actually goes on the wire.
//! For every method this server implements, the legacy body is the modern body minus
//! the fields that revision had not yet added, so there is nothing to keep in step.
//!
//! # What is deliberately not here
//!
//! - `subscriptions/listen` and the `*ListChanged` notifications. A Rite server's
//!   tables are fixed once `@mcp.serve` starts and can never change while it runs, so
//!   the capability is not advertised rather than advertised and never fired.
//! - `notifications/message` / the Logging feature, deprecated upstream in favour of
//!   logging to stderr — which is also the only thing that works on stdio, where
//!   stdout is the wire. `use @mcp.log` writes there.
//! - Multi Round-Trip Requests (`resultType: "input_required"`). Every result this
//!   server produces is `"complete"`, stamped in one place so adding the other value
//!   later is a new arm rather than a refactor.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use parking_lot::Mutex;
use rite_runtime::eval::{McpServeConfig, McpTransport, PendingMcpServer};
use rite_runtime::{
    schema, EvalError, Evaluator, FunctionEntry, HttpMiddleware, Key, RuntimeContext, Value,
};
use rite_sem::McpDeclIr;
use serde_json::{json, Map, Value as Json};
use std::collections::HashMap;
use std::sync::Arc;

/// The other direction: calling servers rather than being one.
#[cfg(not(target_arch = "wasm32"))]
pub mod client;

/// The MCP specification revision this server implements natively.
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

/// The one older revision the compatibility layer answers `initialize` for.
///
/// Chosen because it is what shipped clients negotiate today. When they have moved on,
/// deleting it is a matter of removing [`McpRevision::Legacy`] and the three arms in
/// [`dispatch`] that mention it — everything else is revision-agnostic by construction.
pub const MCP_LEGACY_PROTOCOL_VERSION: &str = "2025-06-18";

/// Every revision `server/discover` advertises and `_meta` negotiation accepts.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &[MCP_PROTOCOL_VERSION, MCP_LEGACY_PROTOCOL_VERSION];

/// `_meta` keys defined by the specification.
mod meta_keys {
    pub const PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
    pub const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
}

/// JSON-RPC error codes. `-32020`..`-32099` is the range the specification reserves for
/// itself; `-32000`..`-32019` stays implementation-defined.
mod codes {
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// This server is shutting down because a body called `@process.exit`.
    pub const SHUTTING_DOWN: i64 = -32000;
    pub const HEADER_MISMATCH: i64 = -32020;
    pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;
    /// What the legacy revision used for a missing resource, before it was aligned
    /// with JSON-RPC's own "invalid params".
    pub const LEGACY_RESOURCE_NOT_FOUND: i64 = -32002;
}

pub struct McpCap;

impl Default for McpCap {
    fn default() -> Self {
        Self::new()
    }
}

impl McpCap {
    pub fn new() -> Self {
        Self
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "serve",
            docs: "Serve the Model Context Protocol and block until shutdown. Takes a server \
                   name, or a config record `⟨name, version, transport, addr, instructions⟩` \
                   where `transport` is `#stdio` (the default) or `#http`. Each `tool`, \
                   `resource` and `prompt` in the block is published to the client, with its \
                   JSON Schema derived from the parameter types declared on it. Under \
                   `#http` the bind address needs `--allow net=<host>` unless it is loopback; \
                   `#stdio` needs no grant, being the process's own streams.",
            arity: 2,
            effectful: true,
            permission: "net",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "progress",
            docs: "Report progress from inside a tool body: `! @mcp.progress(0.5, \"halfway\")`. \
                   Sends a `notifications/progress` on the stream of the call being served. \
                   Only valid inside an `@mcp.serve` body — elsewhere it fails rather than \
                   silently doing nothing.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "log",
            docs: "Middleware marker: `use @mcp.log` inside an `@mcp.serve` block writes one \
                   structured JSON line per request to stderr. Stderr rather than the \
                   protocol's own logging notifications, which are deprecated, and because \
                   under `#stdio` stdout carries the wire.",
            arity: 0,
            effectful: false,
            permission: "",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "tool_schema",
            docs: "The JSON Schema a function would be published with, derived from its \
                   declared parameter types: `@mcp.tool_schema(add)`. Pure — for checking \
                   what a tool advertises without starting a server.",
            arity: 1,
            effectful: false,
            permission: "",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "connect",
            docs: "Open a connection to another MCP server and return `ok(handle)`. The spec \
                   is a record naming one transport: `⟨command: \"npx\", args: [\"-y\", \"…\"]⟩` \
                   starts a server as a subprocess and speaks JSON-RPC on its stdin and \
                   stdout, and `⟨url: \"https://example.com/mcp\"⟩` posts to a Streamable HTTP \
                   endpoint. Also understands `env` (record, stdio only), `headers` (record, \
                   HTTP only) and `timeout_ms` (default 30000, the ceiling on every request \
                   made through the handle). **A `command` spec needs `--allow process`; a \
                   `url` spec needs `--allow net=<host>`** — the grant is checked here and \
                   nowhere else, so the calls that take the handle need none of their own.",
            arity: 1,
            effectful: true,
            permission: "process",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "tools",
            docs: "What a connected server offers: `ok([⟨name, description, input_schema⟩])`. \
                   `input_schema` is the tool's JSON Schema, as a record.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "call_tool",
            docs:
                "Call a tool on a connected server: `! @mcp.call_tool(c, \"add\", ⟨a: 2, b: 3⟩)`. \
                   Answers `ok(value)` — the structured result if the server sent one, \
                   otherwise the text of its content blocks. A tool that fails in band \
                   (`isError`) is `err(⟨kind: \"mcp.tool_error\", tool, message, data⟩)` rather \
                   than a raise, so the reason is readable and the call can be retried. \
                   `message` is the failure's text; `data` is its own fields, present only \
                   when the server sent a structured failure.",
            arity: 3,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "resources",
            docs: "What a connected server publishes: `ok([⟨uri, name, description⟩])`.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "read_resource",
            docs: "Read one resource by URI: `! @mcp.read_resource(c, \"config://app\")`. Answers \
                   `ok(text)`, the contents joined with newlines; a JSON resource comes back as \
                   its text, which `@json.decode` parses.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "prompts",
            docs: "What prompts a connected server offers: \
                   `ok([⟨name, description, arguments⟩])`, where each argument is \
                   `⟨name, required⟩`.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "get_prompt",
            docs: "Render a prompt: `! @mcp.get_prompt(c, \"review\", ⟨code: src⟩)`. Answers \
                   `ok(⟨description, messages⟩)`, with each message `⟨role, text⟩`.",
            arity: 3,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "close",
            docs: "Close a connection handle. Under stdio the server is sent EOF and then \
                   stopped. Closing an unknown or already-closed handle answers ok(none), and \
                   every connection closes when the run ends.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        match method {
            "serve" => serve(perms, ctx).await,
            "progress" => progress(args, ctx),
            // Naming the marker rather than calling it for an effect, exactly as
            // `@http.log` does.
            "log" => Ok(Value::string("mcp.log")),
            "tool_schema" => tool_schema(args),
            // `connect` and everything that takes its handle, plus the unknown-name
            // error: the client half owns every remaining name.
            other => client_call(other, args, perms, ctx).await,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn client_call(
    method: &str,
    args: Vec<Value>,
    perms: &PermissionSet,
    ctx: &RuntimeContext,
) -> Result<Value, EvalError> {
    client::call(method, args, perms, ctx).await
}

#[cfg(target_arch = "wasm32")]
async fn client_call(
    _method: &str,
    _args: Vec<Value>,
    _perms: &PermissionSet,
    _ctx: &RuntimeContext,
) -> Result<Value, EvalError> {
    Err(EvalError::Capability(
        "@mcp.connect requires the native host: the browser runtime has neither \
         subprocesses nor a socket layer"
            .into(),
    ))
}

/// `@mcp.tool_schema(f)` — the schema `f` would be published with.
fn tool_schema(args: Vec<Value>) -> Result<Value, EvalError> {
    let f = args
        .first()
        .ok_or_else(|| EvalError::Message("@mcp.tool_schema expects a function".into()))?;
    let Value::Function(c) = f else {
        return Err(EvalError::Message(format!(
            "@mcp.tool_schema expects a function, got {}",
            f.type_name()
        )));
    };
    let json = match &c.contract {
        Some(contract) => schema::json_schema_for_contract(contract),
        // No declaration to project: every parameter is unconstrained, but each is
        // still a slot that has to be filled.
        None => schema::json_schema_for_params(c.params.iter().map(|p| (p.as_str(), None))),
    };
    Ok(Value::from_json(&json))
}

/// `! @mcp.progress(fraction, message)` from inside a tool body.
fn progress(args: Vec<Value>, ctx: &RuntimeContext) -> Result<Value, EvalError> {
    let Some(notify) = ctx.mcp_notify.clone() else {
        return Err(EvalError::Message(
            "@mcp.progress is only valid inside an @mcp.serve tool body".into(),
        ));
    };
    let mut params = Map::new();
    match args.first() {
        Some(Value::Int(n)) => {
            params.insert("progress".into(), json!(*n));
        }
        Some(Value::Float(f)) => {
            params.insert("progress".into(), json!(*f));
        }
        _ => {
            params.insert("progress".into(), json!(0));
        }
    }
    if let Some(m) = args.get(1).and_then(|v| v.as_str()) {
        params.insert("message".into(), json!(m));
    }
    notify(json!({
        "jsonrpc": "2.0",
        "method": "notifications/progress",
        "params": params,
    }));
    Ok(Value::None)
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// Everything a request needs, captured once when `serve` starts.
#[derive(Clone)]
pub(crate) struct McpServerState {
    config: McpServeConfig,
    tools: Vec<McpDeclIr>,
    resources: Vec<McpDeclIr>,
    prompts: Vec<McpDeclIr>,
    perms: PermissionSet,
    /// Cloning an `Environment` shares its frames, so module state persists across
    /// requests — a top-level counter counts every one.
    module_env: rite_runtime::Environment,
    functions: HashMap<String, FunctionEntry>,
    log: bool,
}

impl McpServerState {
    fn decls(&self, kind: &str) -> &[McpDeclIr] {
        match kind {
            "tool" => &self.tools,
            "resource" => &self.resources,
            _ => &self.prompts,
        }
    }

    fn find<'a>(&'a self, kind: &str, name: &str) -> Option<&'a McpDeclIr> {
        self.decls(kind).iter().find(|d| d.name == name)
    }
}

// ---------------------------------------------------------------------------
// Revisions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum McpRevision {
    Current,
    Legacy,
}

impl McpRevision {
    fn resource_not_found_code(self) -> i64 {
        match self {
            McpRevision::Current => codes::INVALID_PARAMS,
            McpRevision::Legacy => codes::LEGACY_RESOURCE_NOT_FOUND,
        }
    }
}

/// Check the version a request declared, if it declared one.
///
/// An absent version is treated as current rather than refused: the specification only
/// says clients SHOULD send it, and refusing a bare request would make the stdio
/// transport untestable by hand.
fn negotiate(meta: &Json) -> Result<(), (i64, String)> {
    let Some(v) = meta
        .get(meta_keys::PROTOCOL_VERSION)
        .and_then(|v| v.as_str())
    else {
        return Ok(());
    };
    if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) {
        Ok(())
    } else {
        Err((
            codes::UNSUPPORTED_PROTOCOL_VERSION,
            format!(
                "unsupported protocol version `{v}` (this server speaks {})",
                SUPPORTED_PROTOCOL_VERSIONS.join(", ")
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// The normalized request, and the one dispatcher
// ---------------------------------------------------------------------------

pub(crate) struct McpRequest {
    /// `None` for a notification, which gets no response.
    id: Option<Json>,
    method: String,
    params: Json,
    meta: Json,
    revision: McpRevision,
}

impl McpRequest {
    fn from_json(v: &Json, revision: McpRevision) -> Self {
        let params = v.get("params").cloned().unwrap_or_else(|| json!({}));
        let meta = params.get("_meta").cloned().unwrap_or_else(|| json!({}));
        McpRequest {
            id: v.get("id").cloned(),
            method: v
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or_default()
                .to_string(),
            params,
            meta,
            revision,
        }
    }

    fn arg_str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(|v| v.as_str())
    }
}

/// A handler's answer, always in the modern shape. Nothing here knows about revisions.
struct McpOk {
    body: Map<String, Json>,
    /// `Some` only for the methods the specification makes cacheable.
    cache: Option<CacheHint>,
}

struct CacheHint {
    ttl_ms: i64,
    scope: &'static str,
}

impl McpOk {
    fn new(body: Map<String, Json>) -> Self {
        McpOk { body, cache: None }
    }
    fn cached(mut self, ttl_ms: i64, scope: &'static str) -> Self {
        self.cache = Some(CacheHint { ttl_ms, scope });
        self
    }
}

/// The single place that decides what actually goes on the wire.
///
/// The legacy revision predates `resultType`, the cache hints, and the `_meta` server
/// identity, so for it those fields are left off. That is the difference,
/// which is why one dispatcher can serve both.
fn encode_result(res: McpOk, rev: McpRevision, state: &McpServerState) -> Json {
    let mut body = res.body;
    if rev == McpRevision::Current {
        body.insert("resultType".into(), json!("complete"));
        if let Some(c) = res.cache {
            body.insert("ttlMs".into(), json!(c.ttl_ms));
            body.insert("cacheScope".into(), json!(c.scope));
        }
        body.insert(
            "_meta".into(),
            json!({ meta_keys::SERVER_INFO: server_info(state) }),
        );
    }
    Json::Object(body)
}

fn server_info(state: &McpServerState) -> Json {
    json!({"name": state.config.name, "version": state.config.version})
}

fn capabilities() -> Json {
    // No `subscribe` and no `listChanged` anywhere: the tables cannot change while the
    // server runs, so claiming otherwise would be a lie a client could act on.
    json!({
        "tools": {"listChanged": false},
        "resources": {"listChanged": false, "subscribe": false},
        "prompts": {"listChanged": false},
    })
}

fn error_response(id: Option<Json>, code: i64, message: impl Into<String>) -> Json {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Json::Null),
        "error": {"code": code, "message": message.into()},
    })
}

/// Handle one request. `None` means "this was a notification; say nothing".
pub(crate) async fn dispatch(
    state: &McpServerState,
    req: McpRequest,
    notify: &rite_runtime::eval::McpNotifier,
) -> Option<Json> {
    let started = std::time::Instant::now();
    let response = dispatch_inner(state, &req, notify).await;
    if state.log {
        log_line(&req, &response, started.elapsed());
    }
    // A notification carries no id and must not be answered, even to report an error.
    req.id.as_ref()?;
    response
}

async fn dispatch_inner(
    state: &McpServerState,
    req: &McpRequest,
    notify: &rite_runtime::eval::McpNotifier,
) -> Option<Json> {
    let id = req.id.clone();
    if let Err((code, msg)) = negotiate(&req.meta) {
        return Some(error_response(id, code, msg));
    }
    let rev = req.revision;

    let ok = |res: McpOk| {
        Some(json!({
            "jsonrpc": "2.0",
            "id": id.clone().unwrap_or(Json::Null),
            "result": encode_result(res, rev, state),
        }))
    };

    match req.method.as_str() {
        "server/discover" => {
            let mut body = json!({
                "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
                "capabilities": capabilities(),
                "serverInfo": server_info(state),
            })
            .as_object()
            .cloned()
            .unwrap_or_default();
            if let Some(i) = &state.config.instructions {
                body.insert("instructions".into(), json!(i));
            }
            ok(McpOk::new(body))
        }

        // --- the legacy compatibility layer -------------------------------------
        // Three arms, and nothing below this point mentions the older revision.
        "initialize" => ok(McpOk::new(
            json!({
                "protocolVersion": MCP_LEGACY_PROTOCOL_VERSION,
                "capabilities": capabilities(),
                "serverInfo": server_info(state),
            })
            .as_object()
            .cloned()
            .unwrap_or_default(),
        )),
        "notifications/initialized" => None,
        "ping" => ok(McpOk::new(Map::new())),

        // --- listing --------------------------------------------------------------
        "tools/list" => {
            let tools: Vec<Json> = state
                .tools
                .iter()
                .map(|d| {
                    json!({
                        "name": d.name,
                        "description": d.description.clone().unwrap_or_default(),
                        "inputSchema": input_schema(d),
                    })
                })
                .collect();
            let mut body = Map::new();
            body.insert("tools".into(), Json::Array(tools));
            ok(McpOk::new(body).cached(state.config.list_ttl_ms, "public"))
        }
        "prompts/list" => {
            let prompts: Vec<Json> = state
                .prompts
                .iter()
                .map(|d| {
                    json!({
                        "name": d.name,
                        "description": d.description.clone().unwrap_or_default(),
                        "arguments": d.params.iter().map(|p| json!({
                            "name": p.name,
                            "required": true,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let mut body = Map::new();
            body.insert("prompts".into(), Json::Array(prompts));
            ok(McpOk::new(body).cached(state.config.list_ttl_ms, "public"))
        }
        "resources/list" => {
            let resources: Vec<Json> = state
                .resources
                .iter()
                .map(|d| {
                    json!({
                        "uri": d.name,
                        "name": d.name,
                        "description": d.description.clone().unwrap_or_default(),
                    })
                })
                .collect();
            let mut body = Map::new();
            body.insert("resources".into(), Json::Array(resources));
            ok(McpOk::new(body).cached(state.config.list_ttl_ms, "public"))
        }

        // --- invocation -------------------------------------------------------------
        "tools/call" => {
            let Some(name) = req.arg_str("name").map(|s| s.to_string()) else {
                return Some(error_response(
                    id,
                    codes::INVALID_PARAMS,
                    "missing tool name",
                ));
            };
            let Some(decl) = state.find("tool", &name).cloned() else {
                return Some(error_response(
                    id,
                    codes::INVALID_PARAMS,
                    format!("unknown tool `{name}`"),
                ));
            };
            let args = match marshal_args(&decl, req.params.get("arguments")) {
                Ok(a) => a,
                Err(msg) => return Some(error_response(id, codes::INVALID_PARAMS, msg)),
            };
            match run_decl(state, &decl, args, notify).await {
                Err(RunFailure::ShuttingDown(code)) => Some(error_response(
                    id,
                    codes::SHUTTING_DOWN,
                    format!("server is shutting down (exit {code})"),
                )),
                Ok(outcome) => ok(McpOk::new(outcome.into_tool_result())),
            }
        }
        "resources/read" => {
            let Some(uri) = req.arg_str("uri").map(|s| s.to_string()) else {
                return Some(error_response(
                    id,
                    codes::INVALID_PARAMS,
                    "missing resource uri",
                ));
            };
            let Some(decl) = state.find("resource", &uri).cloned() else {
                return Some(error_response(
                    id,
                    rev.resource_not_found_code(),
                    format!("unknown resource `{uri}`"),
                ));
            };
            match run_decl(state, &decl, Vec::new(), notify).await {
                Err(RunFailure::ShuttingDown(code)) => Some(error_response(
                    id,
                    codes::SHUTTING_DOWN,
                    format!("server is shutting down (exit {code})"),
                )),
                Ok(outcome) if outcome.is_error => Some(error_response(
                    id,
                    rev.resource_not_found_code(),
                    outcome.text,
                )),
                Ok(outcome) => {
                    let mut body = Map::new();
                    body.insert(
                        "contents".into(),
                        json!([{
                            "uri": uri,
                            "mimeType": if outcome.structured.is_some() {
                                "application/json"
                            } else {
                                "text/plain"
                            },
                            "text": outcome.text,
                        }]),
                    );
                    // A resource body runs arbitrary Rite — it may read a file or the
                    // clock — so unlike the fixed listings it is not cached by default.
                    ok(McpOk::new(body).cached(0, "private"))
                }
            }
        }
        "prompts/get" => {
            let Some(name) = req.arg_str("name").map(|s| s.to_string()) else {
                return Some(error_response(
                    id,
                    codes::INVALID_PARAMS,
                    "missing prompt name",
                ));
            };
            let Some(decl) = state.find("prompt", &name).cloned() else {
                return Some(error_response(
                    id,
                    codes::INVALID_PARAMS,
                    format!("unknown prompt `{name}`"),
                ));
            };
            let args = match marshal_args(&decl, req.params.get("arguments")) {
                Ok(a) => a,
                Err(msg) => return Some(error_response(id, codes::INVALID_PARAMS, msg)),
            };
            match run_decl(state, &decl, args, notify).await {
                Err(RunFailure::ShuttingDown(code)) => Some(error_response(
                    id,
                    codes::SHUTTING_DOWN,
                    format!("server is shutting down (exit {code})"),
                )),
                Ok(outcome) if outcome.is_error => {
                    Some(error_response(id, codes::INTERNAL, outcome.text))
                }
                Ok(outcome) => {
                    let mut body = Map::new();
                    body.insert(
                        "description".into(),
                        json!(decl.description.clone().unwrap_or_default()),
                    );
                    body.insert(
                        "messages".into(),
                        json!([{
                            "role": "user",
                            "content": {"type": "text", "text": outcome.text},
                        }]),
                    );
                    ok(McpOk::new(body))
                }
            }
        }

        other => Some(error_response(
            id,
            codes::METHOD_NOT_FOUND,
            format!("unknown method `{other}`"),
        )),
    }
}

fn input_schema(d: &McpDeclIr) -> Json {
    schema::json_schema_for_params(d.params.iter().map(|p| (p.name.as_str(), p.ty.as_ref())))
}

// ---------------------------------------------------------------------------
// Running a declaration body
// ---------------------------------------------------------------------------

/// The only failure that is not an in-band result: the body chose an exit status for
/// the whole process, so there is nothing left to answer with.
enum RunFailure {
    ShuttingDown(u8),
}

struct Outcome {
    text: String,
    structured: Option<Json>,
    is_error: bool,
}

impl Outcome {
    fn error(text: impl Into<String>) -> Self {
        Outcome {
            text: text.into(),
            structured: None,
            is_error: true,
        }
    }

    fn into_tool_result(self) -> Map<String, Json> {
        let mut body = Map::new();
        body.insert(
            "content".into(),
            json!([{"type": "text", "text": self.text}]),
        );
        if let Some(s) = self.structured {
            body.insert("structuredContent".into(), s);
        }
        body.insert("isError".into(), json!(self.is_error));
        body
    }
}

/// Map the client's named arguments onto the declaration's positional parameters.
///
/// A missing argument is a protocol error rather than a substituted `none`: the schema
/// said it was required, and letting it through only produces a worse message one layer
/// down. An *extra* argument is ignored — clients bolt on metadata, `additionalProperties`
/// is permissive by default, and rejecting buys no safety.
fn marshal_args(decl: &McpDeclIr, arguments: Option<&Json>) -> Result<Vec<Value>, String> {
    let empty = Map::new();
    let obj = arguments.and_then(|a| a.as_object()).unwrap_or(&empty);
    let mut out = Vec::with_capacity(decl.params.len());
    for p in &decl.params {
        match obj.get(&p.name) {
            Some(v) => out.push(Value::from_json(v)),
            None => {
                return Err(format!(
                    "missing required argument `{}` for `{}`",
                    p.name, decl.name
                ))
            }
        }
    }
    Ok(out)
}

/// Run one declaration body in its own context.
///
/// Argument types are checked by the ordinary contract machinery: the body is wrapped
/// in a `Closure` carrying an `FnContract` built from the declared parameters, so a
/// client passing a string where `int` was declared gets exactly the message a Rite
/// caller would have got — and there is no second type checker to keep in agreement
/// with the first.
async fn run_decl(
    state: &McpServerState,
    decl: &McpDeclIr,
    args: Vec<Value>,
    notify: &rite_runtime::eval::McpNotifier,
) -> Result<Outcome, RunFailure> {
    let mut ctx = RuntimeContext::new();
    crate::install_defaults(&mut ctx, state.perms.clone());
    crate::http::install_module_scope(&mut ctx, &state.module_env, &state.functions);
    ctx.mcp_notify = Some(notify.clone());
    if state.config.transport == McpTransport::Stdio {
        // stdout is the wire under stdio, so everything the body prints goes to
        // stderr. Installed on this context, not the script's, so it lasts exactly as
        // long as the call.
        ctx.sink = Some(stderr_sink());
    }

    let contract = rite_runtime::value::FnContract {
        name: format!("{} {}", decl.kind, decl.name),
        param_names: decl.params.iter().map(|p| p.name.clone()).collect(),
        param_types: decl.params.iter().map(|p| p.ty.clone()).collect(),
        return_type: None,
    };
    let callee = Value::Function(rite_runtime::Closure {
        id: 0,
        name: Some(decl.name.as_str().into()),
        params: contract.param_names.clone(),
        env: Arc::new(parking_lot::RwLock::new(ctx.env.clone())),
        body: std::sync::Arc::new(decl.body.clone()),
        contract: Some(Arc::new(contract)),
    });

    let result = {
        let mut eval = Evaluator::new(&mut ctx);
        eval.call_value_public(callee, args).await
    };

    let outcome = match result {
        Ok(v) | Err(EvalError::Return(v)) => coerce_result(v, &ctx),
        Err(EvalError::Exit(code)) => {
            crate::http::request_exit(code);
            return Err(RunFailure::ShuttingDown(code));
        }
        Err(e) => Outcome::error(e.to_string()),
    };

    // Under stdio the sink already streamed everything to stderr; `flush_handler_io`
    // would send `ctx.stdout` to the real stdout and corrupt the wire.
    if state.config.transport != McpTransport::Stdio {
        crate::http::flush_handler_io(&ctx);
    }
    Ok(outcome)
}

/// Shape a body's return value the way `@http`'s `coerce_response` shapes a handler's.
fn coerce_result(v: Value, ctx: &RuntimeContext) -> Outcome {
    match v {
        // `^ err(e)` is the body saying the call failed — in band, so the model can
        // read the reason and correct itself, rather than as a transport error.
        //
        // The payload is shaped like any other return value, so a record arrives with
        // its fields intact in `structuredContent` instead of being flattened to text.
        // It used to be `to_display`ed and nothing else, so `^ err(⟨kind: "bad_input",
        // message: "cannot divide by zero"⟩)` reached a client as the one string
        // `⟨kind: bad_input, message: cannot divide by zero⟩` — every field gone, and
        // `e.message` on the other side the whole record rendered.
        Value::Result(rite_runtime::value::ResultValue::Err(e)) => {
            let mut outcome = coerce_result((*e).clone(), ctx);
            // An error record carries its reason in `message`, the convention every
            // error record in this tree follows, and the protocol's content block is
            // where a model reads that reason. Pretty-printed JSON of the whole record
            // is worse at that job than the sentence inside it.
            if let Value::Record(fields) = &*e {
                if let Some(m) = fields
                    .get(&Key::String("message".into()))
                    .and_then(|v| v.as_str())
                {
                    outcome.text = m.to_string();
                }
            }
            outcome.is_error = true;
            outcome
        }
        // `^ ok(x)` is a success carrying `x`.
        Value::Result(rite_runtime::value::ResultValue::Ok(inner)) => coerce_result(*inner, ctx),
        Value::String(s) => Outcome {
            text: s.to_string(),
            structured: None,
            is_error: false,
        },
        Value::None => Outcome {
            text: String::new(),
            structured: None,
            is_error: false,
        },
        // A structure gets both forms: text for a model that reads content, and
        // `structuredContent` for a client that wants the data.
        v @ (Value::Record(_) | Value::List(_)) => {
            let j = v.to_json(&ctx.atoms);
            Outcome {
                text: serde_json::to_string_pretty(&j).unwrap_or_default(),
                structured: Some(j),
                is_error: false,
            }
        }
        other => Outcome {
            // `to_display`, not `Display`: only the former can resolve an atom to its
            // name rather than its interner index.
            text: other.to_display(&ctx.atoms),
            structured: None,
            is_error: false,
        },
    }
}

/// Route every stream to stderr. Used for the per-call context under stdio.
fn stderr_sink() -> rite_runtime::OutputSink {
    Arc::new(|_stream, text: &str| {
        crate::http::emit_process_stderr(text);
    })
}

// ---------------------------------------------------------------------------
// Serving
// ---------------------------------------------------------------------------

/// The address the most recent HTTP-transport server bound.
///
/// Its own static rather than `@http`'s: two servers of different kinds in one test
/// binary must not overwrite each other's answer. `serve` blocks until shutdown, so a
/// test cannot read the bound port from its return value — with `addr: "127.0.0.1:0"`
/// there would otherwise be no way to learn which port to talk to.
static MCP_LAST_BOUND_ADDR: Mutex<Option<String>> = Mutex::new(None);

pub fn last_mcp_bound_addr() -> Option<String> {
    MCP_LAST_BOUND_ADDR.lock().clone()
}

pub fn clear_last_mcp_bound_addr() {
    *MCP_LAST_BOUND_ADDR.lock() = None;
}

/// `@mcp.serve` — build the server state and hand off to the transport.
async fn serve(perms: &PermissionSet, ctx: &RuntimeContext) -> Result<Value, EvalError> {
    let pending = ctx.pending_mcp.clone().ok_or_else(|| {
        EvalError::Message(
            "@mcp.serve expects a declaration block: \
             `! @mcp.serve \"calculator\" ⟦ tool \"add\" |a: int| ⟦ … ⟧ ⟧`"
                .into(),
        )
    })?;
    let PendingMcpServer {
        config,
        tools,
        resources,
        prompts,
        middleware,
    } = pending;

    let log = middleware
        .iter()
        .any(|m| matches!(m, HttpMiddleware::Named(n) if n == "log"));

    let state = McpServerState {
        config: config.clone(),
        tools,
        resources,
        prompts,
        perms: perms.clone(),
        module_env: ctx.env.clone(),
        functions: ctx.functions.clone(),
        log,
    };

    match config.transport {
        McpTransport::Stdio => serve_stdio(state).await,
        McpTransport::Http => serve_http(state).await,
    }
}

/// Drain any notifications a body emitted, then the response, onto one writer.
///
/// A notification belongs to the request that produced it and must reach the client
/// before that request's result, which is why they are drained here rather than raced
/// onto the stream from the tool body's own task.
async fn write_json_line<W>(w: &mut W, v: &Json) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let mut line = serde_json::to_string(v).unwrap_or_else(|_| "{}".into());
    line.push('\n');
    w.write_all(line.as_bytes()).await?;
    w.flush().await
}

/// The stdio transport, over any pair of streams.
///
/// Generic rather than hard-wired to the process's stdin/stdout so the wire format can
/// be driven from a test over an in-memory pipe, with no environment hook and no real
/// process stdio involved.
pub(crate) async fn serve_streams<R, W>(
    state: McpServerState,
    reader: R,
    mut writer: W,
) -> Result<Value, EvalError>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    // One ordered byte stream means one request at a time; there is nothing to
    // interleave, and a call's progress notifications cannot collide with another's.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Json>();
    let notify: rite_runtime::eval::McpNotifier = Arc::new(move |v: Json| {
        let _ = tx.send(v);
    });

    let mut revision = McpRevision::Current;
    let mut lines = reader.lines();

    loop {
        // EOF is how a client closes an stdio session: a clean shutdown, not a failure.
        let Ok(Some(line)) = lines.next_line().await else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed: Json = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = error_response(None, codes::INVALID_PARAMS, format!("invalid JSON: {e}"));
                let _ = write_json_line(&mut writer, &err).await;
                continue;
            }
        };

        // A client that sends `initialize` is a legacy client, and stays one for the
        // rest of the connection. `server/discover` never changes the answer, so a
        // modern client that probes with it first is not mistaken for a legacy one,
        // and a cautious client that probes and *then* sends `initialize` still gets
        // the older shape it is expecting.
        if parsed.get("method").and_then(|m| m.as_str()) == Some("initialize") {
            revision = McpRevision::Legacy;
        }

        let req = McpRequest::from_json(&parsed, revision);
        let response = dispatch(&state, req, &notify).await;

        while let Ok(n) = rx.try_recv() {
            let _ = write_json_line(&mut writer, &n).await;
        }
        if let Some(r) = response {
            write_json_line(&mut writer, &r)
                .await
                .map_err(|e| EvalError::Capability(format!("mcp: write failed: {e}")))?;
        }

        if let Some(code) = crate::http::take_exit_request() {
            return Err(EvalError::Exit(code));
        }
    }

    // Deliberately `none`, where the HTTP transport answers a status record: `rite run`
    // prints a script's result value, and under stdio that would put one last
    // non-protocol line on the wire after the session ended. Nothing may reach stdout
    // here that a client could still read.
    Ok(Value::None)
}

/// Streams substituted for the process's own, for tests.
///
/// A real stdio server owns the process's stdin and stdout, which a test cannot borrow.
/// Rather than reach for a subprocess — which would make every protocol assertion a
/// string match on a pipe — the transport is generic over its streams and this slot
/// swaps in a pair backed by memory. It is process-global and so guarded by the test
/// lock, in the same spirit as `RITE_HTTP_TEST`; nothing in a normal run ever sets it.
/// The input to replay, and the buffer the server's output is collected into.
type TestStreams = (String, Arc<Mutex<Vec<u8>>>);

static TEST_STREAMS: Mutex<Option<TestStreams>> = Mutex::new(None);

/// An `AsyncWrite` that appends to a shared buffer.
struct SharedBufWriter(Arc<Mutex<Vec<u8>>>);

impl tokio::io::AsyncWrite for SharedBufWriter {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.lock().extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

/// Run `source` with the stdio transport wired to `input`, and return what the server
/// wrote. The script is parsed, resolved and evaluated normally — only the two streams
/// are substituted, so this exercises the whole path a real session takes.
///
/// Test-only in practice, but not `#[cfg(test)]`: integration tests are separate
/// crates and cannot see a unit-test-only item.
#[cfg(not(target_arch = "wasm32"))]
pub async fn serve_test_streams(
    ctx: &mut RuntimeContext,
    name: &str,
    source: &str,
    input: &str,
) -> Result<String, EvalError> {
    let out = Arc::new(Mutex::new(Vec::new()));
    *TEST_STREAMS.lock() = Some((input.to_string(), out.clone()));
    let result = rite_runtime::run_source(name, source, ctx).await;
    // Clear even on failure, so one failing test cannot strand the next one.
    *TEST_STREAMS.lock() = None;
    result?;
    let bytes = out.lock().clone();
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(not(target_arch = "wasm32"))]
async fn serve_stdio(state: McpServerState) -> Result<Value, EvalError> {
    // Taken into a local before any `await`: the guard itself is not `Send`, and the
    // capability's future has to be.
    let substituted = TEST_STREAMS.lock().take();
    if let Some((input, out)) = substituted {
        let reader = tokio::io::BufReader::new(std::io::Cursor::new(input.into_bytes()));
        return serve_streams(state, reader, SharedBufWriter(out)).await;
    }
    // Not `println!`: under stdio that is the wire. `@http.listen` and `@tcp.listen`
    // announce themselves on stdout precisely because nothing there is listening for
    // a protocol.
    crate::http::emit_process_stderr(&format!(
        "rite: mcp server {} on stdio\n",
        state.config.name
    ));
    let reader = tokio::io::BufReader::new(tokio::io::stdin());
    serve_streams(state, reader, tokio::io::stdout()).await
}

#[cfg(target_arch = "wasm32")]
async fn serve_stdio(_state: McpServerState) -> Result<Value, EvalError> {
    Err(EvalError::Capability(
        "@mcp requires the native host: the browser runtime has no process streams".into(),
    ))
}

#[cfg(not(target_arch = "wasm32"))]
async fn serve_http(state: McpServerState) -> Result<Value, EvalError> {
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use tokio::sync::oneshot;

    let addr = state.config.addr.clone();
    // The same bind policy `@http.listen` applies, through the same function: loopback
    // by default, anything else needs `--allow net=<host>`.
    crate::http::check_bind_perm(&addr, &state.perms, "serve")?;

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| EvalError::Capability(format!("mcp bind failed: {e}")))?;
    let local = listener
        .local_addr()
        .map_err(|e| EvalError::Capability(e.to_string()))?
        .to_string();
    *MCP_LAST_BOUND_ADDR.lock() = Some(local.clone());
    println!("rite: mcp listening on http://{local}/mcp");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let stop = Arc::new(crate::http::arm_server_stop(shutdown_tx));
    let _ = ctrlc::set_handler(crate::http::request_stop);

    // Test mode: stop on a timer, because `serve` otherwise blocks until Ctrl-C and a
    // test has no other way to get its thread back.
    if std::env::var("RITE_MCP_TEST").as_deref() == Ok("1") {
        let secs = std::env::var("RITE_MCP_TEST_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);
        let stop = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
            stop.stop();
        });
    }

    let state = Arc::new(state);
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else { break };
                let state = state.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                        let state = state.clone();
                        async move {
                            Ok::<_, std::convert::Infallible>(handle_http(state, req).await)
                        }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        }
    }
    stop.release();

    if let Some(code) = crate::http::take_exit_request() {
        return Err(EvalError::Exit(code));
    }
    Ok(Value::record(vec![
        (Key::String("addr".into()), Value::string(local)),
        (Key::String("transport".into()), Value::string("http")),
        (Key::String("status".into()), Value::string("stopped")),
    ]))
}

/// One Streamable HTTP request.
///
/// Stateless since 2026-07-28: a plain POST, no session id, no GET stream to resume.
#[cfg(not(target_arch = "wasm32"))]
async fn handle_http(
    state: Arc<McpServerState>,
    req: hyper::Request<hyper::body::Incoming>,
) -> hyper::Response<String> {
    use http_body_util::BodyExt;

    let reply = |code: u16, body: Json| {
        hyper::Response::builder()
            .status(code)
            .header("content-type", "application/json")
            .body(body.to_string())
            .unwrap()
    };

    if req.method() != hyper::Method::POST {
        return reply(
            405,
            error_response(None, codes::METHOD_NOT_FOUND, "MCP expects POST"),
        );
    }
    let header = |name: &str| {
        req.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    };
    let declared_method = header("mcp-method");
    let declared_name = header("mcp-name");
    let version_header = header("mcp-protocol-version");

    let body = match req.into_body().collect().await {
        Ok(b) => b.to_bytes(),
        Err(e) => {
            return reply(
                400,
                error_response(None, codes::INVALID_PARAMS, format!("unreadable body: {e}")),
            )
        }
    };
    let parsed: Json = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return reply(
                400,
                error_response(None, codes::INVALID_PARAMS, format!("invalid JSON: {e}")),
            )
        }
    };

    let method = parsed
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    // There is no connection to pin a revision to, which is what statelessness buys:
    // the revision is a pure function of the request.
    let legacy_method = method == "initialize" || method == "ping";
    let revision = if parsed
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(meta_keys::PROTOCOL_VERSION))
        .is_some()
    {
        McpRevision::Current
    } else if legacy_method
        || version_header
            .as_deref()
            .is_some_and(|v| v.starts_with("2025-"))
    {
        McpRevision::Legacy
    } else {
        McpRevision::Current
    };

    // `Mcp-Method` / `Mcp-Name` are required so an intermediary can route without
    // parsing the body — which means they have to agree with it. Legacy clients
    // predate the requirement and are exempt.
    if !legacy_method {
        if let Some(declared) = &declared_method {
            if declared != &method {
                return reply(
                    400,
                    error_response(
                        parsed.get("id").cloned(),
                        codes::HEADER_MISMATCH,
                        format!("Mcp-Method `{declared}` does not match body method `{method}`"),
                    ),
                );
            }
        }
        // An absent or empty `Mcp-Name` matches anything: several methods have no name
        // to carry, and the permissive reading is the one that does not break clients.
        if let Some(declared) = declared_name.as_deref().filter(|s| !s.is_empty()) {
            let actual = parsed
                .get("params")
                .and_then(|p| p.get("name").or_else(|| p.get("uri")))
                .and_then(|v| v.as_str());
            if let Some(actual) = actual {
                if declared != actual {
                    return reply(
                        400,
                        error_response(
                            parsed.get("id").cloned(),
                            codes::HEADER_MISMATCH,
                            format!("Mcp-Name `{declared}` does not match `{actual}`"),
                        ),
                    );
                }
            }
        }
    }

    // Progress has no stream to ride on for a plain JSON response. Rather than drop it
    // silently, surface it where the operator can see it.
    let log = state.log;
    let notify: rite_runtime::eval::McpNotifier = Arc::new(move |v: Json| {
        if log {
            crate::http::emit_process_stderr(&format!("{v}\n"));
        }
    });

    let request = McpRequest::from_json(&parsed, revision);
    match dispatch(&state, request, &notify).await {
        Some(r) => reply(200, r),
        // A notification is answered with 202 and no body.
        None => hyper::Response::builder()
            .status(202)
            .body(String::new())
            .unwrap(),
    }
}

#[cfg(target_arch = "wasm32")]
async fn serve_http(_state: McpServerState) -> Result<Value, EvalError> {
    Err(EvalError::Capability(
        "@mcp requires the native host: the browser runtime has no socket layer".into(),
    ))
}

/// One structured line per request, for `use @mcp.log`.
fn log_line(req: &McpRequest, response: &Option<Json>, elapsed: std::time::Duration) {
    let failed = response
        .as_ref()
        .map(|r| r.get("error").is_some())
        .unwrap_or(false);
    let name = req
        .arg_str("name")
        .or_else(|| req.arg_str("uri"))
        .unwrap_or("");
    let line = json!({
        "level": "info",
        "method": req.method,
        "name": name,
        "ms": elapsed.as_millis() as u64,
        "ok": !failed,
    });
    crate::http::emit_process_stderr(&format!("{line}\n"));
}
