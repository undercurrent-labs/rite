//! `@db` sandbox: DuckDB's own SQL surface must not route around the capability
//! model. Every test here is a regression for a verified escape from
//! `--allow db` (arbitrary filesystem read *and* write via `read_csv` / `COPY TO`).

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};

/// Run `src` under exactly `perms`, returning the script's value rendered the way
/// `@console.println` would render it (`ok(…)` / `err(…)` included).
async fn run_with(perms: PermissionSet, src: &str) -> Result<String, String> {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, perms);
    match run_source("db-sandbox.rite", src, &mut ctx).await {
        Ok(v) => Ok(v.to_display(&ctx.atoms)),
        Err(e) => Err(e.to_string()),
    }
}

fn db_memory_only() -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::DbMemory);
    p
}

#[tokio::test]
async fn read_csv_outside_sandbox_is_denied() {
    let out = run_with(
        db_memory_only(),
        r#"
conn ← ! @db.open()?
^ ! @db.query(conn, "SELECT * FROM read_csv('/etc/passwd', header=false, sep=':') LIMIT 3")
"#,
    )
    .await
    .expect("script itself should run");
    assert!(
        !out.contains("root:") && !out.contains("/bin/"),
        "leaked /etc/passwd contents: {out}"
    );
    assert!(
        out.contains("err(") && out.contains("disabled by configuration"),
        "expected a DuckDB permission error, got: {out}"
    );
}

#[tokio::test]
async fn copy_to_outside_sandbox_is_denied() {
    let target = std::env::temp_dir().join("rite-db-escape-copy.csv");
    let _ = std::fs::remove_file(&target);
    let src = format!(
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE t(a VARCHAR)")?
! @db.exec(conn, "INSERT INTO t VALUES ('escaped-the-sandbox')")?
^ ! @db.exec(conn, "COPY t TO '{}' (HEADER, DELIMITER ',')")
"#,
        target.display()
    );
    let out = run_with(db_memory_only(), &src)
        .await
        .expect("script itself should run");
    assert!(
        out.contains("err(") && out.contains("disabled by configuration"),
        "expected COPY TO to be refused, got: {out}"
    );
    assert!(
        !target.exists(),
        "COPY TO wrote outside every granted root: {}",
        target.display()
    );
}

#[tokio::test]
async fn attach_outside_sandbox_is_denied() {
    let target = std::env::temp_dir().join("rite-db-escape-attach.duckdb");
    let _ = std::fs::remove_file(&target);
    let src = format!(
        r#"
conn ← ! @db.open()?
^ ! @db.exec(conn, "ATTACH '{}' AS other")
"#,
        target.display()
    );
    let out = run_with(db_memory_only(), &src)
        .await
        .expect("script itself should run");
    assert!(
        out.contains("err("),
        "expected ATTACH to be refused, got: {out}"
    );
    assert!(
        !target.exists(),
        "ATTACH created a database outside the sandbox"
    );
}

#[tokio::test]
async fn script_cannot_re_enable_external_access() {
    // Two independent locks: DuckDB refuses to re-enable external access on a
    // running database, and `lock_configuration` refuses the SET outright.
    let out = run_with(
        db_memory_only(),
        r#"
conn ← ! @db.open()?
reenable ← ! @db.exec(conn, "SET enable_external_access=true")
widen ← ! @db.exec(conn, "SET allowed_directories=['/']")
unlock ← ! @db.exec(conn, "SET lock_configuration=false")
paths ← ! @db.exec(conn, "SET allowed_paths=['/etc/passwd']")
reset ← ! @db.exec(conn, "RESET enable_external_access")
after ← ! @db.query(conn, "SELECT * FROM read_csv('/etc/passwd', header=false, sep=':') LIMIT 1")
^ ⟨reenable: reenable, widen: widen, unlock: unlock, paths: paths, reset: reset, after: after⟩
"#,
    )
    .await
    .expect("script itself should run");
    for field in ["reenable", "widen", "unlock", "paths", "reset"] {
        let at = out
            .find(field)
            .unwrap_or_else(|| panic!("missing {field} in {out}"));
        assert!(
            out[at..].starts_with(&format!("{field}: err(")),
            "`{field}` should have been refused: {out}"
        );
    }
    assert!(
        !out.contains("root:"),
        "re-enabling external access leaked /etc/passwd: {out}"
    );
}

#[tokio::test]
async fn extension_install_is_denied() {
    let out = run_with(
        db_memory_only(),
        r#"
conn ← ! @db.open()?
install ← ! @db.exec(conn, "INSTALL httpfs")
autoinstall ← ! @db.exec(conn, "SET autoinstall_known_extensions=true")
^ ⟨install: install, autoinstall: autoinstall⟩
"#,
    )
    .await
    .expect("script itself should run");
    assert!(
        out.contains("install: err(") && out.contains("autoinstall: err("),
        "extensions must not be installable from a sandboxed script: {out}"
    );
}

#[tokio::test]
async fn memory_workload_still_works_under_allow_db() {
    // The examples/11-db workload: create / insert / query / prepare / transaction.
    let out = run_with(
        db_memory_only(),
        r#"
conn ← ! @db.open()?
! @db.exec(conn, "CREATE TABLE items(id INTEGER, name VARCHAR)")?
! @db.exec(conn, "INSERT INTO items VALUES (1, 'glyph'), (2, 'sigil')")?
rows ← ! @db.query(conn, "SELECT name FROM items ORDER BY id")?
stmt ← ! @db.prepare(conn, "INSERT INTO items VALUES (?, ?)")?
! @db.exec_prepared(stmt, [3, "rune"])?
! @db.close_stmt(stmt)?
! @db.begin(conn)?
! @db.exec(conn, "INSERT INTO items VALUES (99, 'ghost')")?
! @db.rollback(conn)?
count ← ! @db.query(conn, "SELECT count(*) AS n FROM items")?
! @db.close(conn)?
^ ⟨first: rows, total: count⟩
"#,
    )
    .await
    .expect("the legitimate workload must keep working");
    assert!(out.contains("glyph") && out.contains("sigil"), "{out}");
    assert!(
        out.contains("n: 3"),
        "expected 3 rows after rollback: {out}"
    );
}

#[tokio::test]
async fn tunable_settings_survive_the_lock() {
    let out = run_with(
        db_memory_only(),
        r#"
conn ← ! @db.open()?
threads ← ! @db.exec(conn, "SET threads=2")
mem ← ! @db.exec(conn, "SET memory_limit='512MB'")
^ ⟨threads: threads, mem: mem⟩
"#,
    )
    .await
    .expect("script itself should run");
    assert!(
        out.contains("threads: ok(") && out.contains("mem: ok("),
        "harmless performance knobs should stay settable: {out}"
    );
}

#[tokio::test]
async fn file_backed_db_under_granted_root_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::Db(dir.path().to_path_buf()));
    let db_path = dir.path().join("app.duckdb");
    let inside_csv = dir.path().join("out.csv");

    // Somewhere outside the granted root, portably: `/tmp` does not exist on Windows.
    let outside_path = std::env::temp_dir().join("rite-db-escape-root.csv");
    let _ = std::fs::remove_file(&outside_path);
    let outside = outside_path.display().to_string().replace('\\', "/");

    let src = format!(
        r#"
conn ← ! @db.open("{db}")?
! @db.exec(conn, "CREATE TABLE t(id INTEGER, name VARCHAR)")?
! @db.exec(conn, "INSERT INTO t VALUES (1, 'kept')")?
! @db.exec(conn, "CHECKPOINT")?
inside ← ! @db.exec(conn, "COPY t TO '{csv}' (HEADER)")
outside ← ! @db.exec(conn, "COPY t TO '{outside}' (HEADER)")
leak ← ! @db.query(conn, "SELECT * FROM read_csv('/etc/passwd', header=false, sep=':') LIMIT 1")
rows ← ! @db.query(conn, "SELECT name FROM t")?
! @db.close(conn)?
^ ⟨rows: rows, inside: inside, outside: outside, leak: leak⟩
"#,
        db = db_path.display(),
        csv = inside_csv.display()
    );
    let out = run_with(perms, &src)
        .await
        .expect("file-backed db under a granted root must work");

    assert!(out.contains("kept"), "query failed: {out}");
    assert!(db_path.exists(), "database file was not created");
    assert!(
        out.contains("inside: ok(") && inside_csv.exists(),
        "COPY TO inside the granted root must work: {out}"
    );
    assert!(
        out.contains("outside: err("),
        "COPY TO outside the granted root must fail: {out}"
    );
    assert!(
        !out.contains("root:"),
        "read_csv outside the granted root leaked: {out}"
    );
    assert!(!outside_path.exists(), "wrote outside the granted root");
}

#[tokio::test]
async fn allow_all_keeps_full_duckdb_access() {
    // `--allow-all` is the documented opt-out; it must not be broken by hardening.
    let out = run_with(
        PermissionSet::allow_all(),
        r#"
conn ← ! @db.open()?
rows ← ! @db.query(conn, "SELECT count(*) AS n FROM read_csv('/etc/passwd', header=false, sep=':')")?
^ rows
"#,
    )
    .await
    .expect("allow_all should keep external access");
    assert!(out.contains("n: "), "expected a row count, got {out}");
}
