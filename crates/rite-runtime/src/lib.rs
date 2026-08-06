//! Rite runtime: values, environment, async tree-walking evaluator.

pub mod atom;
pub mod budget;
pub mod builtins;
pub mod env;
pub mod eval;
pub mod handles;
pub mod ops;
pub mod patterns;
pub mod schema;
pub mod value;

pub use atom::AtomInterner;
pub use budget::{ExecutionBudget, Limits};
pub use env::Environment;
pub use eval::{
    CapabilityHost, EvalError, Evaluator, FunctionEntry, HttpNextInvoker, OutputSink, OutputStream,
    PendingHttpServer, RuntimeContext, StackFrame,
};
pub use handles::{HandleTable, DEFAULT_OPEN_HANDLE_LIMIT};
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
    run_file_with_bindings(file, ctx, &[]).await
}

/// [`run_file`], with names already bound to values the source does not produce.
///
/// Each `seed` name is defined in `ctx.env` and pre-declared for the resolver, so the
/// source can use it without declaring it. For a host that keeps a session across
/// several evaluations and holds values no literal can spell — a REPL and its open
/// handles — this is how the value survives the next `run_file` rather than being
/// re-derived by running the expression that made it a second time.
pub async fn run_file_with_bindings(
    file: &SourceFile,
    ctx: &mut RuntimeContext,
    seed: &[(String, Value)],
) -> Result<Value, EvalError> {
    let path = file.path.clone();
    // `ctx.module_roots` is honoured whether or not the source has a path of
    // its own. It used to be consulted only in the first branch, so a host that
    // evaluated source it had built in memory — a REPL line, or generated code
    // — had its module search path silently dropped, and a `use` that resolved
    // when the host checked it failed when the host ran it.
    let mut roots = Vec::new();
    if path.is_none() {
        if let Some(dir) = &ctx.script_dir {
            roots.push(dir.clone());
        }
    }
    roots.extend(ctx.module_roots.iter().cloned());
    let names: Vec<String> = seed.iter().map(|(n, _)| n.clone()).collect();
    let (ir, diags) =
        rite_sem::compile_to_ir_with_predeclared(file, path.as_deref(), &roots, &names);
    if diags.has_errors() {
        return Err(EvalError::Compile(diags));
    }
    let ir = ir.ok_or_else(|| EvalError::Message("no IR produced".into()))?;
    // Preserve existing sources; ensure entry present for stack traces
    if ctx.sources.files().is_empty() {
        let _ = ctx.sources.add_file(&file.name, file.as_str());
    }
    for (name, value) in seed {
        ctx.env.define_name(name, value.clone(), false);
    }
    eval_program(&ir, ctx).await
}

/// Evaluate a pre-built ProgramIr (used by compiled binaries and differential tests).
/// Build a closure value whose body is compiled Rust.
///
/// A constructor rather than a struct literal in generated code, so the generated crate
/// never names `parking_lot` or `Arc` — its manifest lists only what the emitted source
/// uses directly, and adding a dependency there is a build failure rather than a fallback.
pub fn native_closure(
    params: Vec<String>,
    ctx: &RuntimeContext,
    func: crate::value::NativeClosureFn,
) -> Value {
    Value::NativeClosure(crate::value::NativeClosure {
        id: crate::eval::next_closure_id(),
        params,
        // Captured by sharing, not copying: frames are shared, so an assignment inside the
        // body still reaches the scope that defined the name.
        env: std::sync::Arc::new(parking_lot::RwLock::new(ctx.env.clone())),
        func,
    })
}

/// Resolve a bare name the way the interpreter does.
///
/// Three tiers, and the order is the semantics: an environment binding wins, then a
/// registered function, then a builtin — as a dispatch token rather than a value, which is
/// what makes builtins shadowable by a local of the same name.
///
/// Public because generated code has to agree. A backend that only checked the environment
/// would report `undefined name \`str\`` for every builtin used as a value, which is
/// exactly what the first version of the lowering did.
pub fn lookup_global(ctx: &RuntimeContext, name: &str) -> Result<Value, EvalError> {
    if let Some(v) = ctx.env.get(name) {
        return Ok(v);
    }
    if ctx.functions.contains_key(name) {
        return ctx
            .env
            .get(name)
            .ok_or_else(|| EvalError::Message(format!("undefined function {}", name)));
    }
    if crate::eval::is_runtime_builtin(name) {
        return Ok(Value::NativeName(name.to_string()));
    }
    Err(EvalError::Message(format!("undefined name `{}`", name)))
}

/// Make a program's top-level functions callable in `ctx`.
///
/// Registers each one under its name and binds it as a closure value, so a body can reach
/// its siblings and every top-level binding whichever order they appear in — and so a
/// function can recurse.
///
/// Public because code generated by `rite build` needs it before running anything: a
/// lowered call, and the interpreted fallback for a function the backend could not lower,
/// both resolve callees through this. Skipping it produces `undefined name` at runtime for
/// the first recursive function, which is a confusing way to learn it.
pub fn register_functions(ir: &ProgramIr, ctx: &mut RuntimeContext) {
    use crate::value::{Closure, FnContract};
    for f in &ir.functions {
        ctx.functions.insert(
            f.name.clone(),
            crate::eval::FunctionEntry {
                params: f.param_names.clone(),
                body: f.body.clone(),
            },
        );
        // Built once per function and shared by every clone of the closure. A
        // function with nothing annotated gets `None`, so the check at the call
        // boundary is a null-pointer test for the overwhelming majority of calls.
        let contract = {
            let c = FnContract {
                name: f.name.clone(),
                param_names: f.param_names.clone(),
                param_types: f.param_types.clone(),
                return_type: f.return_type.clone(),
            };
            if c.is_empty() {
                None
            } else {
                Some(std::sync::Arc::new(c))
            }
        };
        // The capture is the module scope itself (shared frames), so ordering does not
        // matter and recursion resolves.
        let clos = Value::Function(Closure {
            id: crate::eval::next_closure_id(),
            name: Some(f.name.as_str().into()),
            params: f.param_names.clone(),
            env: std::sync::Arc::new(parking_lot::RwLock::new(ctx.env.clone())),
            body: f.body.clone(),
            contract: contract.clone(),
        });
        ctx.env.define_name(&f.name, clos, false);
    }
}

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
