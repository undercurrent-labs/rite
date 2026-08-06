//! The MCP wire format, driven over an in-memory pipe.
//!
//! `serve_streams` is generic over its reader and writer precisely so this test can
//! exist: a real stdio server owns the process's own stdin and stdout, which a test
//! cannot borrow, and testing the protocol through a socket would be testing the HTTP
//! transport instead. Everything here is the same dispatcher a real stdio session runs.
//!
//! The end-to-end *script* path — `! @mcp.serve …` actually parsed, resolved and
//! evaluated — is covered in `mcp_http.rs`, which can use a real socket.

// Each test holds the lock across its awaits — that is the point, since the streams
// slot and the stderr capture it guards are process-global.
#![allow(clippy::await_holding_lock)]

use rite_caps::install_defaults;
use rite_caps::permissions::PermissionSet;
use rite_runtime::RuntimeContext;
use serde_json::{json, Value as Json};
use std::sync::{Mutex, OnceLock};

/// These tests share the process-global stderr capture in `rite-caps::http`, so each
/// holds this for its whole body.
fn mcp_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

const SERVER: &str = r#"
! @mcp.serve "calculator" ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧

  tool "describe" "Answer a record" |who: string| ⟦
    ^ ⟨name: who, status: #ok⟩
  ⟧

  tool "failing" "Always fails" ⟦
    ^ err(⟨kind: "nope", message: "not today"⟩)
  ⟧

  tool "chatty" "Prints to the console" ⟦
    ! @console.println("this must not reach stdout")
    ^ "done"
  ⟧

  tool "slow" "Reports progress" ⟦
    ! @mcp.progress(0.5, "halfway")
    ^ "finished"
  ⟧

  resource "config://app" "App config" ⟦
    ^ ⟨debug: true⟩
  ⟧

  prompt "review" "Review some code" |code: string| ⟦
    ^ "Please review: " + code
  ⟧
⟧
"#;

/// Feed a batch of requests through a real server and collect the response lines.
///
/// The script is evaluated for real — parsed, resolved, desugared — and the resulting
/// `pending_mcp` is what the server serves, so this exercises the whole front end too.
async fn converse(requests: &[Json]) -> Vec<Json> {
    let mut input = String::new();
    for r in requests {
        input.push_str(&serde_json::to_string(r).unwrap());
        input.push('\n');
    }

    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());

    // Run the script up to `@mcp.serve`, which stages the declarations and then hands
    // off to the transport. Swapping the transport for a pipe is the only substitution.
    let output = rite_caps::mcp::serve_test_streams(&mut ctx, "mcp-test.rite", SERVER, &input)
        .await
        .expect("server run failed");

    output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad line {l:?}: {e}")))
        .collect()
}

fn req(id: i64, method: &str, params: Json) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn call(id: i64, name: &str, arguments: Json) -> Json {
    req(
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
}

fn result(v: &Json) -> &Json {
    v.get("result")
        .unwrap_or_else(|| panic!("expected a result, got {v}"))
}

fn error_code(v: &Json) -> i64 {
    v.get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_i64())
        .unwrap_or_else(|| panic!("expected an error, got {v}"))
}

/// The text of a `tools/call` result's first content block.
fn content_text(v: &Json) -> String {
    result(v)["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn server_discover_advertises_versions_and_identity() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(1, "server/discover", json!({}))]).await;
    let r = result(&out[0]);
    assert_eq!(r["serverInfo"]["name"], "calculator");
    let versions = r["protocolVersions"].as_array().unwrap();
    assert!(versions.contains(&json!("2026-07-28")), "{versions:?}");
    // The tables are fixed once the server starts, so nothing may claim otherwise.
    assert_eq!(r["capabilities"]["tools"]["listChanged"], json!(false));
    assert_eq!(r["capabilities"]["resources"]["subscribe"], json!(false));
    assert_eq!(r["resultType"], "complete");
}

/// The whole point of the feature: the schema comes from the declared types.
#[tokio::test]
async fn tools_list_derives_schemas_from_declared_types() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(1, "tools/list", json!({}))]).await;
    let r = result(&out[0]);
    let tools = r["tools"].as_array().unwrap();

    // Declaration order, which the spec now asks servers to keep stable.
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["add", "describe", "failing", "chatty", "slow"],
        "tools are not in declaration order"
    );

    let add = &tools[0];
    assert_eq!(add["description"], "Add two numbers");
    assert_eq!(
        add["inputSchema"],
        json!({
            "type": "object",
            "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
            "required": ["a", "b"],
        })
    );
    // A tool with no parameters still publishes a well-formed object schema.
    assert_eq!(tools[2]["inputSchema"]["type"], "object");
}

#[tokio::test]
async fn list_results_carry_cache_hints() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(1, "tools/list", json!({}))]).await;
    let r = result(&out[0]);
    assert!(r["ttlMs"].as_i64().unwrap() > 0);
    assert_eq!(r["cacheScope"], "public");
}

#[tokio::test]
async fn a_tool_call_returns_its_value() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "add", json!({"a": 2, "b": 3}))]).await;
    assert_eq!(content_text(&out[0]), "5");
    assert_eq!(result(&out[0])["isError"], json!(false));
}

/// A record comes back as readable text *and* as structured data.
#[tokio::test]
async fn a_record_result_carries_structured_content() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "describe", json!({"who": "ada"}))]).await;
    let r = result(&out[0]);
    assert_eq!(r["structuredContent"]["name"], "ada");
    // An atom must reach the wire as its name. This goes through
    // `Value::to_json`, which has the interner; `@json`'s own converter did not
    // and rendered `atom:7` until it was given one.
    assert_eq!(
        r["structuredContent"]["status"], "ok",
        "an atom leaked as its interner index"
    );
}

/// A wrong argument type is the tool failing, not the server failing — the model needs
/// to read the reason and correct itself.
#[tokio::test]
async fn a_contract_violation_is_an_in_band_tool_error() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "add", json!({"a": "two", "b": 3}))]).await;
    let r = result(&out[0]);
    assert_eq!(r["isError"], json!(true), "expected an in-band error: {r}");
    let text = content_text(&out[0]);
    assert!(
        text.contains("int") || text.contains("two"),
        "the message does not say what was wrong: {text}"
    );
}

/// A missing argument is the *client* being wrong, so it is a protocol error.
#[tokio::test]
async fn a_missing_argument_is_a_protocol_error() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "add", json!({"a": 1}))]).await;
    assert_eq!(error_code(&out[0]), -32602);
}

#[tokio::test]
async fn an_extra_argument_is_ignored() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "add", json!({"a": 2, "b": 3, "note": "hi"}))]).await;
    assert_eq!(content_text(&out[0]), "5");
}

#[tokio::test]
async fn a_body_returning_err_is_an_in_band_tool_error() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "failing", json!({}))]).await;
    assert_eq!(result(&out[0])["isError"], json!(true));
    assert!(content_text(&out[0]).contains("not today"));
}

#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "nonexistent", json!({}))]).await;
    assert_eq!(error_code(&out[0]), -32602);
}

/// The sharpest edge in the whole feature: stdout is the wire.
#[tokio::test]
async fn console_output_from_a_tool_never_reaches_the_wire() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "chatty", json!({}))]).await;
    // Every line on stdout parsed as JSON — which `converse` already required — and
    // the printed text appears nowhere in it.
    assert_eq!(out.len(), 1, "extra lines on the wire: {out:?}");
    assert_eq!(content_text(&out[0]), "done");
    let wire = serde_json::to_string(&out[0]).unwrap();
    assert!(
        !wire.contains("must not reach stdout"),
        "console output corrupted the protocol stream: {wire}"
    );
}

/// A progress notification belongs to the request that produced it, and must arrive
/// before that request's result.
#[tokio::test]
async fn progress_arrives_before_the_result() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[call(1, "slow", json!({}))]).await;
    assert_eq!(
        out.len(),
        2,
        "expected a notification and a result: {out:?}"
    );
    assert_eq!(out[0]["method"], "notifications/progress");
    assert_eq!(out[0]["params"]["message"], "halfway");
    assert!(out[0].get("id").is_none(), "a notification carries no id");
    assert_eq!(content_text(&out[1]), "finished");
}

#[tokio::test]
async fn resources_and_prompts_answer() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[
        req(1, "resources/list", json!({})),
        req(2, "resources/read", json!({"uri": "config://app"})),
        req(3, "prompts/list", json!({})),
        req(
            4,
            "prompts/get",
            json!({"name": "review", "arguments": {"code": "x"}}),
        ),
    ])
    .await;

    assert_eq!(result(&out[0])["resources"][0]["uri"], "config://app");
    assert_eq!(
        result(&out[1])["contents"][0]["mimeType"],
        "application/json"
    );
    assert!(result(&out[1])["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("debug"));
    assert_eq!(result(&out[2])["prompts"][0]["name"], "review");
    assert_eq!(
        result(&out[3])["messages"][0]["content"]["text"],
        "Please review: x"
    );
}

#[tokio::test]
async fn an_unknown_resource_uses_the_current_not_found_code() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(1, "resources/read", json!({"uri": "nope://x"}))]).await;
    assert_eq!(error_code(&out[0]), -32602);
}

#[tokio::test]
async fn an_unsupported_protocol_version_is_refused() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(
        1,
        "tools/list",
        json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "1999-01-01"}}),
    )])
    .await;
    assert_eq!(error_code(&out[0]), -32022);
}

#[tokio::test]
async fn a_declared_current_version_is_accepted() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[req(
        1,
        "tools/list",
        json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}),
    )])
    .await;
    assert!(out[0].get("result").is_some(), "{:?}", out[0]);
}

// --- the legacy compatibility layer ---------------------------------------------

#[tokio::test]
async fn a_legacy_client_gets_the_older_shape() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[
        req(1, "initialize", json!({"protocolVersion": "2025-06-18"})),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        req(2, "tools/list", json!({})),
        req(3, "resources/read", json!({"uri": "nope://x"})),
    ])
    .await;

    // The notification is not answered, so there are three lines, not four.
    assert_eq!(out.len(), 3, "a notification was answered: {out:?}");

    let init = result(&out[0]);
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(
        init.get("resultType").is_none(),
        "the legacy revision predates resultType"
    );

    let list = result(&out[1]);
    assert!(list["tools"].as_array().unwrap().len() == 5);
    assert!(list.get("resultType").is_none(), "leaked a modern field");
    assert!(list.get("ttlMs").is_none(), "leaked a modern field");
    assert!(list.get("_meta").is_none(), "leaked a modern field");

    // And the older not-found code.
    assert_eq!(error_code(&out[2]), -32002);
}

/// A modern client probes with `server/discover`; that must never be mistaken for the
/// legacy handshake.
#[tokio::test]
async fn discovering_first_does_not_pin_the_legacy_revision() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[
        req(1, "server/discover", json!({})),
        req(2, "tools/list", json!({})),
    ])
    .await;
    assert_eq!(result(&out[1])["resultType"], "complete");
}

/// A cautious client probes *and then* sends `initialize`. Sending it at all means it
/// is a legacy client, so it should get the older shape from then on.
#[tokio::test]
async fn discovering_then_initializing_still_pins_legacy() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let out = converse(&[
        req(1, "server/discover", json!({})),
        req(2, "initialize", json!({})),
        req(3, "tools/list", json!({})),
    ])
    .await;
    assert_eq!(result(&out[0])["resultType"], "complete");
    assert!(result(&out[2]).get("resultType").is_none());
}

#[tokio::test]
async fn malformed_json_is_reported_without_killing_the_session() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let input =
        "{not json\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n";
    let output = rite_caps::mcp::serve_test_streams(&mut ctx, "t.rite", SERVER, input)
        .await
        .unwrap();
    let lines: Vec<Json> = output
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(error_code(&lines[0]), -32602);
    assert!(
        lines[1].get("result").is_some(),
        "the session did not continue"
    );
}

/// The script's own top-level output is not the server's problem, but it must not be
/// swallowed either — this pins that `@mcp.serve` runs after the rest of the script.
#[tokio::test]
async fn the_script_around_the_server_still_runs() {
    let _g = mcp_test_lock().lock().unwrap_or_else(|e| e.into_inner());
    let src = format!("greeting ← \"hi\"\n{SERVER}");
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let out = rite_caps::mcp::serve_test_streams(&mut ctx, "t.rite", &src, "")
        .await
        .unwrap();
    assert!(out.trim().is_empty(), "unexpected wire traffic: {out}");
}
