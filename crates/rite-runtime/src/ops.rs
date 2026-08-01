//! Operator semantics, as free functions.
//!
//! What `+`, `-`, `∈`, `[…]`, `?` and unary `-` mean in Rite lives here and nowhere else.
//! These were private methods on `Evaluator`, which was fine while the interpreter was the
//! only thing that needed them. `rite build` emitting real Rust needs them too, and the
//! alternative — generated code carrying its own copy — would put two definitions of `+`
//! in the tree. That is the failure mode this codebase keeps paying for: three effect
//! lists, three builtin lists, three column conventions. One definition, two callers.
//!
//! The context parameter is `&AtomInterner`, not `&mut RuntimeContext`. Only `-` on a
//! record and the membership tests need anything at all, and both only need to resolve an
//! atom's name — so adding two integers in generated code does not require a mutable
//! borrow of the world.

use crate::atom::AtomInterner;
use crate::builtins::{list_remove_first, membership, merge_records, try_compare_values};
use crate::value::{Key, ResultValue, Value};
use crate::EvalError;
use rite_sem::{BinaryOpIr, TypeExpr, UnaryOpIr};

/// Unary `-` / `not` / `!`.
pub fn unary(op: UnaryOpIr, v: Value) -> Result<Value, EvalError> {
    match op {
        UnaryOpIr::Neg => match v {
            Value::Int(n) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or_else(|| EvalError::Message("integer overflow".into())),
            Value::Float(f) => Ok(Value::Float(-f)),
            _ => Err(EvalError::Message("cannot negate non-number".into())),
        },
        UnaryOpIr::Not => Ok(Value::Bool(!v.is_truthy())),
        // `!` marks an effect for the reader and the checker; it does not transform.
        UnaryOpIr::Effect => Ok(v),
    }
}

/// `object[index]`. Out of range, and any type that cannot be indexed, give `none`.
pub fn index(obj: &Value, idx: &Value) -> Value {
    match (obj, idx) {
        (Value::List(xs), Value::Int(i)) => {
            if *i < 0 || *i as usize >= xs.len() {
                Value::None
            } else {
                xs[*i as usize].clone()
            }
        }
        (Value::Record(r), Value::String(s)) => r
            .get(&Key::String(s.to_string()))
            .cloned()
            .unwrap_or(Value::None),
        (Value::Record(r), other) => r
            .get(&Key::String(format!("{}", other)))
            .cloned()
            .unwrap_or(Value::None),
        _ => Value::None,
    }
}

/// Postfix `?`: unwrap `ok`, return early from the enclosing function on `err`.
///
/// The early return is an `EvalError::Return`, so a caller that is *not* a function
/// boundary must propagate it rather than treat it as a failure.
pub fn unwrap_try(v: Value) -> Result<Value, EvalError> {
    match v {
        Value::Result(ResultValue::Ok(inner)) => Ok(*inner),
        Value::Result(ResultValue::Err(e)) => Err(EvalError::Return(Value::err(*e))),
        other => Ok(other),
    }
}

/// A binary operator applied to two already-evaluated operands.
///
/// `And` / `Or` are absent on purpose: they short-circuit, so they cannot take
/// pre-evaluated operands and belong to whatever is walking the tree.
pub fn binary(
    atoms: &AtomInterner,
    op: BinaryOpIr,
    l: Value,
    r: Value,
) -> Result<Value, EvalError> {
    match op {
        BinaryOpIr::Add => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_add(*b)
                .map(Value::Int)
                .ok_or_else(|| EvalError::Message("integer overflow".into())),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::string(format!("{}{}", a, b))),
            (Value::List(a), Value::List(b)) => {
                let mut out = a.clone();
                out.append(b.clone());
                Ok(Value::List(out))
            }
            (Value::Record(a), Value::Record(b)) => Ok(Value::Record(merge_records(a, b))),
            (Value::List(a), other) => {
                let mut out = a.clone();
                out.push_back(other.clone());
                Ok(Value::List(out))
            }
            _ => Err(EvalError::Message(format!(
                "cannot add {} and {}",
                l.type_name(),
                r.type_name()
            ))),
        },
        BinaryOpIr::Sub => match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => a
                .checked_sub(*b)
                .map(Value::Int)
                .ok_or_else(|| EvalError::Message("integer overflow".into())),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - *b as f64)),
            (Value::List(a), other) => Ok(Value::List(list_remove_first(a, other))),
            (Value::Record(a), Value::Atom(atom)) => {
                let name = atoms.name(*atom);
                let mut out = a.clone();
                out.shift_remove(&Key::String(name.clone()));
                out.shift_remove(&Key::Atom(name));
                Ok(Value::Record(out))
            }
            _ => Err(EvalError::Message("cannot subtract values".into())),
        },
        BinaryOpIr::Mul => num_binop(&l, &r, |a, b| a.checked_mul(b), |a, b| a * b),
        BinaryOpIr::Div => match (&l, &r) {
            (Value::Int(_), Value::Int(0)) => Err(EvalError::Message("division by zero".into())),
            // `i64::MIN / -1` overflows; Rust panics on that in every profile.
            (Value::Int(a), Value::Int(b)) => a
                .checked_div(*b)
                .map(Value::Int)
                .ok_or_else(|| EvalError::Message("integer overflow".into())),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
            _ => Err(EvalError::Message("cannot divide values".into())),
        },
        BinaryOpIr::Rem => match (&l, &r) {
            (Value::Int(_), Value::Int(0)) => Err(EvalError::Message("division by zero".into())),
            // `i64::MIN % -1` overflows the same way `i64::MIN / -1` does.
            (Value::Int(a), Value::Int(b)) => a
                .checked_rem(*b)
                .map(Value::Int)
                .ok_or_else(|| EvalError::Message("integer overflow".into())),
            _ => Err(EvalError::Message("cannot rem values".into())),
        },
        BinaryOpIr::Eq => Ok(Value::Bool(l.structural_eq(&r))),
        BinaryOpIr::NotEq => Ok(Value::Bool(!l.structural_eq(&r))),
        // An incomparable pair raises rather than answering. It used to collapse to
        // `Equal`, so `"a" <= 1` and `"a" >= 1` were both true about values that are
        // not equal — the relational operators asserting something the equality
        // operator denied.
        BinaryOpIr::Lt => Ok(Value::Bool(try_compare_values(&l, &r)?.is_lt())),
        BinaryOpIr::LtEq => Ok(Value::Bool(try_compare_values(&l, &r)?.is_le())),
        BinaryOpIr::Gt => Ok(Value::Bool(try_compare_values(&l, &r)?.is_gt())),
        BinaryOpIr::GtEq => Ok(Value::Bool(try_compare_values(&l, &r)?.is_ge())),
        // Both operands are already evaluated by the caller; `∈` and `∉` share the same
        // membership test so neither re-runs a side-effecting operand.
        BinaryOpIr::In => Ok(Value::Bool(contains(atoms, &l, &r))),
        BinaryOpIr::NotIn => Ok(Value::Bool(!contains(atoms, &l, &r))),
        BinaryOpIr::And | BinaryOpIr::Or => Err(EvalError::Message(
            "`and` / `or` short-circuit and cannot take pre-evaluated operands".into(),
        )),
    }
}

/// Membership test behind `∈` / `∉`, with atoms also matching by name so
/// `#a ∈ ["a"]` and `#a ∈ ⟨a: 1⟩` hold.
pub fn contains(atoms: &AtomInterner, item: &Value, container: &Value) -> bool {
    if let Value::Atom(id) = item {
        let name = atoms.name(*id);
        match container {
            Value::List(xs) => {
                return xs
                    .iter()
                    .any(|x| x.structural_eq(item) || x.as_str() == Some(name.as_str()))
            }
            Value::Record(rec) => {
                return rec.contains_key(&Key::String(name.clone()))
                    || rec.contains_key(&Key::Atom(name))
            }
            _ => {}
        }
    }
    membership(item, container)
}

/// Shared shape for the arithmetic operators that promote int/float the same way.
fn num_binop(
    l: &Value,
    r: &Value,
    int_op: impl Fn(i64, i64) -> Option<i64>,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, EvalError> {
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => int_op(*a, *b)
            .map(Value::Int)
            .ok_or_else(|| EvalError::Message("integer overflow".into())),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
        _ => Err(EvalError::Message(
            "numeric operation on non-numbers".into(),
        )),
    }
}

/// Does `v` satisfy the declared type `ty`?
///
/// These are the runtime contracts `def f(x: int) -> int` has always *claimed* to
/// enforce — the generated reference has published "checked at runtime on function
/// entry/exit" since before there was any code behind it. Annotations were parsed,
/// printed back by the formatter, and dropped on the way to the IR.
///
/// Structural, not nominal: a value satisfies a type when its shape does. An empty
/// list satisfies `[int]` because there is nothing in it that does not.
///
/// `any` matches everything, which is what makes it useful as an escape hatch on
/// one parameter of an otherwise annotated function.
///
/// Lives here rather than in the evaluator because `rite build` emits calls to it —
/// the compiled path has to reject exactly what the interpreter rejects, and the
/// parity tests compare the two.
pub fn value_matches_type(v: &Value, ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Any(_) => true,
        TypeExpr::List(inner) => match v {
            Value::List(xs) => xs.iter().all(|x| value_matches_type(x, inner)),
            _ => false,
        },
        TypeExpr::Result(inner) => match v {
            Value::Result(r) => match r {
                ResultValue::Ok(x) => value_matches_type(x, inner),
                // `err` carries a failure, not a `T`; the payload is unconstrained.
                ResultValue::Err(_) => true,
            },
            _ => false,
        },
        TypeExpr::Record(fields) => match v {
            Value::Record(rec) => fields.iter().all(|(name, fty)| {
                rec.get(&Key::String(name.name.clone()))
                    .is_some_and(|fv| value_matches_type(fv, fty))
            }),
            _ => false,
        },
        TypeExpr::Named(name) => match name.name.as_str() {
            // `number` accepts either numeric type, because a function taking one
            // almost always means "a number" and Rite promotes int to float freely.
            "number" => matches!(v, Value::Int(_) | Value::Float(_)),
            "any" => true,
            other => v.type_name() == other,
        },
    }
}

/// Render a declared type the way the source wrote it, for a diagnostic.
pub fn type_expr_name(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Any(_) => "any".into(),
        TypeExpr::Named(n) => n.name.clone(),
        TypeExpr::List(inner) => format!("[{}]", type_expr_name(inner)),
        TypeExpr::Result(inner) => format!("result<{}>", type_expr_name(inner)),
        TypeExpr::Record(fields) => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n.name, type_expr_name(t)))
                .collect();
            format!("⟨{}⟩", inner.join(", "))
        }
    }
}

/// Where a value stopped matching its declared type, and what was there instead.
///
/// `[int]` against `[1, "a"]` is a list, so saying "expected [int], got list" names
/// the one thing that was right. This walks to the first element or field that does
/// not fit and reports the path to it.
fn mismatch(v: &Value, ty: &TypeExpr, path: &mut String) -> Option<(String, String)> {
    if value_matches_type(v, ty) {
        return None;
    }
    match (ty, v) {
        (TypeExpr::List(inner), Value::List(xs)) => {
            for (i, x) in xs.iter().enumerate() {
                let mark = path.len();
                path.push_str(&format!("[{i}]"));
                if let Some(found) = mismatch(x, inner, path) {
                    return Some(found);
                }
                path.truncate(mark);
            }
            None
        }
        (TypeExpr::Record(fields), Value::Record(rec)) => {
            for (name, fty) in fields {
                let mark = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(&name.name);
                match rec.get(&Key::String(name.name.clone())) {
                    // Distinguished from a wrong type: the field is not there at all,
                    // and "is none rather than int" would describe a field holding
                    // `none`, which is a different mistake.
                    None => return Some((String::new(), String::new())),
                    Some(fv) => {
                        if let Some(found) = mismatch(fv, fty, path) {
                            return Some(found);
                        }
                    }
                }
                path.truncate(mark);
            }
            None
        }
        (TypeExpr::Result(inner), Value::Result(ResultValue::Ok(x))) => {
            path.push_str(" (ok payload)");
            mismatch(x, inner, path)
        }
        _ => Some((type_expr_name(ty), v.type_name().to_string())),
    }
}

/// `expected X, got Y` for a value that failed `ty`, naming the inner position when
/// the outer shape was fine.
fn explain(v: &Value, ty: &TypeExpr) -> String {
    let mut path = String::new();
    match mismatch(v, ty, &mut path) {
        Some((want, _)) if want.is_empty() => {
            format!("expects {}, but has no field `{path}`", type_expr_name(ty))
        }
        Some((want, got)) if !path.is_empty() => format!(
            "expects {}, but {path} is {got} rather than {want}",
            type_expr_name(ty)
        ),
        Some((want, got)) => format!("expects {want}, got {got}"),
        // Unreachable in practice: callers only ask after a failed match.
        None => format!("expects {}", type_expr_name(ty)),
    }
}

/// Check one argument against its declared type, or explain why it does not fit.
pub fn check_param_type(
    func: &str,
    param: &str,
    v: &Value,
    ty: &TypeExpr,
) -> Result<(), EvalError> {
    if value_matches_type(v, ty) {
        return Ok(());
    }
    Err(EvalError::Message(format!(
        "{func}: parameter `{param}` {}",
        explain(v, ty)
    )))
}

/// Check a return value against the declared return type.
pub fn check_return_type(func: &str, v: &Value, ty: &TypeExpr) -> Result<(), EvalError> {
    if value_matches_type(v, ty) {
        return Ok(());
    }
    Err(EvalError::Message(format!(
        "{func}: declared to return {}, but returned {}",
        type_expr_name(ty),
        match mismatch(v, ty, &mut String::new()) {
            Some((_, got)) => got,
            None => v.type_name().to_string(),
        }
    )))
}
