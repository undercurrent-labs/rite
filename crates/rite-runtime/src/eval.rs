//! Async tree-walking evaluator over ProgramIr.

use crate::atom::AtomInterner;
use crate::budget::{BudgetError, ExecutionBudget};
use crate::builtins::{call_builtin, compare_values, list_remove_first, membership, merge_records};
use crate::env::Environment;
use crate::value::{Closure, Key, ResultValue, Value};
use async_trait::async_trait;
use indexmap::IndexMap;
use rite_core::{Diagnostics, SourceMap, Span};
use rite_sem::{
    BinaryOpIr, BlockIr, EffectKind, EntryPoint, ExprIr, KeyIr, PatternIr, ProgramIr,
    ResultPatKindIr, StageKind, UnaryOpIr, ValueLiteral,
};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Monotonic closure ids. Global on purpose — same reasoning as `NEXT_ID` in the HTTP
/// host: ids only have to be unique, and nothing reads this to make a decision.
static CLOSURE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub enum EvalError {
    Message(String),
    Panic(String),
    Compile(Diagnostics),
    Budget(BudgetError),
    Return(Value),
    Permission(String),
    Capability(String),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Message(m)
            | EvalError::Panic(m)
            | EvalError::Permission(m)
            | EvalError::Capability(m) => write!(f, "{}", m),
            EvalError::Compile(d) => write!(f, "compile error ({} diagnostics)", d.len()),
            EvalError::Budget(b) => write!(f, "{}", b),
            EvalError::Return(_) => write!(f, "return"),
        }
    }
}

impl std::error::Error for EvalError {}

impl EvalError {
    pub fn with_stack(self, ctx: &RuntimeContext) -> Self {
        let trace = ctx.format_stack_trace();
        if trace.is_empty() {
            return self;
        }
        match self {
            EvalError::Message(m) => EvalError::Message(format!("{}{}", m, trace)),
            EvalError::Panic(m) => EvalError::Panic(format!("{}{}", m, trace)),
            EvalError::Capability(m) => EvalError::Capability(format!("{}{}", m, trace)),
            other => other,
        }
    }
}

impl From<BudgetError> for EvalError {
    fn from(b: BudgetError) -> Self {
        EvalError::Budget(b)
    }
}

/// Host capability dispatcher.
#[async_trait]
pub trait CapabilityHost: Send + Sync {
    async fn call(
        &self,
        path: &[String],
        args: Vec<Value>,
        effect: bool,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError>;
}

pub struct NopCapabilities;

#[async_trait]
impl CapabilityHost for NopCapabilities {
    async fn call(
        &self,
        path: &[String],
        _args: Vec<Value>,
        _effect: bool,
        _ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        Err(EvalError::Capability(format!(
            "capability `@{}` not registered",
            path.join(".")
        )))
    }
}

/// Which stream a write belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Receives script output as it is produced.
///
/// Without one, output accumulates in `ctx.stdout`/`ctx.stderr` for the host to drain
/// afterwards — which means a long-running script prints nothing until it exits, and a
/// chatty one holds every line in memory. A host that wants live output installs a sink.
/// Buffering stays the default because the HTTP host deliberately collects a handler's
/// output and emits it with the response, and most tests assert on the buffers.
pub type OutputSink = Arc<dyn Fn(OutputStream, &str) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub name: String,
    pub span: Span,
}

pub struct RuntimeContext {
    pub env: Environment,
    pub atoms: AtomInterner,
    pub budget: ExecutionBudget,
    pub sources: SourceMap,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub capabilities: Arc<dyn CapabilityHost>,
    pub functions: HashMap<String, FunctionEntry>,
    pub call_depth: usize,
    pub rng_seed: u64,
    pub call_stack: Vec<StackFrame>,
    /// Module search roots for runtime import fallback.
    pub module_roots: Vec<std::path::PathBuf>,
    /// Script directory for relative imports.
    pub script_dir: Option<std::path::PathBuf>,
    /// Opaque permission snapshot for host callbacks (JSON or flag blob).
    pub allow_all: bool,
    /// May the script write to the terminal?
    ///
    /// Console calls do not go through `CapabilityHost` — they need `&mut` access to
    /// this context to reach the output buffer or sink, which the trait cannot give
    /// them. The consequence was that `perms.check_console()` in the host capability
    /// was unreachable and `--deny console` printed anyway. The host mirrors the
    /// decision here, the same way it already mirrors `allow_all`.
    pub console_allowed: bool,
    /// Arguments the invoker passed to the script, after `--`. Read by
    /// `@process.args`. Set by the CLI and by compiled binaries; empty otherwise.
    pub script_args: Vec<String>,
    /// Set by the evaluator immediately before it invokes `@http.listen`, and read by
    /// the capability to build its server. See [`PendingHttpServer`].
    pub pending_http: Option<PendingHttpServer>,
    /// Installed by a host that wants output as it happens; see [`OutputSink`]. When
    /// set, `stdout`/`stderr` stay empty and every write goes straight to the sink.
    pub sink: Option<OutputSink>,
    /// Resolves a `http.next` handle for custom middleware, installed by the HTTP host
    /// on the context it builds for one request.
    ///
    /// Per-context rather than global: the callback owns that request's continuations,
    /// so two requests — or two servers — cannot see each other's `next`, and a stale
    /// one cannot outlive the chain that made it.
    pub http_next: Option<HttpNextInvoker>,
}

/// A server that `@http.listen` is about to start.
///
/// The route bodies are IR and the middleware are closures, so they cannot travel as
/// `Value` arguments through `CapabilityHost::call`. They used to be handed over via a
/// pair of process globals plus a registrar function pointer, which meant the capability
/// read its real arguments out of static state — and two servers in one process would
/// have overwritten each other. Riding on the context keeps the handoff scoped to the
/// evaluation that made it.
#[derive(Clone)]
pub struct PendingHttpServer {
    pub addr: String,
    pub routes: Vec<rite_sem::RouteIr>,
    pub middleware: Vec<crate::value::HttpMiddleware>,
}

#[derive(Clone)]
pub struct FunctionEntry {
    pub params: Vec<String>,
    pub body: BlockIr,
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
            atoms: AtomInterner::new(),
            budget: ExecutionBudget::new(),
            sources: SourceMap::new(),
            stdout: Vec::new(),
            stderr: Vec::new(),
            capabilities: Arc::new(NopCapabilities),
            functions: HashMap::new(),
            call_depth: 0,
            rng_seed: 42,
            call_stack: Vec::new(),
            module_roots: Vec::new(),
            script_dir: None,
            allow_all: false,
            // Console is allowed by the default-secure policy; a host that denies it
            // sets this to false when it installs capabilities.
            console_allowed: true,
            script_args: Vec::new(),
            pending_http: None,
            http_next: None,
            sink: None,
        }
    }

    pub fn with_capabilities(mut self, caps: Arc<dyn CapabilityHost>) -> Self {
        self.capabilities = caps;
        self
    }

    /// Write to the script's stdout: straight to the sink if one is installed, else
    /// buffered for the host to drain.
    pub fn print(&mut self, s: impl Into<String>) {
        let s = s.into();
        match &self.sink {
            Some(sink) => sink(OutputStream::Stdout, &s),
            None => self.stdout.push(s),
        }
    }

    /// As [`RuntimeContext::print`], for stderr.
    pub fn print_err(&mut self, s: impl Into<String>) {
        let s = s.into();
        match &self.sink {
            Some(sink) => sink(OutputStream::Stderr, &s),
            None => self.stderr.push(s),
        }
    }

    pub fn format_stack_trace(&self) -> String {
        if self.call_stack.is_empty() {
            return String::new();
        }
        let mut out = String::from("\nstack traceback:\n");
        for (i, frame) in self.call_stack.iter().rev().enumerate() {
            let loc = self
                .sources
                .files()
                .first()
                .map(|f| {
                    let lc = f.line_col(frame.span.start);
                    format!("{}:{}:{}", f.name, lc.line, lc.column)
                })
                .unwrap_or_else(|| format!("span {}", frame.span));
            out.push_str(&format!("  {}: {} at {}\n", i, frame.name, loc));
            if let Some(f) = self.sources.files().first() {
                if let Some(line) = f.line_text(f.line_col(frame.span.start).line) {
                    out.push_str(&format!("      |\n      | {}\n", line.trim_end()));
                }
            }
        }
        out
    }
}

impl Default for RuntimeContext {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Evaluator<'a> {
    ctx: &'a mut RuntimeContext,
}

impl<'a> Evaluator<'a> {
    pub fn new(ctx: &'a mut RuntimeContext) -> Self {
        Self { ctx }
    }

    pub async fn eval_program(&mut self, ir: &ProgramIr) -> Result<Value, EvalError> {
        // Register functions
        for f in &ir.functions {
            self.ctx.functions.insert(
                f.name.clone(),
                FunctionEntry {
                    params: f.param_names.clone(),
                    body: f.body.clone(),
                },
            );
            // Also bind as closures in env. The capture is the module scope itself
            // (shared frames), so a function body sees its siblings and every top-level
            // binding, whichever order they are defined in.
            let clos = Value::Function(Closure {
                id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
                name: Some(f.name.clone()),
                params: f.param_names.clone(),
                env: Arc::new(parking_lot::RwLock::new(self.ctx.env.clone())),
                body: f.body.clone(),
            });
            self.ctx.env.define_name(&f.name, clos, false);
        }

        let mut last = Value::None;
        if let Some(module) = ir.modules.first() {
            for stmt in &module.statements {
                match self.eval_expr(stmt).await {
                    // Top-level `^` / postfix `?` on err: script result is the returned value.
                    Err(EvalError::Return(v)) => {
                        last = v;
                        break;
                    }
                    Err(e) => return Err(e),
                    Ok(v) => last = v,
                }
            }
        }

        match &ir.entry {
            EntryPoint::Main { .. } => {
                if let Some(main) = self.ctx.functions.get("main").cloned() {
                    last = self
                        .call_block(
                            &main.body,
                            &main.params,
                            vec![Value::list(Vec::<Value>::new())],
                        )
                        .await?;
                }
            }
            EntryPoint::Script => {}
        }
        Ok(last)
    }

    pub fn eval_expr<'b>(
        &'b mut self,
        expr: &'b ExprIr,
    ) -> Pin<Box<dyn Future<Output = Result<Value, EvalError>> + Send + 'b>> {
        Box::pin(async move { self.eval_expr_inner(expr).await })
    }

    async fn eval_expr_inner(&mut self, expr: &ExprIr) -> Result<Value, EvalError> {
        self.ctx.budget.tick()?;
        match expr {
            ExprIr::Constant(lit) => Ok(self.lit_to_value(lit)),
            ExprIr::Local(id) => self
                .ctx
                .env
                .get_local(*id)
                .ok_or_else(|| EvalError::Message(format!("undefined local {}", id.0))),
            ExprIr::Global(name) => {
                if let Some(v) = self.ctx.env.get(name) {
                    return Ok(v);
                }
                if self.ctx.functions.contains_key(name) {
                    // return function value from env (registered)
                    return self
                        .ctx
                        .env
                        .get(name)
                        .ok_or_else(|| EvalError::Message(format!("undefined function {}", name)));
                }
                // Shadowable builtins: prefer env binding above; else native dispatch token.
                if is_runtime_builtin(name) {
                    return Ok(Value::NativeName(name.clone()));
                }
                Err(EvalError::Message(format!("undefined name `{}`", name)))
            }
            ExprIr::Bind {
                local,
                name,
                mutable,
                value,
                ..
            } => {
                let v = self.eval_expr(value).await?;
                self.ctx.env.define(name, *local, v.clone(), *mutable);
                Ok(v)
            }
            ExprIr::Assign { value, span, .. } => {
                // Special: Seq(Global(name), value) from desugar
                if let ExprIr::Seq(parts, _) = value.as_ref() {
                    if parts.len() == 2 {
                        if let ExprIr::Global(name) = &parts[0] {
                            let v = self.eval_expr(&parts[1]).await?;
                            self.ctx
                                .env
                                .assign(name, v.clone())
                                .map_err(EvalError::Message)?;
                            return Ok(v);
                        }
                    }
                }
                let _ = span;
                let v = self.eval_expr(value).await?;
                Ok(v)
            }
            ExprIr::Call { callee, args, .. } => {
                let c = self.eval_expr(callee).await?;
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_expr(a).await?);
                }
                self.call_value(c, argv).await
            }
            ExprIr::NativeCall { name, args, .. } => {
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_expr(a).await?);
                }
                self.call_native(name, argv).await
            }
            ExprIr::CapabilityCall {
                path, args, effect, ..
            } => {
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_expr(a).await?);
                }
                let eff = matches!(effect, EffectKind::Effect);
                // Special-case console for when host not fully wired
                if path.first().map(|s| s.as_str()) == Some("console") {
                    return self.eval_console(path, argv).await;
                }
                self.ctx.capabilities.call(path, argv, eff, self.ctx).await
            }
            ExprIr::Closure(c) => Ok(Value::Function(Closure {
                id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
                name: None,
                params: c.param_names.clone(),
                // Capture the defining chain. Frames are shared, so this is a handful of
                // `Arc` clones, the closure keeps them alive after the defining call
                // returns, and mutables assigned through it are visible to that scope.
                env: Arc::new(parking_lot::RwLock::new(self.ctx.env.clone())),
                body: c.body.clone(),
            })),
            ExprIr::Pipeline { input, stages, .. } => {
                let mut val = self.eval_expr(input).await?;
                for stage in stages {
                    val = self.eval_pipeline_stage(val, stage).await?;
                }
                Ok(val)
            }
            ExprIr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let cond = self.eval_expr(condition).await?;
                if cond.is_truthy() {
                    self.eval_block(then_branch).await
                } else if let Some(e) = else_branch {
                    self.eval_block(e).await
                } else {
                    Ok(Value::None)
                }
            }
            ExprIr::Match { value, arms, .. } => {
                let scrut = self.eval_expr(value).await?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &scrut)? {
                        self.ctx.env.push_frame();
                        for (name, val) in bindings {
                            self.ctx.env.define_name(&name, val, false);
                        }
                        let result = self.eval_expr(&arm.body).await;
                        self.ctx.env.pop_frame();
                        return result;
                    }
                }
                Err(EvalError::Message("match failure: no arm matched".into()))
            }
            ExprIr::Return(val, _) => {
                let v = match val {
                    Some(e) => self.eval_expr(e).await?,
                    None => Value::None,
                };
                Err(EvalError::Return(v))
            }
            ExprIr::BuildList(xs, _) => {
                let mut out = im::Vector::new();
                for x in xs {
                    out.push_back(self.eval_expr(x).await?);
                }
                Ok(Value::List(out))
            }
            ExprIr::BuildRecord(entries, _) => {
                let mut rec = IndexMap::new();
                for (k, v) in entries {
                    let key = match k {
                        KeyIr::Ident(s) | KeyIr::String(s) => Key::String(s.clone()),
                        KeyIr::Atom(a) => Key::Atom(a.clone()),
                    };
                    rec.insert(key, self.eval_expr(v).await?);
                }
                Ok(Value::Record(rec))
            }
            ExprIr::Member { object, field, .. } => {
                let obj = self.eval_expr(object).await?;
                Ok(obj.get_field(field))
            }
            ExprIr::Index { object, index, .. } => {
                let obj = self.eval_expr(object).await?;
                let idx = self.eval_expr(index).await?;
                match (&obj, &idx) {
                    (Value::List(xs), Value::Int(i)) => {
                        if *i < 0 || *i as usize >= xs.len() {
                            Ok(Value::None)
                        } else {
                            Ok(xs[*i as usize].clone())
                        }
                    }
                    (Value::Record(r), Value::String(s)) => Ok(r
                        .get(&Key::String(s.to_string()))
                        .cloned()
                        .unwrap_or(Value::None)),
                    (Value::Record(r), other) => {
                        let k = Key::String(format!("{}", other));
                        Ok(r.get(&k).cloned().unwrap_or(Value::None))
                    }
                    _ => Ok(Value::None),
                }
            }
            ExprIr::Unary { op, expr, .. } => {
                let v = self.eval_expr(expr).await?;
                match op {
                    UnaryOpIr::Neg => match v {
                        Value::Int(n) => n
                            .checked_neg()
                            .map(Value::Int)
                            .ok_or_else(|| EvalError::Message("integer overflow".into())),
                        Value::Float(f) => Ok(Value::Float(-f)),
                        _ => Err(EvalError::Message("cannot negate non-number".into())),
                    },
                    UnaryOpIr::Not => Ok(Value::Bool(!v.is_truthy())),
                    UnaryOpIr::Effect => Ok(v),
                }
            }
            ExprIr::Binary {
                op, left, right, ..
            } => self.eval_binary(*op, left, right).await,
            ExprIr::Try { expr, .. } => {
                let v = self.eval_expr(expr).await?;
                match v {
                    Value::Result(ResultValue::Ok(inner)) => Ok(*inner),
                    Value::Result(ResultValue::Err(e)) => Err(EvalError::Return(Value::err(*e))),
                    other => Ok(other),
                }
            }
            ExprIr::Coalesce { left, right, .. } => {
                let l = self.eval_expr(left).await?;
                if matches!(l, Value::None) {
                    self.eval_expr(right).await
                } else {
                    Ok(l)
                }
            }
            ExprIr::Block(b) => self.eval_block(b).await,
            ExprIr::Atom(name, _) => Ok(Value::Atom(self.ctx.atoms.intern(name))),
            ExprIr::Placeholder(_) => Err(EvalError::Message(
                "pipeline placeholder `$` outside pipeline stage".into(),
            )),
            ExprIr::HttpListen {
                addr,
                routes,
                middleware,
                ..
            } => {
                let a = self.eval_expr(addr).await?;
                let addr_str = a
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{}", a));
                // Middleware: `use @http.log` → Named, `use { |req, next| … }` → Function
                let mut mw_specs: Vec<crate::value::HttpMiddleware> = Vec::new();
                for m in middleware {
                    match m {
                        ExprIr::CapabilityCall { path, .. } => {
                            let name = path.join(".");
                            let short = name
                                .strip_prefix("http.")
                                .unwrap_or(name.as_str())
                                .trim_start_matches('@')
                                .to_string();
                            mw_specs.push(crate::value::HttpMiddleware::Named(short));
                        }
                        ExprIr::NativeCall { name, .. } => {
                            mw_specs.push(crate::value::HttpMiddleware::Named(name.clone()));
                        }
                        ExprIr::Global(name) => {
                            mw_specs.push(crate::value::HttpMiddleware::Named(name.clone()));
                        }
                        ExprIr::Closure(_) => {
                            let v = self.eval_expr(m).await?;
                            if let Value::Function(c) = v {
                                mw_specs.push(crate::value::HttpMiddleware::Function(c));
                            }
                        }
                        other => {
                            // Evaluate (e.g. already-resolved values) and classify
                            let v = self.eval_expr(other).await?;
                            match v {
                                Value::Function(c) => {
                                    mw_specs.push(crate::value::HttpMiddleware::Function(c));
                                }
                                Value::Atom(id) => {
                                    let n = self.ctx.atoms.name(id);
                                    let short =
                                        n.strip_prefix("http.").unwrap_or(n.as_str()).to_string();
                                    mw_specs.push(crate::value::HttpMiddleware::Named(short));
                                }
                                Value::String(s) => {
                                    mw_specs
                                        .push(crate::value::HttpMiddleware::Named(s.to_string()));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Build route table with real Rite bodies for per-request evaluation
                let rite_routes: Vec<Value> = routes
                    .iter()
                    .map(|r| {
                        Value::record(vec![
                            (
                                Key::String("method".into()),
                                Value::string(r.method.clone()),
                            ),
                            (Key::String("path".into()), Value::string(r.path.clone())),
                        ])
                    })
                    .collect();
                self.ctx.pending_http = Some(PendingHttpServer {
                    addr: addr_str.clone(),
                    routes: routes.to_vec(),
                    middleware: mw_specs.clone(),
                });
                self.ctx
                    .capabilities
                    .call(
                        &["http".into(), "listen".into()],
                        vec![a, Value::list(rite_routes)],
                        true,
                        self.ctx,
                    )
                    .await
            }
            ExprIr::Seq(xs, _) => {
                let mut last = Value::None;
                for x in xs {
                    last = self.eval_expr(x).await?;
                }
                Ok(last)
            }
        }
    } // end eval_expr_inner

    async fn eval_pipeline_stage(
        &mut self,
        input: Value,
        stage: &rite_sem::PipelineStageIr,
    ) -> Result<Value, EvalError> {
        match &stage.kind {
            StageKind::MemberProjection(field) => match input {
                Value::List(xs) => {
                    let out: Vec<Value> = xs.iter().map(|x| x.get_field(field)).collect();
                    Ok(Value::list(out))
                }
                other => Ok(other.get_field(field)),
            },
            StageKind::Block | StageKind::Call | StageKind::PlaceholderCall => {
                match &stage.expr {
                    ExprIr::NativeCall { name, args, .. } => {
                        let mut argv = vec![input];
                        for a in args {
                            argv.push(self.eval_expr(a).await?);
                        }
                        // special: map with member projection stage already handled
                        self.call_native(name, argv).await
                    }
                    ExprIr::Call { callee, args, .. } => {
                        // Check for $ placeholder in args
                        let mut argv = Vec::new();
                        let mut used_placeholder = false;
                        for a in args {
                            if matches!(a, ExprIr::Placeholder(_)) {
                                argv.push(input.clone());
                                used_placeholder = true;
                            } else {
                                argv.push(self.eval_expr(a).await?);
                            }
                        }
                        if !used_placeholder {
                            argv.insert(0, input);
                        }
                        let c = self.eval_expr(callee).await?;
                        self.call_value(c, argv).await
                    }
                    ExprIr::Closure(c) => {
                        let clos = Value::Function(Closure {
                            id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
                            name: None,
                            params: c.param_names.clone(),
                            env: Arc::new(parking_lot::RwLock::new(self.ctx.env.clone())),
                            body: c.body.clone(),
                        });
                        // If input is list and closure looks like map body — pipeline stage
                        // is itself the function applied to input
                        self.call_value(clos, vec![input]).await
                    }
                    ExprIr::Global(name) => self.call_native(name, vec![input]).await,
                    ExprIr::Member { object, field, .. }
                        if matches!(object.as_ref(), ExprIr::Placeholder(_)) =>
                    {
                        Ok(input.get_field(field))
                    }
                    other => {
                        // Evaluate stage as function then call with input
                        let f = self.eval_expr(other).await?;
                        self.call_value(f, vec![input]).await
                    }
                }
            }
        }
    }

    async fn eval_binary(
        &mut self,
        op: BinaryOpIr,
        left: &ExprIr,
        right: &ExprIr,
    ) -> Result<Value, EvalError> {
        // Short-circuit and/or
        if op == BinaryOpIr::And {
            let l = self.eval_expr(left).await?;
            if !l.is_truthy() {
                return Ok(Value::Bool(false));
            }
            let r = self.eval_expr(right).await?;
            return Ok(Value::Bool(r.is_truthy()));
        }
        if op == BinaryOpIr::Or {
            let l = self.eval_expr(left).await?;
            if l.is_truthy() {
                return Ok(Value::Bool(true));
            }
            let r = self.eval_expr(right).await?;
            return Ok(Value::Bool(r.is_truthy()));
        }

        let l = self.eval_expr(left).await?;
        let r = self.eval_expr(right).await?;
        match op {
            BinaryOpIr::Add => match (&l, &r) {
                (Value::Int(a), Value::Int(b)) => a
                    .checked_add(*b)
                    .map(Value::Int)
                    .ok_or_else(|| EvalError::Message("integer overflow".into())),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
                (Value::String(a), Value::String(b)) => Ok(Value::string(format!("{}{}", a, b))),
                (Value::List(a), Value::List(b)) => {
                    let mut out = a.clone();
                    out.append(b.clone());
                    Ok(Value::List(out))
                }
                (Value::Record(a), Value::Record(b)) => Ok(Value::Record(merge_records(a, b))),
                (Value::List(a), other) => {
                    let mut out = a.clone();
                    out.push_back(other.clone());
                    Ok(Value::List(out))
                }
                _ => Err(EvalError::Message(format!(
                    "cannot add {} and {}",
                    l.type_name(),
                    r.type_name()
                ))),
            },
            BinaryOpIr::Sub => match (&l, &r) {
                (Value::Int(a), Value::Int(b)) => a
                    .checked_sub(*b)
                    .map(Value::Int)
                    .ok_or_else(|| EvalError::Message("integer overflow".into())),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
                (Value::List(a), other) => Ok(Value::List(list_remove_first(a, other))),
                (Value::Record(a), Value::Atom(atom)) => {
                    let name = self.ctx.atoms.name(*atom);
                    let mut out = a.clone();
                    out.shift_remove(&Key::String(name.clone()));
                    out.shift_remove(&Key::Atom(name));
                    Ok(Value::Record(out))
                }
                _ => Err(EvalError::Message("cannot subtract values".into())),
            },
            BinaryOpIr::Mul => num_binop(&l, &r, |a, b| a.checked_mul(b), |a, b| a * b),
            BinaryOpIr::Div => match (&l, &r) {
                (Value::Int(_), Value::Int(0)) => {
                    Err(EvalError::Message("division by zero".into()))
                }
                // `i64::MIN / -1` overflows; Rust panics on that in every profile.
                (Value::Int(a), Value::Int(b)) => a
                    .checked_div(*b)
                    .map(Value::Int)
                    .ok_or_else(|| EvalError::Message("integer overflow".into())),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
                _ => Err(EvalError::Message("cannot divide values".into())),
            },
            BinaryOpIr::Rem => match (&l, &r) {
                (Value::Int(_), Value::Int(0)) => {
                    Err(EvalError::Message("division by zero".into()))
                }
                // `i64::MIN % -1` overflows the same way `i64::MIN / -1` does.
                (Value::Int(a), Value::Int(b)) => a
                    .checked_rem(*b)
                    .map(Value::Int)
                    .ok_or_else(|| EvalError::Message("integer overflow".into())),
                _ => Err(EvalError::Message("cannot rem values".into())),
            },
            BinaryOpIr::Eq => Ok(Value::Bool(l.structural_eq(&r))),
            BinaryOpIr::NotEq => Ok(Value::Bool(!l.structural_eq(&r))),
            BinaryOpIr::Lt => Ok(Value::Bool(compare_values(&l, &r) < 0)),
            BinaryOpIr::LtEq => Ok(Value::Bool(compare_values(&l, &r) <= 0)),
            BinaryOpIr::Gt => Ok(Value::Bool(compare_values(&l, &r) > 0)),
            BinaryOpIr::GtEq => Ok(Value::Bool(compare_values(&l, &r) >= 0)),
            // Both operands are already evaluated exactly once above; `∈` and `∉` share
            // the same membership test so neither re-runs a side-effecting operand.
            BinaryOpIr::In => Ok(Value::Bool(self.contains_value(&l, &r))),
            BinaryOpIr::NotIn => Ok(Value::Bool(!self.contains_value(&l, &r))),
            BinaryOpIr::And | BinaryOpIr::Or => unreachable!(),
        }
    }

    /// Membership test behind `∈` / `∉`, with atoms also matching by name so
    /// `#a ∈ ["a"]` and `#a ∈ ⟨a: 1⟩` hold.
    fn contains_value(&self, item: &Value, container: &Value) -> bool {
        if let Value::Atom(id) = item {
            let name = self.ctx.atoms.name(*id);
            match container {
                Value::List(xs) => {
                    return xs
                        .iter()
                        .any(|x| x.structural_eq(item) || x.as_str() == Some(name.as_str()))
                }
                Value::Record(rec) => {
                    return rec.contains_key(&Key::String(name.clone()))
                        || rec.contains_key(&Key::Atom(name))
                }
                _ => {}
            }
        }
        membership(item, container)
    }

    async fn eval_block(&mut self, block: &BlockIr) -> Result<Value, EvalError> {
        // Nested blocks (if/match bodies, bare blocks) must *propagate* `^` / `return`
        // as `EvalError::Return` so it can exit the enclosing function. Only
        // `call_block` (function/closure boundary) converts Return → Ok.
        self.ctx.env.push_frame();
        let mut last = Value::None;
        let result = async {
            for expr in &block.body {
                match self.eval_expr(expr).await {
                    Err(EvalError::Return(v)) => return Err(EvalError::Return(v)),
                    Err(e) => return Err(e),
                    Ok(v) => last = v,
                }
            }
            Ok(last)
        }
        .await;
        self.ctx.env.pop_frame();
        result
    }

    async fn call_value(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, EvalError> {
        self.ctx.budget.check_depth(self.ctx.call_depth + 1)?;
        self.ctx.call_depth += 1;
        let result = match callee {
            Value::Function(c) => {
                // Lexical scoping: a closure always runs in the environment it captured,
                // extended with the fresh frame `call_block` pushes for its parameters.
                // Frames are shared (see `env::Environment`), so the capture still sees
                // — and assigns through to — the defining scope's mutable bindings, which
                // is what makes `count := count + 1` inside an `each`/`while_loop` body
                // visible to the enclosing scope.
                let mut captured = c.env.read().clone();
                captured.ensure_globals_from(&self.ctx.env);
                let saved = std::mem::replace(&mut self.ctx.env, captured);
                let r = self.call_block(&c.body, &c.params, args).await;
                self.ctx.env = saved;
                r
            }
            Value::NativeName(name) => {
                // Indirection avoids infinitely sized async future (call_value ↔ map/each).
                let name = name.clone();
                Box::pin(self.call_native(&name, args)).await
            }
            Value::NativeFunction(_) => Err(EvalError::Message(
                "native function id call not wired".into(),
            )),
            Value::Handle(h) if h.kind == "http.next" => {
                let invoker = self.ctx.http_next.clone();
                match invoker {
                    Some(f) => Box::pin(f(h.id, args)).await,
                    None => Err(EvalError::Message(
                        "http middleware next() is only valid inside a request handler chain"
                            .into(),
                    )),
                }
            }
            other => Err(EvalError::Message(format!(
                "cannot call value of type {}",
                other.type_name()
            ))),
        };
        self.ctx.call_depth -= 1;
        result
    }

    pub async fn call_block_public(
        &mut self,
        body: &BlockIr,
        params: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.call_block(body, params, args).await
    }

    /// Call a Rite function/closure value (used by HTTP middleware).
    pub async fn call_value_public(
        &mut self,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.call_value(callee, args).await
    }

    async fn call_block(
        &mut self,
        body: &BlockIr,
        params: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        if params.len() != args.len() && !params.is_empty() {
            // Allow under-application only if zero params? Spec: invalid arity error
            if params.len() != args.len() {
                return Err(EvalError::Message(format!(
                    "arity mismatch: expected {} args, got {}",
                    params.len(),
                    args.len()
                )));
            }
        }
        let frame_name = params
            .first()
            .map(|p| format!("fn({})", p))
            .unwrap_or_else(|| "fn".into());
        self.ctx.call_stack.push(StackFrame {
            name: frame_name,
            span: body.span,
        });
        self.ctx.env.push_frame();
        for (i, p) in params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(Value::None);
            self.ctx.env.define_name(p, v, false);
        }
        // also bind block params by local ids if present
        for (i, lid) in body.params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(Value::None);
            let name = params.get(i).cloned().unwrap_or_else(|| format!("${}", i));
            self.ctx.env.define(&name, *lid, v, false);
        }
        let mut last = Value::None;
        let result = async {
            for expr in &body.body {
                match self.eval_expr(expr).await {
                    Err(EvalError::Return(v)) => return Ok(v),
                    Err(e) => return Err(e.with_stack(self.ctx)),
                    Ok(v) => last = v,
                }
            }
            Ok(last)
        }
        .await;
        self.ctx.env.pop_frame();
        self.ctx.call_stack.pop();
        result
    }

    async fn call_native(&mut self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            "map" => self.builtin_map(args).await,
            "keep" => self.builtin_filter(args, true).await,
            "reject" => self.builtin_filter(args, false).await,
            "each" => self.builtin_each(args).await,
            "reduce" => self.builtin_reduce(args).await,
            "find" => self.builtin_find(args).await,
            "any" => self.builtin_any_all(args, true).await,
            "all" => self.builtin_any_all(args, false).await,
            "group" => self.builtin_group(args).await,
            "parallel" => self.builtin_map(args).await, // sequential fallback with same semantics for pure
            "import" => Ok(Value::None),                // module loading handled at higher layer
            "while_loop" => self.builtin_while_loop(args).await,
            "compose" => self.builtin_compose(args).await,
            "print" | "println" => {
                let s = args
                    .first()
                    .map(|v| v.to_display(&self.ctx.atoms))
                    .unwrap_or_default();
                if name == "println" {
                    self.ctx.print(format!("{}\n", s));
                } else {
                    self.ctx.print(s);
                }
                Ok(Value::None)
            }
            other => call_builtin(other, args),
        }
    }

    async fn builtin_while_loop(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let pred = it.next().unwrap_or(Value::None);
        let body = it.next().unwrap_or(Value::None);
        let mut steps = 0u64;
        loop {
            self.ctx.budget.tick()?;
            steps += 1;
            if steps > 1_000_000 {
                return Err(EvalError::Message(
                    "while loop exceeded iteration guard".into(),
                ));
            }
            let c = self.call_value(pred.clone(), vec![Value::None]).await?;
            if !c.is_truthy() {
                break;
            }
            let _ = self.call_value(body.clone(), vec![Value::None]).await?;
        }
        Ok(Value::None)
    }

    async fn builtin_compose(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        // compose(f, g) → function x => f(g(x)); compose(f, g, x) applies immediately.
        let mut it = args.into_iter();
        let f = it.next().unwrap_or(Value::None);
        let g = it.next().unwrap_or(Value::None);
        if let Some(x) = it.next() {
            let y = self.call_value(g, vec![x]).await?;
            return self.call_value(f, vec![y]).await;
        }
        use rite_core::Span;
        use rite_sem::{BlockIr, ExprIr, LocalId};
        // Private frame layered over the ambient scope holds the two composed functions.
        let mut env = self.ctx.env.clone();
        env.push_frame();
        env.define_name("__f", f, false);
        env.define_name("__g", g, false);
        let body = BlockIr {
            params: vec![LocalId(0)],
            body: vec![ExprIr::Call {
                callee: Box::new(ExprIr::Global("__f".into())),
                args: vec![ExprIr::Call {
                    callee: Box::new(ExprIr::Global("__g".into())),
                    args: vec![ExprIr::Global("x".into())],
                    span: Span::DUMMY,
                }],
                span: Span::DUMMY,
            }],
            span: Span::DUMMY,
        };
        Ok(Value::Function(Closure {
            id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
            name: Some("compose".into()),
            params: vec!["x".into()],
            env: Arc::new(parking_lot::RwLock::new(env)),
            body,
        }))
    }

    async fn builtin_map(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            Some(other) => {
                return Err(EvalError::Message(format!(
                    "map expects list, got {}",
                    other.type_name()
                )))
            }
            None => return Ok(Value::list(Vec::<Value>::new())),
        };
        let f = it.next().unwrap_or(Value::None);
        // Member projection style: second arg missing, used from pipeline with projection stages
        let mut out = im::Vector::new();
        for item in list {
            let mapped = match &f {
                Value::Function(_) | Value::NativeFunction(_) => {
                    self.call_value(f.clone(), vec![item]).await?
                }
                Value::None => item,
                other => {
                    // If f is not a function, treat as identity error
                    return Err(EvalError::Message(format!(
                        "map expects function, got {}",
                        other.type_name()
                    )));
                }
            };
            out.push_back(mapped);
        }
        Ok(Value::List(out))
    }

    async fn builtin_filter(&mut self, args: Vec<Value>, keep: bool) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::list(Vec::<Value>::new())),
        };
        let f = it.next().unwrap_or(Value::None);
        let mut out = im::Vector::new();
        for item in list {
            let pred = self.call_value(f.clone(), vec![item.clone()]).await?;
            if pred.is_truthy() == keep {
                out.push_back(item);
            }
        }
        Ok(Value::List(out))
    }

    async fn builtin_each(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        for item in list {
            let _ = self.call_value(f.clone(), vec![item]).await?;
        }
        Ok(Value::None)
    }

    async fn builtin_reduce(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        let mut acc = it.next().unwrap_or(Value::None);
        for item in list {
            acc = self.call_value(f.clone(), vec![acc, item]).await?;
        }
        Ok(acc)
    }

    async fn builtin_find(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        for item in list {
            let pred = self.call_value(f.clone(), vec![item.clone()]).await?;
            if pred.is_truthy() {
                return Ok(item);
            }
        }
        Ok(Value::None)
    }

    async fn builtin_any_all(
        &mut self,
        args: Vec<Value>,
        is_any: bool,
    ) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::Bool(!is_any)),
        };
        let f = it.next();
        for item in list {
            let pred = if let Some(ref func) = f {
                self.call_value(func.clone(), vec![item]).await?
            } else {
                item
            };
            if is_any && pred.is_truthy() {
                return Ok(Value::Bool(true));
            }
            if !is_any && !pred.is_truthy() {
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(!is_any))
    }

    async fn builtin_group(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        // group list by field name if second is string, or function
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::list(Vec::<Value>::new())),
        };
        let key_fn = it.next();
        let mut groups: IndexMap<String, im::Vector<Value>> = IndexMap::new();
        for item in list {
            let key = match &key_fn {
                Some(Value::String(s)) => item.get_field(s).to_string(),
                Some(Value::Function(_)) => {
                    let k = self
                        .call_value(key_fn.clone().unwrap(), vec![item.clone()])
                        .await?;
                    k.to_string()
                }
                _ => item.to_string(),
            };
            groups.entry(key).or_default().push_back(item);
        }
        let out: Vec<Value> = groups
            .into_iter()
            .map(|(k, vs)| Value::list(vec![Value::string(k), Value::List(vs)]))
            .collect();
        Ok(Value::list(out))
    }

    async fn eval_console(
        &mut self,
        path: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        if !self.ctx.console_allowed && !self.ctx.allow_all {
            return Err(EvalError::Permission("console permission denied".into()));
        }
        let method = path.get(1).map(|s| s.as_str()).unwrap_or("print");
        let msg = args
            .first()
            .map(|v| v.to_display(&self.ctx.atoms))
            .unwrap_or_default();
        match method {
            "print" => {
                self.ctx.print(msg);
                Ok(Value::None)
            }
            "println" => {
                self.ctx.print(format!("{}\n", msg));
                Ok(Value::None)
            }
            "warn" | "error" => {
                self.ctx.print_err(format!("{}\n", msg));
                Ok(Value::None)
            }
            "inspect" => {
                self.ctx.print(format!("{:?}\n", args.first()));
                Ok(Value::None)
            }
            "read_line" => Ok(Value::string("")),
            _ => self.ctx.capabilities.call(path, args, true, self.ctx).await,
        }
    }

    fn match_pattern(
        &self,
        pat: &PatternIr,
        value: &Value,
    ) -> Result<Option<Vec<(String, Value)>>, EvalError> {
        let mut bindings = Vec::new();
        if self.match_pattern_inner(pat, value, &mut bindings)? {
            Ok(Some(bindings))
        } else {
            Ok(None)
        }
    }

    fn match_pattern_inner(
        &self,
        pat: &PatternIr,
        value: &Value,
        bindings: &mut Vec<(String, Value)>,
    ) -> Result<bool, EvalError> {
        match pat {
            PatternIr::Wildcard => Ok(true),
            PatternIr::Ident(_, name) => {
                bindings.push((name.clone(), value.clone()));
                Ok(true)
            }
            PatternIr::Atom(name) => match value {
                Value::Atom(id) => Ok(self.ctx.atoms.name(*id) == *name),
                Value::String(s) => Ok(s.as_ref() == name),
                _ => Ok(false),
            },
            PatternIr::Literal(lit) => {
                let lv = self.lit_to_value(lit);
                Ok(lv.structural_eq(value))
            }
            PatternIr::List { elements, rest } => {
                let Value::List(xs) = value else {
                    return Ok(false);
                };
                if rest.is_none() && elements.len() != xs.len() {
                    return Ok(false);
                }
                if elements.len() > xs.len() {
                    return Ok(false);
                }
                for (i, ep) in elements.iter().enumerate() {
                    if !self.match_pattern_inner(ep, &xs[i], bindings)? {
                        return Ok(false);
                    }
                }
                if let Some(r) = rest {
                    let rest_vals: im::Vector<Value> =
                        xs.iter().skip(elements.len()).cloned().collect();
                    if !self.match_pattern_inner(r, &Value::List(rest_vals), bindings)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            PatternIr::Record { fields } => {
                let Value::Record(rec) = value else {
                    return Ok(false);
                };
                for (name, sub) in fields {
                    let v = rec
                        .get(&Key::String(name.clone()))
                        .or_else(|| rec.get(&Key::Atom(name.clone())))
                        .cloned()
                        .unwrap_or(Value::None);
                    if let Some(sp) = sub {
                        if !self.match_pattern_inner(sp, &v, bindings)? {
                            return Ok(false);
                        }
                    } else {
                        bindings.push((name.clone(), v));
                    }
                }
                Ok(true)
            }
            PatternIr::Result { kind, binding } => match (kind, value) {
                (ResultPatKindIr::Ok, Value::Result(ResultValue::Ok(v))) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::Err, Value::Result(ResultValue::Err(v))) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::Some, v) if !matches!(v, Value::None) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::None, Value::None) => Ok(true),
                // also allow ok/err as atoms in records? keep strict
                _ => Ok(false),
            },
        }
    }

    fn lit_to_value(&self, lit: &ValueLiteral) -> Value {
        match lit {
            ValueLiteral::None(_) => Value::None,
            ValueLiteral::Bool(b, _) => Value::Bool(*b),
            ValueLiteral::Int(n, _) => Value::Int(*n),
            ValueLiteral::Float(n, _) => Value::Float(*n),
            ValueLiteral::String(s, _) => Value::string(s.clone()),
        }
    }
}

fn num_binop(
    l: &Value,
    r: &Value,
    int_op: impl Fn(i64, i64) -> Option<i64>,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, EvalError> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => int_op(*a, *b)
            .map(Value::Int)
            .ok_or_else(|| EvalError::Message("integer overflow".into())),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
        _ => Err(EvalError::Message(
            "numeric operation on non-numbers".into(),
        )),
    }
}

/// Async invoker for `http.next` handles used by custom middleware (`next(req)`).
pub type HttpNextInvoker = Arc<
    dyn Fn(u64, Vec<Value>) -> Pin<Box<dyn Future<Output = Result<Value, EvalError>> + Send>>
        + Send
        + Sync,
>;

// silence
#[allow(dead_code)]
fn _span(_s: Span) {}

/// Does a bare name refer to a builtin the interpreter can dispatch?
///
/// Reads `rite_sem`'s list rather than repeating it: this was a second copy of the same
/// 64 names, and the two had to be edited in lockstep for a new builtin to be both
/// accepted by the resolver and callable at runtime.
fn is_runtime_builtin(name: &str) -> bool {
    rite_sem::resolve::BUILTIN_NAMES.contains(&name)
}
