//! The Streamable HTTP transport, end to end through a real socket.
//!
//! `mcp_stdio.rs` covers the protocol itself over an in-memory pipe. What is only
//! reachable here is the transport layer: the bind, the required `Mcp-Method` /
//! `Mcp-Name` routing headers, and the fact that a script written as an ordinary Rite
//! program actually serves.
//!
//! Since the 2026-07-28 revision there is no session and no `Mcp-Session-Id`, so a
//! request is self-contained and there is nothing to set up before one.

// Each test holds the lock across its awaits — that is the point, since `RITE_MCP_TEST`
// and the last-bound-address slot are process-global.
#![allow(clippy::await_holding_lock)]

use rite_caps::install_defaults;
use rite_caps::mcp::{clear_last_mcp_bound_addr, last_mcp_bound_addr};
use rite_caps::permissions::{Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use serde_json::{json, Value as Json};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn mcp_http_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

const SERVER: &str = r#"
! @mcp.serve ⟨name: "calculator", transport: #http, addr: "127.0.0.1:0"⟩ ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧
⟧
"#;

/// Start a server in a background task. It stops itself on the test timer.
async fn spawn_server(source: &'static str) -> tokio::task::JoinHandle<()> {
    clear_last_mcp_bound_addr();
    std::env::set_var("RITE_MCP_TEST", "1");
    std::env::set_var("RITE_MCP_TEST_SECS", "20");
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("mcp-http.rite", source, &mut ctx).await;
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

async fn post(addr: &str, headers: &[(&str, &str)], body: Json) -> (u16, Json) {
    let mut req = reqwest::Client::new()
        .post(format!("http://{addr}/mcp"))
        .header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.body(body.to_string()).send().await.expect("request");
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    let parsed = serde_json::from_str(&text).unwrap_or(Json::Null);
    (status, parsed)
}

#[tokio::test]
async fn a_script_serves_over_http() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let handle = spawn_server(SERVER).await;
    let addr = wait_for_bind().await;

    let (status, body) = post(
        &addr,
        &[("mcp-method", "tools/list")],
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["tools"][0]["name"], "add");
    assert_eq!(
        body["result"]["tools"][0]["inputSchema"]["properties"]["a"]["type"],
        "integer"
    );

    let (status, body) = post(
        &addr,
        &[("mcp-method", "tools/call"), ("mcp-name", "add")],
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "add", "arguments": {"a": 2, "b": 3}}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["content"][0]["text"], "5");

    handle.abort();
    let _ = handle.await;
}

/// The routing headers exist so an intermediary can dispatch without parsing the body,
/// which only holds if they agree with it.
#[tokio::test]
async fn a_header_that_contradicts_the_body_is_refused() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let handle = spawn_server(SERVER).await;
    let addr = wait_for_bind().await;

    let (_, body) = post(
        &addr,
        &[("mcp-method", "prompts/list")],
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}),
    )
    .await;
    assert_eq!(body["error"]["code"], -32020, "{body}");

    let (_, body) = post(
        &addr,
        &[("mcp-method", "tools/call"), ("mcp-name", "subtract")],
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "add", "arguments": {"a": 1, "b": 1}}}),
    )
    .await;
    assert_eq!(body["error"]["code"], -32020, "{body}");

    handle.abort();
    let _ = handle.await;
}

/// Legacy clients predate the routing headers, so requiring them would lock those
/// clients out of the compatibility layer that exists for them.
#[tokio::test]
async fn the_legacy_handshake_is_exempt_from_the_header_requirement() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let handle = spawn_server(SERVER).await;
    let addr = wait_for_bind().await;

    let (status, body) = post(
        &addr,
        &[],
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"protocolVersion": "2025-06-18"}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    assert!(
        body["result"].get("resultType").is_none(),
        "leaked a field the legacy revision predates: {body}"
    );

    handle.abort();
    let _ = handle.await;
}

/// There are no sessions any more, so the same request works with nothing set up first
/// — which is what lets a Rite server sit behind a plain load balancer.
#[tokio::test]
async fn a_request_needs_no_handshake_first() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let handle = spawn_server(SERVER).await;
    let addr = wait_for_bind().await;

    let (status, body) = post(
        &addr,
        &[
            ("mcp-method", "tools/call"),
            ("mcp-name", "add"),
            ("mcp-session-id", "a-session-that-does-not-exist"),
        ],
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": "add", "arguments": {"a": 20, "b": 22},
                          "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["result"]["content"][0]["text"], "42");
    assert_eq!(body["result"]["resultType"], "complete");

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn a_get_is_rejected() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let handle = spawn_server(SERVER).await;
    let addr = wait_for_bind().await;

    let resp = reqwest::get(format!("http://{addr}/mcp")).await.unwrap();
    // The GET stream was removed along with sessions; there is nothing to resume.
    assert_eq!(resp.status().as_u16(), 405);

    handle.abort();
    let _ = handle.await;
}

/// Binding anything but loopback needs the grant, and the check runs before the bind —
/// so this fails immediately rather than starting a server nobody asked for.
#[tokio::test]
async fn a_non_loopback_bind_needs_the_net_grant() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let src = r#"
! @mcp.serve ⟨name: "x", transport: #http, addr: "0.0.0.0:0"⟩ ⟦
  tool "add" "Adds" |a: int| ⟦ ^ a ⟧
⟧
"#;
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let err = run_source("denied.rite", src, &mut ctx)
        .await
        .expect_err("a non-loopback bind was allowed without a grant");
    assert_eq!(err.exit_code(), 5, "{err}");
}

/// Loopback is allowed under the default-secure set, as it is for `@http.listen`.
#[tokio::test]
async fn loopback_binds_without_a_grant() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    clear_last_mcp_bound_addr();
    std::env::set_var("RITE_MCP_TEST", "1");
    std::env::set_var("RITE_MCP_TEST_SECS", "3");
    let handle = tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::default_secure());
        let _ = run_source("loopback.rite", SERVER, &mut ctx).await;
    });
    let addr = wait_for_bind().await;
    assert!(addr.starts_with("127.0.0.1:"), "{addr}");
    handle.abort();
    let _ = handle.await;
}

/// An explicit grant reaches the same place a default-secure loopback bind does.
#[tokio::test]
async fn an_explicit_grant_allows_the_bind() {
    let _g = mcp_http_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::parse("net=127.0.0.1").unwrap());
    clear_last_mcp_bound_addr();
    std::env::set_var("RITE_MCP_TEST", "1");
    std::env::set_var("RITE_MCP_TEST_SECS", "3");
    let handle = tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, perms);
        let _ = run_source("granted.rite", SERVER, &mut ctx).await;
    });
    let _ = wait_for_bind().await;
    handle.abort();
    let _ = handle.await;
}
