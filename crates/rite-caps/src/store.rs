use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use rite_runtime::{EvalError, Key, Value};
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
        },
        NativeFunctionDescriptor {
            name: "set",
            docs: "Set a value in an in-memory namespace.",
            arity: 3,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "delete",
            docs: "Delete a key from a namespace.",
            arity: 2,
            effectful: true,
            permission: "",
        },
    ];

    pub fn call(&mut self, method: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match method {
            "get" => {
                let ns = key_str(args.first())?;
                let key = key_str(args.get(1))?;
                let val = self
                    .namespaces
                    .get(&ns)
                    .and_then(|m| m.get(&key))
                    .cloned()
                    .unwrap_or(Value::None);
                Ok(Value::ok(val))
            }
            "set" => {
                let ns = key_str(args.first())?;
                let key = key_str(args.get(1))?;
                let val = args.get(2).cloned().unwrap_or(Value::None);
                self.namespaces
                    .entry(ns)
                    .or_default()
                    .insert(key, val);
                Ok(Value::ok(Value::None))
            }
            "delete" => {
                let ns = key_str(args.first())?;
                let key = key_str(args.get(1))?;
                if let Some(m) = self.namespaces.get_mut(&ns) {
                    m.shift_remove(&key);
                }
                Ok(Value::ok(Value::None))
            }
            other => Err(EvalError::Capability(format!("unknown @store.{}", other))),
        }
    }
}

fn key_str(v: Option<&Value>) -> Result<String, EvalError> {
    match v {
        Some(Value::String(s)) => Ok(s.to_string()),
        Some(Value::Atom(id)) => Ok(format!("atom:{}", id.0)),
        Some(other) => Ok(format!("{}", other)),
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
fn _k(k: Key) {}
