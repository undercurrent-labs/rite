// Shares the process-global RITE_HTTP_TEST env vars and the PENDING_SERVER /
// LAST_BOUND_ADDR statics in rite-caps::http, so each test holds `http_test_lock()`
// for its whole body. Holding the guard across `.await` is deliberate.
#![allow(clippy::await_holding_lock)]

//! HTTP handlers must see the scope the script defined them in.
//!
//! Handlers run in a fresh `RuntimeContext` per request. That context used to be
//! seeded with function *names* only, so any top-level binding was `undefined name`
//! at request time — a 500 that `rite check` could not predict, which made module
//! config/state unusable in a server.

use rite_caps::http::{clear_last_bound_addr, last_bound_addr};
use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn spawn_server(source: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("module-scope.rite", &source, &mut ctx).await;
    })
}

async fn wait_for_bind(limit: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Some(addr) = last_bound_addr() {
            return addr;
        }
        assert!(start.elapsed() < limit, "server never bound");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn test_mode() {
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "8");
}

#[tokio::test]
async fn handler_sees_top_level_bindings_and_functions() {
    let _guard = http_test_lock().lock().unwrap();
    clear_last_bound_addr();
    test_mode();

    let source = r#"
config ← "TOP-LEVEL-VALUE"
limit ← 42

◆ helper() ⟦ ^ "from-helper" ⟧

@http.listen "127.0.0.1:0" ⟦
  GET "/" |req| ⟦
    ^ 200 ⟨top: config, lim: limit, fn: helper()⟩
  ⟧
⟧
"#
    .to_string();

    let handle = spawn_server(source).await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let body = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("request")
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("TOP-LEVEL-VALUE"),
        "top-level binding not in scope: {body}"
    );
    assert!(body.contains("42"), "top-level int not in scope: {body}");
    assert!(
        body.contains("from-helper"),
        "module function not callable: {body}"
    );
    assert!(
        !body.contains("undefined name"),
        "handler could not resolve a module name: {body}"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn module_scope_reaches_handlers_through_custom_middleware() {
    let _guard = http_test_lock().lock().unwrap();
    clear_last_bound_addr();
    test_mode();

    // A handler reached via `next(req)` runs on a continuation-rebuilt context, which
    // is a separate code path from a direct dispatch — it needs the same scope.
    let source = r#"
greeting ← "hello-from-module"

@http.listen "127.0.0.1:0" ⟦
  use { |req, next| next(req) }

  GET "/" |req| ⟦
    ^ 200 ⟨msg: greeting⟩
  ⟧
⟧
"#
    .to_string();

    let handle = spawn_server(source).await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let body = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("request")
        .text()
        .await
        .unwrap();

    assert!(
        body.contains("hello-from-module"),
        "module scope lost through middleware next(): {body}"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn mutable_module_state_persists_across_requests() {
    let _guard = http_test_lock().lock().unwrap();
    clear_last_bound_addr();
    test_mode();

    // Environment frames are shared, so the module env captured at listen time is the
    // same one every request mutates — a top-level counter/cache behaves like server
    // state rather than resetting per request.
    let source = r#"
hits ↢ 0

@http.listen "127.0.0.1:0" ⟦
  GET "/" |req| ⟦
    hits := hits + 1
    ^ 200 ⟨hits: hits⟩
  ⟧
⟧
"#
    .to_string();

    let handle = spawn_server(source).await;
    let addr = wait_for_bind(Duration::from_secs(3)).await;
    let url = format!("http://{addr}/");

    let mut seen = Vec::new();
    for _ in 0..3 {
        let body = reqwest::get(&url)
            .await
            .expect("request")
            .text()
            .await
            .unwrap();
        seen.push(body);
    }

    assert!(seen[0].contains("\"hits\":1"), "first request: {}", seen[0]);
    assert!(
        seen[1].contains("\"hits\":2"),
        "second request: {}",
        seen[1]
    );
    assert!(seen[2].contains("\"hits\":3"), "third request: {}", seen[2]);

    handle.abort();
    let _ = handle.await;
}
