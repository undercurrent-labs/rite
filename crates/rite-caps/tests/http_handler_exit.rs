// Shares the same process-global listen state as the other HTTP tests, so it holds
// the lock for the whole body. See the note at the top of `http_handlers.rs`.
#![allow(clippy::await_holding_lock)]

//! `@process.exit` inside a request handler ends the *process*, not the request.
//!
//! Handler failures are deliberately contained: the server logs them, answers 500,
//! and keeps accepting. An exit is not a failure, and containing it would mean a
//! script could ask to stop and be quietly overruled by the server it started —
//! `use @http.recover` would even report the exit to the caller as a handler error.

use rite_caps::http::{clear_last_bound_addr, last_bound_addr};
use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, EvalError, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_test_lock() -> &'static Mutex<()> {
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
            panic!("server did not bind within {:?}", timeout);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// `use @http.recover` is on deliberately: it turns handler *failures* into
/// described 500s, and the exit has to pass through it untouched.
const SOURCE: &str = r#"
@http.listen "127.0.0.1:0" ⟦
  use @http.recover

  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧

  GET "/quit" ⟦
    ! @process.exit(9)
  ⟧
⟧
"#;

#[tokio::test]
async fn handler_exit_stops_the_server_and_ends_the_script() {
    let _guard = http_test_lock().lock().unwrap();
    clear_last_bound_addr();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "20");

    // The run's result is what carries the status out, so unlike the other HTTP
    // tests this one keeps it rather than dropping it.
    let server = tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        run_source("handler-exit.rite", SOURCE, &mut ctx).await
    });

    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let client = reqwest::Client::new();

    // Ordinary route first: the server is genuinely up, so what follows is the
    // exit stopping it rather than a server that never started.
    let health = client
        .get(format!("http://{}/health", addr))
        .send()
        .await
        .expect("health request");
    assert_eq!(health.status().as_u16(), 200);

    let quit = client
        .get(format!("http://{}/quit", addr))
        .send()
        .await
        .expect("quit request");
    // The caller in flight is told the truth: this server is going away. Not a 500,
    // which would describe it as a handler that failed.
    assert_eq!(
        quit.status().as_u16(),
        503,
        "an exiting handler should answer 503, not a recovered error"
    );

    // `@http.listen` blocks until shutdown, so the script only finishes if the exit
    // actually stopped the accept loop.
    let outcome = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("script did not finish: the exit did not stop the server")
        .expect("server task panicked");

    match outcome {
        Err(EvalError::Exit(9)) => {}
        Err(e) => panic!("expected exit 9, got error: {e}"),
        Ok(v) => panic!("expected exit 9, but listen returned normally with {v:?}"),
    }
}
