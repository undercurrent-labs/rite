//! Real Axum/Hyper-backed HTTP with Rite route handler evaluation.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use axum::body::Body;
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use indexmap::IndexMap;
use parking_lot::Mutex;
use rite_runtime::{
    EvalError, Evaluator, FunctionEntry, HostHandle, HttpMiddleware, Key, RuntimeContext, Value,
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
    /// Module scope captured when `@http.listen` ran, so handlers can see
    /// top-level bindings (`config ← …`) and not just function names. Cloning an
    /// `Environment` shares its frames, so this is a handful of `Arc` bumps —
    /// and it means module state persists across requests, which is what a
    /// server with a top-level cache or counter should do.
    pub module_env: rite_runtime::Environment,
}

/// Build a server from what the evaluator staged on the context.
///
/// This lived in `rite-caps::lib` behind a function-pointer registrar that
/// `rite-runtime` called through a `OnceLock`, writing the result into two more
/// globals for this capability to pick up. The capability can just read it.
fn server_state_from(
    pending: &rite_runtime::PendingHttpServer,
    perms: &PermissionSet,
    ctx: &RuntimeContext,
) -> ServerState {
    ServerState {
        routes: pending
            .routes
            .iter()
            .map(|r| RiteRoute {
                method: r.method.clone(),
                path: r.path.clone(),
                param_name: Some("req".into()),
                body: r.body.clone(),
            })
            .collect(),
        functions: ctx.functions.clone(),
        perms: perms.clone(),
        middleware: pending
            .middleware
            .iter()
            .map(|m| match m {
                // `use @http.log` reaches here as the full path; the dispatcher
                // matches on the bare name.
                HttpMiddleware::Named(s) => {
                    let s = s.trim_start_matches('@');
                    HttpMiddleware::Named(s.strip_prefix("http.").unwrap_or(s).to_string())
                }
                other => other.clone(),
            })
            .collect(),
        lock: Arc::new(AsyncMutex::new(())),
        // Module scope as it stood when `@http.listen` ran, so handlers resolve the
        // top-level bindings and functions they were written next to.
        module_env: ctx.env.clone(),
    }
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
            name: "get",
            docs: "GET a URL. Returns ok(response) or err(record). Needs --allow net=<host>.",
            arity: 1,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "post",
            docs: "POST a body to a URL. A record body is sent as JSON, a string as-is. Needs --allow net=<host>.",
            arity: 2,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "request",
            docs: "Send a request described by a record: <<method, url, headers, body, timeout_ms>>. Needs --allow net=<host>.",
            arity: 1,
            effectful: true,
            permission: "net",
        },
        NativeFunctionDescriptor {
            name: "response",
            docs: "Build an explicit response record `⟨status, body, headers⟩`. Body and headers are optional; the body defaults to `none` and an explicit `content-type` header overrides the one inferred from the body.",
            arity: 3,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "file",
            docs: "Read a file under `root` and build a response for it, with `content-type` from the extension. The subpath cannot escape `root`. A directory resolves to its `index.html`. Returns ok(response) or err(record). Needs `--allow fs:read=<root>`.",
            arity: 2,
            effectful: true,
            permission: "fs",
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
                // The route bodies and middleware closures arrive on the context, set by
                // the evaluator just before this call — they are IR and closures, so they
                // cannot be `Value` arguments. `listen_legacy` covers the older
                // `listen(addr, routes)` form, which has no route bodies at all.
                match ctx.pending_http.clone() {
                    Some(pending) => {
                        let state = server_state_from(&pending, perms, ctx);
                        *LAST_MIDDLEWARE.lock() = state
                            .middleware
                            .iter()
                            .map(|m| match m {
                                HttpMiddleware::Named(n) => n.clone(),
                                HttpMiddleware::Function(_) => "<custom>".into(),
                            })
                            .collect();
                        self.serve(pending.addr.clone(), state, perms).await
                    }
                    None => self.listen_legacy(args, perms, ctx).await,
                }
            }
            "get" => {
                let url = string_arg(&args, 0, "http.get expects a url string")?;
                self.fetch("GET", &url, None, &Value::None, perms, ctx)
                    .await
            }
            "post" => {
                let url = string_arg(&args, 0, "http.post expects a url string")?;
                let body = args.get(1).cloned().unwrap_or(Value::None);
                self.fetch("POST", &url, None, &body, perms, ctx).await
            }
            "request" => {
                let Some(Value::Record(spec)) = args.first() else {
                    return Err(EvalError::Message(
                        "http.request expects a record: ⟨method, url, headers?, body?⟩".into(),
                    ));
                };
                let field = |k: &str| -> Option<Value> {
                    spec.get(&Key::String(k.into()))
                        .or_else(|| spec.get(&Key::Atom(k.into())))
                        .cloned()
                };
                let url = field("url")
                    .as_ref()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .ok_or_else(|| EvalError::Message("http.request needs a `url` field".into()))?;
                let method = field("method")
                    .as_ref()
                    .and_then(|v| v.as_str().map(str::to_uppercase))
                    .unwrap_or_else(|| "GET".into());
                let headers = field("headers");
                let body = field("body").unwrap_or(Value::None);
                self.fetch(&method, &url, headers.as_ref(), &body, perms, ctx)
                    .await
            }
            "response" => {
                let status = args.first().and_then(|v| v.as_int()).unwrap_or(200);
                let mut fields = vec![
                    (Key::String("status".into()), Value::Int(status)),
                    (
                        Key::String("body".into()),
                        args.get(1).cloned().unwrap_or(Value::None),
                    ),
                ];
                // Only when asked for. A `headers: none` on every response would
                // change what two-argument calls print, and the record is a value
                // scripts pass around and compare, not just something we hand back
                // to the server.
                if let Some(headers) = args.get(2) {
                    if !matches!(headers, Value::None) {
                        fields.push((Key::String("headers".into()), headers.clone()));
                    }
                }
                Ok(Value::record(fields))
            }
            "file" => {
                let root = string_arg(&args, 0, "http.file expects a root directory string")?;
                // A missing or `none` subpath means the root itself, which resolves
                // to its index — `@http.file("public", req.path.rest)` with an empty
                // catch-all is exactly the request for `/`.
                let sub = match args.get(1) {
                    Some(Value::None) | None => String::new(),
                    Some(v) => v.as_str().map(str::to_string).unwrap_or_default(),
                };
                serve_file(&root, &sub, perms)
            }
            // Middleware identifiers are resolved at listen-time via `use` / `⊏`.
            // Calling them as values is a no-op document of the plug-in.
            "log" | "recover" => Ok(Value::Atom(ctx.atoms.intern(&format!("http.{}", method)))),
            other => Err(EvalError::Capability(format!("unknown @http.{}", other))),
        }
    }

    /// Perform an outbound request.
    ///
    /// The response mirrors the shape a *handler* receives, so the two directions of
    /// HTTP read the same way: `status`, `headers` (lowercased), `text` and `json` as
    /// results, and `body` as bytes. The call itself returns `ok(response)` or
    /// `err(⟨kind, message⟩)`, like `@fs.read`, so `? ` unwraps it.
    #[cfg(not(target_arch = "wasm32"))]
    async fn fetch(
        &self,
        method: &str,
        url: &str,
        headers: Option<&Value>,
        body: &Value,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        // Permission is per *host*, matching `--allow net=api.example.com`.
        let host = host_of(url)
            .ok_or_else(|| EvalError::Message(format!("http: cannot parse url `{url}`")))?;
        perms.check_net(&host).map_err(EvalError::Permission)?;

        let verb = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| EvalError::Message(format!("http: bad method `{method}`")))?;
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(e) => return Ok(net_err("http.client", url, &e.to_string())),
        };
        let mut req = client.request(verb, url);

        if let Some(Value::Record(hs)) = headers {
            for (k, v) in hs {
                let name = match k {
                    Key::String(s) => s.clone(),
                    Key::Atom(a) => a.clone(),
                    other => format!("{other:?}"),
                };
                if let Some(val) = v.as_str() {
                    req = req.header(name, val);
                }
            }
        }
        // A record body is JSON; a string is sent verbatim. Anything else is displayed.
        req = match body {
            Value::None => req,
            Value::String(s) => req.body(s.to_string()),
            Value::Record(_) | Value::List(_) => req
                .header("content-type", "application/json")
                .body(serde_json::to_string(&body.to_json(&ctx.atoms)).unwrap_or_default()),
            other => req.body(format!("{other}")),
        };

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return Ok(net_err("http.request", url, &e.to_string())),
        };
        let status = resp.status().as_u16() as i64;
        let mut header_rec = IndexMap::new();
        for (name, value) in resp.headers().iter() {
            if let Ok(v) = value.to_str() {
                // First value wins for repeated headers, as on the server side.
                header_rec
                    .entry(Key::String(name.as_str().to_ascii_lowercase()))
                    .or_insert_with(|| Value::string(v));
            }
        }
        let bytes = match resp.bytes().await {
            Ok(b) => b.to_vec(),
            Err(e) => return Ok(net_err("http.body", url, &e.to_string())),
        };
        let text = String::from_utf8_lossy(&bytes).to_string();
        let json = match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(j) => Value::ok(Value::from_json(&j)),
            Err(e) => Value::err(Value::string(e.to_string())),
        };
        Ok(Value::ok(Value::record(vec![
            (Key::String("status".into()), Value::Int(status)),
            (Key::String("headers".into()), Value::Record(header_rec)),
            (Key::String("text".into()), Value::ok(Value::string(text))),
            (Key::String("json".into()), json),
            (Key::String("body".into()), Value::Bytes(bytes.into())),
        ])))
    }

    /// No socket layer in the browser host — the same answer `@db` gives.
    #[cfg(target_arch = "wasm32")]
    async fn fetch(
        &self,
        _method: &str,
        _url: &str,
        _headers: Option<&Value>,
        _body: &Value,
        _perms: &PermissionSet,
        _ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        Err(EvalError::Capability(
            "outbound @http requires the native host (not available in WASM)".into(),
        ))
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
            module_env: ctx.env.clone(),
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
        let stop = Arc::new(arm_server_stop(shutdown_tx));
        let _ = ctrlc::set_handler(request_stop);

        // For tests: auto-stop after a grace window when RITE_HTTP_TEST=1 so
        // servers don't hang forever if the test task is aborted late. This one
        // stops *this* server only — one server's grace window expiring is no
        // reason to take down another that is still under test.
        let test_mode = std::env::var("RITE_HTTP_TEST").ok().as_deref() == Some("1");
        if test_mode {
            let secs: u64 = std::env::var("RITE_HTTP_TEST_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);
            let stop = stop.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                stop.stop();
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

        // This server is done accepting; drop its switch rather than leaving a
        // sender in the registry for a loop that has already left.
        stop.release();

        // A handler that called `@process.exit` ends the script, not just its own
        // request: `listen` fails with the chosen status instead of returning the
        // stopped record, and it propagates from here like any other exit.
        if let Some(code) = take_exit_request() {
            return Err(EvalError::Exit(code));
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
    // Two passes, specific before catch-all. Declaration order decides between two
    // routes of the same kind, but a catch-all never shadows an exact route it was
    // merely written above — which is what lets an SPA's `GET "/*path"` sit at the
    // top of the block with its API routes below it and still work.
    let matches = |route: &RiteRoute| {
        match_path(&route.path, &path).is_some()
            || normalize_path(&route.path) == path
            || route.path == path
    };
    for catch_all in [false, true] {
        for (idx, route) in state.routes.iter().enumerate() {
            if catch_all_name(&route.path).is_some() != catch_all {
                continue;
            }
            if !route.method.eq_ignore_ascii_case(method.as_str()) {
                continue;
            }
            if matches(route) {
                return dispatch_rite(state.clone(), idx, req).await;
            }
        }
    }

    // The path exists, the method does not. Reporting that as 404 tells a client
    // the resource is absent when it is right there — and costs it the `Allow`
    // header that says which methods would have worked.
    let allowed: Vec<String> = state
        .routes
        .iter()
        .filter(|r| matches(r))
        .map(|r| r.method.to_ascii_uppercase())
        .collect();
    if !allowed.is_empty() {
        let mut seen: Vec<String> = Vec::new();
        for m in allowed {
            if !seen.contains(&m) {
                seen.push(m);
            }
        }
        let allow = seen.join(", ");
        let mut resp = (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"error": "method_not_allowed", "allow": seen})),
        )
            .into_response();
        if let Ok(v) = HeaderValue::try_from(allow) {
            resp.headers_mut().insert(axum::http::header::ALLOW, v);
        }
        return resp;
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

    let form = form_value(&headers, &body_text);

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
        (Key::String("form".into()), form),
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
    install_module_scope(&mut ctx, &state.module_env, &state.functions);

    let param = route.param_name.clone().unwrap_or_else(|| "req".into());
    let result = run_middleware_chain(
        &mut ctx,
        Chain {
            customs: &customs,
            handler_body: &route.body,
            handler_param: &param,
            perms: state.perms.clone(),
            conts: new_continuations(),
        },
        0,
        req_value,
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
        // Ahead of the `recover` arms on purpose: `use @http.recover` turns a
        // handler *failure* into a described 500, and an exit is not a failure —
        // recovering it would let middleware quietly veto ending the process.
        // The client in flight gets 503, which is the truth: this server is going
        // away. Every other in-flight request is still served to completion.
        Err(EvalError::Exit(code)) => {
            request_exit(code);
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
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

/// Monotonic ids for `http.next` handles. Global on purpose: uniqueness is the whole
/// requirement, and a per-request counter would hand out colliding ids.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Run custom middleware outer→inner, then the route handler.
/// Each layer receives `(req, next)` where `next` is a callable `http.next` handle.
/// Everything about one request's middleware chain that does not change as it is
/// walked. Grouped so the recursive step takes the context, the position and the
/// request, rather than eight separate parameters.
struct Chain<'a> {
    customs: &'a [rite_runtime::Closure],
    handler_body: &'a BlockIr,
    handler_param: &'a str,
    perms: PermissionSet,
    /// This request's `next` continuations — see [`Continuations`].
    conts: Continuations,
}

fn run_middleware_chain<'a>(
    ctx: &'a mut RuntimeContext,
    chain: Chain<'a>,
    index: usize,
    req_value: Value,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, EvalError>> + Send + 'a>> {
    Box::pin(async move {
        let Chain {
            customs,
            handler_body,
            handler_param,
            perms,
            conts,
        } = chain;
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
                // Already carries the module scope installed for this request, so a
                // handler reached through `next()` resolves the same names as one
                // reached directly.
                module_env: ctx.env.clone(),
            };
            conts.lock().insert(next_id, cont);
        }

        // Resolve `next` handles against *this* request's continuations.
        ctx.http_next = Some(next_invoker(conts.clone()));

        let next_val = Value::Handle(HostHandle {
            kind: "http.next".into(),
            id: next_id,
        });

        let mut eval = Evaluator::new(ctx);
        let result = eval
            .call_value_public(Value::Function(mw), vec![req_value, next_val])
            .await;

        if index == 0 {
            // The outermost layer owns the chain: once it returns, no `next` handle from
            // this request is callable again.
            ctx.http_next = None;
            conts.lock().clear();
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
    module_env: rite_runtime::Environment,
}

/// Give a per-request context the module scope captured at listen time.
///
/// Handlers run in a fresh `RuntimeContext` (own console buffer, own capability
/// handles), but they must still resolve the names the script defined at the top
/// level. Installing the captured environment supplies both top-level bindings and
/// the function closures the module already bound, so a handler body sees exactly
/// what it saw lexically. `ctx.functions` is still populated because the call path
/// consults it; a function only gets a synthetic closure here if the module scope
/// somehow lacks one (a `legacy listen` route table has no module env at all).
///
/// Shared with `@tcp.listen`, whose per-connection handler has exactly the same
/// requirement: a fresh context that still resolves the names the script defined.
pub(crate) fn install_module_scope(
    ctx: &mut RuntimeContext,
    module_env: &rite_runtime::Environment,
    functions: &HashMap<String, FunctionEntry>,
) {
    ctx.env = module_env.clone();
    for (name, f) in functions {
        ctx.functions.insert(name.clone(), f.clone());
        if ctx.env.get(name).is_none() {
            let clos = Value::Function(rite_runtime::Closure {
                id: 0,
                name: Some(name.as_str().into()),
                params: f.params.clone(),
                env: Arc::new(parking_lot::RwLock::new(module_env.clone())),
                body: f.body.clone(),
                // A handler rebuilt from a route record carries no declaration to enforce.
                contract: None,
            });
            ctx.env.define_name(name, clos, false);
        }
    }
}

/// One request's outstanding `next` continuations, owned by the invoker installed on
/// that request's context.
///
/// This was a process-global `OnceLock<Mutex<HashMap<..>>>` paired with a global
/// invoker slot, so every concurrent request shared one map and one hook: the last
/// chain to start overwrote the invoker, and clearing the map at the end of one chain
/// invalidated handles belonging to another.
type Continuations = Arc<Mutex<HashMap<u64, NextContinuation>>>;

fn new_continuations() -> Continuations {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Build the `next(req)` callback for one request over its own continuation map.
fn next_invoker(conts: Continuations) -> rite_runtime::HttpNextInvoker {
    Arc::new(move |id, args| {
        let conts = conts.clone();
        Box::pin(async move {
            let cont = conts
                .lock()
                .remove(&id)
                .ok_or_else(|| EvalError::Message("middleware next() already used".into()))?;
            let req = args.into_iter().next().unwrap_or(Value::None);
            let mut inner = RuntimeContext::new();
            crate::install_defaults(&mut inner, cont.perms.clone());
            inner.allow_all = cont.perms.allow_all;
            install_module_scope(&mut inner, &cont.module_env, &cont.functions);
            let result = run_middleware_chain(
                &mut inner,
                Chain {
                    customs: &cont.customs,
                    handler_body: &cont.handler_body,
                    handler_param: &cont.handler_param,
                    perms: cont.perms.clone(),
                    conts: conts.clone(),
                },
                cont.index,
                req,
            )
            .await;
            flush_handler_io(&inner);
            result
        })
    })
}

/// Emit buffered handler stdout/stderr to the real process streams (and optional test sink).
///
/// Shared with `@tcp.listen`, so a connection handler's `! @console.println` reaches
/// the server process — and the test capture — the same way a request handler's does.
pub(crate) fn flush_handler_io(ctx: &RuntimeContext) {
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

pub(crate) fn emit_process_stderr(s: &str) {
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

// The three statics below are deliberately global, and are the reason the `http_*`
// test files serialize on their own lock. `@http.listen` *blocks until shutdown*, so a
// test cannot read anything back from its return value — the only way to observe which
// port it bound, what middleware it registered, or what its handlers printed is a side
// channel the test can read while the server runs. They are observation only: nothing in
// a request path reads them, so two servers sharing them cannot affect each other's
// behaviour, only each other's test observations.
//
// Removing them means giving `listen` a non-blocking form that returns a handle. That is
// a language-surface change, not a refactor.
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
                // `^ 200 body headers` — the juxta form's third slot, so the terse
                // return is not the one shape that cannot set a content type.
                let headers = xs.get(2).cloned();
                return status_body(status as u16, body, headers.as_ref(), ctx);
            }
            json_response(200, &Value::List(xs), ctx)
        }
        Value::Record(ref r) => {
            if let Some(Value::Int(status)) = r.get(&Key::String("status".into())) {
                let headers = r.get(&Key::String("headers".into())).cloned();
                if r.contains_key(&Key::String("body".into())) {
                    let body = r
                        .get(&Key::String("body".into()))
                        .cloned()
                        .unwrap_or(Value::None);
                    return status_body(*status as u16, body, headers.as_ref(), ctx);
                }
                // `headers` present without `body` still means "this record is the
                // response envelope", so the header record must not be serialized
                // into the payload describing itself. Whatever else is on the record
                // is the body, exactly as it would be without the envelope fields.
                if headers.is_some() {
                    let rest: IndexMap<Key, Value> = r
                        .iter()
                        .filter(|(k, _)| {
                            let n = k.as_str();
                            n != "status" && n != "headers"
                        })
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let body = if rest.is_empty() {
                        Value::None
                    } else {
                        Value::Record(rest)
                    };
                    return status_body(*status as u16, body, headers.as_ref(), ctx);
                }
                if r.len() > 1 {
                    return status_body(*status as u16, Value::Record(r.clone()), None, ctx);
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

fn status_body(
    status: u16,
    body: Value,
    headers: Option<&Value>,
    ctx: &RuntimeContext,
) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let mut resp = match body {
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
    };
    apply_headers(&mut resp, headers, ctx);
    resp
}

/// Merge a handler's `headers` record onto the response.
///
/// An explicit `content-type` **replaces** the one inferred from the body's type.
/// That is the whole point of the field: the type of the Rust value is a poor
/// proxy for the media type, and without an override a string body is always
/// `text/plain`, which makes a browser render HTML as source text.
///
/// A string value sets the header once; a **list** value emits the name once per
/// element. The list form exists because a record holds one value per key, and
/// `set-cookie` is the header that genuinely needs to repeat.
///
/// A name or value that is not valid in HTTP is skipped rather than failing the
/// request: a handler returning one bad header should not turn a 200 into a 500.
fn apply_headers(resp: &mut Response, headers: Option<&Value>, ctx: &RuntimeContext) {
    let Some(Value::Record(hs)) = headers else {
        return;
    };
    for (k, v) in hs.iter() {
        let Ok(name) = HeaderName::try_from(k.as_str().to_ascii_lowercase()) else {
            continue;
        };
        let values: Vec<String> = match v {
            Value::List(xs) => xs.iter().map(|x| header_text(x, ctx)).collect(),
            other => vec![header_text(other, ctx)],
        };
        let mut first = true;
        for raw in values {
            let Ok(value) = HeaderValue::try_from(raw) else {
                continue;
            };
            if first {
                // Insert, not append: an explicit header replaces the inferred one.
                resp.headers_mut().insert(name.clone(), value);
                first = false;
            } else {
                resp.headers_mut().append(name.clone(), value);
            }
        }
    }
}

/// A header value as HTTP sees it. An atom is its bare name — `#no_cache` is a
/// header value spelling, not the `#`-prefixed display form.
fn header_text(v: &Value, ctx: &RuntimeContext) -> String {
    match v {
        Value::String(s) => s.to_string(),
        Value::Atom(id) => ctx.atoms.name(*id).to_string(),
        other => other.to_display(&ctx.atoms),
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
    check_bind_perm(addr_str, perms, "listen")
}

/// The same policy, for any capability that binds a socket — `@http.listen` and
/// `@udp.bind`. `op` names the operation in the message and nothing else: a second
/// copy of the loopback rules is exactly how the two would drift apart.
pub fn check_bind_perm(addr_str: &str, perms: &PermissionSet, op: &str) -> Result<(), EvalError> {
    if perms.allow_all {
        return Ok(());
    }
    let host = addr_host(addr_str);
    if is_loopback_host(&host) {
        return Ok(());
    }
    // `--allow net=*`, `--allow net=0.0.0.0`, `--allow net=192.168.1.10`, or the
    // whole `host:port` spelled out.
    if perms.check_net(&host).is_ok() || perms.check_net(addr_str).is_ok() {
        return Ok(());
    }
    Err(EvalError::Permission(format!(
        "net permission denied for {op} on `{addr_str}`: binding `{host}` exposes it \
         beyond loopback (only 127.0.0.0/8, ::1 and localhost are allowed by default). \
         Re-run with `--allow net={host}` (or `--allow net=*` / `--allow-all`), \
         or bind `127.0.0.1:<port>`."
    )))
}

/// Host part of a socket address: `"0.0.0.0:8080"` → `"0.0.0.0"`,
/// `"[::1]:8080"` → `"::1"`, `"localhost"` → `"localhost"`.
///
/// Shared with `@udp`, which matches a datagram destination against `net` grants
/// the same way. Note the difference from [`host_of`], which takes a *URL*: this
/// one unwraps IPv6 brackets, because `--allow net=::1` is spelled without them.
pub(crate) fn addr_host(addr_str: &str) -> String {
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
/// `req.form` — an `application/x-www-form-urlencoded` body as a record.
///
/// A result like `req.json`, and for the same reason: "there was no form here" is
/// an ordinary thing for a handler to branch on. The content type is what decides,
/// not the shape of the bytes — a JSON body happens to parse as a single
/// key-with-no-value pair, and silently handing that back as a form would be worse
/// than saying no.
///
/// Decoding matches the query string exactly (`+` is a space, `%xx` is a byte,
/// last value wins for a repeated key), because they are the same encoding.
fn form_value(headers: &axum::http::HeaderMap, body: &str) -> Value {
    let is_form = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| {
            ct.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
        .unwrap_or(false);
    if !is_form {
        return Value::err(Value::string("not a form body"));
    }
    let mut rec: IndexMap<Key, Value> = IndexMap::new();
    for pair in body.split('&').filter(|p| !p.is_empty()) {
        let (raw_key, raw_val) = pair.split_once('=').unwrap_or((pair, ""));
        rec.insert(
            Key::String(form_urldecode(raw_key)),
            Value::string(form_urldecode(raw_val)),
        );
    }
    Value::ok(Value::Record(rec))
}

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

/// Host portion of a URL, without scheme, credentials, port or path.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // Keep IPv6 literals intact; strip a trailing :port otherwise.
    let host = if authority.starts_with('[') {
        authority.split_once(']').map(|(h, _)| format!("{h}]"))?
    } else {
        authority.split(':').next()?.to_string()
    };
    (!host.is_empty()).then_some(host)
}

fn string_arg(args: &[Value], i: usize, msg: &str) -> Result<String, EvalError> {
    args.get(i)
        .and_then(|v| v.as_str().map(str::to_string))
        .ok_or_else(|| EvalError::Message(msg.into()))
}

/// Transport failures are values, not aborts — same shape as `@fs` io errors.
fn net_err(op: &str, url: &str, message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("net.error")),
        (Key::String("operation".into()), Value::string(op)),
        (Key::String("url".into()), Value::string(url)),
        (Key::String("message".into()), Value::string(message)),
    ]))
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

/// Does this pattern end in a `*rest` catch-all?
///
/// Only the last segment counts: `/a/*rest/b` has nothing to capture the tail
/// with, so it is an ordinary literal segment that will simply never match.
fn catch_all_name(pattern: &str) -> Option<&str> {
    pattern
        .trim_matches('/')
        .rsplit('/')
        .next()
        .and_then(|seg| seg.strip_prefix('*'))
}

fn match_path(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let p_parts: Vec<&str> = pattern.trim_matches('/').split('/').collect();
    let path_parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    // A catch-all consumes every remaining segment, so it matches a path that is
    // *at least* as long as the fixed prefix — and also one exactly as long as the
    // prefix, capturing the empty remainder, which is what makes `/assets/*rest`
    // answer a bare `/assets`.
    let tail = p_parts.last().and_then(|s| s.strip_prefix('*'));
    match tail {
        Some(_) if path_parts.len() + 1 < p_parts.len() => return None,
        None if p_parts.len() != path_parts.len() => return None,
        _ => {}
    }
    let fixed = if tail.is_some() {
        p_parts.len() - 1
    } else {
        p_parts.len()
    };
    let mut params = HashMap::new();
    for (p, a) in p_parts.iter().take(fixed).zip(path_parts.iter()) {
        if let Some(name) = p.strip_prefix(':') {
            params.insert(name.to_string(), a.to_string());
        } else if let Some(name) = p.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            params.insert(name.to_string(), a.to_string());
        } else if *p != *a {
            return None;
        }
    }
    if let Some(name) = tail {
        // Unnamed (`*`) is a pure wildcard — it matches without binding anything,
        // which is the fallback-route spelling.
        if !name.is_empty() {
            params.insert(name.to_string(), path_parts[fixed..].join("/"));
        }
    }
    Some(params)
}

/// Read `sub` beneath `root` and build a response record for it.
///
/// Two separate gates, and both are load-bearing:
///
/// 1. **Containment.** The joined path is resolved and must still sit under the
///    resolved root, so `../../etc/passwd` — or a symlink pointing out of the tree —
///    is refused. This is checked *before* the file is opened, and the refusal is
///    `http.forbidden` rather than a not-found, because a traversal attempt is not
///    the same event as a missing asset and a server should be able to log it.
/// 2. **Permission.** The resolved path still goes through `check_fs_read`, so
///    serving a directory needs `--allow fs:read=<root>` like any other read.
///    Containment alone would let `@http.file` read anything under the CWD, which
///    is precisely the authority the permission system exists to withhold.
///
/// A directory resolves to its `index.html`, which is what makes `GET "/"` and an
/// SPA's client-routed deep links work without a special case in the handler.
fn serve_file(root: &str, sub: &str, perms: &PermissionSet) -> Result<Value, EvalError> {
    let root_path = std::path::Path::new(root);
    let root_canon = crate::permissions::canonicalize_loose(root_path);

    // Reject the traversal on the *lexical* path first: joining a subpath that
    // climbs out and then canonicalizing would resolve somewhere real, and the
    // containment check below would catch it — but only after the join has already
    // named a file outside the tree. Refusing `..` outright keeps the intent clear.
    if sub.split('/').any(|seg| seg == "..") {
        return Ok(file_err(
            "http.forbidden",
            sub,
            "path escapes the served root",
        ));
    }

    let joined = root_path.join(sub.trim_start_matches('/'));
    let canon = crate::permissions::canonicalize_loose(&joined);
    if !canon.starts_with(&root_canon) {
        return Ok(file_err(
            "http.forbidden",
            sub,
            "path escapes the served root",
        ));
    }

    let target = if canon.is_dir() {
        canon.join("index.html")
    } else {
        canon
    };

    // Permission errors abort the handler rather than becoming a value, matching
    // `@fs.read`: a missing grant is a bug in how the program was launched, not a
    // condition the script is expected to branch on.
    let target = perms
        .check_fs_read(&target)
        .map_err(EvalError::Permission)?;

    match std::fs::read(&target) {
        Ok(bytes) => Ok(Value::ok(Value::record(vec![
            (Key::String("status".into()), Value::Int(200)),
            (
                Key::String("headers".into()),
                Value::record(vec![(
                    Key::String("content-type".into()),
                    Value::string(content_type_for(&target)),
                )]),
            ),
            (Key::String("body".into()), Value::Bytes(bytes.into())),
        ]))),
        Err(e) => {
            let kind = match e.kind() {
                std::io::ErrorKind::NotFound => "http.not_found",
                std::io::ErrorKind::PermissionDenied => "io.permission_denied",
                _ => "io.error",
            };
            Ok(file_err(kind, sub, &e.to_string()))
        }
    }
}

fn file_err(kind: &str, path: &str, message: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string(kind)),
        (Key::String("operation".into()), Value::string("http.file")),
        (Key::String("path".into()), Value::string(path)),
        (Key::String("message".into()), Value::string(message)),
    ]))
}

/// Media type from the file extension.
///
/// A deliberately small table rather than a mime database: it covers what a static
/// site or a built SPA bundle actually ships, and an unknown extension falls back
/// to `application/octet-stream` — a download prompt, never a guess that lets a
/// browser sniff an upload as script. Text formats carry `charset=utf-8` because
/// Rite source and its output are UTF-8 throughout.
fn content_type_for(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json; charset=utf-8",
        "txt" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

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

/// Stop switches for the servers currently accepting, by id.
///
/// A registry rather than a single slot because a process can be running several
/// at once — `@http` and `@tcp` share this, and the isolation tests stand two HTTP
/// servers up side by side. A single slot made starting the second server drop the
/// first one's sender, which its accept loop reads as "shut down", so server two
/// silently killed server one.
static SERVER_STOPS: Mutex<Vec<(u64, oneshot::Sender<()>)>> = Mutex::new(Vec::new());
static NEXT_SERVER_ID: AtomicU64 = AtomicU64::new(1);

/// Handle for one armed server: stops it, and takes it out of the registry.
pub(crate) struct ServerStop(u64);

/// The status a handler asked to exit with, if one did.
///
/// Kept apart from the stop switch because stopping is not by itself an exit:
/// Ctrl-C leaves this `None` and `listen` returns its usual `⟨addr, status⟩`,
/// while a handler's `@process.exit` makes `listen` fail with `Exit`, so the
/// status leaves the server the same way it would from any other call site —
/// and an embedder still gets a value rather than a dead process.
static EXIT_REQUEST: Mutex<Option<u8>> = Mutex::new(None);

/// Arm the stop switch for a server about to start accepting.
///
/// Clears any exit status left behind when this is the only live server — several
/// run in sequence in one test binary, and a stale status would end the next one.
pub(crate) fn arm_server_stop(tx: oneshot::Sender<()>) -> ServerStop {
    let id = NEXT_SERVER_ID.fetch_add(1, Ordering::Relaxed);
    let mut stops = SERVER_STOPS.lock();
    if stops.is_empty() {
        *EXIT_REQUEST.lock() = None;
    }
    stops.push((id, tx));
    ServerStop(id)
}

impl ServerStop {
    /// Stop just this server: what the test-mode auto-stop timer wants, since one
    /// server's grace window expiring says nothing about another's.
    pub(crate) fn stop(&self) {
        let mut stops = SERVER_STOPS.lock();
        if let Some(i) = stops.iter().position(|(id, _)| *id == self.0) {
            let (_, tx) = stops.remove(i);
            let _ = tx.send(());
        }
    }

    /// Drop this server's switch without firing it — for a loop that has already
    /// left, so the registry does not accumulate senders for dead servers.
    ///
    /// Takes `&self` and is idempotent: the auto-stop task holds a clone of the
    /// handle, and a switch that could only be released by consuming the last
    /// reference would simply never be released while that task was asleep.
    pub(crate) fn release(&self) {
        SERVER_STOPS.lock().retain(|(id, _)| *id != self.0);
    }
}

/// Stop every running server. Ctrl-C means the whole process, and so does a
/// handler calling `@process.exit`.
///
/// Idempotent: senders are taken as they fire, so a second Ctrl-C is a no-op.
pub(crate) fn request_stop() {
    for (_, tx) in std::mem::take(&mut *SERVER_STOPS.lock()) {
        let _ = tx.send(());
    }
}

/// Record a handler's chosen exit status and stop the server.
///
/// First exit wins: two in-flight handlers can call `@process.exit` with
/// different statuses, and the process has only one to give.
pub(crate) fn request_exit(code: u8) {
    EXIT_REQUEST.lock().get_or_insert(code);
    request_stop();
}

/// Take the exit status a handler asked for, if any. Called once, after the
/// accept loop ends.
pub(crate) fn take_exit_request() -> Option<u8> {
    EXIT_REQUEST.lock().take()
}

impl Default for HttpCap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod url_tests {
    use super::host_of;

    /// The permission check keys on this, so a wrong answer either blocks a legitimate
    /// call or — worse — grants one. Credentials and ports must not become part of it.
    #[test]
    fn host_is_extracted_without_scheme_port_path_or_credentials() {
        assert_eq!(
            host_of("http://example.com/path"),
            Some("example.com".into())
        );
        assert_eq!(host_of("https://example.com"), Some("example.com".into()));
        assert_eq!(
            host_of("https://api.example.com:8443/v1?q=1"),
            Some("api.example.com".into())
        );
        assert_eq!(
            host_of("http://127.0.0.1:18200/hello"),
            Some("127.0.0.1".into())
        );
        // `user:pass@` must not be mistaken for the host.
        assert_eq!(
            host_of("http://user:pw@internal.example/x"),
            Some("internal.example".into())
        );
        // A fragment or query directly after the authority still terminates it.
        assert_eq!(
            host_of("http://example.com#frag"),
            Some("example.com".into())
        );
        assert_eq!(
            host_of("http://example.com?q=1"),
            Some("example.com".into())
        );
        // Scheme-relative and bare hosts.
        assert_eq!(host_of("example.com/x"), Some("example.com".into()));
        // IPv6 literals keep their brackets so the port strip does not eat them.
        assert_eq!(host_of("http://[::1]:8080/x"), Some("[::1]".into()));
    }

    #[test]
    fn unparseable_urls_yield_no_host() {
        assert_eq!(host_of(""), None);
        assert_eq!(host_of("http://"), None);
        assert_eq!(host_of("http:///just-a-path"), None);
    }
}
