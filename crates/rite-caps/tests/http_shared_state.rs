// Shares the process-global RITE_HTTP_TEST env vars and the PENDING_SERVER /
// LAST_BOUND_ADDR statics in rite-caps::http, so each test holds `http_test_lock()`
// for its whole body. Holding the guard across `.await` is deliberate.
#![allow(clippy::await_holding_lock)]

//! Handlers share the listen-time capability host and handle table.
//!
//! Each request used to build a fresh host via `install_defaults`, so a
//! handle opened before `@http.listen` indexed into an empty table inside a
//! handler ("db connection closed or invalid") and `@store` state written
//! before `listen` read back as `none`. The field-report service worked
//! around it with one DuckDB writer per request, which corrupted the file.
//!
//! Also here: two overlapping requests through *custom* middleware really do
//! overlap. A server-wide mutex used to serialize every request the moment
//! any `use { |req, next| … }` existed.

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
        let _ = run_source("shared-state.rite", &source, &mut ctx).await;
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
async fn store_state_written_before_listen_is_visible_in_handlers() {
    let _guard = http_test_lock().lock().unwrap();
    test_mode();
    clear_last_bound_addr();

    let src = r#"
! @store.set("ns", "k", "set-before-listen")
@http.listen "127.0.0.1:0" ⟦
  GET "/read" ⟦
    v ← ! @store.get("ns", "k")?
    ^ 200 ⟨v: v⟩
  ⟧
  GET "/bump" ⟦
    n ← ! @store.get("ns", "n")?
    n2 ← (n ?? 0) + 1
    ! @store.set("ns", "n", n2)
    ^ 200 ⟨n: n2⟩
  ⟧
⟧
"#;
    let handle = spawn_server(src.to_string()).await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let read = reqwest::get(format!("http://{addr}/read"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        read.contains("set-before-listen"),
        "pre-listen store entry not visible in handler: {read}"
    );

    // Server-scoped, not per-request: the counter survives across requests.
    let _ = reqwest::get(format!("http://{addr}/bump")).await.unwrap();
    let second = reqwest::get(format!("http://{addr}/bump"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        second.contains("\"n\":2"),
        "store state did not persist across requests: {second}"
    );

    handle.abort();
}

#[tokio::test]
async fn custom_middleware_requests_overlap() {
    let _guard = http_test_lock().lock().unwrap();
    test_mode();
    clear_last_bound_addr();

    // Store-based rendezvous instead of wall-clock: each request bumps a
    // shared arrival counter, then spins until both arrivals are seen. Either
    // both requests are in flight at once and both return `met: true`, or the
    // old serialization is back and the first request exhausts its patience
    // alone. The deadline bounds the loop so a regression fails rather than
    // hangs.
    let src = r#"
@http.listen "127.0.0.1:0" ⟦
  use { |req, next| next(req) }
  GET "/meet" ⟦
    n ← ! @store.get("ns", "arrivals")?
    ! @store.set("ns", "arrivals", (n ?? 0) + 1)
    tries ↢ 0
    met ↢ false
    while (not met) and tries < 200 ⟦
      seen ← ! @store.get("ns", "arrivals")?
      ? (seen ?? 0) >= 2 ⟦ met := true ⟧
      ! @clock.sleep(10)
      tries := tries + 1
    ⟧
    ^ 200 ⟨met: met⟩
  ⟧
⟧
"#;
    let handle = spawn_server(src.to_string()).await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let (a, b) = tokio::join!(
        reqwest::get(format!("http://{addr}/meet")),
        reqwest::get(format!("http://{addr}/meet")),
    );
    let a = a.unwrap().text().await.unwrap();
    let b = b.unwrap().text().await.unwrap();
    assert!(
        a.contains("\"met\":true") && b.contains("\"met\":true"),
        "overlapping custom-middleware requests never saw each other \
         (serialized again?): {a} / {b}"
    );

    handle.abort();
}
