use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Key, Value};
use std::path::PathBuf;

pub struct JsonCap;

impl JsonCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "decode",
            docs: "Parse a JSON string into a Rite value.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "encode",
            docs: "Serialize a value to compact JSON.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "encode_pretty",
            docs: "Serialize a value to pretty JSON.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "read",
            docs: "Read and parse a JSON file.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "write",
            docs: "Write a value as JSON to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        match method {
            "decode" => {
                let text = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("json.decode expects string".into()))?;
                match serde_json::from_str::<serde_json::Value>(text) {
                    Ok(v) => Ok(Value::ok(Value::from_json(&v))),
                    Err(e) => Ok(Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("json.decode")),
                        (Key::String("message".into()), Value::string(e.to_string())),
                    ]))),
                }
            }
            "encode" => {
                // Atoms will show as numbers without interner; use display path
                let v = args.first().cloned().unwrap_or(Value::None);
                let json = value_to_json_string(&v, false);
                Ok(Value::string(json))
            }
            "encode_pretty" => {
                let v = args.first().cloned().unwrap_or(Value::None);
                Ok(Value::string(value_to_json_string(&v, true)))
            }
            "read" => {
                let path = args
                    .first()
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| EvalError::Message("json.read expects path".into()))?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::read_to_string(&path) {
                    Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(v) => Ok(Value::ok(Value::from_json(&v))),
                        Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                    },
                    Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                }
            }
            "write" => {
                let path = args
                    .first()
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| EvalError::Message("json.write expects path".into()))?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let v = args.get(1).cloned().unwrap_or(Value::None);
                let text = value_to_json_string(&v, true);
                match std::fs::write(&path, text) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                }
            }
            other => Err(EvalError::Capability(format!("unknown @json.{}", other))),
        }
    }
}

fn value_to_json_string(v: &Value, pretty: bool) -> String {
    let j = value_to_serde(v);
    if pretty {
        serde_json::to_string_pretty(&j).unwrap_or_else(|_| "null".into())
    } else {
        serde_json::to_string(&j).unwrap_or_else(|_| "null".into())
    }
}

fn value_to_serde(v: &Value) -> serde_json::Value {
    match v {
        Value::None => serde_json::Value::Null,
        Value::Bool(b) => serde_json::json!(b),
        Value::Int(n) => serde_json::json!(n),
        Value::Float(f) => serde_json::json!(f),
        Value::String(s) => serde_json::json!(s.as_ref()),
        Value::Atom(id) => serde_json::json!(format!("atom:{}", id.0)),
        Value::List(xs) => serde_json::Value::Array(xs.iter().map(value_to_serde).collect()),
        Value::Record(r) => {
            let mut map = serde_json::Map::new();
            for (k, v) in r {
                map.insert(k.as_str(), value_to_serde(v));
            }
            serde_json::Value::Object(map)
        }
        Value::Result(rite_runtime::ResultValue::Ok(v)) => {
            serde_json::json!({"ok": value_to_serde(v)})
        }
        Value::Result(rite_runtime::ResultValue::Err(v)) => {
            serde_json::json!({"err": value_to_serde(v)})
        }
        other => serde_json::json!(format!("<{}>", other.type_name())),
    }
}
