use crate::atom::AtomId;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

/// Runtime value model (spec §10).
#[derive(Debug, Clone)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<str>),
    Atom(AtomId),
    List(im::Vector<Value>),
    Record(IndexMap<Key, Value>),
    Function(Closure),
    NativeFunction(NativeFnId),
    /// Named pure/host-dispatch builtin resolved at call time (shadowable by locals).
    NativeName(String),
    Result(ResultValue),
    Handle(HostHandle),
    Bytes(Arc<[u8]>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Key {
    String(String),
    Atom(String),
    Int(i64),
}

impl Key {
    pub fn as_str(&self) -> String {
        match self {
            Key::String(s) => s.clone(),
            Key::Atom(a) => a.clone(),
            Key::Int(i) => i.to_string(),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub id: u64,
    pub name: Option<String>,
    pub params: Vec<String>,
    pub env: Arc<parking_lot::RwLock<crate::env::Environment>>,
    pub body: rite_sem::BlockIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeFnId(pub u32);

#[derive(Debug, Clone)]
pub enum ResultValue {
    Ok(Box<Value>),
    Err(Box<Value>),
}

#[derive(Debug, Clone)]
pub struct HostHandle {
    pub kind: String,
    pub id: u64,
}

impl Value {
    pub fn string(s: impl Into<String>) -> Self {
        Value::String(Arc::from(s.into()))
    }

    pub fn list(items: impl IntoIterator<Item = Value>) -> Self {
        Value::List(items.into_iter().collect())
    }

    pub fn record(entries: impl IntoIterator<Item = (Key, Value)>) -> Self {
        Value::Record(entries.into_iter().collect())
    }

    pub fn ok(v: Value) -> Self {
        Value::Result(ResultValue::Ok(Box::new(v)))
    }

    pub fn err(v: Value) -> Self {
        Value::Result(ResultValue::Err(Box::new(v)))
    }

    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::None | Value::Bool(false))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "none",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Atom(_) => "atom",
            Value::List(_) => "list",
            Value::Record(_) => "record",
            Value::Function(_) => "function",
            Value::NativeFunction(_) => "native",
            Value::NativeName(_) => "native",
            Value::Result(_) => "result",
            Value::Handle(_) => "handle",
            Value::Bytes(_) => "bytes",
        }
    }

    pub fn structural_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::None, Value::None) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Atom(a), Value::Atom(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.structural_eq(y))
            }
            (Value::Record(a), Value::Record(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| b.get(k).map(|w| v.structural_eq(w)).unwrap_or(false))
            }
            (Value::Result(ResultValue::Ok(a)), Value::Result(ResultValue::Ok(b))) => {
                a.structural_eq(b)
            }
            (Value::Result(ResultValue::Err(a)), Value::Result(ResultValue::Err(b))) => {
                a.structural_eq(b)
            }
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Function(a), Value::Function(b)) => a.id == b.id,
            (Value::NativeFunction(a), Value::NativeFunction(b)) => a == b,
            (Value::NativeName(a), Value::NativeName(b)) => a == b,
            (Value::Handle(a), Value::Handle(b)) => a.kind == b.kind && a.id == b.id,
            _ => false,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_field(&self, name: &str) -> Value {
        match self {
            Value::Record(r) => r
                .get(&Key::String(name.to_string()))
                .or_else(|| r.get(&Key::Atom(name.to_string())))
                .cloned()
                .unwrap_or(Value::None),
            _ => Value::None,
        }
    }

    pub fn to_display(&self, atoms: &crate::atom::AtomInterner) -> String {
        match self {
            Value::None => "none".into(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.to_string(),
            Value::Atom(id) => format!("#{}", atoms.name(*id)),
            Value::List(xs) => {
                let inner: Vec<_> = xs.iter().map(|v| v.to_display(atoms)).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Record(r) => {
                let inner: Vec<_> = r
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_display(atoms)))
                    .collect();
                format!("⟨{}⟩", inner.join(", "))
            }
            Value::Function(c) => format!(
                "<fn {}/{}>",
                c.name.as_deref().unwrap_or("anonymous"),
                c.params.len()
            ),
            Value::NativeFunction(n) => format!("<native {}>", n.0),
            Value::NativeName(n) => format!("<native {}>", n),
            Value::Result(ResultValue::Ok(v)) => format!("ok({})", v.to_display(atoms)),
            Value::Result(ResultValue::Err(v)) => format!("err({})", v.to_display(atoms)),
            Value::Handle(h) => format!("<handle {}:{}>", h.kind, h.id),
            Value::Bytes(b) => format!("<bytes len={}>", b.len()),
        }
    }

    /// Serialize serializable values to JSON.
    pub fn to_json(&self, atoms: &crate::atom::AtomInterner) -> serde_json::Value {
        match self {
            Value::None => serde_json::Value::Null,
            Value::Bool(b) => serde_json::json!(b),
            Value::Int(n) => serde_json::json!(n),
            Value::Float(f) => serde_json::json!(f),
            Value::String(s) => serde_json::json!(s.as_ref()),
            Value::Atom(id) => serde_json::json!(atoms.name(*id)),
            Value::List(xs) => {
                serde_json::Value::Array(xs.iter().map(|v| v.to_json(atoms)).collect())
            }
            Value::Record(r) => {
                let mut map = serde_json::Map::new();
                for (k, v) in r {
                    map.insert(k.as_str(), v.to_json(atoms));
                }
                serde_json::Value::Object(map)
            }
            Value::Result(ResultValue::Ok(v)) => {
                serde_json::json!({"ok": v.to_json(atoms)})
            }
            Value::Result(ResultValue::Err(v)) => {
                serde_json::json!({"err": v.to_json(atoms)})
            }
            Value::Bytes(b) => serde_json::json!({"bytes_len": b.len()}),
            other => serde_json::json!(format!("<{}>", other.type_name())),
        }
    }

    pub fn from_json(v: &serde_json::Value) -> Value {
        match v {
            serde_json::Value::Null => Value::None,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Int(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::None
                }
            }
            serde_json::Value::String(s) => Value::string(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::list(arr.iter().map(Value::from_json).collect::<Vec<_>>())
            }
            serde_json::Value::Object(obj) => {
                if obj.len() == 1 {
                    if let Some(inner) = obj.get("ok") {
                        return Value::ok(Value::from_json(inner));
                    }
                    if let Some(inner) = obj.get("err") {
                        return Value::err(Value::from_json(inner));
                    }
                }
                let mut rec = IndexMap::new();
                for (k, v) in obj {
                    rec.insert(Key::String(k.clone()), Value::from_json(v));
                }
                Value::Record(rec)
            }
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.structural_eq(other)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::string(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::string(s)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::None => write!(f, "none"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::Atom(a) => write!(f, "#{}", a.0),
            Value::List(xs) => {
                write!(f, "[")?;
                for (i, v) in xs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Value::Record(r) => {
                write!(f, "⟨")?;
                for (i, (k, v)) in r.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                }
                write!(f, "⟩")
            }
            other => write!(f, "<{}>", other.type_name()),
        }
    }
}
