//! @db DuckDB capability tests (native + duckdb feature).

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};

async fn run_allow_all(src: &str) -> rite_runtime::Value {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    run_source("db.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("run failed: {e}"))
}

#[tokio::test]
async fn memory_create_insert_query() {
    let v = run_allow_all(
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE t(id INTEGER, name VARCHAR)")?
! @db.exec(conn, "INSERT INTO t VALUES (1, 'Ada'), (2, 'Bob')")?
rows ← ! @db.query(conn, "SELECT name FROM t ORDER BY id")?
! @db.close(conn)?
^ rows
"#,
    )
    .await;
    match v {
        rite_runtime::Value::List(rows) => {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].get_field("name").as_str(), Some("Ada"));
            assert_eq!(rows[1].get_field("name").as_str(), Some("Bob"));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

#[tokio::test]
async fn prepared_and_params() {
    let v = run_allow_all(
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE t(id INTEGER, n INTEGER)")?
stmt ← ! @db.prepare(conn, "INSERT INTO t VALUES (?, ?)")?
! @db.exec_prepared(stmt, [1, 10])?
! @db.exec_prepared(stmt, [2, 20])?
rows ← ! @db.query(conn, "SELECT n FROM t WHERE id = ?", [2])?
! @db.close_stmt(stmt)?
! @db.close(conn)?
^ rows
"#,
    )
    .await;
    match v {
        rite_runtime::Value::List(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get_field("n").as_int(), Some(20));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

#[tokio::test]
async fn transaction_rollback() {
    let v = run_allow_all(
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE t(id INTEGER)")?
! @db.begin(conn)?
! @db.exec(conn, "INSERT INTO t VALUES (1)")?
! @db.rollback(conn)?
rows ← ! @db.query(conn, "SELECT * FROM t")?
! @db.close(conn)?
^ rows → count
"#,
    )
    .await;
    assert_eq!(v.as_int(), Some(0));
}

#[tokio::test]
async fn permission_denied_without_allow() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let err = run_source("db.rite", r#"conn ← ! @db.open()"#, &mut ctx)
        .await
        .expect_err("should deny");
    let s = err.to_string();
    assert!(
        s.contains("permission") || s.contains("db"),
        "unexpected error: {s}"
    );
}

#[tokio::test]
async fn allow_db_memory_only() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::DbMemory);
    install_defaults(&mut ctx, perms);
    let v = run_source(
        "db.rite",
        r#"
conn ← ! @db.open()?
rows ← ! @db.query(conn, "SELECT 1 AS x")?
! @db.close(conn)?
^ rows
"#,
        &mut ctx,
    )
    .await
    .expect("open memory with --allow db");
    match v {
        rite_runtime::Value::List(rows) => {
            assert_eq!(rows[0].get_field("x").as_int(), Some(1));
        }
        other => panic!("{other:?}"),
    }
}

/// `INSERT … RETURNING *` used to come back as col0/col1: DESCRIBE rejects an
/// INSERT, the fallback silently returned no names, and every insert in the
/// field report's hot path was followed by a SELECT just to learn its ids.
#[tokio::test]
async fn insert_returning_keeps_column_names() {
    let v = run_allow_all(
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE t(id INTEGER, name VARCHAR)")?
rows ← ! @db.query(conn, "INSERT INTO t VALUES (7, 'Ada') RETURNING *")?
^ rows
"#,
    )
    .await;
    match v {
        rite_runtime::Value::List(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].get_field("id").as_int(),
                Some(7),
                "RETURNING columns lost their names: {rows:?}"
            );
            assert_eq!(rows[0].get_field("name").as_str(), Some("Ada"));
        }
        other => panic!("expected list, got {other:?}"),
    }
}

async fn run_allow_all_result(src: &str) -> Result<rite_runtime::Value, rite_runtime::EvalError> {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    run_source("db.rite", src, &mut ctx).await
}

/// DuckDB's file lock is per process: two opens of one file in one script were
/// two writers on a single-writer database — the actual corruption path, since
/// the cross-process case is already refused by DuckDB itself.
#[tokio::test]
async fn second_open_of_the_same_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.duckdb");
    let src = format!(
        r#"
a ← ! @db.open("{p}")?
b ← ! @db.open("{p}")?
^ b
"#,
        p = path.display()
    );
    let err = run_allow_all_result(&src).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("already open in this script"),
        "expected a double-open refusal, got: {msg}"
    );

    // Closing the first handle releases the path for a fresh open.
    let src = format!(
        r#"
a ← ! @db.open("{p}")?
! @db.close(a)?
b ← ! @db.open("{p}")?
^ 1
"#,
        p = path.display()
    );
    let v = run_allow_all_result(&src)
        .await
        .expect("reopen after close");
    assert_eq!(v.as_int(), Some(1));
}

/// An unknown option key is an error, not a default — the `@process.run` rule.
/// `access_mode` used to be dropped on the floor with every other key, so a
/// "READ_ONLY" handle happily executed CREATE TABLE.
#[tokio::test]
async fn open_options_are_validated_and_read_only_holds() {
    let err = run_allow_all_result(r#"^ ! @db.open(⟨path: ":memory:", bogus_key: 1⟩)?"#)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("unknown option `bogus_key`"),
        "unknown key accepted: {err}"
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.duckdb");
    // Create the file first — READ_ONLY cannot create.
    let create = format!(
        r#"
c ← ! @db.open("{p}")?
! @db.exec(c, "CREATE TABLE t(id INTEGER)")?
! @db.close(c)?
^ 1
"#,
        p = path.display()
    );
    run_allow_all_result(&create).await.expect("create db");

    let src = format!(
        r#"
c ← ! @db.open(⟨path: "{p}", access_mode: "READ_ONLY"⟩)?
^ ! @db.exec(c, "CREATE TABLE nope(id INTEGER)")
"#,
        p = path.display()
    );
    let v = run_allow_all_result(&src).await.expect("script runs");
    let msg = format!("{v:?}");
    assert!(
        msg.contains("read-only"),
        "write through a READ_ONLY handle did not answer a read-only err: {msg}"
    );
}
