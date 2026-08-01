//! Lower `ExprIr` into Rust source.
//!
//! The old backend base64-encoded the IR into the generated crate and called `run_ir`, so
//! a "compiled" binary was the interpreter carrying its program as a payload — exactly as
//! fast as `rite run`, for a multi-minute build. This emits real Rust: control flow becomes
//! Rust control flow, operators become direct calls into [`rite_runtime::ops`], and the
//! per-node boxed future and match-on-variant dispatch disappear.
//!
//! # What it does not do
//!
//! Locals stay in `ctx.env` rather than becoming Rust `let` bindings. A Rite closure
//! captures the environment, so promoting locals out of it would silently break capture —
//! `{ |x| x + n }` would stop seeing `n`. Doing that properly needs escape analysis, and
//! it is a later stage.
//!
//! Anything this cannot lower falls back to interpretation **per function**, so a program
//! always builds and coverage improves without a flag day. `rite build` reports which
//! functions fell back rather than leaving it to be discovered from a benchmark.

use rite_sem::{
    BinaryOpIr, BlockIr, EffectKind, ExprIr, FunctionIr, KeyIr, ProgramIr, UnaryOpIr, ValueLiteral,
};
use std::collections::HashSet;
use std::fmt::Write as _;

/// Lowering context: which functions were compiled, plus the closure bodies hoisted out
/// during lowering.
///
/// A call to a compiled function becomes a direct Rust call rather than a trip through the
/// interpreter's closure machinery — that indirection is where the time goes. Without it a
/// compiled `fib(24)` ran in exactly the same 778ms as the interpreter.
///
/// Closure bodies cannot be emitted inline (Rust has no expression-position `async fn`
/// returning a boxed future), so they are hoisted to module level and referenced by name.
#[derive(Default)]
pub struct Compiled {
    names: HashSet<String>,
    hoisted: std::cell::RefCell<Vec<String>>,
    next_id: std::cell::Cell<usize>,
}

impl Compiled {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn insert(&mut self, name: String) {
        self.names.insert(name);
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Park a generated function at module level, returning its name.
    fn hoist(&self, make: impl FnOnce(&str) -> String) -> String {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let name = format!("rite_closure_{id}");
        let code = make(&name);
        self.hoisted.borrow_mut().push(code);
        name
    }

    /// The hoisted closure bodies, to emit alongside the compiled functions.
    pub fn take_hoisted(&self) -> Vec<String> {
        std::mem::take(&mut self.hoisted.borrow_mut())
    }
}

/// Why a node could not be lowered. Carries the variant name so the build note is specific.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub &'static str);

type Lowered = Result<String, Unsupported>;

/// A Rust string literal for `s`.
fn rust_str(s: &str) -> String {
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
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn binary_op_path(op: BinaryOpIr) -> &'static str {
    use BinaryOpIr::*;
    match op {
        Add => "Add",
        Sub => "Sub",
        Mul => "Mul",
        Div => "Div",
        Rem => "Rem",
        Eq => "Eq",
        NotEq => "NotEq",
        Lt => "Lt",
        LtEq => "LtEq",
        Gt => "Gt",
        GtEq => "GtEq",
        In => "In",
        NotIn => "NotIn",
        And => "And",
        Or => "Or",
    }
}

fn unary_op_path(op: UnaryOpIr) -> &'static str {
    match op {
        UnaryOpIr::Neg => "Neg",
        UnaryOpIr::Not => "Not",
        UnaryOpIr::Effect => "Effect",
    }
}

/// A constant as a Rust expression building the same `Value`.
fn constant(lit: &ValueLiteral) -> String {
    match lit {
        ValueLiteral::None(_) => "Value::None".into(),
        ValueLiteral::Bool(b, _) => format!("Value::Bool({b})"),
        ValueLiteral::Int(n, _) => format!("Value::Int({n}i64)"),
        // `{:?}` on f64 round-trips (`1.0` stays `1.0`), where `{}` would print `1`.
        ValueLiteral::Float(f, _) => format!("Value::Float({f:?}f64)"),
        ValueLiteral::String(s, _) => format!("Value::string({})", rust_str(s)),
    }
}

/// Lower one expression to a Rust expression of type `Value`.
///
/// The emitted code runs inside an `async` block returning `Result<Value, EvalError>`, so
/// `?` is available and an `EvalError::Return` propagates exactly as it does in the
/// tree-walker: outward until a function boundary converts it.
pub fn expr(e: &ExprIr, compiled: &Compiled) -> Lowered {
    Ok(match e {
        ExprIr::Constant(lit) => constant(lit),

        ExprIr::Atom(name, _) => {
            format!("Value::Atom(ctx.atoms.intern({}))", rust_str(name))
        }

        ExprIr::Local(id) => format!(
            "ctx.env.get_local(rite_sem::LocalId({})).ok_or_else(|| \
             EvalError::Message(\"undefined local {}\".to_string()))?",
            id.0, id.0
        ),

        // Not `ctx.env.get`: a bare name resolves in three tiers, and a builtin used as a
        // value (`str`, `map`) lives in the third. Checking only the environment reported
        // `undefined name \`str\`` for every one of them.
        ExprIr::Global(name) => format!("rite_runtime::lookup_global(ctx, {})?", rust_str(name)),

        ExprIr::Bind {
            local,
            name,
            mutable,
            value,
            ..
        } => format!(
            "{{ let __v = {}; ctx.env.define({}, rite_sem::LocalId({}), __v.clone(), {}); __v }}",
            expr(value, compiled)?,
            rust_str(name),
            local.0,
            mutable
        ),

        ExprIr::Seq(parts, _) => {
            let Some((last, rest)) = parts.split_last() else {
                return Ok("Value::None".into());
            };
            let mut out = String::from("{ ");
            for p in rest {
                let _ = write!(out, "let _ = {}; ", expr(p, compiled)?);
            }
            let _ = write!(out, "{} }}", expr(last, compiled)?);
            out
        }

        ExprIr::Block(b) => block(b, compiled)?,

        ExprIr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let els = match else_branch {
                Some(b) => block(b, compiled)?,
                None => "Value::None".into(),
            };
            format!(
                "{{ let __c = {}; if __c.is_truthy() {{ {} }} else {{ {} }} }}",
                expr(condition, compiled)?,
                block(then_branch, compiled)?,
                els
            )
        }

        ExprIr::BuildList(items, _) => {
            let mut parts = Vec::new();
            for i in items {
                parts.push(expr(i, compiled)?);
            }
            format!("Value::list(vec![{}])", parts.join(", "))
        }

        ExprIr::BuildRecord(entries, _) => {
            // `Value::record` takes pairs, so the generated crate never names indexmap —
            // its manifest lists only what the emitted code uses directly.
            let mut parts = Vec::new();
            for (k, v) in entries {
                // Spread has no key variant: it desugars to the `+` merge, so
                // `⟨..base, k: v⟩` arrives here as a Binary(Add) over records.
                let key = match k {
                    KeyIr::Ident(name) | KeyIr::String(name) => {
                        format!("Key::String({}.to_string())", rust_str(name))
                    }
                    KeyIr::Atom(name) => format!("Key::Atom({}.to_string())", rust_str(name)),
                };
                parts.push(format!("({}, {})", key, expr(v, compiled)?));
            }
            format!("Value::record(vec![{}])", parts.join(", "))
        }

        ExprIr::Member { object, field, .. } => {
            format!("{}.get_field({})", expr(object, compiled)?, rust_str(field))
        }

        ExprIr::Index { object, index, .. } => format!(
            "{{ let __o = {}; let __i = {}; rite_runtime::ops::index(&__o, &__i) }}",
            expr(object, compiled)?,
            expr(index, compiled)?
        ),

        ExprIr::Unary {
            op, expr: inner, ..
        } => format!(
            "rite_runtime::ops::unary(rite_sem::UnaryOpIr::{}, {})?",
            unary_op_path(*op),
            expr(inner, compiled)?
        ),

        // `and` / `or` short-circuit, so they cannot go through `ops::binary` — it takes
        // operands already evaluated, and evaluating the right side eagerly would run an
        // effect the program says should not run.
        ExprIr::Binary {
            op: BinaryOpIr::And,
            left,
            right,
            ..
        } => format!(
            "{{ let __l = {}; if __l.is_truthy() {{ {} }} else {{ __l }} }}",
            expr(left, compiled)?,
            expr(right, compiled)?
        ),
        ExprIr::Binary {
            op: BinaryOpIr::Or,
            left,
            right,
            ..
        } => format!(
            "{{ let __l = {}; if __l.is_truthy() {{ __l }} else {{ {} }} }}",
            expr(left, compiled)?,
            expr(right, compiled)?
        ),
        ExprIr::Binary {
            op, left, right, ..
        } => format!(
            "{{ let __l = {}; let __r = {}; \
             rite_runtime::ops::binary(&ctx.atoms, rite_sem::BinaryOpIr::{}, __l, __r)? }}",
            expr(left, compiled)?,
            expr(right, compiled)?,
            binary_op_path(*op)
        ),

        ExprIr::Coalesce { left, right, .. } => format!(
            "{{ let __l = {}; if matches!(__l, Value::None) {{ {} }} else {{ __l }} }}",
            expr(left, compiled)?,
            expr(right, compiled)?
        ),

        ExprIr::Try { expr: inner, .. } => {
            format!("rite_runtime::ops::unwrap_try({})?", expr(inner, compiled)?)
        }

        // `^` unwinds to the enclosing function boundary, which is what `EvalError::Return`
        // means to every layer between here and there.
        ExprIr::Return(value, _) => {
            let v = match value {
                Some(v) => expr(v, compiled)?,
                None => "Value::None".into(),
            };
            format!("return Err(EvalError::Return({v}))")
        }

        ExprIr::NativeCall { name, args, .. } => {
            let argv = args_vec(args, compiled)?;
            // Args are evaluated before the evaluator borrows ctx, so the borrow is
            // confined to the call itself.
            format!(
                "{{ {argv} let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_native_public({}, __args).await? }}",
                rust_str(name)
            )
        }

        ExprIr::Call { callee, args, .. } => {
            // A call to a function this backend also compiled becomes a direct Rust call.
            // Everything else goes through the interpreter's dispatch, which is what makes
            // a closure value, a builtin used as a value, and a fallback function all work.
            if let ExprIr::Global(name) = callee.as_ref() {
                if compiled.contains(name) {
                    let argv = args_vec(args, compiled)?;
                    return Ok(format!("{{ {argv} {}(ctx, __args).await? }}", mangle(name)));
                }
            }
            let argv = args_vec(args, compiled)?;
            format!(
                "{{ let __callee = {}; {argv} \
                 let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_value_public(__callee, __args).await? }}",
                expr(callee, compiled)?
            )
        }

        ExprIr::CapabilityCall {
            path, args, effect, ..
        } => {
            // `@console` is special-cased inside the evaluator (it needs the context's
            // output buffer, not the capability host), so route it back through the
            // interpreter rather than reimplementing that here.
            if path.first().map(String::as_str) == Some("console") {
                return Err(Unsupported("CapabilityCall(@console)"));
            }
            let argv = args_vec(args, compiled)?;
            let eff = matches!(effect, EffectKind::Effect);
            let parts: Vec<String> = path
                .iter()
                .map(|p| format!("{}.to_string()", rust_str(p)))
                .collect();
            format!(
                "{{ {argv} let __path = vec![{}]; let __caps = ctx.capabilities.clone(); \
                 __caps.call(&__path, __args, {eff}, ctx).await? }}",
                parts.join(", ")
            )
        }

        ExprIr::Closure(c) => closure_value(c, compiled)?,

        // Thread the value through the stages, exactly as `eval_pipeline_stage` does. The
        // win is not the threading — it is that a closure argument here is now a compiled
        // body rather than a `BlockIr` the interpreter walks once per element.
        ExprIr::Pipeline { input, stages, .. } => {
            let mut out = format!("{{ let mut __v = {};", expr(input, compiled)?);
            for stage in stages {
                let _ = write!(out, " __v = {};", pipeline_stage(stage, compiled)?);
            }
            out.push_str(" __v }");
            out
        }

        ExprIr::Match { .. } => return Err(Unsupported("Match")),
        ExprIr::HttpListen { .. } => return Err(Unsupported("HttpListen")),
        // `n := v` arrives from the desugarer as Assign{ value: Seq[Global(name), value] }.
        // Assigning through `env.assign` is what makes the write reach the frame that
        // declared the name, including a frame a closure captured.
        ExprIr::Assign { value, .. } => {
            let ExprIr::Seq(parts, _) = value.as_ref() else {
                return Err(Unsupported("Assign"));
            };
            let [ExprIr::Global(name), rhs] = parts.as_slice() else {
                return Err(Unsupported("Assign"));
            };
            format!(
                "{{ let __v = {}; ctx.env.assign({}, __v.clone()).map_err(EvalError::Message)?; __v }}",
                expr(rhs, compiled)?,
                rust_str(name)
            )
        }
        ExprIr::Placeholder(_) => return Err(Unsupported("Placeholder")),
    })
}

/// A closure as a `Value`, with its body hoisted to a module-level function.
fn closure_value(c: &rite_sem::ClosureIr, compiled: &Compiled) -> Lowered {
    let body = block(&c.body, compiled)?;
    let mut binds = String::new();
    for (i, (id, name)) in c.params.iter().zip(&c.param_names).enumerate() {
        let _ = write!(
            binds,
            "ctx.env.define({}, rite_sem::LocalId({}), __args.get({i}).cloned().unwrap_or(Value::None), false); ",
            rust_str(name),
            id.0
        );
    }
    let fn_name = compiled.hoist(|name| {
        format!(
            "fn {name}<'a>(ctx: &'a mut RuntimeContext, __args: Vec<Value>) \
             -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, EvalError>> + Send + 'a>> {{\n    \
             Box::pin(async move {{\n        \
             ctx.budget.tick()?;\n        \
             ctx.env.push_frame();\n        {binds}\n        \
             let __r: Result<Value, EvalError> = async {{ Ok({body}) }}.await;\n        \
             ctx.env.pop_frame();\n        __r\n    }})\n}}\n"
        )
    });
    let params: Vec<String> = c
        .param_names
        .iter()
        .map(|p| format!("{}.to_string()", rust_str(p)))
        .collect();
    Ok(format!(
        "rite_runtime::native_closure(vec![{}], ctx, {fn_name})",
        params.join(", ")
    ))
}

/// One pipeline stage applied to `__v`, mirroring `Evaluator::eval_pipeline_stage`.
fn pipeline_stage(stage: &rite_sem::PipelineStageIr, compiled: &Compiled) -> Lowered {
    use rite_sem::StageKind;
    Ok(match &stage.kind {
        // `xs → map .name` — project a field over a list, or off a single value.
        StageKind::MemberProjection(field) => format!(
            "match __v {{ Value::List(__xs) => Value::list(__xs.iter().map(|__x| __x.get_field({f})).collect::<Vec<_>>()), \
             __other => __other.get_field({f}) }}",
            f = rust_str(field)
        ),
        StageKind::Block | StageKind::Call => match &stage.expr {
            rite_sem::ExprIr::NativeCall { name, args, .. } => {
                let mut parts = vec!["__v".to_string()];
                for a in args {
                    parts.push(expr(a, compiled)?);
                }
                format!(
                    "{{ let __args = vec![{}]; let mut __ev = rite_runtime::Evaluator::new(ctx); \
                     __ev.call_native_public({}, __args).await? }}",
                    parts.join(", "),
                    rust_str(name)
                )
            }
            // A bare name in stage position resolves the way a call callee does:
            // a function this backend compiled is dispatched directly, and anything
            // else goes through `lookup_global` — binding, then function, then
            // builtin. This used to call `call_native_public` unconditionally, which
            // consulted the builtin table and nothing else, so a user's own function
            // was unreachable as a stage and a builtin won over a definition that
            // shadowed it everywhere else. The interpreter had the same bug; both
            // sides move together or `interpreter_ir_parity` fails.
            rite_sem::ExprIr::Global(name) if compiled.contains(name) => format!(
                "{{ let __args = vec![__v]; {}(ctx, __args).await? }}",
                mangle(name)
            ),
            rite_sem::ExprIr::Global(name) => format!(
                "{{ let __c = rite_runtime::lookup_global(ctx, {})?; let __args = vec![__v]; \
                 let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_value_public(__c, __args).await? }}",
                rust_str(name)
            ),
            rite_sem::ExprIr::Closure(c) => format!(
                "{{ let __c = {}; let __args = vec![__v]; \
                 let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_value_public(__c, __args).await? }}",
                closure_value(c, compiled)?
            ),
            // `$` marks where the value goes; without one it is prepended.
            rite_sem::ExprIr::Call { callee, args, .. } => {
                let mut parts = Vec::new();
                let mut used_placeholder = false;
                for a in args {
                    if matches!(a, rite_sem::ExprIr::Placeholder(_)) {
                        used_placeholder = true;
                        parts.push("__v.clone()".to_string());
                    } else {
                        parts.push(expr(a, compiled)?);
                    }
                }
                if !used_placeholder {
                    parts.insert(0, "__v".to_string());
                }
                format!(
                    "{{ let __c = {}; let __args = vec![{}]; \
                     let mut __ev = rite_runtime::Evaluator::new(ctx); \
                     __ev.call_value_public(__c, __args).await? }}",
                    expr(callee, compiled)?,
                    parts.join(", ")
                )
            }
            _ => return Err(Unsupported("PipelineStage")),
        },
    })
}

/// `let __args = vec![…];` with each argument evaluated in order.
fn args_vec(args: &[ExprIr], compiled: &Compiled) -> Lowered {
    let mut parts = Vec::new();
    for a in args {
        parts.push(expr(a, compiled)?);
    }
    Ok(format!("let __args = vec![{}];", parts.join(", ")))
}

/// A block: its own scope, evaluating to its last expression.
///
/// The frame is pushed and popped around an inner `async` block so an early `?` — a Rite
/// `^`, or any error — cannot skip the pop. Awaiting ends the borrow before the pop runs.
pub fn block(b: &BlockIr, compiled: &Compiled) -> Lowered {
    let mut body = String::new();
    for e in &b.body {
        let _ = write!(body, "__last = {}; ", expr(e, compiled)?);
    }
    Ok(format!(
        "{{ ctx.env.push_frame(); \
         let __r: Result<Value, EvalError> = async {{ \
         let mut __last = Value::None; {body} Ok(__last) }}.await; \
         ctx.env.pop_frame(); __r? }}"
    ))
}

/// A whole function as real Rust, or the reason it cannot be.
///
/// Returns a boxed future rather than being a plain `async fn`, because a Rite function
/// may recurse and a directly-recursive `async fn` has an infinitely-sized future. This
/// is the same reason `Evaluator::eval_expr` boxes — but once per *call* instead of once
/// per node, which is the difference the backend is here to make.
///
/// The prologue mirrors `call_value`/`call_block` exactly. Skipping any of it would let a
/// compiled binary escape a guarantee the interpreter makes: the depth check is what turns
/// runaway recursion into an error instead of a stack overflow, and the budget tick is what
/// stops a runaway loop.
pub fn function(f: &FunctionIr, compiled: &Compiled) -> Result<String, Unsupported> {
    let body = block(&f.body, compiled)?;
    let mut out = String::new();
    let _ = writeln!(out, "// rite: fn {} at {:?}", f.name, f.span);
    let _ = writeln!(
        out,
        "pub fn {}<'a>(ctx: &'a mut RuntimeContext, __args: Vec<Value>) \
         -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, EvalError>> + Send + 'a>> {{",
        mangle(&f.name)
    );
    let _ = writeln!(out, "    Box::pin(async move {{");
    let _ = writeln!(out, "        ctx.budget.tick()?;");
    let _ = writeln!(
        out,
        "        ctx.budget.check_depth(ctx.call_depth + 1)?;\n        ctx.call_depth += 1;"
    );
    let _ = writeln!(
        out,
        "        if __args.len() != {} {{ ctx.call_depth -= 1; return Err(EvalError::Message(\
         format!(\"arity mismatch: expected {} args, got {{}}\", __args.len()))); }}",
        f.params.len(),
        f.params.len()
    );
    let _ = writeln!(out, "        ctx.env.push_frame();");
    // Bound under both the name and the local id, as `call_block` does — the body may
    // reference either depending on how the resolver saw it.
    for (i, (id, name)) in f.params.iter().zip(&f.param_names).enumerate() {
        let _ = writeln!(
            out,
            "        ctx.env.define({}, rite_sem::LocalId({}), __args.get({i}).cloned().unwrap_or(Value::None), false);",
            rust_str(name),
            id.0
        );
    }
    let _ = writeln!(
        out,
        "        let __r: Result<Value, EvalError> = async {{ Ok({body}) }}.await;"
    );
    let _ = writeln!(out, "        ctx.env.pop_frame();");
    let _ = writeln!(out, "        ctx.call_depth -= 1;");
    // A function boundary is where `^` stops being an unwind and becomes the value.
    let _ = writeln!(
        out,
        "        match __r {{ Err(EvalError::Return(v)) => Ok(v), other => other }}\n    }})\n}}"
    );
    Ok(out)
}

/// A Rite name as a Rust identifier.
pub fn mangle(name: &str) -> String {
    let mut out = String::from("rite_fn_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            // Rite identifiers accept any non-ASCII byte, and Rust will not.
            let _ = write!(out, "_u{:x}", ch as u32);
        }
    }
    out
}

/// Which functions in `ir` can be lowered, and why the rest cannot.
///
/// Whether a function lowers does not depend on which *other* functions do — a call to an
/// uncompiled one still lowers, through the interpreter's dispatch — so surveying with an
/// empty set and then emitting with the result is stable rather than a fixpoint.
pub fn survey(ir: &ProgramIr) -> (Compiled, Vec<(String, &'static str)>) {
    let empty = Compiled::new();
    let mut ok = Compiled::new();
    let mut fell_back = Vec::new();
    for f in &ir.functions {
        match function(f, &empty) {
            Ok(_) => {
                ok.insert(f.name.clone());
            }
            Err(Unsupported(why)) => fell_back.push((f.name.clone(), why)),
        }
    }
    fell_back.sort();
    (ok, fell_back)
}
