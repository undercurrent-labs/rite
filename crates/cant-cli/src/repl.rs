//! The Cant REPL.
//!
//! # What persists, and why it is not a language feature
//!
//! A Cant program is one flow: no declarations, no bindings, no statements.
//! The language does not change here; the *session* does. Alongside the
//! permissions and budget it started with, a session carries a workbench of
//! named **values**. `:let x = <program>` runs the program and keeps its
//! answer, and `it` is always the last answer.
//!
//! `:let` is a meta-command, not syntax: a `.cant` file containing it does not
//! parse, and no program can bind anything. What persists is the value, not the
//! flow that produced it, so re-using `x` re-runs nothing and repeats no
//! effects. Bindings reach the next line as a generated-Rite preamble (`x <- 5`
//! above the stage functions), so a bound name is an ordinary Rite name inside
//! every stage. Only data values can be bound; a handle or a function has no
//! literal to write, and the refusal says which.
//!
//! # The budget is per line, not per session
//!
//! [`rite_runtime::ExecutionBudget`] derives `Clone` over a `started: Instant`
//! and shares its step counter through an `Arc`, so a budget cloned once per
//! line measures the *session*, not the line. Left alone, a session became
//! unusable sixty seconds after it opened: idle time at the prompt was charged
//! to the next program, which failed with "execution wall-clock timeout
//! exceeded" and kept failing. [`run_line`] calls `restart()` before every
//! evaluation.
//!
//! A session also defaults to **no** wall clock; see `Commands::Repl` in
//! `main.rs`. A timeout bounds a program, and the thing waiting on an
//! interactive line is the person who typed it. `--timeout` still applies if
//! asked for, and then bounds each line rather than the session.
//!
//! # Meta-commands
//!
//! `:expand`, `:graph` and `:explain` take the rest of the line and show that
//! view of it instead of running it; `:trace` runs it and shows per-node
//! emission counts beside the value; `:let` and `:bindings` are the workbench;
//! `:permissions`, `:allow`, `:deny`, `:timeout` and `:steps` are the session's
//! own settings.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cant_syntax::{Dialect, FormatOptions};
use rite_caps::{Permission, PermissionSet};
use rite_runtime::ExecutionBudget;

use crate::highlight::{paint, CantHelper};

/// Colour a value the way the source that produced it would be coloured.
///
/// A displayed value is Rite-literal shaped (`[2, 4]`, `"text"`, `<< a: 1 >>`),
/// so Rite's own classifier applies. It is lossless, so nothing is dropped on
/// the way to the screen.
fn paint_value(display: &str, color: bool) -> String {
    rite_render::term::paint(&rite_render::runs(display), color)
}

/// Diagnostics are deliberately **not** coloured.
///
/// `grammar/palette.json` has no severity colour, and a red invented here would
/// be a second colour table. Only source-shaped output is coloured, where the
/// palette applies.
const _DIAGNOSTICS_ARE_PLAIN: () = ();

const BANNER: &str = "\
cant — each line is a whole program. Values can persist; programs cannot.
  :help                what you can type
  x <- <program>       run it, keep the answer as `x` (`it` is the last answer)
  :expand <program>    the Rite it becomes
  :graph <program>     its topology, as DOT
  :explain <program>   what it does, in prose
  ~> <program>         run it, with per-node emission counts
  :quit                leave";

const HELP: &str = "\
Every line is a complete Cant program: a flow, evaluated and printed.
The language has no bindings — but the session has a workbench of values:

  [1, 2, 3] -> * -> ?{ $ > 1 } -> []      run it (the answer becomes `it`)
  evens <- [2, 4, 6]                      run and keep the answer as `evens`
  evens -> * -> $ * 10 -> []              a bound name works in any stage
  :bindings                               what the workbench holds
  :expand  <program>                      the canonical Rite it becomes
  :graph   <program>                      its topology, as Graphviz DOT
  :explain <program>                      what it does, in prose
  :fmt     <program>                      the same program, formatted
  ~> <program>                            run it, with per-node emission counts
  :permissions                            what this session may reach
  :allow <spec> / :deny <spec>            change that, e.g. `:allow env`
  :timeout <30s|off> / :steps <n|off>     the per-line budget
  :help                                   this
  :quit                                   leave (or Ctrl-D)

A binding holds the value, not the program: nothing re-runs and no effect
repeats. Only data values can be bound. The arrows are sugar for `:let` and
`:trace`, and take the glyphs too: `x ← …`, `⟿ …`. To compare against a
negative number rather than bind, space the operator: `x < -3`.

The budget is per line, not per session, and a session has no wall clock
unless `--timeout` asked for one — Ctrl-C interrupts a running line instead.";

/// Everything a session carries between lines.
struct Session {
    permissions: PermissionSet,
    budget: ExecutionBudget,
    bindings: Bindings,
    /// Whether a person is at the prompt.
    ///
    /// `:allow` needs to know. A REPL's input *is* the program, so in
    /// `cat untrusted.txt | cant repl` a self-granting meta-command would let
    /// the program widen its own capability set. With a terminal on the other
    /// end, the person typing is the authority.
    interactive: bool,
    interrupt: Interrupt,
    /// Whether output carries colour. Resolved once from `--color` and the
    /// environment; see `rite_render::term`.
    color: bool,
    /// The modules every line may reach, and where they are found. Comes from
    /// `--use` / `CANT_USE` / `cant.toml`; `:use` adds to it live. The preamble
    /// field is unused: session bindings live in `bindings` and are folded in
    /// per line.
    environment: cant::Environment,
    /// Variables from `--env-file`, seeded into every line's environment.
    env_values: Vec<(String, String)>,
}

/// The budget of the line currently running, for the interrupt handler.
///
/// `None` between lines. A Ctrl-C at the prompt is rustyline's: the terminal is
/// in raw mode there, so it arrives as a byte rather than a signal. One that
/// lands while nothing is running has nothing to cancel.
type Interrupt = Arc<Mutex<Option<ExecutionBudget>>>;

/// Make Ctrl-C interrupt the running line instead of killing the session.
///
/// This has to be a signal handler rather than a `tokio::select!` arm. The
/// interpreter is `async` but does not yield: evaluating a flow is one long
/// poll, so a `select!` racing the evaluation against `tokio::signal::ctrl_c()`
/// never polls the signal branch, and the Ctrl-C arrives only once the program
/// it was meant to stop has finished. `ctrlc` runs the closure on its own
/// thread, and [`ExecutionBudget::cancel`] is an atomic store the evaluation
/// checks between steps.
///
/// Returns `false` when the platform, or something else in the process, will
/// not give up the handler. The session still works; Ctrl-C ends it.
fn install_interrupt_handler(slot: Interrupt) -> bool {
    ctrlc::set_handler(move || {
        let guard = slot.lock().unwrap_or_else(|e| e.into_inner());
        let Some(budget) = guard.as_ref() else {
            return;
        };
        if budget.is_cancelled() {
            // Asked once already and it has not stopped, so the evaluation is
            // inside a host call that cooperative cancellation cannot reach,
            // such as a socket read with no deadline. Leave with the status a
            // shell expects from an interrupted program.
            eprintln!("\ninterrupted — leaving");
            std::process::exit(130);
        }
        budget.cancel();
        // A server started by this line owns the terminal until it stops, and
        // `@http.listen` installs this same handler for that reason. Only one
        // handler can win, so ours also stops the servers.
        rite_caps::request_stop();
    })
    .is_ok()
}

pub async fn run(
    permissions: PermissionSet,
    budget: ExecutionBudget,
    color: bool,
    environment: cant::Environment,
    env_values: Vec<(String, String)>,
) -> ExitCode {
    let mut editor: rustyline::Editor<CantHelper, rustyline::history::FileHistory> =
        match rustyline::Editor::new() {
            Ok(editor) => editor,
            Err(e) => {
                eprintln!("cant: cannot start the REPL: {e}");
                return ExitCode::from(1);
            }
        };
    editor.set_helper(Some(CantHelper::new(color)));
    let history = history_path();
    if let Some(path) = &history {
        let _ = editor.load_history(path);
    }
    println!("{BANNER}\n");
    let interrupt: Interrupt = Arc::new(Mutex::new(None));
    let interruptible = install_interrupt_handler(Arc::clone(&interrupt));
    let mut session = Session {
        permissions,
        budget,
        bindings: Bindings::default(),
        interactive: std::io::stdin().is_terminal(),
        interrupt,
        color,
        environment,
        env_values,
    };
    if !interruptible {
        eprintln!("cant: Ctrl-C will end the session — no interrupt handler is available");
    }

    let status = loop {
        match editor.readline("cant> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = editor.add_history_entry(trimmed);
                if let Some(helper) = editor.helper_mut() {
                    helper.observe(trimmed);
                }
                if !handle(trimmed, &mut session).await {
                    break ExitCode::SUCCESS;
                }
                // Completion offers what the workbench holds, which the line
                // just run may have added to.
                if let Some(helper) = editor.helper_mut() {
                    helper.set_names(session.bindings.names());
                }
            }
            // Ctrl-C abandons the line, Ctrl-D leaves: the conventions every
            // other REPL uses.
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => break ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("cant: {e}");
                break ExitCode::from(1);
            }
        }
    };
    if let Some(path) = &history {
        let _ = editor.save_history(path);
    }
    status
}

/// Where the line history lives, or `None` when there is no home directory to
/// put it in. Mirrors `rite-repl`'s `~/.rite_history`.
fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let mut path = PathBuf::from(home);
    path.push(".cant_history");
    Some(path)
}

/// The session's workbench: named values, as the Rite literals that rebuild
/// them. Values, not programs — see the module documentation.
#[derive(Default)]
struct Bindings {
    /// Insertion order; a rebind replaces in place, so the preamble stays
    /// stable and `:bindings` keeps its order.
    entries: Vec<(String, String)>,
}

impl Bindings {
    fn set(&mut self, name: &str, literal: String) {
        match self.entries.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = literal,
            None => self.entries.push((name.to_string(), literal)),
        }
    }

    /// The generated-Rite preamble: one `name <- literal` line per binding.
    fn preamble(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|(n, l)| format!("{n} <- {l}"))
            .collect()
    }

    /// Just the names, for completion.
    fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(n, _)| n.clone()).collect()
    }
}

/// Handle one line. Returns `false` when the session should end.
async fn handle(line: &str, session: &mut Session) -> bool {
    if let Some(rest) = meta(line, ":quit").or_else(|| meta(line, ":q")) {
        let _ = rest;
        return false;
    }
    if meta(line, ":help").is_some() || meta(line, ":h").is_some() {
        println!("{HELP}");
        return true;
    }
    if let Some(program) = meta(line, ":expand") {
        show_expand(program, session.color);
        if !session.bindings.entries.is_empty() {
            let names: Vec<&str> = session
                .bindings
                .entries
                .iter()
                .map(|(n, _)| n.as_str())
                .collect();
            println!(
                "// plus this session's bindings, emitted above the functions: {}",
                names.join(", ")
            );
        }
        return true;
    }
    if meta(line, ":bindings").is_some() {
        if session.bindings.entries.is_empty() {
            println!("nothing bound — `:let x = <program>` keeps an answer");
        }
        for (name, literal) in &session.bindings.entries {
            let shown = if literal.chars().count() > 60 {
                let cut: String = literal.chars().take(57).collect();
                format!("{cut}...")
            } else {
                literal.clone()
            };
            println!("{name} <- {shown}");
        }
        return true;
    }
    if meta(line, ":permissions").is_some() {
        println!("{}", describe_permissions(&session.permissions));
        return true;
    }
    if let Some(spec) = meta(line, ":allow") {
        return change_permission(spec, true, session);
    }
    if let Some(spec) = meta(line, ":deny") {
        return change_permission(spec, false, session);
    }
    if let Some(rest) = meta(line, ":timeout") {
        return set_timeout(rest, session);
    }
    if let Some(rest) = meta(line, ":steps") {
        return set_steps(rest, session);
    }
    if meta(line, ":uses").is_some() {
        if session.environment.uses.is_empty() {
            println!("no modules loaded — `:use NAME` adds one");
        } else {
            println!("{}", session.environment.uses.join("  "));
        }
        if !session.environment.module_roots.is_empty() {
            let roots: Vec<String> = session
                .environment
                .module_roots
                .iter()
                .map(|r| r.display().to_string())
                .collect();
            println!("searched: {}", roots.join(", "));
        }
        return true;
    }
    if let Some(name) = meta(line, ":use") {
        return add_use(name, session);
    }
    if let Some(program) = meta(line, ":fmt") {
        show_fmt(program, session.color);
        return true;
    }
    // Sugar, before the plain-program fallthrough. `x <- <program>` binds, in
    // the shape Rite spells a binding and the shape `:bindings` prints, so
    // what reads back is what was typed. `~> <program>` traces. Both have
    // glyph twins (`←`, `⟿`). To *compare* against a negative number instead
    // of binding, space the operator: `x < -3`.
    if let Some(program) = sugar_prefix(line, &["~>", "⟿"]) {
        return trace_command(program, session).await;
    }
    if let Some((name, program)) = binding_sugar(line) {
        return let_command(&name, &program, session).await;
    }
    if let Some(rest) = meta(line, ":let") {
        let Some((name, program)) = rest.split_once('=') else {
            eprintln!("cant: `:let name = <program>` — the `=` separates the name from the flow");
            return true;
        };
        let (name, program) = (name.trim().to_string(), program.trim().to_string());
        return let_command(&name, &program, session).await;
    }
    if let Some(program) = meta(line, ":trace") {
        return trace_command(program, session).await;
    }

    if let Some(program) = meta(line, ":graph") {
        show_graph(program);
        return true;
    }
    if let Some(program) = meta(line, ":explain") {
        show_explain(program);
        return true;
    }
    if line.starts_with(':') && !line.starts_with(":=") {
        // A `:name` that is not a meta-command could be an orbit modifier
        // someone pasted without its orbit, so say what happened rather than
        // handing it to the parser and letting `CANT-P008` explain.
        eprintln!("cant: unknown command `{}` — try `:help`", first_word(line));
        return true;
    }

    let Outcome::Ran(result) = run_line(line, session, false).await else {
        return true;
    };

    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if let Some(display) = &result.display {
        println!("{}", paint_value(display, session.color));
    }
    // `it` is the last answer, updated only on success: a failed line must
    // not replace the value the next line is about to use.
    if result.succeeded() {
        if let Some(value) = &result.value {
            if let Ok(literal) = binding_literal(value) {
                session.bindings.set("it", literal);
            }
        }
    }
    true
}

/// `name <- program` (or `name ← program`): the binding sugar, recognised
/// only when everything before the arrow is a single bindable name — so a
/// flow that merely *contains* an arrow-like sequence falls through to run.
fn binding_sugar(line: &str) -> Option<(String, String)> {
    for arrow in ["<-", "←"] {
        if let Some(at) = line.find(arrow) {
            let name = line[..at].trim();
            let program = line[at + arrow.len()..].trim();
            let bindable = !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if bindable && !program.is_empty() {
                return Some((name.to_string(), program.to_string()));
            }
        }
    }
    None
}

/// `~> program` / `⟿ program` → the program; `None` when the line is not
/// that sugar.
fn sugar_prefix<'a>(line: &'a str, arrows: &[&str]) -> Option<&'a str> {
    for arrow in arrows {
        if let Some(rest) = line.strip_prefix(arrow) {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest);
            }
        }
    }
    None
}

/// The body of `:let` and of the binding arrow.
async fn let_command(name: &str, program: &str, session: &mut Session) -> bool {
    if name.is_empty()
        || !name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        eprintln!(
            "cant: `{name}` is not a bindable name — letters, digits and `_`, starting with a letter"
        );
        return true;
    }
    if program.is_empty() {
        eprintln!("cant: a binding needs a program after the arrow");
        return true;
    }
    let Outcome::Ran(result) = run_line(program, session, false).await else {
        return true;
    };
    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if !result.succeeded() {
        return true;
    }
    let Some(value) = result.value else {
        return true;
    };
    match binding_literal(&value) {
        Ok(literal) => {
            if let Some(display) = &result.display {
                println!("{}", paint_value(display, session.color));
            }
            session.bindings.set(name, literal.clone());
            session.bindings.set("it", literal);
        }
        Err(why) => eprintln!("cant: cannot bind `{name}`: {why}"),
    }
    true
}

/// The body of `:trace` and of the trace arrow.
async fn trace_command(program: &str, session: &mut Session) -> bool {
    if program.trim().is_empty() {
        eprintln!("cant: `~> <program>` runs the program and counts emissions per node");
        return true;
    }
    let Outcome::Ran(result) = run_line(program, session, true).await else {
        return true;
    };
    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if let Some(counts) = &result.trace {
        let shown: Vec<String> = counts.iter().map(|(id, n)| format!("{id}:{n}")).collect();
        println!("trace  {}", shown.join("  "));
    }
    if let Some(display) = &result.display {
        println!("{}", paint_value(display, session.color));
    }
    if result.succeeded() {
        if let Some(value) = &result.value {
            if let Ok(literal) = binding_literal(value) {
                session.bindings.set("it", literal);
            }
        }
    }
    true
}

/// What running one line produced.
///
/// A cancelled budget raises `CANT-O001 budget exceeded`, which would report a
/// program as too expensive when the user simply stopped it. `Interrupted`
/// keeps the two apart.
enum Outcome {
    Ran(Box<cant::ExecutionResult>),
    Interrupted,
}

/// Run one program with the session's bindings in scope, interruptibly.
async fn run_line(program: &str, session: &mut Session, trace: bool) -> Outcome {
    // A new line is a new evaluation unit. Without this both the wall clock and
    // the step counter run from session start — see the module documentation.
    session.budget.restart();
    // Arm Ctrl-C for the length of this line. The clone shares the counter the
    // restart just installed, so `cancel()` from the handler thread reaches
    // this evaluation and no later one.
    arm(&session.interrupt, Some(session.budget.clone()));

    let result = cant::run(
        "<repl>",
        program,
        cant::RunOptions {
            trace,
            preamble: session.bindings.preamble(),
            uses: session.environment.uses.clone(),
            module_roots: session.environment.module_roots.clone(),
            script_dir: None,
            permissions: session.permissions.clone(),
            budget: session.budget.clone(),
            args: Vec::new(),
            output: None,
            env_values: session.env_values.clone(),
        },
    )
    .await;
    arm(&session.interrupt, None);

    // Cancelling raises `BudgetError::Cancelled`, which reaches here as
    // `CANT-O001 budget exceeded`. Accurate, and misleading: the user
    // stopped the program rather than exhausting its budget.
    if session.budget.is_cancelled() {
        println!("interrupted");
        return Outcome::Interrupted;
    }
    Outcome::Ran(Box::new(result))
}

fn arm(slot: &Interrupt, budget: Option<ExecutionBudget>) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = budget;
}

/// A data value, as the Rite literal that rebuilds it.
///
/// `Err` names what cannot travel: a function, a handle, or an atom (whose
/// name lives in a table the finished run took with it). The refusal is deliberate: a binding
/// that silently dropped part of a value would report the wrong thing on the
/// next line.
fn binding_literal(value: &rite_runtime::Value) -> Result<String, String> {
    use rite_runtime::{Key, ResultValue, Value};
    match value {
        Value::None => Ok("none".into()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Int(n) => Ok(n.to_string()),
        Value::Float(f) => {
            if f.is_finite() {
                // Keep the decimal point, or the literal reads back as an int.
                let s = f.to_string();
                Ok(if s.contains('.') || s.contains('e') {
                    s
                } else {
                    format!("{s}.0")
                })
            } else {
                Err("a non-finite float has no literal".into())
            }
        }
        Value::String(s) => Ok(format!("{:?}", s)),
        Value::List(items) => {
            let parts: Result<Vec<String>, String> = items.iter().map(binding_literal).collect();
            Ok(format!("[ {} ]", parts?.join(", ")))
        }
        Value::Record(fields) => {
            let mut parts = Vec::with_capacity(fields.len());
            for (key, field) in fields.iter() {
                let Key::String(name) = key else {
                    return Err("a record keyed by an atom cannot be rebound".into());
                };
                let ident = name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !ident {
                    return Err(format!("record key `{name}` is not an identifier"));
                }
                parts.push(format!("{name}: {}", binding_literal(field)?));
            }
            Ok(format!("<< {} >>", parts.join(", ")))
        }
        Value::Result(ResultValue::Ok(inner)) => Ok(format!("ok({})", binding_literal(inner)?)),
        Value::Result(ResultValue::Err(inner)) => Ok(format!("err({})", binding_literal(inner)?)),
        other => Err(format!(
            "a {} has no literal to write into the next line",
            other.type_name()
        )),
    }
}

/// `:name rest` → `Some(rest)`, and only when the name matches exactly.
fn meta<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(name)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.starts_with(char::is_whitespace)
        .then(|| rest.trim_start())
}

fn first_word(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or(line)
}

// ------------------------------------------------------- session settings

/// What this session may reach, in the spelling `--allow` takes.
///
/// Covers every class the permission set carries. A report that omitted a grant
/// would be trusted and wrong.
fn describe_permissions(perms: &PermissionSet) -> String {
    if perms.allow_all {
        return "all — every permission is granted".into();
    }
    let mut lines = Vec::new();
    let mut on = Vec::new();
    for (name, granted) in [
        ("console", perms.console),
        ("stdin", perms.stdin),
        ("clock", perms.clock),
        ("random", perms.random),
        ("process", perms.process),
        ("sys", perms.sys),
    ] {
        if granted {
            on.push(name);
        }
    }
    if !on.is_empty() {
        lines.push(on.join("  "));
    }
    // Reading and writing the environment are separate grants, so they are
    // separate lines: `env` alone does not imply `env:write`, and a report that
    // ran them together would suggest it did.
    for (label, all, names) in [
        ("env", perms.env_all, &perms.env_vars),
        ("env:write", perms.env_write_all, &perms.env_write_vars),
    ] {
        if all {
            lines.push(label.to_string());
        } else if !names.is_empty() {
            let mut names: Vec<&str> = names.iter().map(String::as_str).collect();
            names.sort_unstable();
            lines.push(format!("{label}={}", names.join(",")));
        }
    }
    for (label, roots) in [("fs:read", &perms.fs_read), ("fs:write", &perms.fs_write)] {
        for root in roots {
            lines.push(format!("{label}={}", root.display()));
        }
    }
    let mut hosts: Vec<&str> = perms.net.iter().map(String::as_str).collect();
    hosts.sort_unstable();
    for host in hosts {
        lines.push(format!("net={host}"));
    }
    if perms.db_memory {
        lines.push("db".into());
    }
    for root in &perms.db_paths {
        lines.push(format!("db={}", root.display()));
    }
    if lines.is_empty() {
        return "nothing — every permission is denied".into();
    }
    lines.join("\n")
}

/// `:allow <spec>` / `:deny <spec>`.
fn change_permission(spec: &str, grant: bool, session: &mut Session) -> bool {
    let verb = if grant { ":allow" } else { ":deny" };
    if spec.is_empty() {
        eprintln!("cant: `{verb} <spec>` — e.g. `{verb} env`, `{verb} fs:read=./data`");
        return true;
    }
    // See `Session::interactive`: the input of a piped REPL is a program, and a
    // program that can grant itself permissions is not constrained by them.
    if !session.interactive {
        eprintln!(
            "cant: `{verb}` needs a terminal — this session's input is a program, \
             and a program must not widen its own permissions"
        );
        eprintln!("  pass `--allow {spec}` on the command line instead");
        return true;
    }
    match Permission::parse(spec) {
        Ok(permission) => {
            if grant {
                session.permissions.grant(permission);
            } else {
                session.permissions.deny(permission);
            }
            println!("{}", describe_permissions(&session.permissions));
        }
        Err(why) => eprintln!("cant: {why}"),
    }
    true
}

/// `:use NAME` — make a module available to every line from here on.
///
/// Checked against the roots the session searches, so a name that will not
/// resolve is refused here rather than failing every subsequent line with an
/// `E026` about generated Rite.
fn add_use(name: &str, session: &mut Session) -> bool {
    if name.is_empty() {
        eprintln!("cant: `:use NAME` — the module to make available to every line");
        return true;
    }
    if session.environment.uses.iter().any(|u| u == name) {
        println!("already loaded: {name}");
        return true;
    }
    let from = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let segments: Vec<String> = name.split('.').map(str::to_string).collect();
    if rite::sem::resolve_module_path(&segments, &from, &session.environment.module_roots).is_none()
    {
        eprintln!("cant: no module `{name}` under {}", from.display());
        eprintln!("  `--module-root DIR` at startup adds a place to look");
        return true;
    }
    session.environment.uses.push(name.to_string());
    println!("{}", session.environment.uses.join("  "));
    true
}

/// `:timeout` shows it, `:timeout off` removes it, `:timeout 30s` sets it.
fn set_timeout(rest: &str, session: &mut Session) -> bool {
    match rest {
        "" => {}
        "off" | "none" => session.budget.timeout = None,
        spec => match rite::parse_duration(spec) {
            Ok(duration) => session.budget.timeout = Some(duration),
            Err(why) => {
                eprintln!("cant: {why}");
                return true;
            }
        },
    }
    match session.budget.timeout {
        Some(d) => println!("timeout: {} per line", show_duration(d)),
        None => println!("timeout: off"),
    }
    true
}

/// `:steps` shows the per-line step ceiling, `:steps off` removes it.
fn set_steps(rest: &str, session: &mut Session) -> bool {
    match rest {
        "" => {}
        "off" | "none" => session.budget.max_steps = u64::MAX,
        spec => match spec.parse::<u64>() {
            Ok(n) => session.budget.max_steps = n,
            Err(_) => {
                eprintln!("cant: `:steps <n>` wants a whole number, or `off`");
                return true;
            }
        },
    }
    if session.budget.max_steps == u64::MAX {
        println!("steps: off");
    } else {
        println!("steps: {} per line", session.budget.max_steps);
    }
    true
}

fn show_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

// ------------------------------------------------------------ other views

fn show_expand(program: &str, color: bool) {
    if program.is_empty() {
        eprintln!("cant: `:expand` needs a program");
        return;
    }
    let (expansion, analysis) = cant::expand("<repl>", program);
    match expansion {
        Some(expansion) => print!("{}", paint_value(&expansion.rite, color)),
        None => eprint!("{}", analysis.render()),
    }
}

fn show_graph(program: &str) {
    if program.is_empty() {
        eprintln!("cant: `:graph` needs a program");
        return;
    }
    let analysis = cant::analyze("<repl>", program);
    if !analysis.diagnostics.is_empty() {
        eprint!("{}", analysis.render());
    }
    if let Some(graph) = &analysis.graph {
        print!("{}", cant::to_dot(graph));
    }
}

fn show_explain(program: &str) {
    if program.is_empty() {
        eprintln!("cant: `:explain` needs a program");
        return;
    }
    let analysis = cant::analyze("<repl>", program);
    if !analysis.diagnostics.is_empty() {
        eprint!("{}", analysis.render());
    }
    if let Some(graph) = &analysis.graph {
        print!(
            "{}",
            cant_sem::explain::render(&cant_sem::explain(graph), false)
        );
    }
}

/// `:fmt <program>` — the formatter the CLI runs, on one line of input.
///
/// Formatted in the dialect it was written in, and compact, so the result can
/// be typed back at the prompt.
fn show_fmt(program: &str, color: bool) {
    if program.is_empty() {
        eprintln!("cant: `:fmt` needs a program");
        return;
    }
    let dialect = cant_syntax::detect(program);
    match cant_syntax::format(
        program,
        FormatOptions {
            dialect: if dialect == Dialect::Glyph {
                Dialect::Glyph
            } else {
                Dialect::Ascii
            },
            compact: true,
            ..FormatOptions::default()
        },
    ) {
        Ok(result) => println!("{}", paint(result.text.trim_end(), color)),
        Err(_) => {
            // The formatter refuses a source that did not parse rather than
            // reprinting a recovery, so report what the parser found.
            let analysis = cant::analyze("<repl>", program);
            eprint!("{}", analysis.render());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            permissions: PermissionSet::default_secure(),
            budget: ExecutionBudget::new(),
            bindings: Bindings::default(),
            interactive: true,
            interrupt: Arc::new(Mutex::new(None)),
            color: false,
            environment: cant::Environment::default(),
            env_values: Vec::new(),
        }
    }

    #[test]
    fn a_meta_command_needs_an_exact_name() {
        assert_eq!(meta(":expand a -> b", ":expand"), Some("a -> b"));
        assert_eq!(meta(":expand", ":expand"), Some(""));
        // `:expander` is not `:expand`, and neither is a program starting with
        // the same letters.
        assert_eq!(meta(":expander x", ":expand"), None);
        assert_eq!(meta("expand x", ":expand"), None);
    }

    #[test]
    fn the_help_text_lists_every_command_it_accepts() {
        for command in [":expand", ":graph", ":explain", ":help", ":quit"] {
            assert!(HELP.contains(command), "`{command}` missing from :help");
            assert!(
                BANNER.contains(command) || command == ":explain" || command == ":graph",
                "`{command}` missing from the banner"
            );
        }
        for command in [
            ":permissions",
            ":allow",
            ":deny",
            ":timeout",
            ":steps",
            ":fmt",
        ] {
            assert!(HELP.contains(command), "`{command}` missing from :help");
        }
    }

    /// The banner has to say the one thing that surprises people: values can
    /// persist now, and programs still cannot.
    #[test]
    fn the_banner_draws_the_values_not_programs_line() {
        assert!(
            BANNER.contains("Values can persist; programs cannot"),
            "{BANNER}"
        );
        assert!(HELP.contains("workbench of values"), "{HELP}");
        assert!(HELP.contains("holds the value, not the program"), "{HELP}");
    }

    /// The bug this file's documentation is about: a budget cloned per line
    /// measures the session unless it is restarted. `restart()` must both move
    /// the wall-clock origin and install a fresh counter — a clone taken before
    /// it must not still be feeding the same total.
    #[test]
    fn restarting_gives_each_line_its_own_counter() {
        let mut budget = ExecutionBudget::new().with_max_steps(3);
        let first = budget.clone();
        for _ in 0..3 {
            first.tick().expect("within the line's own budget");
        }
        assert!(first.tick().is_err(), "the line spent its steps");

        budget.restart();
        let second = budget.clone();
        for _ in 0..3 {
            second
                .tick()
                .expect("a new line starts from zero, not from the last line's total");
        }
    }

    /// A session with no wall clock is the default, and `:timeout` can put one
    /// back without restarting the process.
    #[test]
    fn timeout_can_be_set_and_cleared() {
        let mut s = session();
        s.budget.timeout = None;
        set_timeout("30s", &mut s);
        assert_eq!(s.budget.timeout, Some(Duration::from_secs(30)));
        set_timeout("off", &mut s);
        assert_eq!(s.budget.timeout, None);
        // A spelling the duration parser refuses leaves the setting alone
        // rather than silently choosing something.
        set_timeout("1h", &mut s);
        assert_eq!(s.budget.timeout, None);
    }

    #[test]
    fn steps_can_be_set_and_cleared() {
        let mut s = session();
        set_steps("500", &mut s);
        assert_eq!(s.budget.max_steps, 500);
        set_steps("off", &mut s);
        assert_eq!(s.budget.max_steps, u64::MAX);
        set_steps("lots", &mut s);
        assert_eq!(s.budget.max_steps, u64::MAX);
    }

    #[test]
    fn allow_grants_and_deny_revokes() {
        let mut s = session();
        assert!(!s.permissions.env_all);
        change_permission("env", true, &mut s);
        assert!(s.permissions.env_all);
        change_permission("env", false, &mut s);
        assert!(!s.permissions.env_all);
        // A spec the parser refuses changes nothing.
        change_permission("nonsense", true, &mut s);
        assert!(!s.permissions.env_all);
    }

    /// The escalation this guards against: a piped session's input *is* the
    /// program, so `:allow` from a script would let the program widen its own
    /// permissions.
    #[test]
    fn allow_is_refused_when_nobody_is_at_the_prompt() {
        let mut s = session();
        s.interactive = false;
        change_permission("fs:write=/", true, &mut s);
        assert!(
            s.permissions.fs_write.is_empty(),
            "a non-interactive session must not grant itself anything"
        );
    }

    /// The report has to cover every class the permission set carries. One that
    /// silently omitted a grant would be worse than none, because it would be
    /// believed — this test caught `sys` and `env:write` missing from it.
    #[test]
    fn permissions_are_described_in_the_spelling_allow_takes() {
        let mut s = session();
        assert!(describe_permissions(&s.permissions).contains("console"));
        for spec in [
            "env=HOME",
            "env:write=PORT",
            "sys",
            "process",
            "fs:read=.",
            "net=example.com",
            "db",
        ] {
            change_permission(spec, true, &mut s);
        }
        let shown = describe_permissions(&s.permissions);
        for spec in ["env=HOME", "env:write=PORT", "sys", "process", "net=", "db"] {
            assert!(shown.contains(spec), "`{spec}` missing from:\n{shown}");
        }
        assert!(shown.contains("fs:read="), "{shown}");
        change_permission("all", true, &mut s);
        assert!(describe_permissions(&s.permissions).starts_with("all"));
    }
}
