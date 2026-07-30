//! `rite studio` — local Studio API + embedded UI shell.
//!
//! Security model (every point here is load-bearing; see the tests at the end):
//!
//! * The service **executes arbitrary Rite source**, so it binds loopback only
//!   and is never exposed on a routable interface.
//! * Every route requires the per-session token, compared in constant time.
//!   `/api/v1/version` answers a minimal liveness payload unauthenticated so a
//!   client can tell "running" from "not running" without a secret.
//! * The `Host` header must name this loopback service. Loopback binding alone
//!   does not stop **DNS rebinding**: a page on `evil.com` whose DNS flips to
//!   127.0.0.1 is same-origin with itself, so CORS never applies. Such a request
//!   carries `Host: evil.com`, which is rejected here.
//! * Executed scripts get `PermissionSet::default_secure()` unless the operator
//!   passes `--allow-all`. A browser tab must not silently mean full filesystem,
//!   network and process access.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

pub struct StudioState {
    /// Per-session bearer token; required by every route.
    token: String,
    /// Project root: relative paths in executed scripts resolve here.
    project: Option<PathBuf>,
    /// Port we advertise and validate `Host` against.
    port: u16,
    /// True when `--allow-all` was passed: scripts run with full host access.
    allow_all: bool,
}

impl StudioState {
    #[cfg(test)]
    fn for_test(token: &str, port: u16) -> Self {
        Self {
            token: token.to_string(),
            project: None,
            port,
            allow_all: false,
        }
    }
}

pub async fn run(
    port: u16,
    no_open: bool,
    project: Option<&Path>,
    allow_all: bool,
) -> anyhow::Result<ExitCode> {
    use std::net::SocketAddr;

    let project = match project {
        Some(p) => {
            let root = p
                .canonicalize()
                .map_err(|e| anyhow::anyhow!("--project {}: {e}", p.display()))?;
            if !root.is_dir() {
                anyhow::bail!("--project {} is not a directory", p.display());
            }
            // The Studio evaluator resolves relative paths against the process
            // working directory, so this is what "project root" means here. It
            // is a convenience, not a sandbox: `--allow-all` can still reach
            // outside it.
            std::env::set_current_dir(&root)
                .map_err(|e| anyhow::anyhow!("enter --project {}: {e}", root.display()))?;
            Some(root)
        }
        None => None,
    };

    let token = uuid::Uuid::new_v4().to_string();
    let state = Arc::new(StudioState {
        token: token.clone(),
        project,
        port,
        allow_all,
    });

    let app = Router::new()
        .route("/api/v1/version", get(studio_version))
        .route("/api/v1/parse", post(studio_parse))
        .route("/api/v1/analyze", post(studio_analyze))
        .route("/api/v1/format", post(studio_format))
        .route("/api/v1/run", post(studio_run))
        .route("/api/v1/check", post(studio_check))
        .route("/api/v1/emit-rust", post(studio_emit_rust))
        .route("/", get(studio_index))
        .with_state(state.clone());

    // Loopback only: this endpoint executes arbitrary Rite source, so it must
    // never be reachable from another host.
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let url = format!("http://{addr}/?token={token}");

    println!("Rite Studio local service on http://{addr}");
    println!("  open:          {url}");
    println!("  session token: {token}");
    println!("                 send as `Authorization: Bearer <token>` (or a `token` body field)");
    println!("  permissions:   {}", permission_summary(allow_all));
    if !allow_all {
        println!("                 `rite studio --allow-all` grants full host access");
    }
    if let Some(p) = &state.project {
        println!(
            "  project root:  {} (relative paths resolve here)",
            p.display()
        );
    }
    println!("  loopback only; requests with a non-local Host header are rejected");

    if !no_open {
        if let Err(e) = crate::util::open_in_browser(&url) {
            eprintln!("  note: {e} — open the URL above manually");
        }
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(ExitCode::SUCCESS)
}

fn permission_summary(allow_all: bool) -> &'static str {
    if allow_all {
        "FULL host access (filesystem, network, processes)"
    } else {
        "restricted (console, clock, random; no filesystem, network or process access)"
    }
}

#[derive(serde::Deserialize)]
pub struct SourceBody {
    source: String,
    #[serde(default)]
    dialect: Option<String>,
    /// Session token, for clients that cannot set headers.
    #[serde(default)]
    token: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct TokenQuery {
    #[serde(default)]
    token: Option<String>,
}

/// Constant-time comparison so a wrong guess leaks no per-byte timing.
/// Length is not secret (the token is a fixed-size UUID).
fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get("authorization")?.to_str().ok()?;
    let (scheme, value) = raw.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") {
        Some(value.trim())
    } else {
        None
    }
}

fn split_host_port(host: &str) -> (&str, Option<&str>) {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        // IPv6 literal: [::1]:4041
        return match rest.split_once(']') {
            Some((addr, tail)) => (addr, tail.strip_prefix(':')),
            None => (host, None),
        };
    }
    match host.rsplit_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (host, None),
    }
}

/// True when `Host` names this loopback service on this port.
fn host_is_local(host: &str, port: u16) -> bool {
    let (name, host_port) = split_host_port(host);
    let port_ok = match host_port {
        Some(p) => p.parse::<u16>() == Ok(port),
        // A bare host means the default port 80.
        None => port == 80,
    };
    port_ok
        && matches!(
            name.trim_end_matches('.').to_ascii_lowercase().as_str(),
            "127.0.0.1" | "localhost" | "::1"
        )
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"ok": false, "error": message})),
    )
        .into_response()
}

/// The rejection response, or `None` when the `Host` names this loopback service.
///
/// Returning `Option<Response>` rather than `Result<(), Response>` keeps the
/// large `Response` out of an error variant (clippy::result_large_err).
fn check_host(st: &StudioState, headers: &HeaderMap) -> Option<Response> {
    let host = headers
        .get("host")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    if host.is_empty() {
        return Some(json_error(
            StatusCode::FORBIDDEN,
            "missing Host header; Rite Studio only serves 127.0.0.1/localhost requests",
        ));
    }
    if !host_is_local(host, st.port) {
        // DNS rebinding: the attacker's page sends its own Host.
        return Some(json_error(
            StatusCode::FORBIDDEN,
            "Host header is not this local Studio service (possible DNS rebinding); \
             use http://127.0.0.1:<port>",
        ));
    }
    None
}

/// The rejection response, or `None` when a valid session token was presented.
fn check_token(st: &StudioState, headers: &HeaderMap, body: Option<&str>) -> Option<Response> {
    match bearer_token(headers).or(body) {
        Some(t) if secret_eq(t, &st.token) => None,
        Some(_) => Some(json_error(
            StatusCode::UNAUTHORIZED,
            "invalid session token",
        )),
        None => Some(json_error(
            StatusCode::UNAUTHORIZED,
            "missing session token; send `Authorization: Bearer <token>` \
             (printed by `rite studio`) or a `token` field in the JSON body",
        )),
    }
}

/// Host + token gate for every executing/mutating route.
fn authorize(st: &StudioState, headers: &HeaderMap, body: Option<&str>) -> Option<Response> {
    check_host(st, headers).or_else(|| check_token(st, headers, body))
}

macro_rules! guard {
    ($st:expr, $headers:expr, $body:expr) => {
        if let Some(resp) = authorize(&$st, &$headers, $body) {
            return resp;
        }
    };
}

/// Liveness probe. Unauthenticated callers get version only — no project path,
/// no permission detail. `token_required` is now the truth.
async fn studio_version(State(st): State<Arc<StudioState>>, headers: HeaderMap) -> Response {
    if let Some(resp) = check_host(&st, &headers) {
        return resp;
    }
    let mut body = serde_json::json!({
        "rite": env!("CARGO_PKG_VERSION"),
        "language_version": "1",
        "token_required": true,
    });
    if check_token(&st, &headers, None).is_none() {
        body["authenticated"] = serde_json::json!(true);
        body["permissions"] = serde_json::json!(if st.allow_all {
            "allow-all"
        } else {
            "restricted"
        });
        body["project"] = serde_json::json!(st.project.as_ref().map(|p| p.display().to_string()));
    }
    Json(body).into_response()
}

/// The UI shell is itself token-gated: `http://127.0.0.1:<port>/?token=…`.
/// The page reads the token from its own query string, so it never has to be
/// embedded in a response that an unauthenticated caller could fetch.
async fn studio_index(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
) -> Response {
    if let Some(resp) = check_host(&st, &headers) {
        return resp;
    }
    if check_token(&st, &headers, q.token.as_deref()).is_some() {
        return (
            StatusCode::UNAUTHORIZED,
            Html(UNAUTHORIZED_HTML.to_string()),
        )
            .into_response();
    }
    Html(STUDIO_HTML).into_response()
}

async fn studio_parse(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    Json(serde_json::to_value(rite_wasm::parse(&body.source)).unwrap_or_default()).into_response()
}

async fn studio_analyze(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    Json(rite_wasm::analyze(&body.source)).into_response()
}

async fn studio_format(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    let d = body.dialect.as_deref().unwrap_or("glyph");
    match rite_wasm::format(&body.source, d) {
        Ok(r) => Json(serde_json::json!({
            "ok": true,
            "text": r.text,
            "dialect": r.dialect,
            "source_map": r.source_map,
        }))
        .into_response(),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})).into_response(),
    }
}

async fn studio_run(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    // Await the async runner — do not call run_blocking() here (nested Tokio panic).
    let result = rite_wasm::run(
        &body.source,
        rite_wasm::RunOptions {
            allow_all: st.allow_all,
            // Without --allow-all, scripts run the browser-safe subset: no
            // @process, no real socket binding, and default_secure permissions.
            browser_safe: !st.allow_all,
            timeout_ms: Some(5000),
            seed: Some(42),
            files: Default::default(),
        },
    )
    .await;
    Json(serde_json::to_value(result).unwrap_or_default()).into_response()
}

async fn studio_check(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    Json(rite_wasm::analyze(&body.source)).into_response()
}

async fn studio_emit_rust(
    State(st): State<Arc<StudioState>>,
    headers: HeaderMap,
    Json(body): Json<SourceBody>,
) -> Response {
    guard!(st, headers, body.token.as_deref());
    Json(rite_wasm::emit_rust(&body.source)).into_response()
}

const UNAUTHORIZED_HTML: &str = r#"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"/><title>Rite Studio — token required</title></head>
<body style="font-family:ui-sans-serif,system-ui,sans-serif;background:#0b0f14;color:#e6edf3;padding:2rem">
<h1 style="color:#7ee0ff">Rite Studio — session token required</h1>
<p>This service executes Rite source on your machine, so it is token-gated.</p>
<p>Open the URL printed by <code>rite studio</code>:</p>
<pre style="background:#121821;padding:1rem;border-radius:6px">http://127.0.0.1:&lt;port&gt;/?token=&lt;session token&gt;</pre>
</body></html>"#;

// Minimal embedded Studio shell (full Vue app lives in apps/rite-studio)
const STUDIO_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>Rite Studio</title>
<style>
:root { color-scheme: dark; --bg:#0b0f14; --panel:#121821; --fg:#e6edf3; --accent:#7ee0ff; --pink:#ff7edb; }
* { box-sizing: border-box; }
body { margin:0; font-family: ui-sans-serif, system-ui, sans-serif; background:var(--bg); color:var(--fg); }
header { display:flex; gap:.5rem; flex-wrap:wrap; padding:.75rem 1rem; border-bottom:1px solid #222; align-items:center; }
header h1 { font-size:1rem; margin:0; color:var(--accent); margin-right:auto; }
button, select { background:#1a2332; color:var(--fg); border:1px solid #334; border-radius:6px; padding:.4rem .7rem; cursor:pointer; }
button:hover { border-color:var(--accent); }
main { display:grid; grid-template-columns: 1fr 1fr; min-height: calc(100vh - 52px); }
@media (max-width: 800px) { main { grid-template-columns: 1fr; } }
textarea { width:100%; height:100%; min-height:50vh; background:var(--panel); color:var(--fg); border:0; padding:1rem; font-family: ui-monospace, monospace; font-size:14px; resize:none; }
.side { display:flex; flex-direction:column; border-left:1px solid #222; }
.tabs { display:flex; gap:.25rem; padding:.5rem; border-bottom:1px solid #222; flex-wrap:wrap; }
.tabs button.active { border-color:var(--pink); color:var(--pink); }
pre { margin:0; padding:1rem; overflow:auto; flex:1; white-space:pre-wrap; font-size:13px; }
</style>
</head>
<body>
<header>
  <h1>Rite Studio ◆</h1>
  <button id="run">Run</button>
  <button id="check">Check</button>
  <button id="fmt">Format</button>
  <select id="dialect"><option value="glyph">glyph</option><option value="ascii">ascii</option></select>
  <button id="convert">Convert</button>
  <button id="emit">Emit Rust</button>
</header>
<main>
  <textarea id="src">◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
</textarea>
  <div class="side">
    <div class="tabs">
      <button class="active" data-tab="out">Output</button>
      <button data-tab="diag">Diagnostics</button>
      <button data-tab="ast">AST</button>
      <button data-tab="ir">IR</button>
      <button data-tab="rust">Rust</button>
    </div>
    <pre id="panel">Ready.</pre>
  </div>
</main>
<script>
const TOKEN = new URLSearchParams(location.search).get('token') || '';
const panel = document.getElementById('panel');
const src = document.getElementById('src');
let tab = 'out';
document.querySelectorAll('.tabs button').forEach(b => b.onclick = () => {
  document.querySelectorAll('.tabs button').forEach(x => x.classList.remove('active'));
  b.classList.add('active'); tab = b.dataset.tab; panel.textContent = '…';
});
async function post(path, body) {
  const r = await fetch(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'authorization': 'Bearer ' + TOKEN },
    body: JSON.stringify(body)
  });
  if (r.status === 401 || r.status === 403) {
    return { ok: false, error: 'not authorized (' + r.status + ') — reopen the URL printed by `rite studio`' };
  }
  return r.json();
}
document.getElementById('run').onclick = async () => {
  const j = await post('/api/v1/run', { source: src.value });
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('check').onclick = async () => {
  const j = await post('/api/v1/analyze', { source: src.value });
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('fmt').onclick = async () => {
  const dialect = document.getElementById('dialect').value;
  const j = await post('/api/v1/format', { source: src.value, dialect });
  if (j.ok) src.value = j.text;
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('convert').onclick = async () => {
  const dialect = document.getElementById('dialect').value;
  const j = await post('/api/v1/format', { source: src.value, dialect });
  if (j.ok) src.value = j.text;
};
document.getElementById('emit').onclick = async () => {
  const j = await post('/api/v1/emit-rust', { source: src.value });
  panel.textContent = j.rust || JSON.stringify(j, null, 2);
};
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    #[test]
    fn secret_eq_matches_only_exact() {
        assert!(secret_eq("abc", "abc"));
        assert!(!secret_eq("abc", "abd"));
        assert!(!secret_eq("abc", "ab"));
        assert!(!secret_eq("", "a"));
        assert!(secret_eq("", ""));
    }

    #[test]
    fn host_accepts_only_loopback_on_our_port() {
        assert!(host_is_local("127.0.0.1:4041", 4041));
        assert!(host_is_local("localhost:4041", 4041));
        assert!(host_is_local("LOCALHOST:4041", 4041));
        assert!(host_is_local("[::1]:4041", 4041));

        // DNS rebinding: attacker domain resolving to 127.0.0.1
        assert!(!host_is_local("evil.example:4041", 4041));
        assert!(!host_is_local("rite.studio.local:4041", 4041));
        // Right host, wrong port (a different local service proxying us)
        assert!(!host_is_local("127.0.0.1:9999", 4041));
        // Bare host only valid when we really are on port 80
        assert!(!host_is_local("127.0.0.1", 4041));
        assert!(host_is_local("127.0.0.1", 80));
        // Not loopback
        assert!(!host_is_local("192.168.1.10:4041", 4041));
        assert!(!host_is_local("", 4041));
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Bearer tok-123")])),
            Some("tok-123")
        );
        assert_eq!(
            bearer_token(&headers(&[("authorization", "bearer tok-123")])),
            Some("tok-123")
        );
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Basic tok-123")])),
            None
        );
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    #[test]
    fn authorize_requires_token_and_local_host() {
        let st = StudioState::for_test("secret-token", 4041);
        let local = headers(&[("host", "127.0.0.1:4041")]);

        // No token at all → 401
        let err = authorize(&st, &local, None).expect("must reject");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);

        // Wrong token → 401
        let err = authorize(&st, &local, Some("nope")).expect("must reject");
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);

        // Body token → ok
        assert!(authorize(&st, &local, Some("secret-token")).is_none());

        // Header token → ok
        let hdr = headers(&[
            ("host", "127.0.0.1:4041"),
            ("authorization", "Bearer secret-token"),
        ]);
        assert!(authorize(&st, &hdr, None).is_none());

        // Foreign Host → 403 even with the right token
        let rebind = headers(&[
            ("host", "evil.example:4041"),
            ("authorization", "Bearer secret-token"),
        ]);
        let err = authorize(&st, &rebind, None).expect("must reject");
        assert_eq!(err.status(), StatusCode::FORBIDDEN);

        // Missing Host → 403
        let err = authorize(&st, &HeaderMap::new(), Some("secret-token")).expect("must reject");
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn permission_summary_is_explicit() {
        assert!(permission_summary(false).contains("restricted"));
        assert!(permission_summary(true).contains("FULL"));
    }
}
