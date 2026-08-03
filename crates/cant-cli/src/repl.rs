//! The Cant REPL.
//!
//! # Why nothing persists
//!
//! A Cant program is one flow. It has no declarations, no bindings, and no
//! statements — so there is nothing a line could leave behind for the next one.
//! The specification says to implement persistence "only where semantics are
//! clear", and here the clear semantics are that each line is a whole program.
//!
//! That is not a limitation working around a missing feature; it is what the
//! language is. A REPL that pretended otherwise would have to invent a binding
//! form Cant does not have, and then Cant would have one.
//!
//! What the session *does* carry is the permissions and budget it was started
//! with, because those are properties of the session rather than of a program.
//!
//! # Meta-commands
//!
//! `:expand`, `:graph` and `:explain` take the rest of the line and show that
//! view of it instead of running it — the three ways of looking at a program,
//! reachable without leaving the prompt.

use std::process::ExitCode;

use rite_caps::PermissionSet;
use rite_runtime::ExecutionBudget;

const BANNER: &str = "\
cant — each line is a whole program, and nothing persists between them.
  :help                what you can type
  :expand <program>    the Rite it becomes
  :graph <program>     its topology, as DOT
  :explain <program>   what it does, in prose
  :quit                leave";

const HELP: &str = "\
Every line is a complete Cant program: a flow, evaluated and printed.
Nothing carries over — Cant has no bindings, so there is nothing to carry.

  [1, 2, 3] -> * -> ?{ $ > 1 } -> []      run it
  :expand  <program>                      the canonical Rite it becomes
  :graph   <program>                      its topology, as Graphviz DOT
  :explain <program>                      what it does, in prose
  :help                                   this
  :quit                                   leave (or Ctrl-D)

The permissions and budget this session started with apply to every line.";

pub async fn run(permissions: PermissionSet, budget: ExecutionBudget) -> ExitCode {
    let mut editor = match rustyline::DefaultEditor::new() {
        Ok(editor) => editor,
        Err(e) => {
            eprintln!("cant: cannot start the REPL: {e}");
            return ExitCode::from(1);
        }
    };
    println!("{BANNER}\n");

    loop {
        match editor.readline("cant> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = editor.add_history_entry(trimmed);
                if !handle(trimmed, &permissions, &budget).await {
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

/// Handle one line. Returns `false` when the session should end.
async fn handle(line: &str, permissions: &PermissionSet, budget: &ExecutionBudget) -> bool {
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
        return true;
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

    let result = cant::run(
        "<repl>",
        line,
        cant::RunOptions {
            script_dir: None,
            permissions: permissions.clone(),
            budget: budget.clone(),
            args: Vec::new(),
            output: None,
        },
    )
    .await;

    if !result.diagnostics.is_empty() {
        eprint!("{}", result.render());
    }
    if let Some(display) = &result.display {
        println!("{display}");
    }
    true
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

    /// The banner has to say the one thing that surprises people.
    #[test]
    fn the_banner_says_that_nothing_persists() {
        assert!(BANNER.contains("nothing persists"), "{BANNER}");
        assert!(HELP.contains("Nothing carries over"), "{HELP}");
    }
}
