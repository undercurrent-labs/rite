//! CSV encode/decode/read/write capability (mirrors `@json`).

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use rite_runtime::{AtomInterner, EvalError, Key, Value};
use std::io::Cursor;
use std::path::PathBuf;

pub struct CsvCap;

#[derive(Debug, Clone)]
struct CsvOptions {
    headers: bool,
    delimiter: u8,
    skip_empty: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            headers: true,
            delimiter: b',',
            skip_empty: true,
        }
    }
}

impl CsvOptions {
    fn from_value(v: Option<&Value>) -> Self {
        let mut opts = Self::default();
        let Some(Value::Record(r)) = v else {
            return opts;
        };
        if let Some(h) = r
            .get(&Key::String("headers".into()))
            .or_else(|| r.get(&Key::Atom("headers".into())))
        {
            opts.headers = h.is_truthy();
        }
        if let Some(d) = r
            .get(&Key::String("delimiter".into()))
            .or_else(|| r.get(&Key::Atom("delimiter".into())))
            .and_then(|v| v.as_str())
        {
            if let Some(c) = d.bytes().next() {
                opts.delimiter = c;
            }
        }
        if let Some(s) = r
            .get(&Key::String("skip_empty".into()))
            .or_else(|| r.get(&Key::Atom("skip_empty".into())))
        {
            opts.skip_empty = s.is_truthy();
        }
        opts
    }
}

impl CsvCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "decode",
            docs: "Parse a CSV string into a list of records (ok/err). Options: headers (default true), delimiter, skip_empty.",
            arity: 1,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "encode",
            docs: "Serialize a list of records (or list of lists) to a CSV string.",
            arity: 1,
            effectful: false,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "read",
            docs: "Read and parse a CSV file into a list of records.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "write",
            docs: "Write a list of records as CSV to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        atoms: &AtomInterner,
    ) -> Result<Value, EvalError> {
        match method {
            "decode" => {
                let text = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("csv.decode expects string".into()))?;
                let opts = CsvOptions::from_value(args.get(1));
                Ok(decode_csv(text, &opts))
            }
            "encode" => {
                // A non-list used to encode as an empty CSV — a file that looks
                // written and holds nothing.
                let rows = crate::args::required("csv.encode", &args, 0)?.clone();
                let opts = CsvOptions::from_value(args.get(1));
                // Both arms are results. Success used to be a bare string while
                // failure was err(...), so `@csv.encode(rows)?` unwrapped fine
                // exactly when the rows were malformed and failed when they
                // were not. `csv.write` already wraps both arms.
                match encode_csv(&rows, &opts, atoms) {
                    Ok(s) => Ok(Value::ok(Value::string(s))),
                    Err(e) => Ok(Value::err(Value::string(e))),
                }
            }
            "read" => {
                let path = args
                    .first()
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| EvalError::Message("csv.read expects path".into()))?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                let opts = CsvOptions::from_value(args.get(1));
                match std::fs::read_to_string(&path) {
                    Ok(text) => Ok(decode_csv(&text, &opts)),
                    Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                }
            }
            "write" => {
                let path = args
                    .first()
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .ok_or_else(|| EvalError::Message("csv.write expects path".into()))?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let rows = crate::args::required("csv.write", &args, 1)?.clone();
                let opts = CsvOptions::from_value(args.get(2));
                match encode_csv(&rows, &opts, atoms) {
                    Ok(text) => match std::fs::write(&path, text) {
                        Ok(()) => Ok(Value::ok(Value::None)),
                        Err(e) => Ok(Value::err(Value::string(e.to_string()))),
                    },
                    Err(e) => Ok(Value::err(Value::string(e))),
                }
            }
            other => Err(EvalError::Capability(format!("unknown @csv.{}", other))),
        }
    }
}

fn decode_csv(text: &str, opts: &CsvOptions) -> Value {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(opts.headers)
        .delimiter(opts.delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(Cursor::new(text.as_bytes()));

    let mut rows: Vec<Value> = Vec::new();

    if opts.headers {
        let headers: Vec<String> = match reader.headers() {
            Ok(h) => h.iter().map(|s| s.to_string()).collect(),
            Err(e) => {
                return Value::err(Value::record(vec![
                    (Key::String("kind".into()), Value::string("csv.decode")),
                    (Key::String("message".into()), Value::string(e.to_string())),
                ]));
            }
        };
        for result in reader.records() {
            match result {
                Ok(rec) => {
                    if opts.skip_empty && rec.iter().all(|f| f.trim().is_empty()) {
                        continue;
                    }
                    let mut map = IndexMap::new();
                    for (i, field) in rec.iter().enumerate() {
                        let key = headers
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| format!("col{}", i));
                        map.insert(Key::String(key), Value::string(field));
                    }
                    rows.push(Value::Record(map));
                }
                Err(e) => {
                    return Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("csv.decode")),
                        (Key::String("message".into()), Value::string(e.to_string())),
                    ]));
                }
            }
        }
    } else {
        for result in reader.records() {
            match result {
                Ok(rec) => {
                    if opts.skip_empty && rec.iter().all(|f| f.trim().is_empty()) {
                        continue;
                    }
                    let cells: Vec<Value> = rec.iter().map(Value::string).collect();
                    rows.push(Value::list(cells));
                }
                Err(e) => {
                    return Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("csv.decode")),
                        (Key::String("message".into()), Value::string(e.to_string())),
                    ]));
                }
            }
        }
    }

    Value::ok(Value::list(rows))
}

fn encode_csv(rows: &Value, opts: &CsvOptions, atoms: &AtomInterner) -> Result<String, String> {
    let Value::List(items) = rows else {
        return Err("csv.encode expects a list of records or lists".into());
    };

    let mut buf = Vec::new();
    {
        let mut writer = csv::WriterBuilder::new()
            .delimiter(opts.delimiter)
            .from_writer(&mut buf);

        if items.is_empty() {
            writer.flush().map_err(|e| e.to_string())?;
            return Ok(String::new());
        }

        // Detect shape from first row
        match items.front() {
            Some(Value::Record(_)) => {
                // Collect stable header order from first record, then union later keys
                let mut headers: Vec<String> = Vec::new();
                if let Some(Value::Record(first)) = items.front() {
                    for k in first.keys() {
                        headers.push(k.as_str());
                    }
                }
                for item in items.iter() {
                    if let Value::Record(r) = item {
                        for k in r.keys() {
                            let s = k.as_str();
                            if !headers.iter().any(|h| h == &s) {
                                headers.push(s);
                            }
                        }
                    }
                }
                if opts.headers {
                    writer.write_record(&headers).map_err(|e| e.to_string())?;
                }
                for item in items.iter() {
                    let Value::Record(r) = item else {
                        return Err("csv.encode mixed row types; expected records".into());
                    };
                    let mut fields = Vec::with_capacity(headers.len());
                    for h in &headers {
                        let cell = r
                            .get(&Key::String(h.clone()))
                            .or_else(|| r.get(&Key::Atom(h.clone())))
                            .map(|c| value_to_csv_field(c, atoms))
                            .unwrap_or_default();
                        fields.push(cell);
                    }
                    writer.write_record(&fields).map_err(|e| e.to_string())?;
                }
            }
            Some(Value::List(_)) => {
                for item in items.iter() {
                    let Value::List(cells) = item else {
                        return Err("csv.encode mixed row types; expected lists".into());
                    };
                    let fields: Vec<String> =
                        cells.iter().map(|c| value_to_csv_field(c, atoms)).collect();
                    writer.write_record(&fields).map_err(|e| e.to_string())?;
                }
            }
            Some(other) => {
                return Err(format!(
                    "csv.encode expects records or lists, got {}",
                    other.type_name()
                ));
            }
            None => {}
        }
        writer.flush().map_err(|e| e.to_string())?;
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn value_to_csv_field(v: &Value, atoms: &AtomInterner) -> String {
    match v {
        Value::None => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.to_string(),
        // The atom's name, bare, as `@json.encode` writes it. This wrote the
        // interner index (`atom:0`) into the cell, so a column of atoms landed
        // on disk as numbers that changed between runs.
        Value::Atom(id) => atoms.name(*id),
        // `Display` cannot reach the interner either, so a list or record
        // holding an atom had the same bug one level down.
        other => other.to_display(atoms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_headers() {
        let text = "name,age\nAda,36\nBob,42\n";
        let v = decode_csv(text, &CsvOptions::default());
        match v {
            Value::Result(rite_runtime::ResultValue::Ok(list)) => {
                if let Value::List(rows) = *list {
                    assert_eq!(rows.len(), 2);
                    assert_eq!(rows[0].get_field("name").as_str(), Some("Ada"));
                    assert_eq!(rows[1].get_field("age").as_str(), Some("42"));
                } else {
                    panic!("expected list");
                }
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn encode_roundtrip() {
        let rows = Value::list(vec![Value::record(vec![
            (Key::String("a".into()), Value::string("1")),
            (Key::String("b".into()), Value::string("x,y")),
        ])]);
        let s = encode_csv(&rows, &CsvOptions::default(), &AtomInterner::new()).unwrap();
        let decoded = decode_csv(&s, &CsvOptions::default());
        match decoded {
            Value::Result(rite_runtime::ResultValue::Ok(list)) => {
                if let Value::List(rows) = *list {
                    assert_eq!(rows.len(), 1);
                    assert_eq!(rows[0].get_field("b").as_str(), Some("x,y"));
                }
            }
            other => panic!("{:?}", other),
        }
    }
}
