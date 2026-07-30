// These tests share process-global state (the RITE_HTTP_TEST env vars and the
// PENDING_SERVER / LAST_BOUND_ADDR statics in rite-caps::http), so each test holds
// `http_test_lock()` for its whole body to run them one at a time. Holding the guard
// across `.await` is the point — dropping it early would let tests interleave and
// clobber each other's server registration.
#![allow(clippy::await_holding_lock)]

//! Custom Rite middleware: auth short-circuit and next(req) chain.

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
        let _ = run_source("http-mw.rite", &source, &mut ctx).await;
    })
}

const AUTH_SERVER: &str = r#"
@http.listen "127.0.0.1:0" ⟦
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

  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧

  GET "/secret" ⟦
    ^ 200 ⟨ok: true⟩
  ⟧
⟧
"#;

#[tokio::test]
async fn custom_middleware_registers() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(AUTH_SERVER).await;
    let _addr = wait_for_bind(Duration::from_secs(3)).await;
    let mw = last_registered_middleware();
    assert!(mw.iter().any(|m| m == "log"), "expected log: {mw:?}");
    assert!(
        mw.iter().any(|m| m == "<custom>"),
        "expected custom middleware: {mw:?}"
    );
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn auth_middleware_rejects_without_token() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(AUTH_SERVER).await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://{addr}/secret"))
        .send()
        .await
        .expect("request");
    assert_eq!(res.status().as_u16(), 401);
    let body: serde_json::Value = res.json().await.expect("json");
    assert_eq!(body["error"], "unauthorized");
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn auth_middleware_allows_with_bearer() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(AUTH_SERVER).await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://{addr}/secret"))
        .header("Authorization", "Bearer secret")
        .send()
        .await
        .expect("request");
    assert_eq!(res.status().as_u16(), 200);
    let body: serde_json::Value = res.json().await.expect("json");
    assert_eq!(body["ok"], true);
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}

#[tokio::test]
async fn passthrough_middleware_calls_next() {
    let _g = http_lock().lock().unwrap();
    begin_test_io_capture();
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use { |req, next|
    next(req)
  }
  GET "/ping" ⟦
    ^ 200 ⟨pong: true⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let client = reqwest::Client::new();
    let res = client
        .get(format!("http://{addr}/ping"))
        .send()
        .await
        .expect("request");
    assert_eq!(res.status().as_u16(), 200);
    let body: serde_json::Value = res.json().await.expect("json");
    assert_eq!(body["pong"], true);
    handle.abort();
    let _ = handle.await;
    let _ = take_test_io_capture();
}
