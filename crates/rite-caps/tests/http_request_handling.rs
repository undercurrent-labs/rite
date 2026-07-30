//! Request-handling contracts for the Rite HTTP host: query decoding, body
//! limits, concurrency, and the auto-injected `/health` route.
//!
//! These tests share process-global listen state, so they take a lock.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

/// Async mutex (not `std`): these tests hold the guard across awaits, and a
/// blocking guard there is what `clippy::await_holding_lock` warns about.
fn http_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn wait_for_bind(timeout: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Some(addr) = rite_caps::http::last_bound_addr() {
            return addr;
        }
        assert!(
            start.elapsed() <= timeout,
            "server did not bind within {timeout:?}"
        );
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}

async fn spawn_server(source: &str) -> tokio::task::JoinHandle<()> {
    rite_caps::http::clear_last_bound_addr();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "20");
    let source = source.to_string();
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let _ = run_source("http-req.rite", &source, &mut ctx).await;
    })
}

#[tokio::test]
async fn query_string_is_percent_decoded() {
    let _g = http_lock().lock().await;
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/echo" |req| ⟦
    ^ 200 ⟨q: req.query⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let body = reqwest::get(format!(
        "http://{addr}/echo?name=a%20b&plus=a+b&sym=%2B%26%3D&path=%2Fetc%2Fpasswd&uni=%E2%9A%A1&dup=1&dup=2&flag&bad=%zz"
    ))
    .await
    .expect("request")
    .text()
    .await
    .expect("body");

    let json: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("{e}: {body}"));
    let q = &json["q"];
    assert_eq!(q["name"], "a b", "percent escape not decoded: {body}");
    assert_eq!(q["plus"], "a b", "`+` not decoded as space: {body}");
    assert_eq!(q["sym"], "+&=", "reserved characters not decoded: {body}");
    assert_eq!(q["path"], "/etc/passwd", "{body}");
    assert_eq!(q["uni"], "⚡", "multi-byte UTF-8 not decoded: {body}");
    // Documented: a repeated key keeps its last value.
    assert_eq!(q["dup"], "2", "{body}");
    // A bare key is present with an empty value; a malformed escape is kept as-is.
    assert_eq!(q["flag"], "", "{body}");
    assert_eq!(q["bad"], "%zz", "{body}");

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn oversized_body_gets_413_not_an_empty_body() {
    let _g = http_lock().lock().await;
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  POST "/upload" |req| ⟦
    text ← req.text?
    ^ 200 ⟨len: text → count⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let client = reqwest::Client::new();

    // Just under the 1 MiB limit: handled normally.
    let ok_body = "x".repeat(1_048_000);
    let resp = client
        .post(format!("http://{addr}/upload"))
        .body(ok_body.clone())
        .send()
        .await
        .expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains(&ok_body.len().to_string()),
        "body should reach the handler intact: {text}"
    );

    // Over the limit: 413, not a silently emptied body.
    let resp = client
        .post(format!("http://{addr}/upload"))
        .body("x".repeat(2 * 1_048_576))
        .send()
        .await
        .expect("request");
    assert_eq!(
        resp.status().as_u16(),
        413,
        "oversized body must be rejected"
    );
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("payload_too_large") && text.contains("1048576"),
        "413 body should name the limit: {text}"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn plain_handlers_serve_concurrently() {
    let _g = http_lock().lock().await;
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use @http.log
  GET "/slow" ⟦
    ! @clock.sleep(400)
    ^ 200 ⟨ok: true⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let url = format!("http://{addr}/slow");

    let t0 = std::time::Instant::now();
    let (a, b) = tokio::join!(
        reqwest::get(url.clone()),
        // Separate client so the connection pool cannot serialize us.
        reqwest::Client::new().get(&url).send()
    );
    let elapsed = t0.elapsed();
    assert_eq!(a.expect("first").status().as_u16(), 200);
    assert_eq!(b.expect("second").status().as_u16(), 200);
    assert!(
        elapsed < Duration::from_millis(700),
        "two 400ms handlers took {elapsed:?}: requests without custom middleware \
         must not be serialized behind the handler mutex"
    );

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn custom_middleware_requests_stay_correct() {
    // Custom middleware is deliberately serialized (it drives process-global
    // continuation state); this only asserts that concurrent requests still get
    // their own correct response, whatever the scheduling.
    let _g = http_lock().lock().await;
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  use { |req, next|
    next(req)
  }
  GET "/echo/:word" |req| ⟦
    ^ 200 ⟨word: req.path.word⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let (a, b) = tokio::join!(
        reqwest::get(format!("http://{addr}/echo/alpha")),
        reqwest::Client::new()
            .get(format!("http://{addr}/echo/beta"))
            .send()
    );
    let a = a.expect("first");
    let b = b.expect("second");
    assert_eq!(a.status().as_u16(), 200);
    assert_eq!(b.status().as_u16(), 200);
    let ta = a.text().await.unwrap();
    let tb = b.text().await.unwrap();
    assert!(ta.contains("alpha"), "{ta}");
    assert!(tb.contains("beta"), "{tb}");

    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
async fn health_is_auto_injected_and_overridable() {
    let _g = http_lock().lock().await;
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/other" ⟦ ^ 200 ⟨ok: true⟩ ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("request");
    assert_eq!(resp.status().as_u16(), 200);
    let text = resp.text().await.unwrap();
    assert!(
        text.contains("\"status\":\"ok\""),
        "a server with no /health route answers it anyway: {text}"
    );
    handle.abort();
    let _ = handle.await;

    // A route of the script's own wins over the injected one.
    let handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦ ^ 200 ⟨status: #mine⟩ ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let text = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("request")
        .text()
        .await
        .unwrap();
    assert!(text.contains("mine"), "user route must win: {text}");
    handle.abort();
    let _ = handle.await;
}
