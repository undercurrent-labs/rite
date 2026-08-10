// Shares the process-global RITE_HTTP_TEST env vars and the PENDING_SERVER /
// LAST_BOUND_ADDR statics in rite-caps::http, so each test holds
// `http_test_lock()` for its whole body. Holding the guard across `.await` is
// deliberate.
#![allow(clippy::await_holding_lock)]

//! Concurrent writes through one shared `@db` connection.
//!
//! Handlers now share the listen-time capability host, which is what lets a
//! server hold a single DuckDB connection instead of opening one per request.
//! That is the fix for the scry-core field report's worst incident — but it
//! also moves concurrent writes onto one connection, so the thing worth
//! proving is that they are actually serialized inside `DbCap` and no write is
//! lost or interleaved into corruption.
//!
//! Custom middleware is present on purpose: before the server-wide mutex was
//! removed these requests could not overlap at all, so the test would have
//! passed for the wrong reason.

use rite_caps::http::{clear_last_bound_addr, last_bound_addr};
use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn spawn_server(source: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        // Surfaced rather than discarded: a server that fails before `listen`
        // otherwise shows up only as "server never bound" ten lines away.
        if let Err(e) = run_source("shared-db.rite", &source, &mut ctx).await {
            eprintln!("shared-db server failed: {e:?}");
        }
    })
}

async fn wait_for_bind(limit: Duration) -> String {
    let start = std::time::Instant::now();
    loop {
        if let Some(addr) = last_bound_addr() {
            return addr;
        }
        assert!(start.elapsed() < limit, "server never bound");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn test_mode() {
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "8");
}

const WRITERS: usize = 24;

#[tokio::test]
async fn concurrent_writes_through_one_shared_connection_all_land() {
    let _guard = http_test_lock().lock().unwrap();
    test_mode();
    clear_last_bound_addr();

    let dir = tempfile::tempdir().unwrap();
    // DuckDB accepts forward slashes everywhere, and a backslash would start an
    // escape sequence in the Rite string below.
    let db_path = dir
        .path()
        .join("service.duckdb")
        .display()
        .to_string()
        .replace('\\', "/");

    // One connection, opened before `listen` and used by every handler — the
    // shape the field report could not express, which is why it ran one writer
    // per request against a single-writer file.
    let src = format!(
        r#"
conn ← ! @db.open("{db_path}")?
! @db.exec(conn, "CREATE TABLE hits(n INTEGER, tag VARCHAR)")?

@http.listen "127.0.0.1:0" ⟦
  use {{ |req, next| next(req) }}

  GET "/add/:n" |req| ⟦
    rows ← ! @db.query(
      conn,
      "INSERT INTO hits VALUES (CAST(? AS INTEGER), 'w') RETURNING *",
      [req.path.n]
    )?
    ^ 200 ⟨wrote: rows⟩
  ⟧

  GET "/count" ⟦
    rows ← ! @db.query(conn, "SELECT count(*) AS c, count(DISTINCT n) AS d FROM hits")?
    ^ 200 ⟨rows: rows⟩
  ⟧
⟧
"#
    );

    let handle = spawn_server(src).await;
    let addr = wait_for_bind(Duration::from_secs(5)).await;

    // Every writer fires at once. With the shared connection these all queue on
    // DbCap's own lock; a missing lock would show up as lost rows or a corrupt
    // file on the read-back below.
    let mut set = Vec::new();
    for i in 0..WRITERS {
        let url = format!("http://{addr}/add/{i}");
        set.push(tokio::spawn(async move {
            reqwest::get(&url).await.unwrap().text().await.unwrap()
        }));
    }
    // The tasks are already in flight; this only collects them.
    let mut bodies: Vec<String> = Vec::with_capacity(WRITERS);
    for task in set {
        bodies.push(task.await.expect("writer task"));
    }

    // Each write answers its own `RETURNING *` row, with real column names —
    // so this doubles as an end-to-end check of the RETURNING fix under load.
    for (i, body) in bodies.iter().enumerate() {
        assert!(
            body.contains("\"n\":") && body.contains("\"tag\":\"w\""),
            "writer {i} lost its RETURNING column names: {body}"
        );
    }

    let counted = reqwest::get(format!("http://{addr}/count"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        counted.contains(&format!("\"c\":{WRITERS}")),
        "expected {WRITERS} rows after {WRITERS} concurrent writes, got: {counted}"
    );
    assert!(
        counted.contains(&format!("\"d\":{WRITERS}")),
        "rows were written but values collided or interleaved: {counted}"
    );

    handle.abort();

    // The file must still be a readable database afterwards: the incident this
    // guards against corrupted it rather than losing rows.
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let reread = run_source(
        "verify.rite",
        &format!(
            r#"
c ← ! @db.open("{db_path}")?
rows ← ! @db.query(c, "SELECT count(*) AS c FROM hits")?
^ rows
"#
        ),
        &mut ctx,
    )
    .await
    .expect("database still opens after concurrent writes");
    assert!(
        format!("{reread:?}").contains(&WRITERS.to_string()),
        "re-opened database lost rows: {reread:?}"
    );
}
