// Holds `http_test_lock()` like every HTTP test, per the repo convention —
// holding the guard across `.await` is deliberate.
#![allow(clippy::await_holding_lock)]

//! `@http.request` honours `timeout_ms`.
//!
//! The field was documented in the descriptor and never read: every request
//! ran under a hardcoded 30s client timeout, so `timeout_ms: 300000` died at
//! 30s reported as a generic network error while the server finished the
//! work.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// One-shot HTTP server that answers 200 after `delay`.
async fn slow_server(delay: std::time::Duration) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                tokio::time::sleep(delay).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                    .await;
            });
        }
    });
    addr.to_string()
}

async fn run(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    match run_source("timeout.rite", src, &mut ctx).await {
        Ok(v) => v.to_display(&ctx.atoms),
        Err(e) => format!("raise: {e}"),
    }
}

#[tokio::test]
async fn timeout_ms_below_the_delay_times_out_as_a_timeout() {
    let _guard = http_test_lock().lock().unwrap();
    let addr = slow_server(std::time::Duration::from_millis(800)).await;
    let out = run(&format!(
        r#"^ ! @http.request(⟨url: "http://{addr}/", timeout_ms: 100⟩)"#
    ))
    .await;
    assert!(
        out.contains("err(") && out.contains("http.timeout"),
        "expected err with kind http.timeout, got: {out}"
    );
}

#[tokio::test]
async fn timeout_ms_above_the_delay_succeeds() {
    let _guard = http_test_lock().lock().unwrap();
    let addr = slow_server(std::time::Duration::from_millis(300)).await;
    let out = run(&format!(
        r#"
resp ← ! @http.request(⟨url: "http://{addr}/", timeout_ms: 5000⟩)?
^ resp.status
"#
    ))
    .await;
    assert_eq!(out, "200", "slow-but-within-timeout request failed: {out}");
}
