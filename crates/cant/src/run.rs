//! Running and building a Cant program.
//!
//! Both go through generated Rite: `cant run` hands the expansion to
//! `rite_runtime`, `cant build` writes it to disk and hands the path to
//! `rite_compiler`. There is no Cant evaluator and no Cant compiler backend —
//! ADR 0002, and the reason a Cant program gets interpreter/compiler parity,
//! capability enforcement and native builds without any of them being
//! reimplemented.
//!
//! Failures come back as Cant diagnostics wherever a span makes that possible.
//! A runtime error usually has none — Rite's `EvalError` carries a message, not
//! a location — so those are reported with the Rite text and the Cant code, which
//! is honest about what is known rather than guessing at a line.

use std::path::{Path, PathBuf};

use cant_syntax::diagnostic::*;
use cant_syntax::CantDiagnostics;
use rite_caps::PermissionSet;
use rite_core::{FileId, SourceFile};
use rite_runtime::{EvalError, ExecutionBudget, RuntimeContext, Value};

use crate::{check, CheckResult, Expansion};

/// How to run a program.
pub struct RunOptions {
    /// Where relative paths and module imports resolve from.
    ///
    /// `None` for `-e` and stdin, which have no directory of their own; the
    /// process's working directory is used, matching `rite run`.
    pub script_dir: Option<PathBuf>,
    pub permissions: PermissionSet,
    pub budget: ExecutionBudget,
    /// Everything after `--`, readable with `! @process.args`.
    pub args: Vec<String>,
    /// Where the guest's `@console` output goes. `None` means the host's own
    /// streams, which is what a CLI wants.
    pub output: Option<rite_runtime::OutputSink>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            script_dir: None,
            permissions: PermissionSet::default_secure(),
            budget: ExecutionBudget::new(),
            args: Vec::new(),
            output: None,
        }
    }
}

/// What a run produced.
pub struct ExecutionResult {
    /// The program's value, rendered the way `rite run` renders it — the only
    /// form in which an atom shows its name rather than its index.
    pub display: Option<String>,
    pub value: Option<Value>,
    pub diagnostics: CantDiagnostics,
    pub exit_code: u8,
    /// The Rite that was executed, so a caller can show it when something went
    /// wrong.
    pub expansion: Option<Expansion>,
    /// The check that preceded the run, for its source map and diagnostics.
    pub check: CheckResult,
}

impl ExecutionResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }

    pub fn render(&self) -> String {
        self.diagnostics.render_all(&self.check.analysis.sources)
    }
}

/// Expand a Cant program and run it on Rite's runtime.
pub async fn run(name: &str, text: &str, options: RunOptions) -> ExecutionResult {
    let checked = check(name, text);
    if checked.has_errors() {
        let exit_code = checked.exit_code();
        return ExecutionResult {
            display: None,
            value: None,
            diagnostics: checked.diagnostics.clone(),
            exit_code,
            expansion: checked.expansion.clone(),
            check: checked,
        };
    }

    let Some(expansion) = checked.expansion.clone() else {
        // `check` reported nothing and produced nothing: there was no program.
        return ExecutionResult {
            display: None,
            value: None,
            diagnostics: checked.diagnostics.clone(),
            exit_code: 0,
            expansion: None,
            check: checked,
        };
    };

    let mut ctx = RuntimeContext::new();
    ctx.budget = options.budget.clone();
    ctx.script_args = options.args.clone();
    if let Some(dir) = &options.script_dir {
        ctx.script_dir = Some(dir.clone());
        ctx.module_roots.push(dir.clone());
    }
    ctx.sink = Some(options.output.clone().unwrap_or_else(inherit_output));
    rite_caps::install_defaults(&mut ctx, options.permissions.clone());

    // The generated source gets its own file id so a diagnostic from it cannot
    // be mistaken for one about the `.cant`.
    let generated = SourceFile::new(GENERATED_FILE, "<generated>.rite", &expansion.rite);
    let outcome = rite_runtime::run_file(&generated, &mut ctx).await;
    flush(&ctx);

    let cant_file = checked
        .analysis
        .sources
        .files()
        .first()
        .map(|f| f.id)
        .unwrap_or(FileId(0));

    let mut diagnostics = CantDiagnostics::new();
    match outcome {
        Ok(value) => {
            let display = if matches!(value, Value::None) {
                None
            } else {
                Some(value.to_display(&ctx.atoms))
            };
            ExecutionResult {
                display,
                value: Some(value),
                diagnostics,
                exit_code: 0,
                expansion: Some(expansion),
                check: checked,
            }
        }
        Err(error) => {
            // **Rite's status, always.** Reclassifying here was tried and
            // reverted: `cant run` reported an orbit limit as 8 while
            // `rite run <cant expand>` reported the panic it really is as 1, and
            // the differential harness caught the two paths disagreeing about
            // the same execution. Parity is worth more than a tidier number, and
            // the *code* — `CANT-O002` — is the stable identifier the
            // specification actually asks for.
            let exit_code = error.exit_code();
            for diagnostic in runtime_diagnostics(&error, &expansion, cant_file) {
                diagnostics.push(diagnostic);
            }
            ExecutionResult {
                display: None,
                value: None,
                diagnostics,
                exit_code,
                expansion: Some(expansion),
                check: checked,
            }
        }
    }
}

/// A file id that cannot collide with the `.cant` source's.
const GENERATED_FILE: FileId = FileId(u32::MAX - 1);

/// Turn a Rite failure into Cant diagnostics.
///
/// `EvalError::Compile` carries real diagnostics with spans, so those are
/// remapped precisely. The rest carry a message and nothing else — Rite's
/// runtime errors are not span-bearing — so they are reported with that message
/// and no location. Attaching the program's whole span would look like precision
/// that is not there.
fn runtime_diagnostics(
    error: &EvalError,
    expansion: &Expansion,
    cant_file: FileId,
) -> Vec<CantDiagnostic> {
    match error {
        EvalError::Compile(diagnostics) => {
            let remapped: Vec<_> = diagnostics
                .iter()
                .map(|d| cant_sem::remap_diagnostic(d, &expansion.map, cant_file))
                .collect();
            cant_sem::expand::collapse_cascades(remapped, &expansion.prefix)
        }
        // The script chose its own status. Not a failure, and nothing to say
        // about it — the same silence `rite run` keeps.
        EvalError::Exit(_) => Vec::new(),
        EvalError::Permission(message) => vec![CantDiagnostic::error(
            CANT_R002_PERMISSION_DENIED,
            format!("permission denied: {message}"),
        )
        .with_help("grant it with `--allow`, or `--allow-all` for a trusted program")],
        EvalError::Budget(budget) => vec![CantDiagnostic::error(
            CANT_O001_BUDGET_EXHAUSTED,
            format!("budget exceeded: {budget}"),
        )
        .with_note(
            "an orbit's `:max` bounds its candidates; the step and time budgets bound \
             everything else",
        )],
        other => vec![runtime_failure(&other.to_string())],
    }
}

/// Classify a runtime message, and strip what the user must not be shown.
///
/// Generated code tags the failures Cant is responsible for with their own code
/// — `CANT-O001: orbit at …`, `CANT-R003: scatter expected a list …` — so this
/// does not have to pattern-match prose. The tag is deliberately left in the
/// generated Rite too: someone running the expansion directly with `rite run`
/// sees a code they can look up rather than an anonymous panic.
///
/// The stack traceback is removed. Rite appends one per frame, and every frame
/// names a generated function in `<generated>.rite` — three of them, for a
/// two-stage program. §2.4 is explicit that a generated implementation detail
/// must not be what someone is shown, and a traceback through scaffolding is
/// nothing anyone can act on. `cant expand` is the way to see it.
fn runtime_failure(message: &str) -> CantDiagnostic {
    let message = message
        .split("\nstack traceback:")
        .next()
        .unwrap_or(message)
        .trim_end();

    for (tag, code, note) in [
        (
            "CANT-O002: ",
            CANT_O002_ORBIT_LIMIT_REACHED,
            "an orbit stops at its `:max` rather than returning half an answer; \
             raise it with `:max N` if the traversal really is that large",
        ),
        (
            "CANT-R003: ",
            CANT_R003_SCATTER_NOT_A_LIST,
            "`*` expands a list into one emission per element, so it needs a list",
        ),
    ] {
        if let Some(rest) = message.strip_prefix(tag) {
            return CantDiagnostic::error(code, rest.to_string()).with_note(note);
        }
    }

    CantDiagnostic::error(CANT_R001_RITE_RUNTIME, message.to_string())
        .with_note("run `cant expand` to see the Rite this came from")
}

fn inherit_output() -> rite_runtime::OutputSink {
    std::sync::Arc::new(|stream, text: &str| {
        use std::io::Write;
        match stream {
            rite_runtime::OutputStream::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(text.as_bytes());
                let _ = out.flush();
            }
            rite_runtime::OutputStream::Stderr => {
                let mut err = std::io::stderr().lock();
                let _ = err.write_all(text.as_bytes());
                let _ = err.flush();
            }
        }
    })
}

/// Write anything the runtime buffered rather than streamed.
///
/// The sink above streams, so these are normally empty — but they are drained on
/// every exit path anyway, because `rite run` learned the hard way that output
/// drained only in the success arm means a program that prints and then fails
/// prints nothing.
fn flush(ctx: &RuntimeContext) {
    use std::io::Write;
    if !ctx.stdout.is_empty() {
        let mut out = std::io::stdout().lock();
        for chunk in &ctx.stdout {
            let _ = out.write_all(chunk.as_bytes());
        }
        let _ = out.flush();
    }
    if !ctx.stderr.is_empty() {
        let mut err = std::io::stderr().lock();
        for chunk in &ctx.stderr {
            let _ = err.write_all(chunk.as_bytes());
        }
        let _ = err.flush();
    }
}

// ---------------------------------------------------------------- building

pub struct BuildOptions {
    pub release: bool,
    /// Write the generated Rust instead of linking a binary.
    pub emit_rust: bool,
    pub output: Option<PathBuf>,
    /// Baked into the binary at build time, as `rite build` does.
    pub permissions: PermissionSet,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            release: false,
            emit_rust: false,
            output: None,
            permissions: PermissionSet::default_secure(),
        }
    }
}

pub struct BuildResult {
    pub binary: Option<PathBuf>,
    /// Where the generated Rite was written. Kept, not deleted: it is the
    /// artifact `cant expand` prints, and having it on disk beside the binary is
    /// what makes a compiled Cant program auditable after the fact.
    pub generated: Option<PathBuf>,
    pub diagnostics: CantDiagnostics,
    pub exit_code: u8,
}

/// Compile a Cant program to a native binary, through Rite's compiler.
pub fn build(path: &Path, options: BuildOptions) -> BuildResult {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            return BuildResult {
                binary: None,
                generated: None,
                diagnostics: one(CantDiagnostic::error(
                    CANT_R001_RITE_RUNTIME,
                    format!("cannot read {}: {e}", path.display()),
                )),
                exit_code: 1,
            }
        }
    };
    let name = path.display().to_string();
    let checked = check(&name, &text);
    if checked.has_errors() {
        return BuildResult {
            binary: None,
            generated: None,
            exit_code: checked.exit_code(),
            diagnostics: checked.diagnostics,
        };
    }
    let Some(expansion) = checked.expansion else {
        return BuildResult {
            binary: None,
            generated: None,
            diagnostics: CantDiagnostics::new(),
            exit_code: 0,
        };
    };

    // Beside the source, under `.rite/cant/`, mirroring where `rite build` puts
    // its generated crate. Beside rather than in a temporary directory because
    // `rite_compiler::build_script` resolves modules relative to the file it is
    // given, so a generated program that ever grows a `use` has to sit where the
    // user's modules are.
    let dir = path
        .parent()
        .unwrap_or(Path::new("."))
        .join(".rite")
        .join("cant");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return BuildResult {
            binary: None,
            generated: None,
            diagnostics: one(CantDiagnostic::error(
                CANT_X001_GENERATED_RITE_INVALID,
                format!("cannot create {}: {e}", dir.display()),
            )),
            exit_code: 1,
        };
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program");
    let generated = dir.join(format!("{stem}.rite"));
    if let Err(e) = std::fs::write(&generated, &expansion.rite) {
        return BuildResult {
            binary: None,
            generated: None,
            diagnostics: one(CantDiagnostic::error(
                CANT_X001_GENERATED_RITE_INVALID,
                format!("cannot write {}: {e}", generated.display()),
            )),
            exit_code: 1,
        };
    }

    match rite_compiler::build_script(
        &generated,
        options.release,
        options.emit_rust,
        options.output.as_deref(),
        &options.permissions,
    ) {
        Ok(binary) => BuildResult {
            binary: Some(binary),
            generated: Some(generated),
            diagnostics: CantDiagnostics::new(),
            exit_code: 0,
        },
        Err(e) => BuildResult {
            binary: None,
            generated: Some(generated.clone()),
            diagnostics: one(
                CantDiagnostic::error(CANT_X001_GENERATED_RITE_INVALID, e).with_note(format!(
                    "the generated Rite is at {} — `rite build` it directly to see more",
                    generated.display()
                )),
            ),
            exit_code: 6,
        },
    }
}

fn one(diagnostic: CantDiagnostic) -> CantDiagnostics {
    let mut out = CantDiagnostics::new();
    out.push(diagnostic);
    out
}
