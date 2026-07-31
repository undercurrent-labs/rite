//! Public embedding API for Rite.

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_core::{Diagnostics, SourceFile};
use rite_runtime::{check_source, run_file, EvalError, RuntimeContext, Value};
use rite_sem::{compile_to_ir, ir_to_json, ProgramIr};
use rite_syntax::{parse_source, Program};
use std::path::Path;

pub use rite_caps as caps;
pub use rite_core as core;
pub use rite_runtime as runtime;
pub use rite_sem as sem;
pub use rite_syntax as syntax;

/// High-level engine for embedding Rite.
pub struct RiteEngine {
    pub perms: PermissionSet,
    pub budget: rite_runtime::ExecutionBudget,
    /// Where a guest script's `@console` output goes.
    ///
    /// `None` means the host's own stdout and stderr, which is what `rite run`
    /// does and what a host almost always wants. It used to mean *nowhere*: the
    /// engine built a context, the script's output buffered into it, and the
    /// context was dropped when the run returned — so an embedded
    /// `! @console.println("…")` printed nothing at all and said nothing about it.
    output: Option<rite_runtime::OutputSink>,
}

pub struct RiteEngineBuilder {
    perms: PermissionSet,
    budget: rite_runtime::ExecutionBudget,
    output: Option<rite_runtime::OutputSink>,
}

/// Send a guest's output to the host's own streams, as `rite run` does.
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

impl RiteEngine {
    pub fn builder() -> RiteEngineBuilder {
        RiteEngineBuilder {
            perms: PermissionSet::default_secure(),
            budget: rite_runtime::ExecutionBudget::new(),
            output: None,
        }
    }

    pub async fn run_source(&self, name: &str, source: &str) -> Result<Value, EvalError> {
        let mut ctx = RuntimeContext::new();
        ctx.budget = self.budget.clone();
        // Installed before the run, not flushed after it, so a script that prints
        // as it goes reaches the host as it goes — a server or a long loop should
        // not hold its output until it finishes, and one that never finishes
        // should not lose all of it.
        ctx.sink = Some(self.output.clone().unwrap_or_else(inherit_output));
        install_defaults(&mut ctx, self.perms.clone());
        run_file(
            &SourceFile::new(rite_core::FileId(0), name, source),
            &mut ctx,
        )
        .await
    }

    pub async fn run_path(&self, path: impl AsRef<Path>) -> Result<Value, EvalError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| EvalError::Message(e.to_string()))?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("script.rite");
        self.run_source(name, &text).await
    }

    pub fn check_source(&self, name: &str, source: &str) -> Diagnostics {
        check_source(name, source)
    }

    pub fn compile_ir(&self, name: &str, source: &str) -> Result<ProgramIr, Diagnostics> {
        let (ir, diags) = compile_to_ir(&SourceFile::new(rite_core::FileId(0), name, source));
        if diags.has_errors() {
            return Err(diags);
        }
        ir.ok_or(diags)
    }

    pub fn parse(&self, name: &str, source: &str) -> Result<Program, Diagnostics> {
        let (p, d, _) = parse_source(name, source);
        if d.has_errors() {
            return Err(d);
        }
        p.ok_or(d)
    }
}

impl RiteEngineBuilder {
    /// No-op: the builtins are always installed.
    ///
    /// Kept because it is public API and removing it would break hosts that call
    /// it, but it has never selected anything — a host that omits it gets exactly
    /// the same engine.
    #[deprecated(
        note = "does nothing: builtins are always installed. Remove the call; the engine is unchanged."
    )]
    pub fn with_default_builtins(self) -> Self {
        self
    }

    /// Send the guest's `@console` output somewhere other than the host's own
    /// stdout and stderr — a log, a buffer, a UI pane.
    ///
    /// The sink is called as the script writes, not at the end, so a long-running
    /// guest streams rather than accumulating.
    pub fn with_output(
        mut self,
        sink: impl Fn(rite_runtime::OutputStream, &str) + Send + Sync + 'static,
    ) -> Self {
        self.output = Some(std::sync::Arc::new(sink));
        self
    }

    pub fn allow_all(mut self) -> Self {
        self.perms = PermissionSet::allow_all();
        self
    }

    pub fn allow(mut self, spec: &str) -> Result<Self, String> {
        self.perms.grant(Permission::parse(spec)?);
        Ok(self)
    }

    pub fn with_permissions(mut self, perms: PermissionSet) -> Self {
        self.perms = perms;
        self
    }

    pub fn with_budget(mut self, budget: rite_runtime::ExecutionBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn build(self) -> anyhow::Result<RiteEngine> {
        Ok(RiteEngine {
            perms: self.perms,
            budget: self.budget,
            output: self.output,
        })
    }
}

/// Convenience: run a script file with allow-all (dev).
pub async fn run_file_allow_all(path: impl AsRef<Path>) -> Result<Value, EvalError> {
    RiteEngine::builder()
        .allow_all()
        .build()
        .map_err(|e| EvalError::Message(e.to_string()))?
        .run_path(path)
        .await
}

pub fn ir_json(ir: &ProgramIr) -> serde_json::Value {
    ir_to_json(ir)
}

// re-export format
pub fn format_source(source: &str, ascii: bool) -> Result<String, String> {
    rite_fmt::format_source(source, ascii)
}
