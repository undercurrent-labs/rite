//! The Cant REPL.
//!
//! # What persists, and why it is not a language feature
//!
//! A Cant program is one flow: no declarations, no bindings, no statements.
//! The language does not change here. What changed is the *session*: alongside
//! the permissions and budget it started with, a session now carries a
//! workbench of named **values** — `:let x = <program>` runs the program and
//! keeps its answer, and `it` is always the last answer.
//!
//! The distinction is load-bearing. `:let` is a meta-command, not syntax: a
//! `.cant` file containing `:let` does not parse, and no program can bind
//! anything. What persists is the value, not the flow that produced it —
//! re-using `x` re-runs nothing and repeats no effects. Bindings reach the
//! next line as a generated-Rite preamble (`x <- 5` above the stage
//! functions), so a bound name is an ordinary Rite name inside every stage.
//! Only data values can be bound; a handle or a function has no literal to
//! write, and the refusal says so.
//!
//! # Meta-commands
//!
//! `:expand`, `:graph` and `:explain` take the rest of the line and show that
//! view of it instead of running it; `:trace` runs it and shows per-node
//! emission counts beside the value; `:let` and `:bindings` are the workbench.

use std::process::ExitCode;

use rite_caps::PermissionSet;
use rite_runtime::ExecutionBudget;

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
  ~> <program>                            run it, with per-node emission counts
  :help                                   this
  :quit                                   leave (or Ctrl-D)

A binding holds the value, not the program: nothing re-runs and no effect
repeats. Only data values can be bound. The arrows are sugar for `:let` and
`:trace`, and take the glyphs too: `x ← …`, `⟿ …`. To compare against a
negative number rather than bind, space the operator: `x < -3`. The
permissions and budget this session started with apply to every line.";

pub async fn run(permissions: PermissionSet, budget: ExecutionBudget) -> ExitCode {
    let mut editor = match rustyline::DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("cant: cannot start the REPL: {e}");
            return ExitCode::from(1);
        }
    };
    println!("{BANNER}\n");
    let mut bindings = Bindings::default();

    loop {
        match editor.readline("cant> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = editor.add_history_entry(trimmed);
                if !handle(trimmed, &permissions, &budget, &mut bindings).await {
                    return ExitCode::SUCCESS;
                }
            }
            // Ctrl-C abandons the line, Ctrl-D leaves — the conventions every
            // other REPL has, and the ones a hand already knows.
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(rustyline::error::ReadlineError::Eof) => return ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("cant: {e}");
                return ExitCode::from(1);
            }
        }
    }
}

/// The session's workbench: named values, as the Rite literals that rebuild
/// them. Values, not programs — see the module documentation.
#[derive(Default)]
struct Bindings {
    /// Insertion order; a rebind replaces in place, so the preamble stays
    /// stable and `:bindings` reads as history.
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
}

/// Handle one line. Returns `false` when the session should end.
async fn handle(
    line: &str,
    permissions: &PermissionSet,
    budget: &ExecutionBudget,
    bindings: &mut Bindings,
) -> bool {
    if let Some(rest) = meta(line, ":quit").or_else(|| meta(line, ":q")) {
        let _ = rest;
        return false;
    }
    if meta(line, ":help").is_some() || meta(line, ":h").is_some() {
        println!("{HELP}");
        return true;
    }
    if let Some(program) = meta(line, ":expand") {
        show_expand(program);
        if !bindings.entries.is_empty() {
            let names: Vec<&str> = bindings.entries.iter().map(|(n, _)| n.as_str()).collect();
            println!(
                "// plus this session's bindings, emitted above the functions: {}",
                names.join(", ")
            );
        }
        return true;
    }
    if meta(line, ":bindings").is_some() {
        if bindings.entries.is_empty() {
            println!("nothing bound — `:let x = <program>` keeps an answer");
        }
        for (name, literal) in &bindings.entries {
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
    // Sugar, before the plain-program fallthrough: `x <- <program>` binds, the
    // way Rite itself spells a binding — and exactly the shape `:bindings`
    // prints, so what you read back is what you type. `~> <program>` traces:
    // the wavy arrow is "run it, and watch the flow". Both have glyph twins
    // (`←`, `⟿`), because every spelling in this repository has two. To
    // *compare* against a negative number instead of binding, space the
    // operator: `x < -3`.
    if let Some(program) = sugar_prefix(line, &["~>", "⟿"]) {
        return trace_command(program, permissions, budget, bindings).await;
    }
    if let Some((name, program)) = binding_sugar(line) {
        return let_command(&name, program, permissions, budget, bindings).await;
    }
    if let Some(rest) = meta(line, ":let") {
        let Some((name, program)) = rest.split_once('=') else {
            eprintln!("cant: `:let name = <program>` — the `=` separates the name from the flow");
            return true;
        };
        return let_command(name.trim(), program.trim(), permissions, budget, bindings).await;
    }
    if let Some(program) = meta(line, ":trace") {
        return trace_command(program, permissions, budget, bindings).await;
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

    let result = run_line(line, permissions, budget, bindings, false).await;

    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if let Some(display) = &result.display {
        println!("{display}");
    }
    // `it` is the last answer, updated only on success — a failed line must
    // not quietly replace the value someone is about to use.
    if result.succeeded() {
        if let Some(value) = &result.value {
            if let Ok(literal) = binding_literal(value) {
                bindings.set("it", literal);
            }
        }
    }
    true
}

/// `name <- program` (or `name ← program`): the binding sugar, recognised
/// only when everything before the arrow is a single bindable name — so a
/// flow that merely *contains* an arrow-like sequence falls through to run.
fn binding_sugar(line: &str) -> Option<(String, &str)> {
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
                return Some((name.to_string(), program));
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
async fn let_command(
    name: &str,
    program: &str,
    permissions: &PermissionSet,
    budget: &ExecutionBudget,
    bindings: &mut Bindings,
) -> bool {
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
    let result = run_line(program, permissions, budget, bindings, false).await;
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
                println!("{display}");
            }
            bindings.set(name, literal.clone());
            bindings.set("it", literal);
        }
        Err(why) => eprintln!("cant: cannot bind `{name}`: {why}"),
    }
    true
}

/// The body of `:trace` and of the trace arrow.
async fn trace_command(
    program: &str,
    permissions: &PermissionSet,
    budget: &ExecutionBudget,
    bindings: &mut Bindings,
) -> bool {
    if program.trim().is_empty() {
        eprintln!("cant: `~> <program>` runs the program and counts emissions per node");
        return true;
    }
    let result = run_line(program, permissions, budget, bindings, true).await;
    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if let Some(counts) = &result.trace {
        let shown: Vec<String> = counts.iter().map(|(id, n)| format!("{id}:{n}")).collect();
        println!("trace  {}", shown.join("  "));
    }
    if let Some(display) = &result.display {
        println!("{display}");
    }
    if result.succeeded() {
        if let Some(value) = &result.value {
            if let Ok(literal) = binding_literal(value) {
                bindings.set("it", literal);
            }
        }
    }
    true
}

/// Run one program with the session's bindings in scope.
async fn run_line(
    program: &str,
    permissions: &PermissionSet,
    budget: &ExecutionBudget,
    bindings: &Bindings,
    trace: bool,
) -> cant::ExecutionResult {
    cant::run(
        "<repl>",
        program,
        cant::RunOptions {
            trace,
            preamble: bindings.preamble(),
            script_dir: None,
            permissions: permissions.clone(),
            budget: budget.clone(),
            args: Vec::new(),
            output: None,
        },
    )
    .await
}

/// A data value, as the Rite literal that rebuilds it.
///
/// `Err` names what cannot travel: a function, a handle, or an atom (whose
/// name lives in a table the finished run took with it). Refusing loudly is
/// the contract — a binding that silently dropped part of a value would be a
/// workbench that lies.
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

fn show_expand(program: &str) {
    if program.is_empty() {
        eprintln!("cant: `:expand` needs a program");
        return;
    }
    let (expansion, analysis) = cant::expand("<repl>", program);
    match expansion {
        Some(expansion) => print!("{}", expansion.rite),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
