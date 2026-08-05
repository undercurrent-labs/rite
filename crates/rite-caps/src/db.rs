//! `@db` capability backed by DuckDB (native only).
//!
//! Connections and prepared statements are opaque `Value::Handle`s.
//! Disabled on wasm targets — calls return a clear capability error.

// Most of this module only exists when DuckDB is linked in. Without the feature the
// capability still answers, with a clear error, so the imports and state it needs are
// gated rather than the whole file. `rite build` compiles this crate without the feature
// for any program that never touches `@db`, and that configuration has to be warning-clean
// or a user with `-Dwarnings` cannot build at all.
use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use parking_lot::Mutex;
use rite_runtime::{EvalError, Value};
use std::path::Path;

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use indexmap::IndexMap;
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use rite_runtime::{HostHandle, Key};
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use std::collections::HashMap;
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use std::path::PathBuf;
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use duckdb::types::Value as DuckValue;
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
use duckdb::{params_from_iter, Connection};

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
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
    #[cfg_attr(
        not(all(feature = "duckdb", not(target_arch = "wasm32"))),
        allow(dead_code)
    )]
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
            docs: "Open a DuckDB connection. Path omitted or \":memory:\" → in-memory. Needs --allow db or --allow db=path. DuckDB's own file/network access (read_csv, COPY TO, ATTACH, extensions) is sandboxed to the granted db= / fs:write roots.",
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
        limits: rite_runtime::Limits,
    ) -> Result<Value, EvalError> {
        #[cfg(not(all(feature = "duckdb", not(target_arch = "wasm32"))))]
        {
            let _ = (method, args, perms, limits);
            return Err(EvalError::Capability(
                "@db requires the native DuckDB host (not available in WASM / this build)".into(),
            ));
        }

        #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
        {
            self.call_native(method, args, perms, limits)
        }
    }

    #[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
    fn call_native(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        limits: rite_runtime::Limits,
    ) -> Result<Value, EvalError> {
        match method {
            "open" => self.open(args, perms),
            "close" => self.close_conn(args),
            "exec" => self.exec(args),
            "query" => self.query(args, limits),
            "prepare" => self.prepare(args),
            "query_prepared" => self.query_prepared(args, limits),
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
        // A script controls arbitrary SQL, and DuckDB's SQL surface is a second
        // filesystem/network host (read_csv, COPY TO, ATTACH, INSTALL/LOAD, httpfs).
        // Constrain it to the granted permissions before the handle escapes — fail
        // closed if the sandbox cannot be applied.
        harden_connection(&conn, perms)?;
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
    fn query(&self, args: Vec<Value>, limits: rite_runtime::Limits) -> Result<Value, EvalError> {
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
        collected(
            query_rows(conn, sql, &params, limits.max_collection_size),
            "db.query",
        )
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
    fn query_prepared(
        &self,
        args: Vec<Value>,
        limits: rite_runtime::Limits,
    ) -> Result<Value, EvalError> {
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
        collected(
            query_rows(conn, &sql, &params, limits.max_collection_size),
            "db.query_prepared",
        )
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

/// Settings a sandboxed script may still tune after the configuration is locked.
/// Deliberately limited to performance/formatting knobs: none of them can widen
/// file, network, or extension access.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
const DUCKDB_UNLOCKED_SETTINGS: &[&str] = &[
    "threads",
    "memory_limit",
    "max_memory",
    "preserve_insertion_order",
    "default_null_order",
    "default_order",
    "errors_as_json",
];

/// Clamp a fresh DuckDB connection down to the script's granted permissions.
///
/// DuckDB SQL can read and write files (`read_csv`, `COPY … TO`), attach other
/// databases, and install/load extensions that speak HTTP/S3 — none of which go
/// through `PermissionSet`. Without this, `--allow db` alone would be equivalent
/// to `--allow fs:read=/ --allow fs:write=/ --allow net=*`.
///
/// | Grants | DuckDB file/network access |
/// |---|---|
/// | `--allow db` (memory only) | none — `enable_external_access=false` |
/// | `--allow db=./data` | only under `./data` (plus any `fs:write` root) |
/// | `--allow-all` | unrestricted |
///
/// `fs:read`-only roots are intentionally *not* exposed: DuckDB's
/// `allowed_directories` has no read-only mode, so listing a read root there
/// would silently upgrade it to a write root. Load read-only data with
/// `@csv.read` / `@fs.read` and insert it instead.
///
/// The lock is not advisory: `allowed_configs` + `lock_configuration=true` make
/// every security-relevant `SET` fail, and DuckDB additionally refuses
/// `SET enable_external_access=true` while the database is running.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn harden_connection(conn: &Connection, perms: &PermissionSet) -> Result<(), EvalError> {
    // `--allow-all` is the documented opt-out from every restriction.
    if perms.allow_all {
        return Ok(());
    }

    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for root in perms.db_paths.iter().chain(perms.fs_write.iter()) {
        let root_s = root.to_string_lossy().to_string();
        if root.is_dir() {
            dirs.push(root_s);
        } else {
            // A grant naming a single database file (`--allow db=./data/app.duckdb`),
            // or a root that does not exist yet. Grant the path itself plus the two
            // sidecars DuckDB writes next to a database: `<db>.wal` and the
            // `<db>.tmp` spill directory.
            files.push(root_s.clone());
            files.push(format!("{root_s}.wal"));
            dirs.push(format!("{root_s}.tmp"));
            dirs.push(root_s);
        }
    }

    let mut stmts: Vec<String> = Vec::new();
    // Allow-list first: with external access off these are the only paths left.
    if !dirs.is_empty() {
        stmts.push(format!("SET allowed_directories={}", sql_str_list(&dirs)));
    }
    if !files.is_empty() {
        stmts.push(format!("SET allowed_paths={}", sql_str_list(&files)));
    }
    // The main gate. Everything outside the allow-list now fails with
    // "file system operations are disabled by configuration".
    stmts.push("SET enable_external_access=false".into());
    // No fetching, loading, or trusting extensions from inside the sandbox.
    stmts.push("SET autoinstall_known_extensions=false".into());
    stmts.push("SET allow_community_extensions=false".into());
    stmts.push("SET allow_unsigned_extensions=false".into());
    // Persistent secrets live in ~/.duckdb — outside every granted root.
    stmts.push("SET allow_persistent_secrets=false".into());
    // Keep a few harmless knobs settable, then freeze the rest.
    stmts.push(format!(
        "SET allowed_configs={}",
        sql_str_list(DUCKDB_UNLOCKED_SETTINGS)
    ));
    stmts.push("SET lock_configuration=true".into());

    for stmt in &stmts {
        conn.execute_batch(stmt).map_err(|e| {
            EvalError::Capability(format!(
                "db.open: could not sandbox the connection (`{stmt}`): {e}"
            ))
        })?;
    }
    Ok(())
}

/// Render `['a', 'b']` with SQL single-quote escaping.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn sql_str_list<S: AsRef<str>>(items: &[S]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|s| format!("'{}'", s.as_ref().replace('\'', "''")))
        .collect();
    format!("[{}]", inner.join(", "))
}

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
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
/// How a query can fail: the database saying no is a value the script can
/// branch on; a result set blowing the run's collection ceiling is a budget
/// error, exactly as `range` over the same ceiling is.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
enum QueryError {
    Db(String),
    OverLimit(usize),
}

/// Fold a query outcome into the value/error shape `db.query` answers.
#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn collected(rows: Result<Vec<Value>, QueryError>, who: &str) -> Result<Value, EvalError> {
    match rows {
        Ok(rows) => Ok(Value::ok(Value::list(rows))),
        Err(QueryError::Db(e)) => Ok(Value::err(Value::string(e))),
        Err(QueryError::OverLimit(max)) => Err(EvalError::Budget(
            rite_runtime::budget::BudgetError::CollectionTooLarge {
                who: who.to_string(),
                len: max.saturating_add(1),
                max,
            },
        )),
    }
}

#[cfg(all(feature = "duckdb", not(target_arch = "wasm32")))]
fn query_rows(
    conn: &Connection,
    sql: &str,
    params: &[DuckValue],
    max_rows: usize,
) -> Result<Vec<Value>, QueryError> {
    // DuckDB panics on column_count/column_name until the statement is stepped.
    // Prefer DESCRIBE for unbound SQL; otherwise infer width from the first row.
    let names = resolve_column_names(conn, sql, params.len());

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| QueryError::Db(e.to_string()))?;
    let mut rows = stmt
        .query(params_from_iter(params.iter()))
        .map_err(|e| QueryError::Db(e.to_string()))?;

    let mut out = Vec::new();
    let mut width: Option<usize> = (!names.is_empty()).then_some(names.len());
    while let Some(row) = rows.next().map_err(|e| QueryError::Db(e.to_string()))? {
        if out.len() >= max_rows {
            return Err(QueryError::OverLimit(max_rows));
        }
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
