use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use rite_runtime::{AtomInterner, EvalError, Key, Value};
use std::collections::HashMap;

pub struct StoreCap {
    namespaces: HashMap<String, IndexMap<String, Value>>,
}

impl StoreCap {
    pub fn new() -> Self {
        Self {
            namespaces: HashMap::new(),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "get",
            docs: "Get a value from an in-memory namespace.",
            arity: 2,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "set",
            docs: "Set a value in an in-memory namespace.",
            arity: 3,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "delete",
            docs: "Delete a key from a namespace.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: true,
        },
    ];

    pub fn call(
        &mut self,
        method: &str,
        args: Vec<Value>,
        atoms: &AtomInterner,
    ) -> Result<Value, EvalError> {
        match method {
            "get" => {
                let ns = key_str(args.first(), atoms)?;
                let key = key_str(args.get(1), atoms)?;
                let val = self
                    .namespaces
                    .get(&ns)
                    .and_then(|m| m.get(&key))
                    .cloned()
                    .unwrap_or(Value::None);
                Ok(Value::ok(val))
            }
            "set" => {
                let ns = key_str(args.first(), atoms)?;
                let key = key_str(args.get(1), atoms)?;
                let val = args.get(2).cloned().unwrap_or(Value::None);
                self.namespaces.entry(ns).or_default().insert(key, val);
                Ok(Value::ok(Value::None))
            }
            "delete" => {
                let ns = key_str(args.first(), atoms)?;
                let key = key_str(args.get(1), atoms)?;
                if let Some(m) = self.namespaces.get_mut(&ns) {
                    m.shift_remove(&key);
                }
                Ok(Value::ok(Value::None))
            }
            other => Err(EvalError::Capability(format!("unknown @store.{}", other))),
        }
    }
}

/// A namespace or key, as the string this store indexes by.
///
/// Atoms keep their `#` so they cannot collide with the string of the same
/// name: `@store.set("PRO", 1)` and `@store.set(#PRO, 2)` are two keys. They
/// used to render as the interner index (`atom:0`), which collided with
/// whatever else happened to intern first and changed between runs.
fn key_str(v: Option<&Value>, atoms: &AtomInterner) -> Result<String, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.to_string()),
        Some(Value::Atom(id)) => Ok(format!("#{}", atoms.name(*id))),
        Some(other) => Ok(other.to_display(atoms)),
        None => Err(EvalError::Message("store expects key".into())),
    }
}

impl Default for StoreCap {
    fn default() -> Self {
        Self::new()
    }
}

// silence
#[allow(dead_code)]
fn _k(_k: Key) {}
