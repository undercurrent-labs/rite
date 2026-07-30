//! Real Axum/Hyper-backed HTTP with Rite route handler evaluation.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use indexmap::IndexMap;
use parking_lot::Mutex;
use rite_runtime::{
    set_http_next_invoker, EvalError, Evaluator, FunctionEntry, HostHandle, HttpMiddleware, Key,
    RuntimeContext, Value,
};
use rite_sem::BlockIr;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex as AsyncMutex};

pub struct HttpCap {
    pub last_addr: Mutex<Option<String>>,
}

#[derive(Clone)]
pub struct RiteRoute {
    pub method: String,
    pub path: String,
    pub param_name: Option<String>,
    pub body: BlockIr,
}

#[derive(Clone)]
pub struct ServerState {
    pub routes: Vec<RiteRoute>,
    pub functions: HashMap<String, FunctionEntry>,
    pub perms: PermissionSet,
    /// Middleware from `use @http.log` / `use { |req, next| … }`.
    pub middleware: Vec<HttpMiddleware>,
    pub lock: Arc<AsyncMutex<()>>,
}

impl HttpCap {
    pub fn new() -> Self {
        Self {
            last_addr: Mutex::new(None),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "listen",
            docs: "Start an HTTP server and block until shutdown. Loopback (127.0.0.0/8, ::1, localhost) binds by default; any other interface — including the wildcards 0.0.0.0 and [::] — needs --allow net=<host>.",
            arity: 2,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "response",
            docs: "Build an explicit HTTP response record.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "log",
            docs: "Middleware: log each request as `rite: METHOD path status duration` to stderr. Enable with `use @http.log` or `⊏ @http.log`.",
            arity: 0,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "recover",
            docs: "Middleware: convert handler panics/errors into JSON 500 responses. Enable with `use @http.recover` or `⊏ @http.recover`.",
            arity: 0,
            effectful: false,
            permission: "",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        match method {
            "listen" => {
                if let Some(state) = take_pending_server() {
                    self.listen_with_state(state, perms).await
                } else {
                    self.listen_legacy(args, perms, ctx).await
                }
            }
            "response" => {
                let status = args.first().and_then(|v| v.as_int()).unwrap_or(200);
                Ok(Value::record(vec![
                    (Key::String("status".into()), Value::Int(status)),
                    (
                        Key::String("body".into()),
                        args.get(1).cloned().unwrap_or(Value::None),
                    ),
                ]))
            }
            // Middleware identifiers are resolved at listen-time via `use` / `⊏`.
            // Calling them as values is a no-op document of the plug-in.
            "log" | "recover" => Ok(Value::Atom(ctx.atoms.intern(&format!("http.{}", method)))),
            other => Err(EvalError::Capability(format!("unknown @http.{}", other))),
        }
    }

    async fn listen_with_state(
        &self,
        state: ServerState,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        let addr_str = PENDING_ADDR
            .lock()
            .clone()
            .unwrap_or_else(|| "127.0.0.1:0".into());
        *PENDING_ADDR.lock() = None;
        self.serve(addr_str, state, perms).await
    }

    async fn listen_legacy(
        &self,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        let addr_str = args
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:0")
            .to_string();
        let routes: Vec<RiteRoute> = match args.get(1) {
            Some(Value::List(xs)) => xs
                .iter()
                .filter_map(|v| {
                    Some(RiteRoute {
                        method: v.get_field("method").as_str()?.to_string(),
                        path: v.get_field("path").as_str()?.to_string(),
                        param_name: Some("req".into()),
                        body: BlockIr {
                            params: vec![],
                            body: vec![],
                            span: rite_core::Span::DUMMY,
                        },
                    })
                })
                .collect(),
            _ => vec![],
        };
        let state = ServerState {
            routes,
            functions: ctx.functions.clone(),
            perms: perms.clone(),
            middleware: vec![],
            lock: Arc::new(AsyncMutex::new(())),
        };
        self.serve(addr_str, state, perms).await
    }

    async fn serve(
        &self,
        addr_str: String,
        state: ServerState,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        check_listen_perm(&addr_str, perms)?;

        let listener = tokio::net::TcpListener::bind(&addr_str)
            .await
            .map_err(|e| EvalError::Capability(format!("bind failed: {}", e)))?;
        let local = listener
            .local_addr()
            .map_err(|e| EvalError::Capability(e.to_string()))?;
        let local_s = local.to_string();
        *self.last_addr.lock() = Some(local_s.clone());
        *LAST_BOUND_ADDR.lock() = Some(local_s.clone());
        // Surface the bound URL immediately — port 0 is useless without this.
        // (Also helps when the process blocks until Ctrl-C.)
        println!("rite: listening on http://{}", local_s);
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let state = Arc::new(state);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        // Always block until shutdown so background accepts stay alive for the process.
        // Port 0 still exposes the bound address via last_addr and the return value after stop;
        // while running, last_addr is already set for tests.
        let shutdown_tx = Arc::new(Mutex::new(Some(shutdown_tx)));
        let stx = shutdown_tx.clone();
        let _ = ctrlc::set_handler(move || {
            if let Some(tx) = stx.lock().take() {
                let _ = tx.send(());
            }
        });

        // For tests: auto-stop after a grace window when RITE_HTTP_TEST=1 so
        // servers don't hang forever if the test task is aborted late.
        let test_mode = std::env::var("RITE_HTTP_TEST").ok().as_deref() == Some("1");
        if test_mode {
            let secs: u64 = std::env::var("RITE_HTTP_TEST_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);
            let stx = shutdown_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                if let Some(tx) = stx.lock().take() {
                    let _ = tx.send(());
                }
            });
        }

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break,
                acc = listener.accept() => {
                    let Ok((stream, _)) = acc else { break; };
                    let state = state.clone();
                    tokio::spawn(async move {
                        let io = hyper_util::rt::TokioIo::new(stream);
                        let service = hyper::service::service_fn(move |req: Request<hyper::body::Incoming>| {
                            let state = state.clone();
                            async move {
                                let req = req.map(Body::new);
                                let resp = dispatch_fallback((*state).clone(), req).await;
                                Ok::<_, Infallible>(resp)
                            }
                        });
                        let _ = hyper::server::conn::http1::Builder::new()
                            .serve_connection(io, service)
                            .await;
                    });
                }
            }
        }

        Ok(Value::record(vec![
            (Key::String("addr".into()), Value::string(local.to_string())),
            (Key::String("status".into()), Value::string("stopped")),
        ]))
    }
}

async fn dispatch_fallback(state: ServerState, req: Request<Body>) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    // `GET /health` answers `{"status":"ok"}` unless the script defines the route
    // itself — a convenience for probes that every server gets for free.
    if method == Method::GET
        && (path == "/health" || path == "/health/")
        && !state
            .routes
            .iter()
            .any(|r| r.path == "/health" || r.path == "health")
    {
        return Json(json!({"status": "ok"})).into_response();
    }
    for (idx, route) in state.routes.iter().enumerate() {
        if !route.method.eq_ignore_ascii_case(method.as_str()) {
            continue;
        }
        if match_path(&route.path, &path).is_some()
            || normalize_path(&route.path) == path
            || route.path == path
        {
            return dispatch_rite(state.clone(), idx, req).await;
        }
    }
    (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
}

/// Largest request body buffered for a handler (`req.text` / `req.json` / `req.body`).
/// A larger body is rejected with `413 Payload Too Large` rather than silently
/// arriving as an empty body.
const MAX_REQUEST_BODY_BYTES: usize = 1_048_576; // 1 MiB

async fn dispatch_rite(state: ServerState, idx: usize, req: Request<Body>) -> Response {
    let route = match state.routes.get(idx) {
        Some(r) => r.clone(),
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "missing route").into_response();
        }
    };

    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let headers = req.headers().clone();
    // Query pairs are `application/x-www-form-urlencoded`: percent-escapes and
    // `+`-for-space must be decoded on both halves (`?name=a%20b` → `a b`).
    // A repeated key keeps its **last** value (`?a=1&a=2` → `2`), matching how
    // most frameworks bind a scalar field; insertion order is preserved so the
    // `req.query` record is stable.
    let mut query: IndexMap<String, String> = IndexMap::new();
    if let Some(q) = uri.query() {
        for pair in q.split('&').filter(|p| !p.is_empty()) {
            let (raw_key, raw_val) = pair.split_once('=').unwrap_or((pair, ""));
            query.insert(form_urldecode(raw_key), form_urldecode(raw_val));
        }
    }

    let path_params = match_path(&route.path, &path).unwrap_or_default();
    let body_bytes = match axum::body::to_bytes(req.into_body(), MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            // `to_bytes` collapses two very different failures into one error:
            // the body outgrew the cap, or the client/transport broke.
            let inner = e.into_inner();
            if inner
                .downcast_ref::<http_body_util::LengthLimitError>()
                .is_some()
            {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(json!({
                        "error": "payload_too_large",
                        "limit_bytes": MAX_REQUEST_BODY_BYTES,
                    })),
                )
                    .into_response();
            }
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "invalid_request_body"})),
            )
                .into_response();
        }
    };
    let body_text = String::from_utf8_lossy(&body_bytes).to_string();
    let json_val = serde_json::from_str::<serde_json::Value>(&body_text).ok();

    let mut path_rec = IndexMap::new();
    for (k, v) in &path_params {
        path_rec.insert(Key::String(k.clone()), Value::string(v.clone()));
    }
    let mut query_rec = IndexMap::new();
    for (k, v) in &query {
        query_rec.insert(Key::String(k.clone()), Value::string(v.clone()));
    }
    let mut headers_rec = IndexMap::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        if let Ok(v) = value.to_str() {
            // First value wins for multi-value headers (good enough for auth).
            headers_rec
                .entry(Key::String(key))
                .or_insert_with(|| Value::string(v));
        }
    }

    let req_value = Value::record(vec![
        (Key::String("method".into()), Value::string(method.as_str())),
        (Key::String("path".into()), Value::Record(path_rec)),
        (Key::String("query".into()), Value::Record(query_rec)),
        (Key::String("headers".into()), Value::Record(headers_rec)),
        (Key::String("uri".into()), Value::string(path.clone())),
        (
            Key::String("text".into()),
            Value::ok(Value::string(body_text)),
        ),
        (
            Key::String("json".into()),
            match json_val {
                Some(j) => Value::ok(Value::from_json(&j)),
                None => Value::err(Value::string("invalid json")),
            },
        ),
        (
            Key::String("body".into()),
            Value::Bytes(body_bytes.to_vec().into()),
        ),
    ]);

    let use_log = state
        .middleware
        .iter()
        .any(|m| matches!(m, HttpMiddleware::Named(n) if n == "log"));
    let use_recover = state
        .middleware
        .iter()
        .any(|m| matches!(m, HttpMiddleware::Named(n) if n == "recover"));
    let customs: Vec<rite_runtime::Closure> = state
        .middleware
        .iter()
        .filter_map(|m| match m {
            HttpMiddleware::Function(c) => Some(c.clone()),
            _ => None,
        })
        .collect();
    let t0 = std::time::Instant::now();

    // Custom (closure) middleware is driven through process-global state: the
    // `next()` invoker hook installed by `set_http_next_invoker` plus the
    // continuation map, which the outermost layer *clears* when it unwinds
    // (`run_middleware_chain`, index == 0). Two overlapping requests would wipe
    // each other's continuations, so those requests are serialized behind this
    // mutex — one custom-middleware request at a time.
    //
    // Nothing else in the handler path is shared: each request builds its own
    // RuntimeContext and its own capability host below, and named middleware
    // (`use @http.log`, `use @http.recover`) is just a flag. So plain handlers
    // and named-middleware handlers skip the mutex and run concurrently.
    let _guard = if customs.is_empty() {
        None
    } else {
        Some(state.lock.lock().await)
    };

    let mut ctx = RuntimeContext::new();
    crate::install_defaults(&mut ctx, state.perms.clone());
    for (name, f) in &state.functions {
        ctx.functions.insert(name.clone(), f.clone());
        let clos = Value::Function(rite_runtime::Closure {
            id: 0,
            name: Some(name.clone()),
            params: f.params.clone(),
            env: Arc::new(parking_lot::RwLock::new(rite_runtime::Environment::new())),
            body: f.body.clone(),
        });
        ctx.env.define_name(name, clos, false);
    }

    let param = route.param_name.clone().unwrap_or_else(|| "req".into());
    let result = run_middleware_chain(
        &mut ctx,
        &customs,
        0,
        req_value,
        &route.body,
        &param,
        state.perms.clone(),
    )
    .await;

    // Handler console output is buffered on the per-request RuntimeContext —
    // flush it so `! @console.println` is visible in the server process.
    flush_handler_io(&ctx);

    // `use @http.recover` is what turns a failure into a *described* 500. Without
    // it the client gets a bare 500 and the detail goes to the server's stderr:
    // EvalError text carries source spans, values, and panic messages, which is
    // an information leak to hand to an arbitrary HTTP caller.
    let response = match result {
        Ok(v) => coerce_response(v, &ctx),
        Err(EvalError::Return(v)) => coerce_response(v, &ctx),
        Err(EvalError::Panic(m)) if use_recover => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "panic", "message": m})),
        )
            .into_response(),
        Err(e) if use_recover => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "handler_error", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => {
            let kind = if matches!(e, EvalError::Panic(_)) {
                "panic"
            } else {
                "error"
            };
            emit_process_stderr(&format!(
                "rite: {} {} handler {}: {} (add `use @http.recover` to return it as JSON)\n",
                method.as_str(),
                path,
                kind,
                e
            ));
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    };

    if use_log {
        let status = response.status().as_u16();
        let ms = t0.elapsed().as_millis();
        // Apache-ish one-liner access log to the process stderr (doesn't mix with handler body).
        let line = format!("rite: {} {} {} {}ms", method.as_str(), path, status, ms);
        emit_process_stderr(&format!("{line}\n"));
    }

    response
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Run custom middleware outer→inner, then the route handler.
/// Each layer receives `(req, next)` where `next` is a callable `http.next` handle.
fn run_middleware_chain<'a>(
    ctx: &'a mut RuntimeContext,
    customs: &'a [rite_runtime::Closure],
    index: usize,
    req_value: Value,
    handler_body: &'a BlockIr,
    handler_param: &'a str,
    perms: PermissionSet,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, EvalError>> + Send + 'a>> {
    Box::pin(async move {
        if index >= customs.len() {
            let mut eval = Evaluator::new(ctx);
            return eval
                .call_block_public(handler_body, &[handler_param.to_string()], vec![req_value])
                .await;
        }

        let mw = customs[index].clone();
        let next_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        {
            let cont = NextContinuation {
                customs: customs.to_vec(),
                index: index + 1,
                handler_body: handler_body.clone(),
                handler_param: handler_param.to_string(),
                perms: perms.clone(),
                functions: ctx.functions.clone(),
            };
            next_continuations().lock().insert(next_id, cont);
        }

        // Ensure global next() invoker is installed (idempotent).
        ensure_http_next_invoker();

        let next_val = Value::Handle(HostHandle {
            kind: "http.next".into(),
            id: next_id,
        });

        let mut eval = Evaluator::new(ctx);
        let result = eval
            .call_value_public(Value::Function(mw), vec![req_value, next_val])
            .await;

        if index == 0 {
            set_http_next_invoker(None);
            next_continuations().lock().clear();
        }

        result
    })
}

#[derive(Clone)]
struct NextContinuation {
    customs: Vec<rite_runtime::Closure>,
    index: usize,
    handler_body: BlockIr,
    handler_param: String,
    perms: PermissionSet,
    functions: HashMap<String, FunctionEntry>,
}

fn next_continuations() -> &'static Mutex<HashMap<u64, NextContinuation>> {
    static MAP: std::sync::OnceLock<Mutex<HashMap<u64, NextContinuation>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_http_next_invoker() {
    // Always refresh so a prior clear doesn't leave us without a hook.
    set_http_next_invoker(Some(Arc::new(|id, args| {
        Box::pin(async move {
            let cont = next_continuations()
                .lock()
                .remove(&id)
                .ok_or_else(|| EvalError::Message("middleware next() already used".into()))?;
            let req = args.into_iter().next().unwrap_or(Value::None);
            let mut inner = RuntimeContext::new();
            crate::install_defaults(&mut inner, cont.perms.clone());
            inner.allow_all = cont.perms.allow_all;
            for (name, f) in &cont.functions {
                inner.functions.insert(name.clone(), f.clone());
                let clos = Value::Function(rite_runtime::Closure {
                    id: 0,
                    name: Some(name.clone()),
                    params: f.params.clone(),
                    env: Arc::new(parking_lot::RwLock::new(rite_runtime::Environment::new())),
                    body: f.body.clone(),
                });
                inner.env.define_name(name, clos, false);
            }
            let result = run_middleware_chain(
                &mut inner,
                &cont.customs,
                cont.index,
                req,
                &cont.handler_body,
                &cont.handler_param,
                cont.perms.clone(),
            )
            .await;
            flush_handler_io(&inner);
            result
        })
    })));
}

/// Emit buffered handler stdout/stderr to the real process streams (and optional test sink).
fn flush_handler_io(ctx: &RuntimeContext) {
    if !ctx.stdout.is_empty() {
        for line in &ctx.stdout {
            emit_process_stdout(line);
        }
    }
    if !ctx.stderr.is_empty() {
        for line in &ctx.stderr {
            emit_process_stderr(line);
        }
    }
}

fn emit_process_stdout(s: &str) {
    print!("{s}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    if let Some(cap) = TEST_IO.lock().as_mut() {
        cap.stdout.push_str(s);
    }
}

fn emit_process_stderr(s: &str) {
    eprint!("{s}");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    if let Some(cap) = TEST_IO.lock().as_mut() {
        cap.stderr.push_str(s);
    }
}

/// In-process capture of HTTP handler I/O for tests (same process as the server task).
#[derive(Debug, Default, Clone)]
pub struct TestIoCapture {
    pub stdout: String,
    pub stderr: String,
}

static TEST_IO: Mutex<Option<TestIoCapture>> = Mutex::new(None);
static LAST_MIDDLEWARE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Begin capturing process-emitted handler stdout/stderr and access logs.
pub fn begin_test_io_capture() {
    *TEST_IO.lock() = Some(TestIoCapture::default());
}

/// Stop capturing and return the accumulated buffers (empty if never started).
pub fn take_test_io_capture() -> TestIoCapture {
    TEST_IO.lock().take().unwrap_or_default()
}

/// Middleware names from the last `@http.listen` registration (for contract tests).
/// Custom closures appear as `"<custom>"`.
pub fn last_registered_middleware() -> Vec<String> {
    LAST_MIDDLEWARE.lock().clone()
}

pub fn coerce_response(v: Value, ctx: &RuntimeContext) -> Response {
    match v {
        Value::List(xs) if !xs.is_empty() => {
            if let Some(status) = xs[0].as_int() {
                let body = xs.get(1).cloned().unwrap_or(Value::None);
                return status_body(status as u16, body, ctx);
            }
            json_response(200, &Value::List(xs), ctx)
        }
        Value::Record(ref r) => {
            if let Some(Value::Int(status)) = r.get(&Key::String("status".into())) {
                if r.contains_key(&Key::String("body".into())) {
                    let body = r
                        .get(&Key::String("body".into()))
                        .cloned()
                        .unwrap_or(Value::None);
                    return status_body(*status as u16, body, ctx);
                }
                if r.len() > 1 {
                    return status_body(*status as u16, Value::Record(r.clone()), ctx);
                }
            }
            json_response(200, &Value::Record(r.clone()), ctx)
        }
        Value::String(s) => (
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            s.to_string(),
        )
            .into_response(),
        Value::Int(n) if (100..600).contains(&n) => StatusCode::from_u16(n as u16)
            .unwrap_or(StatusCode::OK)
            .into_response(),
        other => json_response(200, &other, ctx),
    }
}

fn status_body(status: u16, body: Value, ctx: &RuntimeContext) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    match body {
        Value::String(s) => (
            code,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            s.to_string(),
        )
            .into_response(),
        Value::None => code.into_response(),
        Value::Bytes(b) => (
            code,
            [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
            b.to_vec(),
        )
            .into_response(),
        other => {
            let j = other.to_json(&ctx.atoms);
            (code, Json(j)).into_response()
        }
    }
}

fn json_response(status: u16, v: &Value, ctx: &RuntimeContext) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    (code, Json(v.to_json(&ctx.atoms))).into_response()
}

/// Bind-address policy (docs/book/http.md): loopback is allowed under the
/// default-secure posture, every other interface needs an explicit `net` grant.
///
/// The wildcards `0.0.0.0` and `[::]` are *more* exposed than a single LAN IP —
/// they serve the whole network — so they are treated as non-loopback and denied
/// by default. The host is parsed, never substring-matched: a hostname such as
/// `evil-127.0.0.1.example.com` is not loopback.
pub fn check_listen_perm(addr_str: &str, perms: &PermissionSet) -> Result<(), EvalError> {
    if perms.allow_all {
        return Ok(());
    }
    let host = listen_host(addr_str);
    if is_loopback_host(&host) {
        return Ok(());
    }
    // `--allow net=*`, `--allow net=0.0.0.0`, `--allow net=192.168.1.10`, or the
    // whole `host:port` spelled out.
    if perms.check_net(&host).is_ok() || perms.check_net(addr_str).is_ok() {
        return Ok(());
    }
    Err(EvalError::Permission(format!(
        "net permission denied for listen on `{addr_str}`: binding `{host}` exposes the server \
         beyond loopback (only 127.0.0.0/8, ::1 and localhost are allowed by default). \
         Re-run with `--allow net={host}` (or `--allow net=*` / `--allow-all`), \
         or listen on `127.0.0.1:<port>`."
    )))
}

/// Host part of a listen address: `"0.0.0.0:8080"` → `"0.0.0.0"`,
/// `"[::1]:8080"` → `"::1"`, `"localhost"` → `"localhost"`.
fn listen_host(addr_str: &str) -> String {
    use std::net::{IpAddr, SocketAddr};
    let addr = addr_str.trim();
    if let Ok(sa) = addr.parse::<SocketAddr>() {
        return sa.ip().to_string();
    }
    if let Ok(ip) = addr.parse::<IpAddr>() {
        return ip.to_string();
    }
    if let Some(rest) = addr.strip_prefix('[') {
        // Bracketed IPv6 that failed to parse as a SocketAddr (e.g. bad port).
        if let Some((host, _port)) = rest.split_once("]:") {
            return host.to_string();
        }
        if let Some(host) = rest.strip_suffix(']') {
            return host.to_string();
        }
    }
    match addr.rsplit_once(':') {
        Some((host, _port)) => host.to_string(),
        None => addr.to_string(),
    }
}

/// Loopback = 127.0.0.0/8, `::1`, or the literal name `localhost`.
///
/// Any other name is *not* resolved: a DNS answer is attacker-influenced and
/// resolution failures would have to fail open or block the bind, so an
/// unrecognised name simply needs `--allow net=<name>`.
fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Decode one `application/x-www-form-urlencoded` component: `+` → space,
/// `%XX` → byte. A malformed escape is kept verbatim (`%zz` stays `%zz`) rather
/// than dropping input, and the decoded bytes are read as UTF-8 lossily.
fn form_urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi << 4) | lo);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn normalize_path(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' {
            out.push('{');
            while let Some(&c2) = chars.peek() {
                if c2.is_alphanumeric() || c2 == '_' {
                    out.push(c2);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push('}');
        } else {
            out.push(c);
        }
    }
    if out.is_empty() {
        "/".into()
    } else {
        out
    }
}

fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let p_parts: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if p_parts.len() != path_parts.len() {
        return None;
    }
    let mut params = HashMap::new();
    for (p, a) in p_parts.iter().zip(path_parts.iter()) {
        if let Some(name) = p.strip_prefix(':') {
            params.insert(name.to_string(), a.to_string());
        } else if let Some(name) = p.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            params.insert(name.to_string(), a.to_string());
        } else if *p != *a {
            return None;
        }
    }
    Some(params)
}

static PENDING_SERVER: Mutex<Option<ServerState>> = Mutex::new(None);
static PENDING_ADDR: Mutex<Option<String>> = Mutex::new(None);
/// Most recent successfully bound listen address (for tests / tooling).
static LAST_BOUND_ADDR: Mutex<Option<String>> = Mutex::new(None);

/// Address of the last `@http.listen` bind (e.g. `"127.0.0.1:54321"`).
pub fn last_bound_addr() -> Option<String> {
    LAST_BOUND_ADDR.lock().clone()
}

/// Clear the last-bound address (tests).
pub fn clear_last_bound_addr() {
    *LAST_BOUND_ADDR.lock() = None;
}

pub fn set_pending_server(addr: String, state: ServerState) {
    *LAST_MIDDLEWARE.lock() = state
        .middleware
        .iter()
        .map(|m| match m {
            HttpMiddleware::Named(n) => n.clone(),
            HttpMiddleware::Function(_) => "<custom>".into(),
        })
        .collect();
    *PENDING_ADDR.lock() = Some(addr);
    *PENDING_SERVER.lock() = Some(state);
}

fn take_pending_server() -> Option<ServerState> {
    PENDING_SERVER.lock().take()
}

impl Default for HttpCap {
    fn default() -> Self {
        Self::new()
    }
}
