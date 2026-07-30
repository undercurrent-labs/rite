// Shares the process-global RITE_HTTP_TEST env vars with the other http tests.
#![allow(clippy::await_holding_lock)]

//! Outbound `@http.get` / `post` / `request`.
//!
//! Until these existed, `--allow net=host` granted nothing at all: the only thing `net`
//! gated was the *bind address* of `@http.listen`, while the book documented outbound
//! calls needing it. The permission is checked per host, so a grant for one host does
//! not open another.

use rite_caps::http::{clear_last_bound_addr, last_bound_addr};
use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Run `src` with exactly the given permission specs, returning stdout.
async fn run_with(src: &str, specs: &[&str]) -> Result<String, String> {
    let mut perms = PermissionSet::default_secure();
    for s in specs {
        perms.grant(Permission::parse(s).expect("permission spec"));
    }
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, perms);
    match run_source("client.rite", src, &mut ctx).await {
        Ok(_) => Ok(ctx.stdout.join("")),
        Err(e) => Err(e.to_string()),
    }
}

/// A server that echoes what it was sent, so the client side can be checked.
async fn spawn_echo() -> (tokio::task::JoinHandle<()>, String) {
    clear_last_bound_addr();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "10");
    let handle = tokio::spawn(async {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let src = r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/hello" ⟦ ^ 200 ⟨msg: "hi", n: 7⟩ ⟧
  POST "/echo" |req| ⟦
    payload ← req.json?
    ^ 200 ⟨saw: payload.name⟩
  ⟧
⟧
"#;
        let _ = run_source("srv.rite", src, &mut ctx).await;
    });
    let start = std::time::Instant::now();
    loop {
        if let Some(a) = last_bound_addr() {
            return (handle, a);
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "server never bound"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn get_returns_a_response_record() {
    let _guard = http_test_lock().lock().unwrap();
    let (srv, addr) = spawn_echo().await;
    let src = format!(
        "resp ← ! @http.get(\"http://{addr}/hello\")?\n\
         body ← resp.json?\n\
         ! @console.println(str(resp.status) + \" \" + body.msg + \" \" + str(body.n))\n"
    );
    let out = run_with(&src, &["net=127.0.0.1"]).await.expect("request");
    assert!(out.contains("200 hi 7"), "{out}");
    srv.abort();
    let _ = srv.await;
}

#[tokio::test]
async fn post_sends_a_record_as_json() {
    let _guard = http_test_lock().lock().unwrap();
    let (srv, addr) = spawn_echo().await;
    let src = format!(
        "resp ← ! @http.post(\"http://{addr}/echo\", ⟨name: \"aura\"⟩)?\n\
         ! @console.println(resp.json?.saw)\n"
    );
    let out = run_with(&src, &["net=127.0.0.1"]).await.expect("request");
    assert!(out.contains("aura"), "{out}");
    srv.abort();
    let _ = srv.await;
}

#[tokio::test]
async fn request_takes_a_method_and_headers() {
    let _guard = http_test_lock().lock().unwrap();
    let (srv, addr) = spawn_echo().await;
    let src = format!(
        "resp ← ! @http.request(⟨\n  method: \"GET\",\n  url: \"http://{addr}/hello\",\n  \
         headers: ⟨accept: \"application/json\"⟩\n⟩)?\n\
         ! @console.println(str(resp.status))\n"
    );
    let out = run_with(&src, &["net=127.0.0.1"]).await.expect("request");
    assert!(out.contains("200"), "{out}");
    srv.abort();
    let _ = srv.await;
}

#[tokio::test]
async fn outbound_needs_a_net_grant_for_that_host() {
    let src = "r ← ! @http.get(\"http://127.0.0.1:9/nope\")?\n";
    let denied = run_with(src, &[]).await.expect_err("must be denied");
    assert!(denied.contains("net permission denied"), "{denied}");

    // A grant for a different host must not help.
    let wrong = run_with(src, &["net=example.com"])
        .await
        .expect_err("wrong host must not open 127.0.0.1");
    assert!(wrong.contains("net permission denied"), "{wrong}");
}

#[tokio::test]
async fn a_transport_failure_is_a_value_not_an_abort() {
    // Port 9 (discard) refuses; the call should return err(...) rather than kill the run.
    let src = "r ← ! @http.get(\"http://127.0.0.1:9/nope\")\n\
               ! @console.println(str(is_err(r)))\n";
    let out = run_with(src, &["net=127.0.0.1"]).await.expect("no abort");
    assert!(out.contains("true"), "expected err(...), got {out}");
}
