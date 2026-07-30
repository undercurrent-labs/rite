//! Rite runtime: values, environment, async tree-walking evaluator.

pub mod atom;
pub mod budget;
pub mod builtins;
pub mod env;
pub mod eval;
pub mod value;

pub use atom::AtomInterner;
pub use budget::ExecutionBudget;
pub use env::Environment;
pub use eval::{
    CapabilityHost, EvalError, Evaluator, FunctionEntry, HttpNextInvoker, OutputSink, OutputStream,
    PendingHttpServer, RuntimeContext, StackFrame,
};
pub use value::*;

use rite_core::{Diagnostics, SourceFile, SourceMap};
use rite_sem::{compile_to_ir, ProgramIr};

/// Run a source file through parse → IR → evaluate.
pub async fn run_source(
    name: &str,
    text: &str,
    ctx: &mut RuntimeContext,
) -> Result<Value, EvalError> {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, text);
    let file = sources.get(id).unwrap().clone();
    run_file(&file, ctx).await
}

pub async fn run_file(file: &SourceFile, ctx: &mut RuntimeContext) -> Result<Value, EvalError> {
    let path = file.path.clone();
    let roots = ctx.module_roots.clone();
    let (ir, diags) = if let Some(ref p) = path {
        rite_sem::compile_to_ir_with_roots(file, Some(p), &roots)
    } else if let Some(ref dir) = ctx.script_dir {
        rite_sem::compile_to_ir_with_roots(file, None, std::slice::from_ref(dir))
    } else {
        compile_to_ir(file)
    };
    if diags.has_errors() {
        return Err(EvalError::Compile(diags));
    }
    let ir = ir.ok_or_else(|| EvalError::Message("no IR produced".into()))?;
    // Preserve existing sources; ensure entry present for stack traces
    if ctx.sources.files().is_empty() {
        let _ = ctx.sources.add_file(&file.name, file.as_str());
    }
    eval_program(&ir, ctx).await
}

/// Evaluate a pre-built ProgramIr (used by compiled binaries and differential tests).
pub async fn run_ir(ir: &ProgramIr, ctx: &mut RuntimeContext) -> Result<Value, EvalError> {
    eval_program(ir, ctx).await
}

pub async fn eval_program(ir: &ProgramIr, ctx: &mut RuntimeContext) -> Result<Value, EvalError> {
    let mut evaluator = Evaluator::new(ctx);
    evaluator.eval_program(ir).await
}

pub fn check_source(name: &str, text: &str) -> Diagnostics {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, text);
    let file = sources.get(id).unwrap().clone();
    let (_ir, diags) = compile_to_ir(&file);
    diags
}
