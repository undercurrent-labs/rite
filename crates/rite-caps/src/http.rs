//! Real Axum/Hyper-backed HTTP with Rite route handler evaluation.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use indexmap::IndexMap;
use parking_lot::Mutex;
use rite_runtime::{EvalError, Evaluator, FunctionEntry, Key, RuntimeContext, Value};
use rite_sem::BlockIr;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
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
            docs: "Start an HTTP server and block until shutdown.",
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
            "log" | "recover" => Ok(Value::None),
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
        *self.last_addr.lock() = Some(local.to_string());

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

        // For port 0 / tests: also auto-stop after a short window when RITE_HTTP_TEST=1
        let test_mode = std::env::var("RITE_HTTP_TEST").ok().as_deref() == Some("1");
        if test_mode {
            let stx = shutdown_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
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
            (
                Key::String("addr".into()),
                Value::string(local.to_string()),
            ),
            (Key::String("status".into()), Value::string("stopped")),
        ]))
    }
}

async fn dispatch_fallback(state: ServerState, req: Request<Body>) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    if method == Method::GET && (path == "/health" || path == "/health/") {
        if !state
            .routes
            .iter()
            .any(|r| r.path == "/health" || r.path == "health")
        {
            return Json(json!({"status": "ok"})).into_response();
        }
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
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "not_found"})),
    )
        .into_response()
}

async fn dispatch_rite(state: ServerState, idx: usize, req: Request<Body>) -> Response {
    let _guard = state.lock.lock().await;
    let route = match state.routes.get(idx) {
        Some(r) => r.clone(),
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "missing route").into_response();
        }
    };

    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let query: HashMap<String, String> = uri
        .query()
        .map(|q| {
            q.split('&')
                .filter_map(|pair| {
                    let mut it = pair.splitn(2, '=');
                    Some((
                        it.next()?.to_string(),
                        it.next().unwrap_or("").to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let path_params = match_path(&route.path, &path).unwrap_or_default();
    let body_bytes = axum::body::to_bytes(req.into_body(), 1_048_576)
        .await
        .unwrap_or_default();
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

    let req_value = Value::record(vec![
        (
            Key::String("method".into()),
            Value::string(method.as_str()),
        ),
        (Key::String("path".into()), Value::Record(path_rec)),
        (Key::String("query".into()), Value::Record(query_rec)),
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
    let mut eval = Evaluator::new(&mut ctx);
    let result = eval
        .call_block_public(&route.body, &[param], vec![req_value])
        .await;

    match result {
        Ok(v) => coerce_response(v, &ctx),
        Err(EvalError::Return(v)) => coerce_response(v, &ctx),
        Err(EvalError::Panic(m)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "panic", "message": m})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "handler_error", "message": e.to_string()})),
        )
            .into_response(),
    }
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
            [(
                axum::http::header::CONTENT_TYPE,
                "application/octet-stream",
            )],
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

fn check_listen_perm(addr_str: &str, perms: &PermissionSet) -> Result<(), EvalError> {
    if perms.allow_all
        || perms.net.contains("*")
        || perms.net.contains("127.0.0.1")
        || perms.net.contains("localhost")
        || addr_str.contains("127.0.0.1")
        || addr_str.contains("localhost")
        || addr_str.starts_with("0.0.0.0")
    {
        return Ok(());
    }
    perms.check_net("listen").map_err(EvalError::Permission)
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

pub fn set_pending_server(addr: String, state: ServerState) {
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
