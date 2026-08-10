//! Async tree-walking evaluator over `ProgramIr`.
//!
//! Split by role: this file holds the runtime types ([`RuntimeContext`], [`EvalError`],
//! the capability and output traits) and the classification of which nodes can be
//! evaluated without suspending ([`is_sync`]). The evaluator's methods live in
//! submodules — `expr` for expressions and blocks, `call` for calls and dispatch, `hof`
//! for builtins that take a callback, `values` for pattern matching and literal
//! lowering, `console` for the terminal shortcut — each adding an `impl` block to the
//! one [`Evaluator`].

mod call;
mod console;
mod expr;
mod hof;
mod values;

use crate::atom::AtomInterner;
use crate::budget::{BudgetError, ExecutionBudget};
use crate::env::Environment;
use crate::value::{Key, Value};
use async_trait::async_trait;
use rite_core::{Diagnostics, SourceMap, Span};
use rite_sem::{BlockIr, ExprIr, KeyIr};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

/// Monotonic closure ids. Global on purpose — same reasoning as `NEXT_ID` in the HTTP
/// host: ids only have to be unique, and nothing reads this to make a decision.
static CLOSURE_ID: AtomicU64 = AtomicU64::new(1);

/// Can this whole subtree be evaluated without suspending?
///
/// Inspection only — it must not evaluate anything, because a `false` answer means the
/// async path will run the subtree from the start. Every variant is listed explicitly
/// rather than with a `_ => false` catch-all, so a new `ExprIr` variant fails to compile
/// here instead of silently taking the slow path forever.
fn is_sync(expr: &ExprIr) -> bool {
    match expr {
        ExprIr::Constant(_) | ExprIr::Atom(..) | ExprIr::Local(_) | ExprIr::Global(_) => true,
        ExprIr::Unary { expr, .. } => is_sync(expr),
        ExprIr::Binary { left, right, .. } => is_sync(left) && is_sync(right),
        ExprIr::Member { object, .. } => is_sync(object),
        ExprIr::Index { object, index, .. } => is_sync(object) && is_sync(index),
        ExprIr::Coalesce { left, right, .. } => is_sync(left) && is_sync(right),
        ExprIr::Bind { value, .. } | ExprIr::Assign { value, .. } => is_sync(value),
        ExprIr::Try { expr, .. } => is_sync(expr),
        ExprIr::Seq(parts, _) | ExprIr::BuildList(parts, _) => parts.iter().all(is_sync),
        ExprIr::BuildRecord(entries, _) => entries.iter().all(|(_, v)| is_sync(v)),
        // These reach user code, a host call, a closure body, or the scheduler.
        ExprIr::Call { .. }
        | ExprIr::NativeCall { .. }
        | ExprIr::CapabilityCall { .. }
        | ExprIr::Closure(_)
        | ExprIr::Pipeline { .. }
        | ExprIr::If { .. }
        | ExprIr::Match { .. }
        | ExprIr::Block(_)
        | ExprIr::Return(..)
        | ExprIr::HttpListen { .. }
        | ExprIr::McpServe { .. }
        | ExprIr::Placeholder(_) => false,
    }
}

/// Lower an IR record key to a runtime key.
fn key_of(k: &KeyIr) -> Key {
    match k {
        KeyIr::Ident(s) | KeyIr::String(s) => Key::String(s.clone()),
        KeyIr::Atom(a) => Key::Atom(a.clone()),
    }
}

#[derive(Debug)]
pub enum EvalError {
    Message(String),
    Panic(String),
    Compile(Diagnostics),
    Budget(BudgetError),
    Return(Value),
    Permission(String),
    Capability(String),
    /// The script asked to end the process with a chosen status, via `@process.exit`.
    ///
    /// This is a control-flow signal, not a failure: it carries no message and must
    /// reach the top of the call stack untouched. Nothing may convert it to a value
    /// or swallow it the way `Return` is swallowed at a function boundary — an exit
    /// that can be caught is not an exit. It is an `EvalError` rather than a direct
    /// `std::process::exit` so that an embedder hosting `RiteEngine` sees a variant
    /// it can decide about, instead of having its own process killed by a guest.
    Exit(u8),
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
            EvalError::Exit(c) => write!(f, "exit {}", c),
        }
    }
}

impl std::error::Error for EvalError {}

impl EvalError {
    /// The process exit status this failure ends a run with.
    ///
    /// Rite's published contract: 0 success, 1 runtime, 2 usage, 3 parse, 4 resolve,
    /// 5 permission, 6 compile, 7 test, 8 budget — with `@process.exit` handing the
    /// choice to the script. This is the single definition of that mapping: `rite
    /// run`, the `main` generated for a compiled binary, and the conformance runner
    /// all call it, so a compiled program cannot come to a different conclusion
    /// about the same failure than the interpreter did.
    pub fn exit_code(&self) -> u8 {
        match self {
            EvalError::Exit(c) => *c,
            // 3 for a source that would not parse, 4 for one that parsed and would
            // not resolve — which is what the table has always claimed and what the
            // binary did not do: this arm used to answer 3 for every rejection with
            // an error in it, so an undefined name exited 3 where the contract
            // promised 4.
            EvalError::Compile(d) => d.rejection_exit_code(),
            EvalError::Permission(_) => 5,
            EvalError::Budget(_) => 8,
            EvalError::Message(_)
            | EvalError::Panic(_)
            | EvalError::Capability(_)
            | EvalError::Return(_) => 1,
        }
    }

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
    /// Shared, so a forked task interns into the same table. Every method takes
    /// `&self` (there is an `RwLock` inside), so call sites are unaffected.
    pub atoms: Arc<AtomInterner>,
    pub budget: ExecutionBudget,
    pub sources: SourceMap,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
    pub capabilities: Arc<dyn CapabilityHost>,
    pub functions: HashMap<String, FunctionEntry>,
    pub call_depth: usize,
    /// Pin `@random` to a reproducible sequence, or `None` to seed from the OS.
    ///
    /// Read by `rite_caps::install_defaults`. It was a `u64` defaulting to 42 that
    /// nothing ever read, which is why a host setting it saw no effect — `Option`
    /// distinguishes "the host asked for 42" from "the host said nothing".
    pub rng_seed: Option<u64>,
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
    /// Host resources this run holds open — see [`HandleTable`].
    ///
    /// Per-context for the same reason `http_next` is: a handle must not outlive
    /// the run that opened it, and dropping this context is what closes anything
    /// the script left open. An embedder's process does not exit between runs, so
    /// "the OS will clean up" is not an answer there.
    pub handles: Arc<crate::handles::HandleTable>,
    /// Resolves a `http.next` handle for custom middleware, installed by the HTTP host
    /// on the context it builds for one request.
    ///
    /// Per-context rather than global: the callback owns that request's continuations,
    /// so two requests — or two servers — cannot see each other's `next`, and a stale
    /// one cannot outlive the chain that made it.
    pub http_next: Option<HttpNextInvoker>,
    /// Staged by the evaluator immediately before it calls `@mcp.serve`, read by the
    /// capability. See [`PendingMcpServer`].
    pub pending_mcp: Option<PendingMcpServer>,
    /// Installed by the MCP host on the fresh context it builds for one request, so
    /// `@mcp.progress` inside a tool body reaches that request's stream. `None`
    /// everywhere else, which is what makes calling it outside a tool an error rather
    /// than a silent no-op.
    pub mcp_notify: Option<McpNotifier>,
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

/// A server that `@mcp.serve` is about to start.
///
/// Staged on the context for the same reason as [`PendingHttpServer`]: the declaration
/// bodies are `BlockIr` and cannot travel as `Value` arguments through
/// `CapabilityHost::call`. Declarations are split by kind here rather than in the host
/// so the host receives three tables it can answer `tools/list`, `resources/list` and
/// `prompts/list` from directly, each already in declaration order.
#[derive(Clone)]
pub struct PendingMcpServer {
    pub config: McpServeConfig,
    pub tools: Vec<rite_sem::McpDeclIr>,
    pub resources: Vec<rite_sem::McpDeclIr>,
    pub prompts: Vec<rite_sem::McpDeclIr>,
    /// Reuses `@http`'s middleware value: the shape is identical, and `use @mcp.log`
    /// is classified by the same code that classifies `use @http.log`.
    pub middleware: Vec<crate::value::HttpMiddleware>,
}

/// Where an MCP server listens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpTransport {
    /// Newline-delimited JSON-RPC on the process's own stdin/stdout. The default,
    /// because it is the form a local MCP client launches directly.
    Stdio,
    /// Streamable HTTP: a single POST endpoint. Stateless since the 2026-07-28
    /// revision, so there is no session to keep.
    Http,
}

/// The normalized `@mcp.serve` configuration.
///
/// The source may write a bare string (the server name) or a record; both arrive here
/// as the same struct, resolved in the evaluator where the atom interner is reachable.
#[derive(Clone, Debug)]
pub struct McpServeConfig {
    pub name: String,
    pub version: String,
    pub transport: McpTransport,
    /// Only meaningful for [`McpTransport::Http`].
    pub addr: String,
    pub instructions: Option<String>,
    /// The `ttlMs` freshness hint stamped on list results. The declaration tables are
    /// fixed once `serve` starts, so they are safely cacheable for a long time.
    pub list_ttl_ms: i64,
}

impl McpServeConfig {
    /// Everything but the name has a usable default, so `@mcp.serve "name"` works.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: "0.1.0".into(),
            transport: McpTransport::Stdio,
            addr: "127.0.0.1:0".into(),
            instructions: None,
            list_ttl_ms: 3_600_000,
        }
    }
}

/// Emits a server-to-client notification for the request being served.
///
/// Deliberately simpler than [`HttpNextInvoker`]: a notification has no response, so
/// this is synchronous and fire-and-forget, and `@mcp.progress` can stay an ordinary
/// capability call instead of needing a callable `Value::Handle` and an arm in
/// `call_value`.
pub type McpNotifier = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

/// Next id for a freshly constructed closure.
pub fn next_closure_id() -> u64 {
    CLOSURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone)]
pub struct FunctionEntry {
    pub params: Vec<String>,
    /// Shared, not copied: the functions map is cloned per HTTP/TCP/MCP
    /// handler dispatch, and bodies are immutable once built.
    pub body: std::sync::Arc<BlockIr>,
}

impl RuntimeContext {
    pub fn new() -> Self {
        Self {
            env: Environment::new(),
            atoms: Arc::new(AtomInterner::new()),
            budget: ExecutionBudget::new(),
            sources: SourceMap::new(),
            stdout: Vec::new(),
            stderr: Vec::new(),
            capabilities: Arc::new(NopCapabilities),
            functions: HashMap::new(),
            call_depth: 0,
            rng_seed: None,
            call_stack: Vec::new(),
            module_roots: Vec::new(),
            script_dir: None,
            allow_all: false,
            // Console is allowed by the default-secure policy; a host that denies it
            // sets this to false when it installs capabilities.
            console_allowed: true,
            script_args: Vec::new(),
            pending_http: None,
            pending_mcp: None,
            mcp_notify: None,
            http_next: None,
            sink: None,
            handles: Arc::new(crate::handles::HandleTable::default()),
        }
    }

    /// A context for running one branch of `parallel` concurrently with others.
    ///
    /// The evaluator holds `&mut RuntimeContext`, so two closures cannot run at
    /// once against one context — each branch needs its own. What must be shared
    /// is shared by construction rather than copied:
    ///
    /// * `capabilities` — an `Arc`, so host state (`@store`, `@game`, open `@db`
    ///   handles) is the same object every branch sees.
    /// * `budget` — counts through an `Arc`, so branches spend from one allowance
    ///   instead of each getting a fresh one.
    /// * `atoms` — one interner, so an atom created in a branch resolves in the
    ///   parent afterwards.
    /// * `env` — frames are `Arc<RwLock<_>>`; the branch shares the globals it was
    ///   defined against, and the callee pushes its own frame for locals.
    ///
    /// Output is *not* shared. Each branch buffers its own, and `parallel` splices
    /// them back in input order, so output does not interleave by scheduling
    /// accident — the same program prints the same thing twice running.
    ///
    /// A sink, if one is installed, is deliberately dropped: streaming
    /// concurrently would produce exactly the interleaving the buffering avoids.
    pub fn fork(&self) -> Self {
        Self {
            env: self.env.clone(),
            atoms: Arc::clone(&self.atoms),
            budget: self.budget.clone(),
            sources: self.sources.clone(),
            stdout: Vec::new(),
            stderr: Vec::new(),
            capabilities: Arc::clone(&self.capabilities),
            functions: self.functions.clone(),
            call_depth: self.call_depth,
            rng_seed: self.rng_seed,
            call_stack: self.call_stack.clone(),
            module_roots: self.module_roots.clone(),
            script_dir: self.script_dir.clone(),
            allow_all: self.allow_all,
            console_allowed: self.console_allowed,
            script_args: self.script_args.clone(),
            pending_http: None,
            pending_mcp: None,
            mcp_notify: None,
            http_next: None,
            sink: None,
            // Shared, like the capability host: a handle opened before the fork
            // must still work inside a branch, and one a branch opens must not
            // vanish when that branch's context is dropped.
            handles: Arc::clone(&self.handles),
        }
    }

    /// Take a finished branch's output back into this context, in order.
    pub fn absorb(&mut self, branch: Self) {
        for line in branch.stdout {
            self.print(line);
        }
        for line in branch.stderr {
            self.print_err(line);
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
pub(crate) fn is_runtime_builtin(name: &str) -> bool {
    rite_sem::resolve::BUILTIN_NAMES.contains(&name)
}
