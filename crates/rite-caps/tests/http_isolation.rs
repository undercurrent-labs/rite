// Shares the process-global RITE_HTTP_TEST env vars, so serialize.
#![allow(clippy::await_holding_lock)]

//! Two servers in one process must not share HTTP state.
//!
//! `@http.listen` used to hand its route table over through process globals
//! (`PENDING_SERVER`/`PENDING_ADDR` behind a `OnceLock` registrar), and middleware
//! `next()` resolved through a global invoker over a single global continuation map. Any
//! second server or concurrent chain overwrote the first. Both now travel on the
//! `RuntimeContext` for the evaluation that created them.

use rite_caps::http::{clear_last_bound_addr, last_bound_addr};
use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn spawn(source: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("iso.rite", &source, &mut ctx).await;
    })
}

async fn wait_for_bind(limit: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Some(a) = last_bound_addr() {
            return a;
        }
        assert!(start.elapsed() < limit, "server never bound");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn server(marker: &str) -> String {
    // Custom middleware so the `next` machinery is exercised on both servers.
    format!(
        r#"
tag ← "{marker}"

@http.listen "127.0.0.1:0" ⟦
  use {{ |req, next| next(req) }}

  GET "/who" |req| ⟦
    ^ 200 ⟨tag: tag⟩
  ⟧
⟧
"#
    )
}

#[tokio::test]
async fn two_servers_keep_their_own_routes_and_middleware() {
    let _guard = http_test_lock().lock().unwrap();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "8");

    // Start the first, learn its address, then start the second. Under the old globals
    // the second registration replaced the first server's pending state.
    clear_last_bound_addr();
    let first = spawn(server("first")).await;
    let addr_a = wait_for_bind(Duration::from_secs(3)).await;

    clear_last_bound_addr();
    let second = spawn(server("second")).await;
    let addr_b = wait_for_bind(Duration::from_secs(3)).await;
    assert_ne!(addr_a, addr_b, "both servers bound the same port");

    // Concurrently, so the two middleware chains are in flight together. That is the
    // shape the old globals could not survive: whichever chain finished first cleared
    // the shared continuation map and dropped the global invoker out from under the
    // other, so its `next` handle came back as "already used".
    for _ in 0..4 {
        let (a, b) = tokio::join!(
            reqwest::get(format!("http://{addr_a}/who")),
            reqwest::get(format!("http://{addr_b}/who")),
        );
        let a = a.expect("first server").text().await.unwrap();
        let b = b.expect("second server").text().await.unwrap();
        assert!(a.contains("first"), "first server answered: {a}");
        assert!(b.contains("second"), "second server answered: {b}");
        assert!(
            !a.contains("already used") && !b.contains("already used"),
            "a chain lost its continuation: {a} / {b}"
        );
    }

    first.abort();
    second.abort();
    let _ = first.await;
    let _ = second.await;
}
