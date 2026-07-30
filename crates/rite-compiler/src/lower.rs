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
use std::fmt::Write as _;

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
pub fn expr(e: &ExprIr) -> Lowered {
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
            expr(value)?,
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
                let _ = write!(out, "let _ = {}; ", expr(p)?);
            }
            let _ = write!(out, "{} }}", expr(last)?);
            out
        }

        ExprIr::Block(b) => block(b)?,

        ExprIr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let els = match else_branch {
                Some(b) => block(b)?,
                None => "Value::None".into(),
            };
            format!(
                "{{ let __c = {}; if __c.is_truthy() {{ {} }} else {{ {} }} }}",
                expr(condition)?,
                block(then_branch)?,
                els
            )
        }

        ExprIr::BuildList(items, _) => {
            let mut parts = Vec::new();
            for i in items {
                parts.push(expr(i)?);
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
                parts.push(format!("({}, {})", key, expr(v)?));
            }
            format!("Value::record(vec![{}])", parts.join(", "))
        }

        ExprIr::Member { object, field, .. } => {
            format!("{}.get_field({})", expr(object)?, rust_str(field))
        }

        ExprIr::Index { object, index, .. } => format!(
            "{{ let __o = {}; let __i = {}; rite_runtime::ops::index(&__o, &__i) }}",
            expr(object)?,
            expr(index)?
        ),

        ExprIr::Unary {
            op, expr: inner, ..
        } => format!(
            "rite_runtime::ops::unary(rite_sem::UnaryOpIr::{}, {})?",
            unary_op_path(*op),
            expr(inner)?
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
            expr(left)?,
            expr(right)?
        ),
        ExprIr::Binary {
            op: BinaryOpIr::Or,
            left,
            right,
            ..
        } => format!(
            "{{ let __l = {}; if __l.is_truthy() {{ __l }} else {{ {} }} }}",
            expr(left)?,
            expr(right)?
        ),
        ExprIr::Binary {
            op, left, right, ..
        } => format!(
            "{{ let __l = {}; let __r = {}; \
             rite_runtime::ops::binary(&ctx.atoms, rite_sem::BinaryOpIr::{}, __l, __r)? }}",
            expr(left)?,
            expr(right)?,
            binary_op_path(*op)
        ),

        ExprIr::Coalesce { left, right, .. } => format!(
            "{{ let __l = {}; if matches!(__l, Value::None) {{ {} }} else {{ __l }} }}",
            expr(left)?,
            expr(right)?
        ),

        ExprIr::Try { expr: inner, .. } => {
            format!("rite_runtime::ops::unwrap_try({})?", expr(inner)?)
        }

        // `^` unwinds to the enclosing function boundary, which is what `EvalError::Return`
        // means to every layer between here and there.
        ExprIr::Return(value, _) => {
            let v = match value {
                Some(v) => expr(v)?,
                None => "Value::None".into(),
            };
            format!("return Err(EvalError::Return({v}))")
        }

        ExprIr::NativeCall { name, args, .. } => {
            let argv = args_vec(args)?;
            // Args are evaluated before the evaluator borrows ctx, so the borrow is
            // confined to the call itself.
            format!(
                "{{ {argv} let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_native_public({}, __args).await? }}",
                rust_str(name)
            )
        }

        ExprIr::Call { callee, args, .. } => {
            let argv = args_vec(args)?;
            format!(
                "{{ let __callee = {}; {argv} \
                 let mut __ev = rite_runtime::Evaluator::new(ctx); \
                 __ev.call_value_public(__callee, __args).await? }}",
                expr(callee)?
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
            let argv = args_vec(args)?;
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

        ExprIr::Match { .. } => return Err(Unsupported("Match")),
        ExprIr::Closure(_) => return Err(Unsupported("Closure")),
        ExprIr::Pipeline { .. } => return Err(Unsupported("Pipeline")),
        ExprIr::HttpListen { .. } => return Err(Unsupported("HttpListen")),
        ExprIr::Assign { .. } => return Err(Unsupported("Assign")),
        ExprIr::Placeholder(_) => return Err(Unsupported("Placeholder")),
    })
}

/// `let __args = vec![…];` with each argument evaluated in order.
fn args_vec(args: &[ExprIr]) -> Lowered {
    let mut parts = Vec::new();
    for a in args {
        parts.push(expr(a)?);
    }
    Ok(format!("let __args = vec![{}];", parts.join(", ")))
}

/// A block: its own scope, evaluating to its last expression.
///
/// The frame is pushed and popped around an inner `async` block so an early `?` — a Rite
/// `^`, or any error — cannot skip the pop. Awaiting ends the borrow before the pop runs.
pub fn block(b: &BlockIr) -> Lowered {
    let mut body = String::new();
    for e in &b.body {
        let _ = write!(body, "__last = {}; ", expr(e)?);
    }
    Ok(format!(
        "{{ ctx.env.push_frame(); \
         let __r: Result<Value, EvalError> = async {{ \
         let mut __last = Value::None; {body} Ok(__last) }}.await; \
         ctx.env.pop_frame(); __r? }}"
    ))
}

/// A whole function as an `async fn`, or the reason it cannot be one.
pub fn function(f: &FunctionIr) -> Result<String, Unsupported> {
    let body = block(&f.body)?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// rite: fn {} at {:?}\n\
         pub async fn {}(ctx: &mut RuntimeContext, __params: Vec<Value>) -> Result<Value, EvalError> {{",
        f.name,
        f.span,
        mangle(&f.name)
    );
    // Parameters are bound into a fresh frame, the way `call_block` does it, so the body
    // finds them under both their id and their name.
    let _ = writeln!(out, "    ctx.env.push_frame();");
    for (i, (id, name)) in f.params.iter().zip(&f.param_names).enumerate() {
        let _ = writeln!(
            out,
            "    ctx.env.define({}, rite_sem::LocalId({}), __params.get({i}).cloned().unwrap_or(Value::None), false);",
            rust_str(name),
            id.0
        );
    }
    let _ = writeln!(
        out,
        "    let __r: Result<Value, EvalError> = async {{ Ok({body}) }}.await;"
    );
    let _ = writeln!(out, "    ctx.env.pop_frame();");
    // A function boundary is where `^` stops being an unwind and becomes the value.
    let _ = writeln!(
        out,
        "    match __r {{ Err(EvalError::Return(v)) => Ok(v), other => other }}\n}}"
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
pub fn survey(ir: &ProgramIr) -> (Vec<String>, Vec<(String, &'static str)>) {
    let mut ok = Vec::new();
    let mut fell_back = Vec::new();
    for f in &ir.functions {
        match function(f) {
            Ok(_) => ok.push(f.name.clone()),
            Err(Unsupported(why)) => fell_back.push((f.name.clone(), why)),
        }
    }
    (ok, fell_back)
}
