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
use crate::builtins::{compare_values, list_remove_first, membership, merge_records};
use crate::value::{Key, ResultValue, Value};
use crate::EvalError;
use rite_sem::{BinaryOpIr, UnaryOpIr};

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
        BinaryOpIr::Lt => Ok(Value::Bool(compare_values(&l, &r) < 0)),
        BinaryOpIr::LtEq => Ok(Value::Bool(compare_values(&l, &r) <= 0)),
        BinaryOpIr::Gt => Ok(Value::Bool(compare_values(&l, &r) > 0)),
        BinaryOpIr::GtEq => Ok(Value::Bool(compare_values(&l, &r) >= 0)),
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
