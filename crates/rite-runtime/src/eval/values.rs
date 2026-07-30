//! Pattern matching and IR literal lowering.

use super::*;
use crate::value::{Key, ResultValue, Value};
use rite_sem::{PatternIr, ResultPatKindIr, ValueLiteral};

impl<'a> Evaluator<'a> {
    pub(super) fn match_pattern(
        &self,
        pat: &PatternIr,
        value: &Value,
    ) -> Result<Option<Vec<(String, Value)>>, EvalError> {
        let mut bindings = Vec::new();
        if self.match_pattern_inner(pat, value, &mut bindings)? {
            Ok(Some(bindings))
        } else {
            Ok(None)
        }
    }

    pub(super) fn match_pattern_inner(
        &self,
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
            PatternIr::Atom(name) => match value {
                Value::Atom(id) => Ok(self.ctx.atoms.name(*id) == *name),
                Value::String(s) => Ok(s.as_ref() == name),
                _ => Ok(false),
            },
            PatternIr::Literal(lit) => {
                let lv = self.lit_to_value(lit);
                Ok(lv.structural_eq(value))
            }
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
                    if !self.match_pattern_inner(ep, &xs[i], bindings)? {
                        return Ok(false);
                    }
                }
                if let Some(r) = rest {
                    let rest_vals: im::Vector<Value> =
                        xs.iter().skip(elements.len()).cloned().collect();
                    if !self.match_pattern_inner(r, &Value::List(rest_vals), bindings)? {
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
                        if !self.match_pattern_inner(sp, &v, bindings)? {
                            return Ok(false);
                        }
                    } else {
                        bindings.push((name.clone(), v));
                    }
                }
                Ok(true)
            }
            PatternIr::Result { kind, binding } => match (kind, value) {
                (ResultPatKindIr::Ok, Value::Result(ResultValue::Ok(v))) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::Err, Value::Result(ResultValue::Err(v))) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::Some, v) if !matches!(v, Value::None) => {
                    if let Some(b) = binding {
                        self.match_pattern_inner(b, v, bindings)
                    } else {
                        Ok(true)
                    }
                }
                (ResultPatKindIr::None, Value::None) => Ok(true),
                // also allow ok/err as atoms in records? keep strict
                _ => Ok(false),
            },
        }
    }

    pub(super) fn lit_to_value(&self, lit: &ValueLiteral) -> Value {
        match lit {
            ValueLiteral::None(_) => Value::None,
            ValueLiteral::Bool(b, _) => Value::Bool(*b),
            ValueLiteral::Int(n, _) => Value::Int(*n),
            ValueLiteral::Float(n, _) => Value::Float(*n),
            ValueLiteral::String(s, _) => Value::string(s.clone()),
        }
    }
}
