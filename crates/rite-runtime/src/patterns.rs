//! Pattern matching and IR literal lowering, as free functions.
//!
//! Extracted for the same reason as [`crate::ops`]: `rite build` emitting real Rust has to
//! reach the matcher the interpreter uses, and a second copy of "what `~ ok v ⟦…⟧` means"
//! is exactly the duplication this tree keeps paying for.
//!
//! Takes `&AtomInterner` rather than the whole context — the only thing any branch needs
//! is an atom's name, so an atom pattern can compare against a string.

use crate::atom::AtomInterner;
use crate::value::{Key, ResultValue, Value};
use crate::EvalError;
use rite_sem::{PatternIr, ResultPatKindIr, ValueLiteral};

/// Match `value` against `pat`, returning the names it binds, or `None` if it does not
/// match.
///
/// The `Result` is currently infallible — no branch below constructs an `Err`. It is kept
/// because a pattern form that *can* fail (a guard, a contract-checked binding) would
/// otherwise change this signature and every call site at once.
pub fn match_pattern(
    atoms: &AtomInterner,
    pat: &PatternIr,
    value: &Value,
) -> Result<Option<Vec<(String, Value)>>, EvalError> {
    let mut bindings = Vec::new();
    if match_inner(atoms, pat, value, &mut bindings)? {
        Ok(Some(bindings))
    } else {
        Ok(None)
    }
}

/// The recursive worker. Appends to `bindings` as it goes, so a partial match may leave
/// entries behind — callers discard them, since a failed arm binds nothing.
pub fn match_inner(
    atoms: &AtomInterner,
    pat: &PatternIr,
    value: &Value,
    bindings: &mut Vec<(String, Value)>,
) -> Result<bool, EvalError> {
    match pat {
        PatternIr::Wildcard => Ok(true),
        PatternIr::Ident(_, name) => {
            bindings.push((name.clone(), value.clone()));
            Ok(true)
        }
        // An atom pattern also matches the equivalent string, so a record key or a
        // JSON-decoded tag matches `#ok`.
        PatternIr::Atom(name) => match value {
            Value::Atom(id) => Ok(atoms.name(*id) == *name),
            Value::String(s) => Ok(s.as_ref() == name),
            _ => Ok(false),
        },
        PatternIr::Literal(lit) => Ok(literal_value(lit).structural_eq(value)),
        PatternIr::List { elements, rest } => {
            let Value::List(xs) = value else {
                return Ok(false);
            };
            if rest.is_none() && elements.len() != xs.len() {
                return Ok(false);
            }
            if elements.len() > xs.len() {
                return Ok(false);
            }
            for (i, ep) in elements.iter().enumerate() {
                if !match_inner(atoms, ep, &xs[i], bindings)? {
                    return Ok(false);
                }
            }
            if let Some(r) = rest {
                let rest_vals: im::Vector<Value> =
                    xs.iter().skip(elements.len()).cloned().collect();
                if !match_inner(atoms, r, &Value::List(rest_vals), bindings)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        PatternIr::Record { fields } => {
            let Value::Record(rec) = value else {
                return Ok(false);
            };
            for (name, sub) in fields {
                let v = rec
                    .get(&Key::String(name.clone()))
                    .or_else(|| rec.get(&Key::Atom(name.clone())))
                    .cloned()
                    .unwrap_or(Value::None);
                if let Some(sp) = sub {
                    if !match_inner(atoms, sp, &v, bindings)? {
                        return Ok(false);
                    }
                } else {
                    bindings.push((name.clone(), v));
                }
            }
            Ok(true)
        }
        PatternIr::Result { kind, binding } => match (kind, value) {
            (ResultPatKindIr::Ok, Value::Result(ResultValue::Ok(v)))
            | (ResultPatKindIr::Err, Value::Result(ResultValue::Err(v))) => match binding {
                Some(b) => match_inner(atoms, b, v, bindings),
                None => Ok(true),
            },
            (ResultPatKindIr::Some, v) if !matches!(v, Value::None) => match binding {
                Some(b) => match_inner(atoms, b, v, bindings),
                None => Ok(true),
            },
            (ResultPatKindIr::None, Value::None) => Ok(true),
            _ => Ok(false),
        },
    }
}

/// An IR literal as a runtime value.
pub fn literal_value(lit: &ValueLiteral) -> Value {
    match lit {
        ValueLiteral::None(_) => Value::None,
        ValueLiteral::Bool(b, _) => Value::Bool(*b),
        ValueLiteral::Int(n, _) => Value::Int(*n),
        ValueLiteral::Float(n, _) => Value::Float(*n),
        ValueLiteral::String(s, _) => Value::string(s.clone()),
    }
}
