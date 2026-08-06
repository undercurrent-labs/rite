//! Protobuf schemas and messages: compile a `.proto`, then decode and encode by
//! message name.
//!
//! **Native by default rather than native-only.** `protox` and `prost-reflect`
//! are pure Rust and do compile for wasm32 — that was measured, not assumed —
//! but they add 1.1 MB to a 962 KB browser bundle, so the `proto` feature is on
//! for the CLI and off for the wasm build. Nothing in this file needs a host, so
//! switching it on there is a one-word change in `rite-wasm/Cargo.toml`.
//!
//! **Schemas are handles, not a hidden pool.** `@proto.compile` answers a
//! `Value::Handle` and everything else takes it as an argument, so `decode` and
//! `encode` are honest functions of their arguments and need no `!`. The
//! alternative — a global pool keyed by message name — would have made decoding
//! depend on load order, which is the `@store.get` exception in `HOST_EFFECTS`
//! and not worth invoking for a codec.
//!
//! **Compilation touches no filesystem.** protox resolves imports through a
//! `FileResolver`; the disk-reading one it ships (`IncludeFileResolver`) is
//! deliberately unused, so only `@proto.load_file` needs `fs:read`.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Value};

#[cfg(feature = "proto")]
use parking_lot::Mutex;
#[cfg(feature = "proto")]
use rite_runtime::Key;
#[cfg(feature = "proto")]
use rite_runtime::{AtomInterner, HostHandle, Limits};
#[cfg(feature = "proto")]
use std::collections::HashMap;
#[cfg(feature = "proto")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "proto")]
use prost::Message;
#[cfg(feature = "proto")]
use prost_reflect::{
    DescriptorPool, DynamicMessage, EnumDescriptor, FieldDescriptor, Kind, MapKey,
    MessageDescriptor, Value as PbValue,
};

#[derive(Default)]
pub struct ProtoCap {
    #[cfg(feature = "proto")]
    pools: Mutex<HashMap<u64, DescriptorPool>>,
}

#[cfg(feature = "proto")]
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// How deep a decoded message may nest.
///
/// Protobuf has no depth limit of its own, and a submessage that references its
/// own type can be nested as deeply as the sender likes. The ceiling is on the
/// Rite value being built, not on the wire bytes, so it bounds memory rather
/// than parse time.
#[cfg(feature = "proto")]
const MAX_DEPTH: usize = 64;

impl ProtoCap {
    pub fn new() -> Self {
        Self::default()
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "compile",
            docs: "Compile `.proto` source text into a schema handle. Answers `ok(handle)`, or `err` naming the line that failed to parse.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "compile_all",
            docs: "Compile several `.proto` sources that import each other, as a record of file name to source. Answers `ok(handle)`.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "load_file",
            docs: "Read and compile a `.proto` file, resolving its imports from the containing directory. Answers `ok(handle)`.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "load_set",
            docs: "Build a schema handle from an encoded `FileDescriptorSet` (`protoc --descriptor_set_out`). Answers `ok(handle)`.",
            arity: 1,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "decode",
            docs: "Decode bytes as the named message. Answers `ok(record)` holding only the fields the message actually set.",
            arity: 3,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "encode",
            docs: "Encode a record as the named message. Answers `ok(bytes)`, or `err` for a key that is not a field of that message.",
            arity: 3,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "messages",
            docs: "Every message type the schema defines, as a list of fully-qualified names.",
            arity: 1,
            effectful: false,
            permission: "",
        },
    ];

    #[cfg(not(feature = "proto"))]
    pub fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        atoms: &rite_runtime::AtomInterner,
        limits: rite_runtime::Limits,
    ) -> Result<Value, EvalError> {
        let _ = (method, args, perms, atoms, limits);
        Err(EvalError::Capability(
            "@proto requires the `proto` feature (off in this build — the browser \
             bundle omits it to stay under a megabyte)"
                .into(),
        ))
    }

    #[cfg(feature = "proto")]
    pub fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        atoms: &AtomInterner,
        limits: Limits,
    ) -> Result<Value, EvalError> {
        match method {
            "compile" => {
                let src = str_arg("proto.compile", &args, 0)?;
                Ok(self.compile_files(&[("schema.proto".to_string(), src)]))
            }
            "compile_all" => {
                let rec = match args.first() {
                    Some(Value::Record(r)) => r,
                    _ => {
                        return Err(EvalError::Message(
                            "proto.compile_all expects a record of file name to source".into(),
                        ))
                    }
                };
                let mut files = Vec::with_capacity(rec.len());
                for (k, v) in rec.iter() {
                    let Some(s) = v.as_str() else {
                        return Err(EvalError::Message(format!(
                            "proto.compile_all: source for `{}` is not a string",
                            k.as_str()
                        )));
                    };
                    files.push((k.as_str(), s.to_string()));
                }
                Ok(self.compile_files(&files))
            }
            "load_file" => {
                let path = std::path::PathBuf::from(str_arg("proto.load_file", &args, 0)?);
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::read_to_string(&path) {
                    Ok(src) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "schema.proto".to_string());
                        Ok(self.compile_files(&[(name, src)]))
                    }
                    Err(e) => Ok(err("proto.load_file", e.to_string())),
                }
            }
            "load_set" => {
                let bytes = match args.first() {
                    Some(Value::Bytes(b)) => b.to_vec(),
                    Some(Value::String(s)) => s.as_bytes().to_vec(),
                    _ => {
                        return Err(EvalError::Message(
                            "proto.load_set expects an encoded FileDescriptorSet as bytes".into(),
                        ))
                    }
                };
                match DescriptorPool::decode(bytes.as_slice()) {
                    Ok(pool) => Ok(Value::ok(self.store(pool))),
                    Err(e) => Ok(err("proto.load_set", e.to_string())),
                }
            }
            "messages" => {
                let pool = match self.pool(args.first()) {
                    Ok(p) => p,
                    Err(e) => return Ok(e),
                };
                // Map fields compile to a synthetic `Foo.BarEntry` message. Listing
                // them would name types nobody wrote.
                let names: Vec<Value> = pool
                    .all_messages()
                    .filter(|m| !m.is_map_entry())
                    .map(|m| Value::string(m.full_name()))
                    .collect();
                Ok(Value::ok(Value::list(names)))
            }
            "decode" => {
                let pool = match self.pool(args.first()) {
                    Ok(p) => p,
                    Err(e) => return Ok(e),
                };
                let name = str_arg("proto.decode", &args, 1)?;
                let Some(desc) = pool.get_message_by_name(&name) else {
                    return Ok(err("proto.decode", format!("no message named `{name}`")));
                };
                let bytes = match args.get(2) {
                    Some(Value::Bytes(b)) => b.to_vec(),
                    Some(Value::String(s)) => s.as_bytes().to_vec(),
                    _ => {
                        return Err(EvalError::Message(
                            "proto.decode expects bytes as its third argument".into(),
                        ))
                    }
                };
                let dm = match DynamicMessage::decode(desc, bytes.as_slice()) {
                    Ok(dm) => dm,
                    Err(e) => return Ok(err("proto.decode", e.to_string())),
                };
                match message_to_value(&dm, atoms, limits, 0) {
                    Ok(v) => Ok(Value::ok(v)),
                    Err(e) => Ok(err("proto.decode", e)),
                }
            }
            "encode" => {
                let pool = match self.pool(args.first()) {
                    Ok(p) => p,
                    Err(e) => return Ok(e),
                };
                let name = str_arg("proto.encode", &args, 1)?;
                let Some(desc) = pool.get_message_by_name(&name) else {
                    return Ok(err("proto.encode", format!("no message named `{name}`")));
                };
                let value = args.get(2).cloned().unwrap_or(Value::None);
                match value_to_message(&desc, &value, atoms, 0) {
                    Ok(dm) => Ok(Value::ok(Value::Bytes(dm.encode_to_vec().into()))),
                    Err(e) => Ok(err("proto.encode", e)),
                }
            }
            other => Err(EvalError::Capability(format!("unknown @proto.{}", other))),
        }
    }

    #[cfg(feature = "proto")]
    fn store(&self, pool: DescriptorPool) -> Value {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.pools.lock().insert(id, pool);
        Value::Handle(HostHandle {
            kind: "proto".into(),
            id,
        })
    }

    /// The pool a handle names, or the `err` value to answer with.
    #[cfg(feature = "proto")]
    fn pool(&self, v: Option<&Value>) -> Result<DescriptorPool, Value> {
        match v {
            Some(Value::Handle(h)) if h.kind == "proto" => self
                .pools
                .lock()
                .get(&h.id)
                .cloned()
                .ok_or_else(|| err("proto", format!("schema handle {} is not open", h.id))),
            _ => Err(err(
                "proto",
                "expects a schema handle from @proto.compile as its first argument",
            )),
        }
    }

    #[cfg(feature = "proto")]
    fn compile_files(&self, files: &[(String, String)]) -> Value {
        match compile_pool(files) {
            Ok(pool) => Value::ok(self.store(pool)),
            Err(e) => err("proto.compile", e),
        }
    }
}

/// Compiles `.proto` sources held in memory.
#[cfg(feature = "proto")]
fn compile_pool(files: &[(String, String)]) -> Result<DescriptorPool, String> {
    use protox::file::{File, FileResolver};

    struct InMemory(Vec<(String, String)>);

    impl FileResolver for InMemory {
        fn open_file(&self, name: &str) -> Result<File, protox::Error> {
            match self.0.iter().find(|(n, _)| n == name) {
                Some((n, src)) => File::from_source(n, src),
                None => Err(protox::Error::file_not_found(name)),
            }
        }
    }

    let owned: Vec<(String, String)> = files.to_vec();
    let mut compiler = protox::Compiler::with_file_resolver(InMemory(owned));
    for (name, _) in files {
        compiler.open_file(name).map_err(|e| e.to_string())?;
    }
    Ok(compiler.descriptor_pool())
}

#[cfg(feature = "proto")]
fn message_to_value(
    dm: &DynamicMessage,
    atoms: &AtomInterner,
    limits: Limits,
    depth: usize,
) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err(format!("message nests deeper than {MAX_DEPTH} levels"));
    }
    // `fields()` yields only the fields this message set. An unset proto3 scalar
    // is absent from the record rather than present as zero, which is the only
    // mapping that can tell "unset" from "set to the default".
    let mut out = Vec::new();
    for (field, value) in dm.fields() {
        let v = pb_to_value(&field, value, atoms, limits, depth)?;
        out.push((Key::String(field.name().into()), v));
    }
    Ok(Value::record(out))
}

#[cfg(feature = "proto")]
fn pb_to_value(
    field: &FieldDescriptor,
    value: &PbValue,
    atoms: &AtomInterner,
    limits: Limits,
    depth: usize,
) -> Result<Value, String> {
    match value {
        PbValue::List(items) => {
            if items.len() > limits.max_collection_size {
                return Err(format!(
                    "repeated field `{}` has {} items, over the {} limit",
                    field.name(),
                    items.len(),
                    limits.max_collection_size
                ));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(scalar_to_value(field, item, atoms, limits, depth)?);
            }
            Ok(Value::list(out))
        }
        PbValue::Map(entries) => {
            if entries.len() > limits.max_collection_size {
                return Err(format!(
                    "map field `{}` has {} entries, over the {} limit",
                    field.name(),
                    entries.len(),
                    limits.max_collection_size
                ));
            }
            // The map's value type lives on the synthetic entry message, not on
            // the map field itself, so an enum-valued map needs that descriptor
            // to name its variants.
            let value_field = match field.kind() {
                Kind::Message(entry) => entry.get_field_by_name("value"),
                _ => None,
            };
            let mut out = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let key = match k {
                    MapKey::String(s) => Key::String(s.clone()),
                    MapKey::I32(n) => Key::Int(*n as i64),
                    MapKey::I64(n) => Key::Int(*n),
                    MapKey::U32(n) => Key::Int(*n as i64),
                    MapKey::U64(n) => Key::Int(i64::try_from(*n).map_err(|_| {
                        format!("map key {n} in `{}` exceeds int range", field.name())
                    })?),
                    MapKey::Bool(b) => Key::String(b.to_string()),
                };
                let val = match &value_field {
                    Some(vf) => scalar_to_value(vf, v, atoms, limits, depth)?,
                    None => scalar_to_value(field, v, atoms, limits, depth)?,
                };
                out.push((key, val));
            }
            Ok(Value::record(out))
        }
        other => scalar_to_value(field, other, atoms, limits, depth),
    }
}

#[cfg(feature = "proto")]
fn scalar_to_value(
    field: &FieldDescriptor,
    value: &PbValue,
    atoms: &AtomInterner,
    limits: Limits,
    depth: usize,
) -> Result<Value, String> {
    Ok(match value {
        PbValue::Bool(b) => Value::Bool(*b),
        PbValue::I32(n) => Value::Int(*n as i64),
        PbValue::I64(n) => Value::Int(*n),
        PbValue::U32(n) => Value::Int(*n as i64),
        // Rite has no unsigned 64-bit type. Answering a string past `i64::MAX`
        // would make the same field decode to a different Rite type depending on
        // magnitude, so this fails loudly instead.
        PbValue::U64(n) => Value::Int(i64::try_from(*n).map_err(|_| {
            format!(
                "field `{}` holds {n}, which is past the largest Rite int ({})",
                field.name(),
                i64::MAX
            )
        })?),
        PbValue::F32(f) => Value::Float(*f as f64),
        PbValue::F64(f) => Value::Float(*f),
        PbValue::String(s) => {
            if s.len() > limits.max_string_size {
                return Err(format!(
                    "field `{}` holds {} bytes of text, over the {} limit",
                    field.name(),
                    s.len(),
                    limits.max_string_size
                ));
            }
            Value::string(s)
        }
        PbValue::Bytes(b) => {
            if b.len() > limits.max_string_size {
                return Err(format!(
                    "field `{}` holds {} bytes, over the {} limit",
                    field.name(),
                    b.len(),
                    limits.max_string_size
                ));
            }
            Value::Bytes(b.to_vec().into())
        }
        // Enums answer atoms: `#PRO` matches and compares like one written in the
        // source, because the interner keys on the name.
        PbValue::EnumNumber(n) => match enum_of(field) {
            Some(e) => match e.get_value(*n) {
                Some(v) => Value::Atom(atoms.intern(v.name())),
                // A number with no name is a variant added by a newer schema.
                None => Value::Int(*n as i64),
            },
            None => Value::Int(*n as i64),
        },
        PbValue::Message(m) => message_to_value(m, atoms, limits, depth + 1)?,
        PbValue::List(_) | PbValue::Map(_) => {
            return Err(format!(
                "field `{}` nests a list inside a list",
                field.name()
            ))
        }
    })
}

#[cfg(feature = "proto")]
fn enum_of(field: &FieldDescriptor) -> Option<EnumDescriptor> {
    match field.kind() {
        Kind::Enum(e) => Some(e),
        _ => None,
    }
}

#[cfg(feature = "proto")]
fn value_to_message(
    desc: &MessageDescriptor,
    value: &Value,
    atoms: &AtomInterner,
    depth: usize,
) -> Result<DynamicMessage, String> {
    if depth > MAX_DEPTH {
        return Err(format!("record nests deeper than {MAX_DEPTH} levels"));
    }
    let Value::Record(rec) = value else {
        return Err(format!(
            "expects a record for `{}`, got {}",
            desc.full_name(),
            value.type_name()
        ));
    };
    let mut dm = DynamicMessage::new(desc.clone());
    for (k, v) in rec.iter() {
        let name = k.as_str();
        // A key that is not a field is an error, not a silent drop: a typo'd
        // field name would otherwise encode to a valid message missing data.
        let Some(field) = desc.get_field_by_name(&name) else {
            return Err(format!(
                "`{}` has no field named `{name}`",
                desc.full_name()
            ));
        };
        if matches!(v, Value::None) {
            continue;
        }
        dm.set_field(&field, value_to_pb(&field, v, atoms, depth)?);
    }
    Ok(dm)
}

#[cfg(feature = "proto")]
fn value_to_pb(
    field: &FieldDescriptor,
    value: &Value,
    atoms: &AtomInterner,
    depth: usize,
) -> Result<PbValue, String> {
    if field.is_map() {
        let Value::Record(rec) = value else {
            return Err(format!(
                "field `{}` is a map and expects a record, got {}",
                field.name(),
                value.type_name()
            ));
        };
        let entry = match field.kind() {
            Kind::Message(entry) => entry,
            _ => return Err(format!("map field `{}` has no entry type", field.name())),
        };
        let key_field = entry
            .get_field_by_name("key")
            .ok_or_else(|| format!("map field `{}` has no key", field.name()))?;
        let value_field = entry
            .get_field_by_name("value")
            .ok_or_else(|| format!("map field `{}` has no value", field.name()))?;
        let mut out = std::collections::HashMap::new();
        for (k, v) in rec.iter() {
            let key = match (k, key_field.kind()) {
                (Key::Int(n), Kind::Int32 | Kind::Sint32 | Kind::Sfixed32) => {
                    MapKey::I32(*n as i32)
                }
                (Key::Int(n), Kind::Uint32 | Kind::Fixed32) => MapKey::U32(*n as u32),
                (Key::Int(n), Kind::Uint64 | Kind::Fixed64) => MapKey::U64(*n as u64),
                (Key::Int(n), _) => MapKey::I64(*n),
                (other, _) => MapKey::String(other.as_str()),
            };
            out.insert(key, value_to_pb(&value_field, v, atoms, depth)?);
        }
        return Ok(PbValue::Map(out));
    }
    if field.is_list() {
        let Value::List(items) = value else {
            return Err(format!(
                "field `{}` is repeated and expects a list, got {}",
                field.name(),
                value.type_name()
            ));
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items.iter() {
            out.push(single_to_pb(field, item, atoms, depth)?);
        }
        return Ok(PbValue::List(out));
    }
    single_to_pb(field, value, atoms, depth)
}

#[cfg(feature = "proto")]
fn single_to_pb(
    field: &FieldDescriptor,
    value: &Value,
    atoms: &AtomInterner,
    depth: usize,
) -> Result<PbValue, String> {
    let mismatch = || {
        format!(
            "field `{}` expects {:?}, got {}",
            field.name(),
            field.kind(),
            value.type_name()
        )
    };
    Ok(match field.kind() {
        Kind::Bool => PbValue::Bool(value.is_truthy()),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => {
            PbValue::I32(value.as_int().ok_or_else(mismatch)? as i32)
        }
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => {
            PbValue::I64(value.as_int().ok_or_else(mismatch)?)
        }
        Kind::Uint32 | Kind::Fixed32 => {
            let n = value.as_int().ok_or_else(mismatch)?;
            PbValue::U32(
                u32::try_from(n).map_err(|_| {
                    format!("field `{}` is unsigned and cannot hold {n}", field.name())
                })?,
            )
        }
        Kind::Uint64 | Kind::Fixed64 => {
            let n = value.as_int().ok_or_else(mismatch)?;
            PbValue::U64(
                u64::try_from(n).map_err(|_| {
                    format!("field `{}` is unsigned and cannot hold {n}", field.name())
                })?,
            )
        }
        Kind::Float => PbValue::F32(as_f64(value).ok_or_else(mismatch)? as f32),
        Kind::Double => PbValue::F64(as_f64(value).ok_or_else(mismatch)?),
        Kind::String => match value {
            Value::String(s) => PbValue::String(s.to_string()),
            Value::Atom(id) => PbValue::String(atoms.name(*id)),
            _ => return Err(mismatch()),
        },
        Kind::Bytes => match value {
            Value::Bytes(b) => PbValue::Bytes(b.to_vec().into()),
            Value::String(s) => PbValue::Bytes(s.as_bytes().to_vec().into()),
            _ => return Err(mismatch()),
        },
        Kind::Enum(e) => {
            // Atoms and strings both name a variant; an int is the raw number,
            // which is how a variant this schema does not know still round-trips.
            let name = match value {
                Value::Atom(id) => Some(atoms.name(*id)),
                Value::String(s) => Some(s.to_string()),
                Value::Int(n) => return Ok(PbValue::EnumNumber(*n as i32)),
                _ => None,
            }
            .ok_or_else(mismatch)?;
            match e.get_value_by_name(&name) {
                Some(v) => PbValue::EnumNumber(v.number()),
                None => {
                    return Err(format!(
                        "`{}` is not a variant of enum `{}`",
                        name,
                        e.full_name()
                    ))
                }
            }
        }
        Kind::Message(m) => PbValue::Message(value_to_message(&m, value, atoms, depth + 1)?),
    })
}

#[cfg(feature = "proto")]
fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

#[cfg(feature = "proto")]
fn str_arg(who: &str, args: &[Value], idx: usize) -> Result<String, EvalError> {
    args.get(idx)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            EvalError::Message(format!("{who} expects a string at argument {}", idx + 1))
        })
}

#[cfg(feature = "proto")]
fn err(kind: &str, message: impl Into<String>) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string(kind)),
        (Key::String("message".into()), Value::string(message.into())),
    ]))
}
