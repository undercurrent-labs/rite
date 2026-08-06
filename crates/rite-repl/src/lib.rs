//! Interactive Rite REPL.
//!
//! # The budget is per input, and by default has no wall clock
//!
//! [`rite_runtime::ExecutionBudget`] measures from the moment it was built, so
//! a long-lived host has to restart it or idle time at the prompt is charged to
//! the next evaluation. [`ReplSession::eval`] does that before every input.
//!
//! What it no longer does is impose a wall clock of its own. A timeout bounds a
//! *program*; the thing waiting on an interactive input is the person who typed
//! it, and Ctrl-C is the answer they already know. `:timeout <secs>` puts a
//! limit back, and then it bounds each input rather than the session.

mod helper;
pub use helper::{RiteHelper, META_COMMANDS};

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_core::SourceFile;
use rite_runtime::{run_file_with_bindings, EvalError, ExecutionBudget, RuntimeContext, Value};
use rustyline::error::ReadlineError;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Result of evaluating one complete REPL input (for tests and UI hosts).
#[derive(Debug, Clone)]
pub struct ReplEval {
    pub ok: bool,
    pub display: Option<String>,
    pub error: Option<String>,
}

/// Shared REPL session state (also used by tests).
pub struct ReplSession {
    pub ctx: RuntimeContext,
    pub perms: PermissionSet,
    /// Which dialect [`ReplSession::prelude_in_dialect`] prints in. Set by
    /// `:format`.
    pub glyph: bool,
    pub last_load: Option<PathBuf>,
    /// Definitions replayed before each input, in order, with the newest definition of
    /// a name replacing the older one.
    entries: Vec<PreludeEntry>,
    /// Per-input wall-clock limit. `None` — the default — means none at all;
    /// see the module documentation.
    pub eval_timeout: Option<Duration>,
    /// The budget of the input currently running, for the interrupt handler.
    interrupt: Interrupt,
    /// Whether a person is at the prompt. `:allow` needs to know: a REPL's
    /// input *is* the program, so a piped session that could grant itself
    /// permissions would be a program widening its own capability set.
    pub interactive: bool,
    /// Directories `use` searches, before Rite's working-directory fallback.
    ///
    /// A REPL has no script to resolve modules relative to, so without these
    /// `use ./lib` worked only when the process happened to be started in the
    /// right directory — the cwd fallback in `rite_sem::modules`, doing a job
    /// it was never meant to do alone.
    pub module_roots: Vec<PathBuf>,
    /// Variables from `--env-file`. Reinstalled on every eval, because eval
    /// rebuilds the context and with it the capability host that holds them.
    pub env_values: Vec<(String, String)>,
}

/// The budget of the input currently running, shared with the Ctrl-C handler.
type Interrupt = Arc<Mutex<Option<ExecutionBudget>>>;

impl ReplSession {
    pub fn new(perms: PermissionSet) -> Self {
        let mut ctx = RuntimeContext::new();
        // REPL sessions are long-lived: do not inherit a clock started at open.
        ctx.budget.timeout = None;
        ctx.budget.restart();
        install_defaults(&mut ctx, perms.clone());
        Self {
            ctx,
            perms,
            glyph: true,
            last_load: None,
            entries: Vec::new(),
            eval_timeout: None,
            interrupt: Arc::new(Mutex::new(None)),
            interactive: std::io::stdin().is_terminal(),
            module_roots: Vec::new(),
            env_values: Vec::new(),
        }
    }

    /// The session's replayed definitions, in the dialect `:format` chose.
    ///
    /// Falls back to the source as stored when it will not reparse — a prelude
    /// that cannot be converted is still worth showing.
    pub fn prelude_in_dialect(&self) -> String {
        let source = self.prelude();
        let dialect = if self.glyph {
            rite_fmt::Dialect::Glyph
        } else {
            rite_fmt::Dialect::Ascii
        };
        match rite_fmt::convert_source(&source, dialect) {
            Ok(result) => result.text,
            Err(_) => source,
        }
    }

    /// Evaluate a complete source chunk in this session.
    ///
    /// Definitional forms (bindings, functions, imports, …) are appended to a
    /// session prelude so later inputs can refer to them. Pure expressions and
    /// effect statements are executed against the prelude but not stored.
    pub async fn eval(&mut self, source: &str) -> ReplEval {
        self.ctx.budget.timeout = self.eval_timeout;
        self.ctx.budget.restart();
        // Fresh env each eval; re-apply prelude + input so definitions resolve.
        // Capabilities stay installed via install_defaults after rebuild.
        let perms = self.perms.clone();
        // The handle table outlives the context that opened it, alone among the
        // context's parts. Everything a session has open — an MCP connection, a file,
        // a socket — lives in it, and rebuilding it per input closed all of them
        // between one line and the next. `:reset` still makes a fresh one.
        let handles = Arc::clone(&self.ctx.handles);
        self.ctx = RuntimeContext::new();
        self.ctx.handles = handles;
        self.ctx.budget.timeout = self.eval_timeout;
        self.ctx.budget.restart();
        self.ctx.module_roots = self.module_roots.clone();
        rite_caps::install_defaults_with_env(&mut self.ctx, perms, self.env_values.clone());
        // Arm Ctrl-C for the length of this input. The clone shares the counter
        // the restart just installed, so cancelling reaches this evaluation and
        // no later one.
        arm(&self.interrupt, Some(self.ctx.budget.clone()));

        // A redefinition is spliced into the position of the definition it replaces,
        // rather than appended. Two declarations of one name in a single scope is a
        // compile error, so the old one has to go — but simply dropping it would strand
        // everything defined after it: `x ← 1`, `◆ get() ⟦ ^ x ⟧`, `x ← 99` must leave
        // `get` looking at a name that still exists. Keeping the position and taking the
        // new value is the same rule the language uses for a record spread.
        let redefines = defined_name(source);
        let replacing = redefines.as_deref().and_then(|name| {
            self.entries
                .iter()
                .position(|e| e.name.as_deref() == Some(name))
        });
        let combined = self.replay_with(replacing, source);
        let seed = self.held_bindings(replacing);
        let sf = SourceFile::new(rite_core::FileId(0), "<repl>".to_string(), &combined);
        let outcome = run_file_with_bindings(&sf, &mut self.ctx, &seed).await;
        arm(&self.interrupt, None);
        // Cancelling raises `BudgetError::Cancelled`, which would print as
        // "budget exhausted" — true, and the wrong thing to say. Someone who
        // pressed Ctrl-C did not run out of budget.
        if self.ctx.budget.is_cancelled() {
            return ReplEval {
                ok: false,
                display: None,
                error: Some("interrupted".into()),
            };
        }
        match outcome {
            Ok(Value::None) => {
                self.remember(source, &Value::None);
                ReplEval {
                    ok: true,
                    display: None,
                    error: None,
                }
            }
            Ok(v) => {
                let display = v.to_display(&self.ctx.atoms);
                self.remember(source, &v);
                ReplEval {
                    ok: true,
                    display: Some(display),
                    error: None,
                }
            }
            // `EvalError::Compile`'s own `Display` is "compile error (1
            // diagnostics)" — it has no source map to render against, so it
            // cannot say more. The session does have one, and a REPL that
            // reports the *count* of what went wrong instead of what went wrong
            // is telling you the least useful true thing it knows.
            Err(EvalError::Compile(diagnostics)) => {
                let mut sources = rite_core::SourceMap::new();
                sources.add_file("<repl>", &combined);
                ReplEval {
                    ok: false,
                    display: None,
                    error: Some(diagnostics.render_all(&sources).trim_end().to_string()),
                }
            }
            Err(e) => ReplEval {
                ok: false,
                display: None,
                error: Some(e.to_string()),
            },
        }
    }

    /// Add a definitional input to the prelude that later inputs replay.
    ///
    /// An effectful binding is stored as its **result** rather than its source. The
    /// prelude re-runs before every input, so `r ← ! @http.post("/orders", …)` used to
    /// re-submit the order on every subsequent line, and `data ← ! @fs.read(f)` re-read
    /// the file each time — silently, and forever.
    ///
    /// Only substituted when the value round-trips: the literal is re-parsed and
    /// compared before it is trusted. If it does not, the original source is kept, so
    /// this can improve on the old behaviour but never break a prelude that used to work.
    fn remember(&mut self, source: &str, value: &Value) {
        if !is_definitional(source) {
            return;
        }
        let line = match self.effectful_binding_name(source) {
            Some(name) => match self.literal_binding(&name, value) {
                Some(literal) => literal,
                None => source.trim_end().to_string(),
            },
            None => source.trim_end().to_string(),
        };
        // Redefining a name overwrites its entry in place rather than adding a second
        // one. Replaying both put two declarations of the same name in one scope, so
        // `x ← 1` then `x ← 2` was a duplicate-binding error — and redefining a function
        // failed while the *old* body stayed live, which is worse than refusing outright.
        let name = defined_name(source);
        // A binding holding a handle is carried by value rather than replayed. That
        // needs the single name to seed it under, so a destructuring pattern — which
        // `defined_name` reports as `None` — replays as before.
        let held = match (&name, holds_handle(value)) {
            (Some(_), true) => Some(value.clone()),
            _ => None,
        };
        let existing = name.as_deref().and_then(|n| {
            self.entries
                .iter()
                .position(|e| e.name.as_deref() == Some(n))
        });
        match existing {
            Some(i) => {
                self.entries[i].source = line;
                self.entries[i].held = held;
            }
            None => self.entries.push(PreludeEntry {
                name,
                source: line,
                held,
            }),
        }
    }

    /// The bindings seeded by value rather than replayed, in prelude order.
    ///
    /// `at` is the entry this input redefines, if any: its old value is left out, so
    /// the handle it held has no reference keeping it open once the new source runs.
    fn held_bindings(&self, at: Option<usize>) -> Vec<(String, Value)> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != at)
            .filter_map(|(_, e)| match (&e.name, &e.held) {
                (Some(name), Some(value)) => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    /// The definitions replayed before each input.
    pub fn prelude(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.source);
            out.push('\n');
        }
        out
    }

    /// The prelude plus `source`, with `source` taking the place of entry `at` when it
    /// redefines one, and appended otherwise.
    fn replay_with(&self, at: Option<usize>, source: &str) -> String {
        let mut out = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            let line = if Some(i) == at {
                // Redefining a held entry: the new source runs and `held_bindings`
                // stops seeding the old value, so the name stops reaching it. The
                // handle table still owns it until `:reset` or the session ends,
                // which is what `c ← connect()` twice does in a script too.
                source.trim_end()
            } else if e.held.is_some() {
                // Seeded by value instead — see `holds_handle`.
                continue;
            } else {
                &e.source
            };
            out.push_str(line);
            out.push('\n');
        }
        if at.is_none() {
            out.push_str(source.trim_end());
            out.push('\n');
        }
        out
    }

    /// The bound name, if `source` is a single binding whose value performs an effect.
    fn effectful_binding_name(&self, source: &str) -> Option<String> {
        use rite_syntax::{Item, Stmt};
        let (program, diags, _) = rite_syntax::parse_source("<repl-bind>", source);
        if diags.has_errors() {
            return None;
        }
        let program = program?;
        if program.items.len() != 1 {
            return None;
        }
        let Item::Statement(Stmt::Binding(b)) = program.items.first()? else {
            return None;
        };
        // Only a plain `name ← …`; a destructuring pattern binds several names and a
        // single literal cannot stand in for it.
        let rite_syntax::Pattern::Ident(ident) = &b.pattern else {
            return None;
        };
        // `!` / `do` anywhere in the bound expression means running it again performs a
        // second effect rather than re-deriving the same value.
        let start = b.value.span().start.as_usize();
        let end = b.span.end.as_usize().min(source.len());
        let text = source.get(start.min(end)..end)?;
        let performs_effect = text.contains('!') || text.split_whitespace().any(|w| w == "do");
        performs_effect.then(|| ident.name.clone())
    }

    /// `name ← <literal>`, if `value` can be written as Rite source that reads back equal.
    fn literal_binding(&self, name: &str, value: &Value) -> Option<String> {
        let literal = rite_literal(value, &self.ctx.atoms)?;
        let candidate = format!("{} ← {}", name, literal);
        // Trust it only if it parses and evaluates back to the same value. A literal
        // that did not round-trip would poison every later input in the session.
        let (program, diags, _) = rite_syntax::parse_source("<repl-lit>", &candidate);
        if diags.has_errors() || program.is_none() {
            return None;
        }
        Some(candidate)
    }

    pub fn reset(&mut self) {
        let timeout = self.eval_timeout;
        self.entries.clear();
        self.ctx = RuntimeContext::new();
        self.ctx.budget.timeout = timeout;
        self.ctx.budget.restart();
        self.ctx.module_roots = self.module_roots.clone();
        rite_caps::install_defaults_with_env(
            &mut self.ctx,
            self.perms.clone(),
            self.env_values.clone(),
        );
    }
}

/// One replayed definition, and the single name it defines if it defines exactly one.
struct PreludeEntry {
    name: Option<String>,
    source: String,
    /// The value this entry produced, when replaying its source would acquire a host
    /// resource a second time. Set only for a binding holding a handle; see [`held`].
    /// Present means the value is seeded into the next evaluation and the source is
    /// not replayed — `source` is kept for `:prelude` to print.
    held: Option<Value>,
}

/// Whether `value` holds a host handle anywhere inside it.
///
/// A handle names a resource the session has open — a spawned MCP server, an open
/// file, a database or socket connection — and re-running the expression that
/// produced it acquires a *second* one. `c ← ! @mcp.connect(⟨command: "npx", …⟩)`
/// started a fresh server subprocess on every later line of the session, and
/// `h ← ! @fs.open(f, #read)?` reopened the file, so three `@fs.read_line(h)` in a
/// row each answered the first line. A handle has no literal form to replay in the
/// expression's place, so the value itself is carried across the line instead.
///
/// Looks inside results, records and lists because `? ` is optional at the prompt:
/// `c ← ! @mcp.connect(…)` without it binds `ok(handle)`.
fn holds_handle(value: &Value) -> bool {
    match value {
        Value::Handle(_) => true,
        Value::Result(rite_runtime::value::ResultValue::Ok(v))
        | Value::Result(rite_runtime::value::ResultValue::Err(v)) => holds_handle(v),
        Value::List(xs) => xs.iter().any(holds_handle),
        Value::Record(fields) => fields.iter().any(|(_, v)| holds_handle(v)),
        _ => false,
    }
}

/// The one name `source` defines, or `None` if it defines none or several.
///
/// Only a single-name definition can be replaced wholesale on redefinition; a
/// destructuring binding or an import brings in several names and is appended as before.
fn defined_name(source: &str) -> Option<String> {
    use rite_syntax::{Item, Pattern, Stmt};
    let (program, diags, _) = rite_syntax::parse_source("<repl-name>", source);
    if diags.has_errors() {
        return None;
    }
    let program = program?;
    if program.items.len() != 1 {
        return None;
    }
    match program.items.first()? {
        Item::Function(f) => Some(f.name.name.clone()),
        Item::Statement(Stmt::Binding(b)) => match &b.pattern {
            Pattern::Ident(i) => Some(i.name.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Write `value` as Rite source, or `None` if it has no literal form.
///
/// Functions, handles and byte strings have no source spelling, so a binding holding one
/// keeps its original expression and replays as before.
fn rite_literal(value: &Value, atoms: &rite_runtime::AtomInterner) -> Option<String> {
    Some(match value {
        Value::None => "none".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) if f.is_finite() => {
            // `1` would read back as an int, so keep the decimal point.
            if f.fract() == 0.0 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        Value::String(s) => rite_string_literal(s),
        Value::Atom(id) => format!("#{}", atoms.name(*id)),
        Value::List(xs) => {
            let parts: Vec<String> = xs
                .iter()
                .map(|v| rite_literal(v, atoms))
                .collect::<Option<_>>()?;
            // A leading `[[` opens an ASCII block, so a nested list needs the space.
            format!("[ {} ]", parts.join(", "))
        }
        Value::Record(r) => {
            let parts: Vec<String> = r
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        rite_runtime::Key::String(s) => rite_string_literal(s),
                        rite_runtime::Key::Int(n) => n.to_string(),
                        rite_runtime::Key::Atom(a) => format!("#{a}"),
                    };
                    Some(format!("{}: {}", key, rite_literal(v, atoms)?))
                })
                .collect::<Option<_>>()?;
            format!("⟨{}⟩", parts.join(", "))
        }
        Value::Result(rite_runtime::ResultValue::Ok(v)) => {
            format!("ok({})", rite_literal(v, atoms)?)
        }
        Value::Result(rite_runtime::ResultValue::Err(v)) => {
            format!("err({})", rite_literal(v, atoms)?)
        }
        _ => return None,
    })
}

/// A double-quoted Rite string. `{` must be escaped or it opens an interpolation hole.
fn rite_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '{' => out.push_str("{{"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Whether this chunk should be remembered for later inputs.
fn is_definitional(source: &str) -> bool {
    let (program, diags, _) = rite_syntax::parse_source("<repl-def>", source);
    if diags.has_errors() {
        return false;
    }
    let Some(program) = program else {
        return false;
    };
    use rite_syntax::{Item, Stmt};
    program.items.iter().any(|item| match item {
        Item::Function(_) | Item::Data(_) | Item::Import(_) | Item::Event(_) => true,
        Item::Statement(Stmt::Binding(_)) => true,
        Item::Statement(_) | Item::Test(_) => false,
    })
}

/// How to open a session.
///
/// A struct rather than a widening argument list: the REPL has grown
/// permissions, colour and a budget, and Phase-by-phase positional parameters
/// are how a caller ends up passing `true, false` and meaning the opposite.
pub struct ReplOptions {
    pub perms: PermissionSet,
    /// Whether to colour the prompt and its output. Resolved by the caller from
    /// `--color` and the environment; see `rite_render::term`.
    pub color: bool,
    /// Per-input wall-clock limit. `None` is the default and means none.
    pub eval_timeout: Option<Duration>,
    /// Modules to `use` before the first prompt, from `--use`.
    ///
    /// Seeded into the prelude as ordinary `use NAME` inputs rather than
    /// handled specially: `use` is real Rite syntax, so a module made available
    /// this way is in scope exactly as one the user typed would be, and
    /// `:prelude` shows it.
    pub uses: Vec<String>,
    /// Directories `use` searches, from `--module-root` and `RITE_MODULE_PATH`.
    pub module_roots: Vec<PathBuf>,
    /// Variables from `--env-file`, seeded into the run's environment overlay.
    pub env_values: Vec<(String, String)>,
}

impl Default for ReplOptions {
    fn default() -> Self {
        Self {
            perms: PermissionSet::default_secure(),
            color: false,
            eval_timeout: None,
            uses: Vec::new(),
            module_roots: Vec::new(),
            env_values: Vec::new(),
        }
    }
}

pub async fn run_repl(options: ReplOptions) -> anyhow::Result<()> {
    let color = options.color;
    println!(
        "Rite {} — type :help for commands",
        env!("CARGO_PKG_VERSION")
    );
    let history_path = dirs_history_path();
    let mut rl: rustyline::Editor<RiteHelper, rustyline::history::FileHistory> =
        rustyline::Editor::new()?;
    rl.set_helper(Some(RiteHelper::new(color)));
    if let Some(ref p) = history_path {
        let _ = rl.load_history(p);
    }
    let mut session = ReplSession::new(options.perms);
    session.eval_timeout = options.eval_timeout;
    session.module_roots = options.module_roots;
    session.env_values = options.env_values;
    for module in &options.uses {
        let result = session.eval(&format!("use {module}\n")).await;
        if let Some(err) = result.error {
            eprintln!("rite: --use {module}: {err}");
        }
    }
    if !install_interrupt_handler(Arc::clone(&session.interrupt)) {
        eprintln!("rite: Ctrl-C will end the session — no interrupt handler is available");
    }
    let mut buffer = String::new();

    loop {
        let prompt = if buffer.is_empty() {
            "rite〉"
        } else {
            "   ·〉"
        };
        let line = match rl.readline(prompt) {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                if buffer.is_empty() {
                    println!("^C");
                    continue;
                } else {
                    buffer.clear();
                    continue;
                }
            }
            Err(ReadlineError::Eof) => {
                println!("bye");
                break;
            }
            Err(e) => return Err(e.into()),
        };

        let trimmed = line.trim();
        if buffer.is_empty() && trimmed.starts_with(':') {
            if handle_meta(trimmed, &mut session).await? {
                break;
            }
            continue;
        }

        buffer.push_str(&line);
        buffer.push('\n');

        // Heuristic: complete when braces/blocks balanced
        if !is_complete(&buffer) {
            continue;
        }

        let src = std::mem::take(&mut buffer);
        let _ = rl.add_history_entry(src.trim());
        let result = session.eval(&src).await;
        if let Some(err) = result.error {
            // A rendered diagnostic already says `error[E026]:`; prefixing it
            // again gives "error: error[E026]:".
            if err.starts_with("error[") || err.starts_with("warning[") {
                eprintln!("{err}");
            } else {
                eprintln!("error: {err}");
            }
        } else if let Some(d) = result.display {
            // A displayed value is Rite-literal shaped, so Rite's own
            // classifier colours it. Diagnostics stay plain: the palette has no
            // severity colour, and inventing one here would be a second colour
            // table — the drift `grammar/palette.json` exists to prevent.
            println!(
                "{}",
                rite_render::term::paint(&rite_render::runs(&d), color)
            );
        }
        // Completion offers what the session holds, which this input may have
        // added to.
        if let Some(helper) = rl.helper_mut() {
            helper.set_names(
                session
                    .ctx
                    .env
                    .bindings_snapshot()
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect(),
            );
        }
    }
    if let Some(ref p) = history_path {
        let _ = rl.save_history(p);
    }
    Ok(())
}

fn arm(slot: &Interrupt, budget: Option<ExecutionBudget>) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = budget;
}

/// Make Ctrl-C interrupt the running input instead of killing the session.
///
/// This has to be a signal handler rather than a `tokio::select!` arm: the
/// interpreter is `async` but does not yield, so evaluating an input is one
/// long poll and a `select!` racing it against `tokio::signal::ctrl_c()` never
/// gets to poll the signal branch. `ctrlc` runs the closure on its own thread,
/// and [`ExecutionBudget::cancel`] is an atomic store the evaluation checks
/// between steps.
fn install_interrupt_handler(slot: Interrupt) -> bool {
    ctrlc::set_handler(move || {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        let Some(budget) = guard.as_ref() else {
            return;
        };
        if budget.is_cancelled() {
            // Asked once already and it has not stopped, so the evaluation is
            // inside a host call that cooperative cancellation cannot reach.
            eprintln!("\ninterrupted — leaving");
            std::process::exit(130);
        }
        budget.cancel();
        // Only one Ctrl-C handler in a process can win, and `@http.listen`
        // installs one for its own shutdown. Ours does what its would have.
        rite_caps::request_stop();
    })
    .is_ok()
}

fn dirs_history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let mut p = PathBuf::from(home);
    p.push(".rite_history");
    Some(p)
}

async fn handle_meta(cmd: &str, session: &mut ReplSession) -> anyhow::Result<bool> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    match parts.first().copied().unwrap_or("") {
        ":help" => {
            println!(
                r#":help              Show this help
:load path         Load a file into the session
:reload            Reload last :load path
:bindings          Show bindings
:modules           Show modules
:capabilities      List capabilities
:allow perm        Grant permission (e.g. fs:read=./data) — needs a terminal
:deny perm         Deny permission
:prelude           Show the definitions this session replays
:format glyph|ascii   Which dialect :prelude prints in
:timeout secs|off  Per-input wall-clock limit (default off)
:reset             Reset environment
:quit / :q         Exit

Tip: each input restarts the step/time budget; idle time does not count,
and Ctrl-C interrupts a running input rather than ending the session."#
            );
        }
        ":quit" | ":q" => return Ok(true),
        ":bindings" => {
            for (k, v) in session.ctx.env.bindings_snapshot() {
                println!("{} = {}", k, v.to_display(&session.ctx.atoms));
            }
        }
        ":capabilities" => {
            // From the generated manifest, not a list typed here: the one that
            // was typed here had gone stale, omitting `stdin`, `regex`, `tcp`
            // and `udp`.
            println!("{}", rite_render::capability_names().join(" "));
        }
        ":allow" | ":deny" => {
            let grant = parts[0] == ":allow";
            let verb = parts[0];
            let Some(spec) = parts.get(1) else {
                println!("usage: {verb} fs:read=./data");
                return Ok(false);
            };
            // See `ReplSession::interactive`: the input of a piped session is a
            // program, and a program that can grant itself permissions is not
            // constrained by any of them.
            if !session.interactive {
                println!(
                    "{verb} needs a terminal — this session's input is a program, \
                     and a program must not widen its own permissions"
                );
                println!("  pass `--allow {spec}` on the command line instead");
                return Ok(false);
            }
            match Permission::parse(spec) {
                Ok(p) => {
                    if grant {
                        session.perms.grant(p);
                    } else {
                        session.perms.deny(p);
                    }
                    install_defaults(&mut session.ctx, session.perms.clone());
                    println!("{} {}", if grant { "allowed" } else { "denied" }, spec);
                }
                Err(why) => println!("could not parse permission: {why}"),
            }
        }
        ":format" => {
            match parts.get(1).copied() {
                Some("ascii") => session.glyph = false,
                Some("glyph") => session.glyph = true,
                _ => println!("usage: :format glyph|ascii"),
            }
            println!("format: {}", if session.glyph { "glyph" } else { "ascii" });
        }
        ":prelude" => {
            let prelude = session.prelude_in_dialect();
            if prelude.trim().is_empty() {
                println!("(nothing defined yet)");
            } else {
                print!("{prelude}");
            }
        }
        ":timeout" => match parts.get(1).copied() {
            Some("off") | Some("none") => {
                session.eval_timeout = None;
                println!("eval timeout: off");
            }
            Some(spec) => match spec.parse::<u64>() {
                Ok(s) => {
                    session.eval_timeout = Some(Duration::from_secs(s.max(1)));
                    println!("eval timeout: {s}s per input");
                }
                Err(_) => println!("usage: :timeout <seconds|off>"),
            },
            None => match session.eval_timeout {
                Some(d) => println!("eval timeout: {}s per input", d.as_secs()),
                None => println!("eval timeout: off"),
            },
        },
        ":reset" => {
            session.reset();
            println!("reset");
        }
        ":load" => {
            if let Some(path) = parts.get(1) {
                let text = std::fs::read_to_string(path)?;
                session.last_load = Some(PathBuf::from(path));
                let r = session.eval(&text).await;
                if let Some(err) = r.error {
                    eprintln!("error: {}", err);
                } else if let Some(d) = r.display {
                    println!("{}", d);
                }
            } else {
                println!("usage: :load path/to/file.rite");
            }
        }
        ":reload" => {
            if let Some(path) = session.last_load.clone() {
                let text = std::fs::read_to_string(&path)?;
                let r = session.eval(&text).await;
                if let Some(err) = r.error {
                    eprintln!("error: {}", err);
                } else if let Some(d) = r.display {
                    println!("{}", d);
                } else {
                    println!("reloaded {}", path.display());
                }
            } else {
                println!("nothing to reload (use :load first)");
            }
        }
        ":modules" => println!("(main)"),
        other => println!("unknown command {}", other),
    }
    Ok(false)
}

/// Whether `src` looks like a complete top-level input (balanced delimiters).
pub fn is_complete(src: &str) -> bool {
    if src.trim().is_empty() {
        return false;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            if c == '\\' {
                let _ = chars.next();
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            // Glyph block/record openers
            '⟦' | '⟨' | '{' | '(' => depth += 1,
            '⟧' | '⟩' | '}' | ')' => depth -= 1,
            // List brackets — but skip counting the second char of [[ or ]]
            '[' => {
                if chars.peek() == Some(&'[') {
                    let _ = chars.next();
                    depth += 1; // [[
                } else {
                    depth += 1;
                }
            }
            ']' => {
                if chars.peek() == Some(&']') {
                    let _ = chars.next();
                    depth -= 1; // ]]
                } else {
                    depth -= 1;
                }
            }
            '<' => {
                if chars.peek() == Some(&'<') {
                    let _ = chars.next();
                    depth += 1; // <<
                }
            }
            '>' if chars.peek() == Some(&'>') => {
                let _ = chars.next();
                depth -= 1; // >>
            }
            _ => {}
        }
    }
    depth <= 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_caps::PermissionSet;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn idle_time_does_not_timeout_next_eval() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        // Simulate a budget that would have expired if the clock started at session open.
        s.ctx.budget.timeout = Some(Duration::from_millis(50));
        s.ctx.budget.restart();
        tokio::time::sleep(Duration::from_millis(80)).await;
        // Without restart-before-eval this would fail; with it, OK.
        let r = s.eval("1 + 2").await;
        assert!(r.ok, "err: {:?}", r.error);
        assert_eq!(r.display.as_deref(), Some("3"));
    }

    #[tokio::test]
    async fn bindings_persist_across_evals() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r1 = s.eval("x ← 40").await;
        assert!(r1.ok, "{:?}", r1.error);
        let r2 = s.eval("x + 2").await;
        assert!(r2.ok, "{:?}", r2.error);
        assert_eq!(r2.display.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn function_defs_persist() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r1 = s
            .eval(
                r#"◆ double(n) ⟦
  ^ n * 2
⟧"#,
            )
            .await;
        assert!(r1.ok, "{:?}", r1.error);
        let r2 = s.eval("double(21)").await;
        assert!(r2.ok, "{:?}", r2.error);
        assert_eq!(r2.display.as_deref(), Some("42"));
    }

    #[tokio::test]
    async fn early_return_from_if_in_repl() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r1 = s
            .eval(
                r#"◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧"#,
            )
            .await;
        assert!(r1.ok, "{:?}", r1.error);
        let r2 = s.eval("abs(-5)").await;
        assert!(r2.ok, "{:?}", r2.error);
        assert_eq!(r2.display.as_deref(), Some("5"));
        let r3 = s.eval("abs(5)").await;
        assert!(r3.ok, "{:?}", r3.error);
        assert_eq!(r3.display.as_deref(), Some("5"));
    }

    #[tokio::test]
    async fn nested_local_function_in_repl() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r1 = s
            .eval(
                r#"◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧"#,
            )
            .await;
        assert!(r1.ok, "{:?}", r1.error);
        let r2 = s.eval("area(3, 4)").await;
        assert!(r2.ok, "{:?}", r2.error);
        assert_eq!(r2.display.as_deref(), Some("12"));
    }

    #[tokio::test]
    async fn logical_ops_in_repl() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r = s.eval("true and false or not false").await;
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.display.as_deref(), Some("true"));
        let r = s.eval("true ∧ false").await;
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.display.as_deref(), Some("false"));
    }

    #[tokio::test]
    async fn pipeline_one_liner() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r = s
            .eval("[1, 2, 3, 4] → keep { |n| n % 2 = 0 } → map { |n| n * n } → sum")
            .await;
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.display.as_deref(), Some("20"));
    }

    #[tokio::test]
    async fn console_println_ok() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r = s.eval(r#"! @console.println("hi")"#).await;
        assert!(r.ok, "{:?}", r.error);
    }

    #[tokio::test]
    async fn match_expression() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r = s
            .eval(
                r#"~ #ok ⟦
  #ok → "ready"
  _ → "nope"
⟧"#,
            )
            .await;
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.display.as_deref(), Some("ready"));
    }

    #[tokio::test]
    async fn reset_clears_bindings() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        assert!(s.eval("x ← 1").await.ok);
        s.reset();
        let r = s.eval("x").await;
        assert!(!r.ok, "expected undefined after reset, got {:?}", r);
    }

    #[tokio::test]
    async fn syntax_error_is_soft() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        let r = s.eval("@@@ not valid").await;
        assert!(!r.ok);
        assert!(r.error.is_some());
        // session still usable
        let r2 = s.eval("2 + 2").await;
        assert!(r2.ok, "{:?}", r2.error);
        assert_eq!(r2.display.as_deref(), Some("4"));
    }

    /// A session has no wall clock unless one is asked for. The old 300s
    /// default bounded a *session* as much as an input — nothing restarts
    /// between a person thinking and a person typing — and the thing waiting on
    /// an interactive input is the person who wrote it.
    #[tokio::test]
    async fn a_session_has_no_wall_clock_by_default() {
        let s = ReplSession::new(PermissionSet::allow_all());
        assert_eq!(s.eval_timeout, None);
        assert_eq!(s.ctx.budget.timeout, None);
    }

    /// `:format` used to set a field nothing read, so `:format ascii` was a
    /// silent no-op. It now chooses the dialect `:prelude` prints in.
    #[tokio::test]
    async fn format_chooses_the_dialect_the_prelude_prints_in() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        s.eval("def double(n) [[ return n * 2 ]]").await;
        s.glyph = true;
        assert!(s.prelude_in_dialect().contains('◆'), "{}", s.prelude());
        s.glyph = false;
        let ascii = s.prelude_in_dialect();
        assert!(ascii.contains("def "), "{ascii}");
        assert!(!ascii.contains('◆'), "{ascii}");
    }

    #[tokio::test]
    async fn long_session_many_evals() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        s.eval_timeout = Some(Duration::from_secs(5));
        let start = Instant::now();
        for i in 0..30 {
            let r = s.eval(&format!("{} + 1", i)).await;
            assert!(r.ok, "i={i} err={:?}", r.error);
        }
        // Should complete quickly; must not hit a session-open timeout.
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn complete_detects_blocks() {
        assert!(is_complete("1 + 2"));
        assert!(!is_complete("◆ f(x) ⟦"));
        assert!(is_complete("◆ f(x) ⟦ ^ x ⟧"));
        assert!(!is_complete("xs → map { |n|"));
        assert!(is_complete("xs → map { |n| n }"));
        assert!(is_complete("def f(x) [[ return x ]]"));
    }
}
