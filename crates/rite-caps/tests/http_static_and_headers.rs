//! Response headers, catch-all routes, `@http.file` static serving, 405, and
//! `req.form`.
//!
//! These tests share process-global listen state, so they take a lock.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

/// Async mutex (not `std`): the guard is held across awaits, which is exactly what
/// `clippy::await_holding_lock` objects to for the blocking kind.
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
        let _ = run_source("http-static.rite", &source, &mut ctx).await;
    })
}

/// A site tree: `index.html`, a stylesheet, a binary asset, and a file one level
/// up that nothing served from `public/` may ever reach.
fn site_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let public = dir.path().join("public");
    std::fs::create_dir_all(public.join("assets")).expect("mkdir");
    std::fs::write(public.join("index.html"), "<!doctype html><h1>home</h1>").expect("index");
    std::fs::write(public.join("assets/app.css"), "body{color:red}").expect("css");
    std::fs::write(public.join("assets/logo.png"), [0x89u8, b'P', b'N', b'G']).expect("png");
    std::fs::write(dir.path().join("secret.txt"), "do not serve me").expect("secret");
    dir
}

fn public_of(dir: &tempfile::TempDir) -> String {
    dir.path().join("public").display().to_string()
}

#[tokio::test]
async fn explicit_content_type_overrides_the_inferred_one() {
    let _g = http_lock().lock().await;
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/page" ⟦
    ^ ⟨
      status: 200,
      body: "<h1>hi</h1>",
      headers: ⟨"content-type": "text/html; charset=utf-8"⟩
    ⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let resp = reqwest::get(format!("http://{addr}/page"))
        .await
        .expect("request");

    // The whole point: a string body used to be `text/plain` unconditionally, which
    // makes a browser render the markup as source text.
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    assert_eq!(resp.text().await.expect("body"), "<h1>hi</h1>");
}

#[tokio::test]
async fn a_list_header_value_repeats_the_header() {
    let _g = http_lock().lock().await;
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/login" ⟦
    ^ ⟨
      status: 204,
      headers: ⟨"set-cookie": ["a=1; Path=/", "b=2; Path=/"]⟩
    ⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let resp = reqwest::get(format!("http://{addr}/login"))
        .await
        .expect("request");

    assert_eq!(resp.status().as_u16(), 204);
    let cookies: Vec<&str> = resp
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    assert_eq!(cookies, vec!["a=1; Path=/", "b=2; Path=/"]);
    // `⟨status, headers⟩` with no body is an envelope, not a JSON payload
    // describing its own headers.
    assert_eq!(resp.text().await.expect("body"), "");
}

#[tokio::test]
async fn response_helper_takes_headers_as_a_third_argument() {
    let _g = http_lock().lock().await;
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/go" ⟦
    ^ @http.response(302, none, ⟨location: "/elsewhere"⟩)
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let resp = client
        .get(format!("http://{addr}/go"))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status().as_u16(), 302);
    assert_eq!(
        resp.headers().get("location").and_then(|v| v.to_str().ok()),
        Some("/elsewhere"),
    );
}

#[tokio::test]
async fn a_catch_all_does_not_shadow_a_specific_route() {
    let _g = http_lock().lock().await;
    // The catch-all is declared *first* on purpose: precedence is by specificity,
    // not by declaration order, or an SPA fallback could never sit at the top.
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/*rest" |req| ⟦
    ^ 200 ⟨caught: req.path.rest⟩
  ⟧

  GET "/api/ping" ⟦
    ^ 200 ⟨pong: true⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let specific: serde_json::Value = reqwest::get(format!("http://{addr}/api/ping"))
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(specific["pong"], true, "catch-all shadowed a literal route");

    let caught: serde_json::Value = reqwest::get(format!("http://{addr}/deep/link/here"))
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(caught["caught"], "deep/link/here");
}

#[tokio::test]
async fn static_files_are_served_with_a_mime_type_from_the_extension() {
    let _g = http_lock().lock().await;
    let dir = site_tree();
    let _handle = spawn_server(&format!(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/*rest" |req| ⟦
    ^ ! @http.file("{root}", req.path.rest)?
  ⟧
⟧
"#,
        root = public_of(&dir)
    ))
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let css = reqwest::get(format!("http://{addr}/assets/app.css"))
        .await
        .expect("request");
    assert_eq!(
        css.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/css; charset=utf-8"),
    );
    assert_eq!(css.text().await.expect("body"), "body{color:red}");

    let png = reqwest::get(format!("http://{addr}/assets/logo.png"))
        .await
        .expect("request");
    assert_eq!(
        png.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/png"),
    );
    assert_eq!(png.bytes().await.expect("bytes").as_ref(), b"\x89PNG");
}

#[tokio::test]
async fn the_served_root_resolves_to_its_index() {
    let _g = http_lock().lock().await;
    let dir = site_tree();
    let _handle = spawn_server(&format!(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/*rest" |req| ⟦
    ^ ! @http.file("{root}", req.path.rest)?
  ⟧
⟧
"#,
        root = public_of(&dir)
    ))
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let resp = reqwest::get(format!("http://{addr}/"))
        .await
        .expect("get /");
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    assert!(resp.text().await.expect("body").contains("<h1>home</h1>"));
}

#[tokio::test]
async fn a_subpath_cannot_escape_the_served_root() {
    let _g = http_lock().lock().await;
    let dir = site_tree();
    // `allow_all` is deliberate: containment is checked before the permission
    // layer, so this proves the root anchoring itself holds rather than relying on
    // a narrow grant to do the work.
    let _handle = spawn_server(&format!(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/*rest" |req| ⟦
    hit ← ! @http.file("{root}", req.path.rest)
    ^ ~ hit ⟦
      ok page → page
      err e → ⟨status: 403, body: e.kind⟩
    ⟧
  ⟧
⟧
"#,
        root = public_of(&dir)
    ))
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    // Not through reqwest's URL builder, which would normalise the `..` away
    // before it ever reached the server.
    let raw = format!(
        "GET /assets/../../secret.txt HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    );
    let body = raw_request(&addr, &raw).await;
    assert!(
        body.contains("403") && body.contains("http.forbidden"),
        "traversal was not refused: {body}"
    );
    assert!(
        !body.contains("do not serve me"),
        "served a file outside the root: {body}"
    );
}

#[tokio::test]
async fn an_spa_deep_link_falls_back_to_the_index() {
    let _g = http_lock().lock().await;
    let dir = site_tree();
    let _handle = spawn_server(&format!(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/api/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧

  GET "/*rest" |req| ⟦
    hit ← ! @http.file("{root}", req.path.rest)
    ^ ~ hit ⟦
      ok page → page
      err e → ! @http.file("{root}", "index.html")?
    ⟧
  ⟧
⟧
"#,
        root = public_of(&dir)
    ))
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    // A client-routed path with no file behind it gets the shell, not a 404.
    let deep = reqwest::get(format!("http://{addr}/settings/profile"))
        .await
        .expect("request");
    assert_eq!(deep.status().as_u16(), 200);
    assert_eq!(
        deep.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("text/html; charset=utf-8"),
    );
    assert!(deep.text().await.expect("body").contains("<h1>home</h1>"));

    // …and the API route underneath it still answers as itself.
    let api: serde_json::Value = reqwest::get(format!("http://{addr}/api/health"))
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(api["status"], "ok");
}

#[tokio::test]
async fn a_known_path_with_the_wrong_method_is_405() {
    let _g = http_lock().lock().await;
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/items" ⟦ ^ 200 ⟨items: []⟩ ⟧
  PUT "/items" ⟦ ^ 200 ⟨put: true⟩ ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;
    let resp = reqwest::Client::new()
        .delete(format!("http://{addr}/items"))
        .send()
        .await
        .expect("request");

    assert_eq!(resp.status().as_u16(), 405, "wrong method reported as 404");
    let allow = resp
        .headers()
        .get("allow")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        allow.contains("GET") && allow.contains("PUT"),
        "allow: {allow}"
    );

    // An unknown path is still a 404 — 405 is only for a path that exists.
    let missing = reqwest::get(format!("http://{addr}/nope"))
        .await
        .expect("request");
    assert_eq!(missing.status().as_u16(), 404);
}

#[tokio::test]
async fn a_urlencoded_body_arrives_as_req_form() {
    let _g = http_lock().lock().await;
    let _handle = spawn_server(
        r#"
@http.listen "127.0.0.1:0" ⟦
  POST "/submit" |req| ⟦
    fields ← req.form?
    ^ 200 ⟨name: fields.name, note: fields.note⟩
  ⟧

  POST "/json-here" |req| ⟦
    ^ 200 ⟨form_ok: is_ok(req.form)⟩
  ⟧
⟧
"#,
    )
    .await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    let form: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/submit"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body("name=ada+lovelace&note=%E2%9A%A1+works")
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(form["name"], "ada lovelace");
    assert_eq!(form["note"], "⚡ works");

    // The content type decides, not the shape of the bytes: a JSON body is not a
    // form, even though it would "parse" as one key with no value.
    let json: serde_json::Value = reqwest::Client::new()
        .post(format!("http://{addr}/json-here"))
        .header("content-type", "application/json")
        .body(r#"{"a":1}"#)
        .send()
        .await
        .expect("request")
        .json()
        .await
        .expect("json");
    assert_eq!(json["form_ok"], false);
}

/// Send a request the HTTP client would refuse to send verbatim.
async fn raw_request(addr: &str, request: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await.expect("read response");
    String::from_utf8_lossy(&buf).to_string()
}
