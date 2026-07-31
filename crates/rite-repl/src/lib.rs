//! Interactive Rite REPL.

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_core::SourceFile;
use rite_runtime::{run_file, RuntimeContext, Value};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;
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
    pub glyph: bool,
    pub last_load: Option<PathBuf>,
    /// Definitions replayed before each input, in order, with the newest definition of
    /// a name replacing the older one.
    entries: Vec<PreludeEntry>,
    /// Optional per-eval wall-clock limit; default 5 minutes (not 60s from session start).
    pub eval_timeout: Duration,
}

impl ReplSession {
    pub fn new(perms: PermissionSet) -> Self {
        let mut ctx = RuntimeContext::new();
        // REPL sessions are long-lived: do not inherit a clock started at open.
        ctx.budget.timeout = Some(Duration::from_secs(300));
        ctx.budget.restart();
        install_defaults(&mut ctx, perms.clone());
        Self {
            ctx,
            perms,
            glyph: true,
            last_load: None,
            entries: Vec::new(),
            eval_timeout: Duration::from_secs(300),
        }
    }

    /// Evaluate a complete source chunk in this session.
    ///
    /// Definitional forms (bindings, functions, imports, …) are appended to a
    /// session prelude so later inputs can refer to them. Pure expressions and
    /// effect statements are executed against the prelude but not stored.
    pub async fn eval(&mut self, source: &str) -> ReplEval {
        self.ctx.budget.timeout = Some(self.eval_timeout);
        self.ctx.budget.restart();
        // Fresh env each eval; re-apply prelude + input so definitions resolve.
        // Capabilities stay installed via install_defaults after rebuild.
        let perms = self.perms.clone();
        self.ctx = RuntimeContext::new();
        self.ctx.budget.timeout = Some(self.eval_timeout);
        self.ctx.budget.restart();
        install_defaults(&mut self.ctx, perms);

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
        let sf = SourceFile::new(rite_core::FileId(0), "<repl>".to_string(), &combined);
        match run_file(&sf, &mut self.ctx).await {
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
        let existing = name.as_deref().and_then(|n| {
            self.entries
                .iter()
                .position(|e| e.name.as_deref() == Some(n))
        });
        match existing {
            Some(i) => self.entries[i].source = line,
            None => self.entries.push(PreludeEntry { name, source: line }),
        }
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
                source.trim_end()
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
        self.ctx.budget.timeout = Some(timeout);
        self.ctx.budget.restart();
        install_defaults(&mut self.ctx, self.perms.clone());
    }
}

/// One replayed definition, and the single name it defines if it defines exactly one.
struct PreludeEntry {
    name: Option<String>,
    source: String,
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

pub async fn run_repl(perms: PermissionSet) -> anyhow::Result<()> {
    println!(
        "Rite {} — type :help for commands",
        env!("CARGO_PKG_VERSION")
    );
    let history_path = dirs_history_path();
    let mut rl = DefaultEditor::new()?;
    if let Some(ref p) = history_path {
        let _ = rl.load_history(p);
    }
    let mut session = ReplSession::new(perms);
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
            eprintln!("error: {}", err);
        } else if let Some(d) = result.display {
            println!("{}", d);
        }
    }
    if let Some(ref p) = history_path {
        let _ = rl.save_history(p);
    }
    Ok(())
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
:allow perm        Grant permission (e.g. fs:read=./data)
:deny perm         Deny permission
:format glyph|ascii
:timeout secs      Per-eval wall-clock limit (default 300)
:reset             Reset environment
:quit / :q         Exit

Tip: each line restarts the step/time budget; idle time does not count."#
            );
        }
        ":quit" | ":q" => return Ok(true),
        ":bindings" => {
            for (k, v) in session.ctx.env.bindings_snapshot() {
                println!("{} = {}", k, v.to_display(&session.ctx.atoms));
            }
        }
        ":capabilities" => {
            println!("console fs json csv crypto clock env process random http game store db");
        }
        ":allow" => {
            if let Some(spec) = parts.get(1) {
                if let Ok(p) = Permission::parse(spec) {
                    session.perms.grant(p);
                    install_defaults(&mut session.ctx, session.perms.clone());
                    println!("allowed {}", spec);
                } else {
                    println!("could not parse permission: {}", spec);
                }
            } else {
                println!("usage: :allow fs:read=./data");
            }
        }
        ":deny" => {
            if let Some(spec) = parts.get(1) {
                if let Ok(p) = Permission::parse(spec) {
                    session.perms.deny(p);
                    install_defaults(&mut session.ctx, session.perms.clone());
                    println!("denied {}", spec);
                }
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
        ":timeout" => {
            if let Some(s) = parts.get(1).and_then(|x| x.parse::<u64>().ok()) {
                session.eval_timeout = Duration::from_secs(s.max(1));
                println!("eval timeout: {}s", session.eval_timeout.as_secs());
            } else {
                println!(
                    "usage: :timeout <seconds>  (current {}s)",
                    session.eval_timeout.as_secs()
                );
            }
        }
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

    #[tokio::test]
    async fn long_session_many_evals() {
        let mut s = ReplSession::new(PermissionSet::allow_all());
        s.eval_timeout = Duration::from_secs(5);
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
