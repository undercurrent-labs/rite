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
