//! `@db` capability backed by DuckDB (native only).
//!
//! Connections and prepared statements are opaque `Value::Handle`s.
//! Disabled on wasm targets — calls return a clear capability error.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use indexmap::IndexMap;
use parking_lot::Mutex;
use rite_runtime::{EvalError, HostHandle, Key, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use duckdb::types::Value as DuckValue;
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use duckdb::{params_from_iter, Connection};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Inner state held under a mutex so `Connection` (!Sync) can live in `HostCapabilities`.
struct DbInner {
    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    conns: HashMap<u64, Connection>,
    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    /// stmt_id → (conn_id, sql)
    stmts: HashMap<u64, (u64, String)>,
}

pub struct DbCap {
    inner: Mutex<DbInner>,
}

impl DbCap {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(DbInner {
                #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
                conns: HashMap::new(),
                #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
                stmts: HashMap::new(),
            }),
        }
    }

    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "open",
            docs: "Open a DuckDB connection. Path omitted or \":memory:\" → in-memory. Needs --allow db or --allow db=path.",
            arity: 0,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "close",
            docs: "Close a database connection handle.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "exec",
            docs: "Execute SQL that does not return rows (DDL/DML). Optional params list.",
            arity: 2,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "query",
            docs: "Run a SQL query and return ok(list of records). Optional params list.",
            arity: 2,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "prepare",
            docs: "Prepare a SQL statement; returns a statement handle.",
            arity: 2,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "query_prepared",
            docs: "Execute a prepared statement as a query. Optional params list.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "exec_prepared",
            docs: "Execute a prepared statement without returning rows. Optional params list.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "close_stmt",
            docs: "Drop a prepared statement handle.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "begin",
            docs: "BEGIN a transaction on the connection.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "commit",
            docs: "COMMIT the current transaction.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
        NativeFunctionDescriptor {
            name: "rollback",
            docs: "ROLLBACK the current transaction.",
            arity: 1,
            effectful: true,
            permission: "db",
        },
    ];

    pub fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            let _ = (method, args, perms);
            return Err(EvalError::Capability(
                "@db requires the native DuckDB host (not available in WASM / this build)".into(),
            ));
        }

        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.call_native(method, args, perms)
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn call_native(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        match method {
            "open" => self.open(args, perms),
            "close" => self.close_conn(args),
            "exec" => self.exec(args),
            "query" => self.query(args),
            "prepare" => self.prepare(args),
            "query_prepared" => self.query_prepared(args),
            "exec_prepared" => self.exec_prepared(args),
            "close_stmt" => self.close_stmt(args),
            "begin" => self.tx(args, "BEGIN"),
            "commit" => self.tx(args, "COMMIT"),
            "rollback" => self.tx(args, "ROLLBACK"),
            other => Err(EvalError::Capability(format!("unknown @db.{}", other))),
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn open(&self, args: Vec<Value>, perms: &PermissionSet) -> Result<Value, EvalError> {
        perms.check_db_open().map_err(EvalError::Permission)?;
        let path_arg = args.first();
        let conn = match path_arg {
            None | Some(Value::None) => Connection::open_in_memory()
                .map_err(|e| EvalError::Capability(format!("db.open: {}", e)))?,
            Some(Value::String(s)) if s.is_empty() || s.as_ref() == ":memory:" => {
                Connection::open_in_memory()
                    .map_err(|e| EvalError::Capability(format!("db.open: {}", e)))?
            }
            Some(Value::String(s)) => {
                let path = PathBuf::from(s.as_ref());
                let path = perms.check_db_path(&path).map_err(EvalError::Permission)?;
                Connection::open(&path)
                    .map_err(|e| EvalError::Capability(format!("db.open: {}", e)))?
            }
            Some(Value::Record(r)) => {
                let path = r
                    .get(&Key::String("path".into()))
                    .or_else(|| r.get(&Key::Atom("path".into())))
                    .and_then(|v| v.as_str())
                    .unwrap_or(":memory:");
                if path.is_empty() || path == ":memory:" {
                    Connection::open_in_memory()
                        .map_err(|e| EvalError::Capability(format!("db.open: {}", e)))?
                } else {
                    let p = PathBuf::from(path);
                    let p = perms.check_db_path(&p).map_err(EvalError::Permission)?;
                    Connection::open(&p)
                        .map_err(|e| EvalError::Capability(format!("db.open: {}", e)))?
                }
            }
            other => {
                return Err(EvalError::Message(format!(
                    "db.open expects path string or none, got {}",
                    other.map(|v| v.type_name()).unwrap_or("none")
                )));
            }
        };
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.inner.lock().conns.insert(id, conn);
        Ok(Value::ok(Value::Handle(HostHandle {
            kind: "db.conn".into(),
            id,
        })))
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn close_conn(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let id = handle_id(args.first(), "db.conn")?;
        let mut inner = self.inner.lock();
        inner.conns.remove(&id);
        inner.stmts.retain(|_, (cid, _)| *cid != id);
        Ok(Value::ok(Value::None))
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn exec(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let id = handle_id(args.first(), "db.conn")?;
        let sql = args
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or_else(|| EvalError::Message("db.exec expects sql string".into()))?;
        let params = value_params(args.get(2));
        let inner = self.inner.lock();
        let conn = inner
            .conns
            .get(&id)
            .ok_or_else(|| EvalError::Message("db connection closed or invalid".into()))?;
        match conn.execute(sql, params_from_iter(params.iter())) {
            Ok(_) => Ok(Value::ok(Value::None)),
            Err(e) => Ok(Value::err(Value::string(e.to_string()))),
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn query(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let id = handle_id(args.first(), "db.conn")?;
        let sql = args
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or_else(|| EvalError::Message("db.query expects sql string".into()))?;
        let params = value_params(args.get(2));
        let inner = self.inner.lock();
        let conn = inner
            .conns
            .get(&id)
            .ok_or_else(|| EvalError::Message("db connection closed or invalid".into()))?;
        match query_rows(conn, sql, &params) {
            Ok(rows) => Ok(Value::ok(Value::list(rows))),
            Err(e) => Ok(Value::err(Value::string(e))),
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn prepare(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let cid = handle_id(args.first(), "db.conn")?;
        let sql = args
            .get(1)
            .and_then(|v| v.as_str())
            .ok_or_else(|| EvalError::Message("db.prepare expects sql string".into()))?
            .to_string();
        let mut inner = self.inner.lock();
        if !inner.conns.contains_key(&cid) {
            return Ok(Value::err(Value::string("db connection closed or invalid")));
        }
        {
            let conn = inner.conns.get(&cid).unwrap();
            if let Err(e) = conn.prepare(&sql) {
                return Ok(Value::err(Value::string(e.to_string())));
            }
        }
        let sid = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        inner.stmts.insert(sid, (cid, sql));
        Ok(Value::ok(Value::Handle(HostHandle {
            kind: "db.stmt".into(),
            id: sid,
        })))
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn query_prepared(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let sid = handle_id(args.first(), "db.stmt")?;
        let params = value_params(args.get(1));
        let inner = self.inner.lock();
        let (cid, sql) = inner
            .stmts
            .get(&sid)
            .cloned()
            .ok_or_else(|| EvalError::Message("db statement closed or invalid".into()))?;
        let conn = inner
            .conns
            .get(&cid)
            .ok_or_else(|| EvalError::Message("db connection closed or invalid".into()))?;
        match query_rows(conn, &sql, &params) {
            Ok(rows) => Ok(Value::ok(Value::list(rows))),
            Err(e) => Ok(Value::err(Value::string(e))),
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn exec_prepared(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let sid = handle_id(args.first(), "db.stmt")?;
        let params = value_params(args.get(1));
        let inner = self.inner.lock();
        let (cid, sql) = inner
            .stmts
            .get(&sid)
            .cloned()
            .ok_or_else(|| EvalError::Message("db statement closed or invalid".into()))?;
        let conn = inner
            .conns
            .get(&cid)
            .ok_or_else(|| EvalError::Message("db connection closed or invalid".into()))?;
        match conn.execute(&sql, params_from_iter(params.iter())) {
            Ok(_) => Ok(Value::ok(Value::None)),
            Err(e) => Ok(Value::err(Value::string(e.to_string()))),
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn close_stmt(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let sid = handle_id(args.first(), "db.stmt")?;
        self.inner.lock().stmts.remove(&sid);
        Ok(Value::ok(Value::None))
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn tx(&self, args: Vec<Value>, sql: &str) -> Result<Value, EvalError> {
        let id = handle_id(args.first(), "db.conn")?;
        let inner = self.inner.lock();
        let conn = inner
            .conns
            .get(&id)
            .ok_or_else(|| EvalError::Message("db connection closed or invalid".into()))?;
        match conn.execute_batch(sql) {
            Ok(()) => Ok(Value::ok(Value::None)),
            Err(e) => Ok(Value::err(Value::string(e.to_string()))),
        }
    }
}

impl Default for DbCap {
    fn default() -> Self {
        Self::new()
    }
}

fn handle_id(v: Option<&Value>, kind: &str) -> Result<u64, EvalError> {
    match v {
        Some(Value::Handle(h)) if h.kind == kind => Ok(h.id),
        Some(Value::Handle(h)) => Err(EvalError::Message(format!(
            "expected handle {}, got {}",
            kind, h.kind
        ))),
        _ => Err(EvalError::Message(format!("expected handle {}", kind))),
    }
}

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn value_params(v: Option<&Value>) -> Vec<DuckValue> {
    let Some(Value::List(xs)) = v else {
        return Vec::new();
    };
    xs.iter()
        .map(|val| match val {
            Value::None => DuckValue::Null,
            Value::Bool(b) => DuckValue::Boolean(*b),
            Value::Int(n) => DuckValue::BigInt(*n),
            Value::Float(f) => DuckValue::Double(*f),
            Value::String(s) => DuckValue::Text(s.to_string()),
            other => DuckValue::Text(format!("{}", other)),
        })
        .collect()
}

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn query_rows(conn: &Connection, sql: &str, params: &[DuckValue]) -> Result<Vec<Value>, String> {
    // DuckDB panics on column_count/column_name until the statement is stepped.
    // Prefer DESCRIBE for unbound SQL; otherwise infer width from the first row.
    let names = resolve_column_names(conn, sql, params.len());

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(params_from_iter(params.iter()))
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    let mut width: Option<usize> = (!names.is_empty()).then_some(names.len());
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let n = match width {
            Some(n) => n,
            None => {
                // Probe columns until get fails (DuckDB has no pre-execute column_count).
                let mut c = 0usize;
                while row_value(row, c).is_ok() {
                    c += 1;
                    if c > 256 {
                        break;
                    }
                }
                width = Some(c);
                c
            }
        };
        let mut rec = IndexMap::new();
        for i in 0..n {
            let name = names.get(i).cloned().unwrap_or_else(|| format!("col{}", i));
            let v = row_value(row, i).unwrap_or(Value::None);
            rec.insert(Key::String(name), v);
        }
        out.push(Value::Record(rec));
    }
    Ok(out)
}

/// Column names for a SELECT via `DESCRIBE (sql)`.
/// Bound `?` params are replaced with NULL for describe-only analysis.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn resolve_column_names(conn: &Connection, sql: &str, _param_count: usize) -> Vec<String> {
    let for_describe = sql.replace('?', "NULL");
    let describe_sql = format!("DESCRIBE ({for_describe})");
    let Ok(mut stmt) = conn.prepare(&describe_sql) else {
        return Vec::new();
    };
    let Ok(mut rows) = stmt.query([]) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    while let Ok(Some(row)) = rows.next() {
        if let Ok(name) = row.get::<_, String>(0) {
            names.push(name);
        } else if let Ok(Some(name)) = row.get::<_, Option<String>>(0) {
            names.push(name);
        }
    }
    names
}

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn row_value(row: &duckdb::Row<'_>, idx: usize) -> Result<Value, String> {
    if let Ok(v) = row.get::<_, Option<i64>>(idx) {
        return Ok(match v {
            Some(n) => Value::Int(n),
            None => Value::None,
        });
    }
    if let Ok(v) = row.get::<_, Option<f64>>(idx) {
        return Ok(match v {
            Some(f) => Value::Float(f),
            None => Value::None,
        });
    }
    if let Ok(v) = row.get::<_, Option<bool>>(idx) {
        return Ok(match v {
            Some(b) => Value::Bool(b),
            None => Value::None,
        });
    }
    if let Ok(v) = row.get::<_, Option<String>>(idx) {
        return Ok(match v {
            Some(s) => Value::string(s),
            None => Value::None,
        });
    }
    Err(format!("unsupported column type at index {idx}"))
}

#[allow(dead_code)]
fn _path_ty(_: &Path) {}
