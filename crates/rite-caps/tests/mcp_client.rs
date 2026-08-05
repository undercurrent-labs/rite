//! `@mcp.connect` and everything that takes its handle, against a real server.
//!
//! The server is Rite's own, over the HTTP transport, in this process: `@mcp.serve`
//! binds a loopback port and the client script talks to it through a socket. Nothing
//! here is a mock, so the encoders on both sides are tested against each other — a
//! client that decoded `inputSchema` under the wrong name would fail here even though
//! both halves passed their own tests.
//!
//! The stdio transport needs a subprocess to be a server, so it is covered where the
//! CLI binary is available: `crates/rite-cli/tests/mcp_client_stdio.rs`.
//!
//! The legacy handshake is covered by `an_initialize_fallback_is_used_…` below, which
//! stands up a server that has never heard of `server/discover`.

// Each test holds the lock across its awaits, since `RITE_MCP_TEST` and the
// last-bound-address slot are process-global.
#![allow(clippy::await_holding_lock)]

use rite_caps::install_defaults;
use rite_caps::mcp::{clear_last_mcp_bound_addr, last_mcp_bound_addr};
use rite_caps::permissions::{Permission, PermissionSet};
use rite_runtime::{run_source, EvalError, RuntimeContext, Value};
use serde_json::{json, Value as Json};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn mcp_client_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

const SERVER: &str = r#"
! @mcp.serve ⟨name: "calculator", transport: #http, addr: "127.0.0.1:0"⟩ ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧

  tool "stats" "Summarise a list of numbers" |xs: [int]| ⟦
    ^ ⟨count: xs → count, total: xs → sum⟩
  ⟧

  tool "failing" "Always fails" ⟦
    ^ err(⟨kind: "nope", message: "not today"⟩)
  ⟧

  resource "config://app" "App config" ⟦
    ^ "debug=true"
  ⟧

  prompt "review" "Review some code" |code: string| ⟦
    ^ "Please review: " + code
  ⟧
⟧
"#;

async fn spawn_server() -> tokio::task::JoinHandle<()> {
    clear_last_mcp_bound_addr();
    std::env::set_var("RITE_MCP_TEST", "1");
    std::env::set_var("RITE_MCP_TEST_SECS", "20");
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("mcp-server.rite", SERVER, &mut ctx).await;
    })
}

/// `serve` blocks until shutdown, so the bound port cannot come back from its return
/// value — with `addr: "…:0"` there would otherwise be no way to learn it.
async fn wait_for_bind() -> String {
    for _ in 0..250 {
        if let Some(a) = last_mcp_bound_addr() {
            return a;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server never bound");
}

/// Run a client script with the permissions it should have had.
async fn run_client(source: &str, perms: PermissionSet) -> Result<Value, EvalError> {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, perms);
    run_source("mcp-client.rite", source, &mut ctx).await
}

fn loopback_net() -> PermissionSet {
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::Net("127.0.0.1".into()));
    perms
}

#[tokio::test]
async fn a_script_drives_another_server_over_http() {
    let _g = mcp_client_lock().lock().unwrap_or_else(|e| e.into_inner());
    let server = spawn_server().await;
    let addr = wait_for_bind().await;

    let source = format!(
        r#"
c ← ! @mcp.connect(⟨url: "http://{addr}/mcp"⟩)?
tools ← ! @mcp.tools(c)?
sum ← ! @mcp.call_tool(c, "add", ⟨a: 2, b: 3⟩)?
stats ← ! @mcp.call_tool(c, "stats", ⟨xs: [1, 2, 3]⟩)?
failed ← ! @mcp.call_tool(c, "failing", ⟨⟩)
resources ← ! @mcp.resources(c)?
config ← ! @mcp.read_resource(c, "config://app")?
prompts ← ! @mcp.prompts(c)?
review ← ! @mcp.get_prompt(c, "review", ⟨code: "x = 1"⟩)?
! @mcp.close(c)
⟨tools: tools, sum: sum, stats: stats, failed: failed, resources: resources,
 config: config, prompts: prompts, review: review⟩
"#
    );
    let out = run_client(&source, loopback_net())
        .await
        .expect("client script failed");

    // Listing: the schema derived from `|a: int, b: int|` arrives as a record.
    let tools = out.get_field("tools");
    let Value::List(tools) = &tools else {
        panic!("expected a list of tools, got {tools}");
    };
    assert_eq!(tools.len(), 3);
    let add = &tools[0];
    assert_eq!(add.get_field("name").as_str(), Some("add"));
    assert_eq!(
        add.get_field("description").as_str(),
        Some("Add two numbers")
    );
    assert_eq!(
        add.get_field("input_schema")
            .get_field("properties")
            .get_field("a")
            .get_field("type")
            .as_str(),
        Some("integer")
    );

    // A tool answering a scalar comes back as its text; one answering a record comes
    // back as the record, because the server sent `structuredContent` alongside.
    assert_eq!(out.get_field("sum").as_str(), Some("5"));
    let stats = out.get_field("stats");
    assert_eq!(stats.get_field("count").as_int(), Some(3));
    assert_eq!(stats.get_field("total").as_int(), Some(6));

    // `isError` is a successful response carrying a failure, so it is an `err` value
    // and the script above could keep going without `?`.
    let failed = out.get_field("failed");
    let Value::Result(rite_runtime::value::ResultValue::Err(e)) = &failed else {
        panic!("expected err from a failing tool, got {failed}");
    };
    assert_eq!(e.get_field("kind").as_str(), Some("mcp.tool_error"));
    assert_eq!(e.get_field("tool").as_str(), Some("failing"));
    assert!(
        e.get_field("message")
            .as_str()
            .unwrap_or_default()
            .contains("not today"),
        "the tool's own reason did not survive: {e}"
    );

    let resources = out.get_field("resources");
    let Value::List(resources) = &resources else {
        panic!("expected a list of resources, got {resources}");
    };
    assert_eq!(resources[0].get_field("uri").as_str(), Some("config://app"));
    assert_eq!(out.get_field("config").as_str(), Some("debug=true"));

    let prompts = out.get_field("prompts");
    let Value::List(prompts) = &prompts else {
        panic!("expected a list of prompts, got {prompts}");
    };
    assert_eq!(prompts[0].get_field("name").as_str(), Some("review"));
    let arguments = prompts[0].get_field("arguments");
    let Value::List(arguments) = &arguments else {
        panic!("expected prompt arguments, got {arguments}");
    };
    assert_eq!(arguments[0].get_field("name").as_str(), Some("code"));
    assert_eq!(arguments[0].get_field("required"), Value::Bool(true));

    let review = out.get_field("review");
    let messages = review.get_field("messages");
    let Value::List(messages) = &messages else {
        panic!("expected prompt messages, got {messages}");
    };
    assert_eq!(messages[0].get_field("role").as_str(), Some("user"));
    assert_eq!(
        messages[0].get_field("text").as_str(),
        Some("Please review: x = 1")
    );

    server.abort();
    let _ = server.await;
}

/// A name the server does not have is a JSON-RPC refusal, which is a value.
#[tokio::test]
async fn an_unknown_tool_is_an_err_value_naming_the_code() {
    let _g = mcp_client_lock().lock().unwrap_or_else(|e| e.into_inner());
    let server = spawn_server().await;
    let addr = wait_for_bind().await;

    let source = format!(
        r#"
c ← ! @mcp.connect(⟨url: "http://{addr}/mcp"⟩)?
r ← ! @mcp.call_tool(c, "subtract", ⟨a: 1, b: 1⟩)
! @mcp.close(c)
r
"#
    );
    let out = run_client(&source, loopback_net()).await.expect("script");
    let Value::Result(rite_runtime::value::ResultValue::Err(e)) = &out else {
        panic!("expected err, got {out}");
    };
    assert_eq!(e.get_field("kind").as_str(), Some("mcp.error"));
    assert_eq!(e.get_field("operation").as_str(), Some("tools/call"));
    assert!(
        e.get_field("message")
            .as_str()
            .unwrap_or_default()
            .contains("subtract"),
        "the message did not name the tool: {e}"
    );

    server.abort();
    let _ = server.await;
}

/// Closing twice is careful, not wrong — the convention `@tcp.close` set.
#[tokio::test]
async fn closing_twice_is_not_an_error() {
    let _g = mcp_client_lock().lock().unwrap_or_else(|e| e.into_inner());
    let server = spawn_server().await;
    let addr = wait_for_bind().await;

    let source = format!(
        r#"
c ← ! @mcp.connect(⟨url: "http://{addr}/mcp"⟩)?
! @mcp.close(c)
! @mcp.close(c)?
"#
    );
    run_client(&source, loopback_net()).await.expect("script");

    server.abort();
    let _ = server.await;
}

/// A closed handle is a bug in the script, so it raises rather than answering `err`.
#[tokio::test]
async fn calling_through_a_closed_handle_raises() {
    let _g = mcp_client_lock().lock().unwrap_or_else(|e| e.into_inner());
    let server = spawn_server().await;
    let addr = wait_for_bind().await;

    let source = format!(
        r#"
c ← ! @mcp.connect(⟨url: "http://{addr}/mcp"⟩)?
! @mcp.close(c)
! @mcp.tools(c)
"#
    );
    let err = run_client(&source, loopback_net())
        .await
        .expect_err("a closed handle answered");
    assert!(
        err.to_string().contains("closed or invalid"),
        "unhelpful message: {err}"
    );

    server.abort();
    let _ = server.await;
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

/// The grant is checked at `connect`, so this is the whole permission surface.
#[tokio::test]
async fn a_stdio_connect_needs_the_process_grant() {
    let source = r#"! @mcp.connect(⟨command: "true"⟩)?"#;
    let err = run_client(source, PermissionSet::default_secure())
        .await
        .expect_err("a subprocess was started without --allow process");
    assert!(
        matches!(err, EvalError::Permission(_)),
        "expected a permission error, got {err:?}"
    );
    assert!(err.to_string().contains("process"), "{err}");
}

#[tokio::test]
async fn an_http_connect_needs_the_net_grant_for_that_host() {
    let source = r#"! @mcp.connect(⟨url: "http://example.com/mcp"⟩)?"#;
    let err = run_client(source, PermissionSet::default_secure())
        .await
        .expect_err("a host was reached without --allow net");
    assert!(
        matches!(err, EvalError::Permission(_)),
        "expected a permission error, got {err:?}"
    );

    // A grant for one host is not a grant for another.
    let mut elsewhere = PermissionSet::default_secure();
    elsewhere.grant(Permission::Net("other.example".into()));
    let err = run_client(source, elsewhere)
        .await
        .expect_err("a grant for another host was accepted");
    assert!(matches!(err, EvalError::Permission(_)), "{err:?}");
}

/// `--allow process` does not become a way to reach the network, or the reverse.
#[tokio::test]
async fn one_transports_grant_does_not_cover_the_other() {
    let mut process_only = PermissionSet::default_secure();
    process_only.grant(Permission::Process);
    let err = run_client(
        r#"! @mcp.connect(⟨url: "http://example.com/mcp"⟩)?"#,
        process_only,
    )
    .await
    .expect_err("--allow process reached the network");
    assert!(matches!(err, EvalError::Permission(_)), "{err:?}");

    let err = run_client(r#"! @mcp.connect(⟨command: "true"⟩)?"#, loopback_net())
        .await
        .expect_err("--allow net started a subprocess");
    assert!(matches!(err, EvalError::Permission(_)), "{err:?}");
}

/// The marker discipline covers the client half too: without `!` this is E021, so a
/// script cannot reach another server without saying so.
#[test]
fn the_client_calls_need_their_effect_marker() {
    fn codes(source: &str) -> Vec<String> {
        let mut sources = rite_core::SourceMap::new();
        let id = sources.add_file("marker.rite", source);
        let file = sources.get(id).unwrap().clone();
        let (_, diagnostics) = rite_sem::compile_to_ir(&file);
        diagnostics.iter().map(|d| d.code.as_str()).collect()
    }

    for call in [
        r#"@mcp.connect(⟨command: "true"⟩)"#,
        "@mcp.tools(c)",
        r#"@mcp.call_tool(c, "add", ⟨⟩)"#,
        r#"@mcp.read_resource(c, "config://app")"#,
        "@mcp.close(c)",
    ] {
        let unmarked = format!("c ← 1\nx ← {call}\nx");
        assert!(
            codes(&unmarked).iter().any(|c| c == "E021"),
            "expected E021 for the unmarked `{call}`, got {:?}",
            codes(&unmarked)
        );
        // The other half of the gate: it stops firing once the marker is there.
        let marked = format!("c ← 1\nx ← ! {call}\nx");
        assert!(
            !codes(&marked).iter().any(|c| c == "E021"),
            "E021 fired on the marked `{call}`: {:?}",
            codes(&marked)
        );
    }
}

// ---------------------------------------------------------------------------
// The legacy handshake
// ---------------------------------------------------------------------------

/// A hand-rolled HTTP endpoint that refuses `server/discover`.
///
/// Rite's own server answers it, so the fallback is unreachable against the one server
/// this test file otherwise has. Shipped MCP servers today are all on the older
/// revision, which makes this the path most real connections take — it needs a server
/// that behaves like one. Answers are canned; only the handshake shape matters.
async fn spawn_legacy_server() -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = text.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
                let parsed: Json = serde_json::from_str(body).unwrap_or(Json::Null);
                let method = parsed
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or_default();
                let id = parsed.get("id").cloned().unwrap_or(Json::Null);

                // A modern client stamps `_meta`; a legacy one must not, and this
                // server would not know what to do with it.
                let stamped = parsed.get("params").and_then(|p| p.get("_meta")).is_some();

                let reply = match method {
                    "server/discover" => json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32601, "message": "unknown method"}}),
                    "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "serverInfo": {"name": "legacy", "version": "0"}}}),
                    "notifications/initialized" => Json::Null,
                    // `stamped` rides inside the schema because that is the one field
                    // of a tool the client passes through whole; it lets the test see
                    // whether the request carried a `_meta` this revision cannot read.
                    "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {
                        "tools": [{"name": "echo", "description": "Echo",
                                   "inputSchema": {"type": "object",
                                                   "stamped": stamped}}]}}),
                    _ => json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32601, "message": "unknown method"}}),
                };
                let payload = if reply.is_null() {
                    String::new()
                } else {
                    reply.to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                     content-length: {}\r\nconnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            });
        }
    });
    (addr, handle)
}

#[tokio::test]
async fn an_initialize_fallback_is_used_when_discover_is_unknown() {
    let (addr, server) = spawn_legacy_server().await;

    let source = format!(
        r#"
c ← ! @mcp.connect(⟨url: "http://{addr}/mcp"⟩)?
tools ← ! @mcp.tools(c)?
! @mcp.close(c)
tools
"#
    );
    let out = run_client(&source, loopback_net()).await.expect("script");
    let Value::List(tools) = &out else {
        panic!("expected tools, got {out}");
    };
    assert_eq!(tools[0].get_field("name").as_str(), Some("echo"));
    assert_eq!(
        tools[0].get_field("input_schema").get_field("stamped"),
        Value::Bool(false),
        "the connection fell back to 2025-06-18 but still stamped a `_meta` version"
    );

    server.abort();
    let _ = server.await;
}
