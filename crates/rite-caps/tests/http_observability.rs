// These tests share process-global state (the RITE_HTTP_TEST env vars and the
// PENDING_SERVER / LAST_BOUND_ADDR statics in rite-caps::http), so each test holds
// `http_test_lock()` for its whole body to run them one at a time. Holding the guard
// across `.await` is the point — dropping it early would let tests interleave and
// clobber each other's server registration.
#![allow(clippy::await_holding_lock)]

//! HTTP observability contracts: middleware wiring, access logs, handler console flush.
//!
//! These tests prove that advertised side effects actually happen — not only that
//! routes return 200. They share process-global HTTP state and must not run in parallel.

use rite_caps::http::{
    begin_test_io_capture, clear_last_bound_addr, last_bound_addr, last_registered_middleware,
    take_test_io_capture,
};
use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn wait_for_bind(timeout: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Some(addr) = last_bound_addr() {
            return addr;
        }
        if start.elapsed() > timeout {
            panic!("server did not bind within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

async fn spawn_server(source: &str) -> tokio::task::JoinHandle<()> {
    clear_last_bound_addr();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "12");
    let source = source.to_string();
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("http-obs.rite", &source, &mut ctx).await;
    })
}

#[tokio::test]
async fn middleware_registration_use_ascii() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use @http.log
  use @http.recover
  GET "/x" ⟦ ^ 200 ⟨ok: true⟩ ⟧
⟧
"#,
    )
    .await;
    let _addr = wait_for_bind(Duration::from_secs(3)).await;
    let mw = last_registered_middleware();
    assert!(
        mw.iter().any(|m| m == "log"),
        "expected log middleware registered, got {mw:?}"
    );
    assert!(
        mw.iter().any(|m| m == "recover"),
        "expected recover middleware registered, got {mw:?}"
    );
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn middleware_registration_glyph_use() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  ⊏ @http.log
  GET "/x" ⟦ ^ 200 ⟨ok: true⟩ ⟧
⟧
"#,
    )
    .await;
    let _addr = wait_for_bind(Duration::from_secs(3)).await;
    let mw = last_registered_middleware();
    assert_eq!(
        mw,
        vec!["log".to_string()],
        "glyph ⊏ should wire log: {mw:?}"
    );
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn access_log_on_when_log_middleware_present() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use @http.log
  GET "/health" ⟦ ^ 200 ⟨status: #ok⟩ ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let url = format!("http://{addr}/health");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    // Allow log line to be recorded
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cap = take_test_io_capture();
    assert!(
        cap.stderr.contains("rite: GET /health 200"),
        "expected access log on stderr capture, got stderr={:?} stdout={:?}",
        cap.stderr,
        cap.stdout
    );
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn access_log_off_when_no_log_middleware() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦ ^ 200 ⟨status: #ok⟩ ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let url = format!("http://{addr}/health");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cap = take_test_io_capture();
    assert!(
        !cap.stderr.contains("rite: GET /health"),
        "access log must not appear without use @http.log; stderr={:?}",
        cap.stderr
    );
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn handler_console_println_flushed_to_capture() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/ping" ⟦
    ! @console.println("obs-ping-marker")
    ^ 200 ⟨pong: true⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let url = format!("http://{addr}/ping");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cap = take_test_io_capture();
    assert!(
        cap.stdout.contains("obs-ping-marker"),
        "handler console must flush to process stdout; capture={:?}",
        cap.stdout
    );
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn recover_returns_json_500_on_handler_error() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use @http.recover
  GET "/boom" ⟦
    panic("intentional")
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let url = format!("http://{addr}/boom");
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.expect("request");
    assert_eq!(resp.status().as_u16(), 500);
    let body = resp.text().await.unwrap();
    // `recover` is what makes the 500 structured and self-describing.
    let json: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("expected JSON, got {body:?}: {e}"));
    assert_eq!(json["error"], "panic", "{body}");
    assert!(
        json["message"]
            .as_str()
            .unwrap_or("")
            .contains("intentional"),
        "recover should report the panic message: {body}"
    );
    // Server should still be up for another request
    let resp2 = client
        .get(format!("http://{addr}/boom"))
        .send()
        .await
        .expect("second request");
    assert_eq!(resp2.status().as_u16(), 500);
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn without_recover_handler_failure_is_a_bare_500_logged_to_stderr() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/boom" ⟦
    panic("intentional-secret-detail")
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let resp = reqwest::get(format!("http://{addr}/boom"))
        .await
        .expect("request");
    assert_eq!(resp.status().as_u16(), 500);
    let body = resp.text().await.unwrap();
    // Without `recover` the client learns nothing: EvalError text carries spans,
    // values and panic messages that must not leak to an arbitrary caller.
    assert!(
        body.is_empty(),
        "no-recover 500 must not describe the failure, got body {body:?}"
    );
    assert!(
        !body.contains("intentional-secret-detail"),
        "handler detail leaked to the client: {body}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cap = take_test_io_capture();
    assert!(
        cap.stderr.contains("intentional-secret-detail"),
        "the operator must still see the failure on stderr; stderr={:?}",
        cap.stderr
    );
    assert!(
        cap.stderr.contains("use @http.recover"),
        "stderr should point at the middleware that changes this; stderr={:?}",
        cap.stderr
    );
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn console_and_log_together() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  ⊏ @http.log
  GET "/both" ⟦
    ! @console.println("both-console")
    ^ 200 ⟨ok: true⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/both"))
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let cap = take_test_io_capture();
    assert!(
        cap.stdout.contains("both-console"),
        "stdout={:?}",
        cap.stdout
    );
    assert!(
        cap.stderr.contains("rite: GET /both 200"),
        "stderr={:?}",
        cap.stderr
    );
    handle.abort();
    let _ = handle.await;
}
