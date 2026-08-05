//! External input honours the run's size ceilings.
#![allow(clippy::await_holding_lock)] // the HTTP test lock spans awaits, as in http_client.rs

//!
//! `max_string_size` and `max_collection_size` bounded only what a script
//! built itself (`range`, `repeat`, `concat`); a file, a subprocess, an HTTP
//! response or a query result could still buffer an unbounded amount from
//! outside the program. These tests hold each of those paths to the knobs.
//!
//! The knobs default to unlimited, so every test also proves the bounded path
//! still works under a ceiling it fits inside.

use rite_caps::fs::FsCap;
use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, ResultValue, RuntimeContext, Value};
use std::sync::{Mutex, OnceLock};

fn http_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn fs_perms(dir: &std::path::Path) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::parse(&format!("fs:read={}", dir.display())).unwrap());
    p
}

fn ctx_with_string_limit(n: usize) -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    ctx.budget.max_string_size = n;
    ctx
}

#[tokio::test]
async fn fs_reads_refuse_a_file_over_the_string_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    std::fs::write(&path, "x".repeat(100)).unwrap();
    let perms = fs_perms(dir.path());
    let ctx = ctx_with_string_limit(50);
    let arg = vec![Value::string(path.display().to_string())];

    for method in ["read", "read_bytes", "lines"] {
        let err = FsCap
            .call(method, arg.clone(), &perms, &ctx)
            .await
            .expect_err("100 bytes over a 50-byte ceiling must refuse");
        assert!(
            err.to_string().contains("50"),
            "@fs.{method} should name the ceiling: {err}"
        );
    }

    // Under the ceiling the same calls answer normally.
    let ctx = ctx_with_string_limit(200);
    for method in ["read", "read_bytes", "lines"] {
        let v = FsCap.call(method, arg.clone(), &perms, &ctx).await.unwrap();
        assert!(
            matches!(v, Value::Result(ResultValue::Ok(_))),
            "@fs.{method} under the ceiling: {v}"
        );
    }
}

#[tokio::test]
async fn fs_read_chunk_caps_the_callers_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "abc").unwrap();
    let perms = fs_perms(dir.path());
    let ctx = ctx_with_string_limit(1024);
    let handle = match FsCap
        .call(
            "open",
            vec![
                Value::string(path.display().to_string()),
                Value::string("read"),
            ],
            &perms,
            &ctx,
        )
        .await
        .unwrap()
    {
        Value::Result(ResultValue::Ok(h)) => *h,
        other => panic!("open failed: {other}"),
    };
    // The buffer is allocated at the caller's size before any byte arrives,
    // so the ceiling applies to the request, not the file.
    let err = FsCap
        .call(
            "read_chunk",
            vec![handle.clone(), Value::Int(1_000_000)],
            &perms,
            &ctx,
        )
        .await
        .expect_err("a 1 MB request over a 1 KiB ceiling must refuse");
    assert!(err.to_string().contains("1024"), "{err}");
}

#[tokio::test]
async fn process_capture_stops_at_the_ceiling() {
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::parse("process").unwrap());

    let mut ctx = RuntimeContext::new();
    ctx.budget.max_string_size = 1024;
    install_defaults(&mut ctx, perms.clone());
    let src = r#"! @process.run("sh", ["-c", "head -c 100000 /dev/zero"])"#;
    let err = run_source("cap.rite", src, &mut ctx)
        .await
        .expect_err("100 KB of output over a 1 KiB ceiling must refuse");
    assert!(err.to_string().contains("process.run"), "{err}");

    // Under the ceiling the capture arrives whole.
    let mut ctx = RuntimeContext::new();
    ctx.budget.max_string_size = 1024;
    install_defaults(&mut ctx, perms);
    let src = r#"
r ← ! @process.run("sh", ["-c", "printf hello"])?
! @console.println(r.stdout)
"#;
    run_source("ok.rite", src, &mut ctx)
        .await
        .expect("small capture");
    assert!(ctx.stdout.join("").contains("hello"));
}

#[cfg(feature = "duckdb")]
#[tokio::test]
async fn db_query_stops_at_the_collection_ceiling() {
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::parse("db").unwrap());

    let mut ctx = RuntimeContext::new();
    ctx.budget.max_collection_size = 10;
    install_defaults(&mut ctx, perms.clone());
    let src = r#"
conn ← ! @db.open(":memory:")?
rows ← ! @db.query(conn, "select * from range(100)")
"#;
    let err = run_source("db.rite", src, &mut ctx)
        .await
        .expect_err("100 rows over a 10-row ceiling must refuse");
    assert!(err.to_string().contains("db.query"), "{err}");

    let mut ctx = RuntimeContext::new();
    ctx.budget.max_collection_size = 10;
    install_defaults(&mut ctx, perms);
    let src = r#"
conn ← ! @db.open(":memory:")?
rows ← ! @db.query(conn, "select * from range(5)")?
! @console.println(str(count(rows)))
"#;
    run_source("db_ok.rite", src, &mut ctx)
        .await
        .expect("5 rows fit");
    assert!(ctx.stdout.join("").contains('5'));
}

#[tokio::test]
async fn http_response_bodies_stop_at_the_ceiling() {
    let _guard = http_test_lock().lock().unwrap();
    rite_caps::http::clear_last_bound_addr();
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "10");
    let srv = tokio::spawn(async {
        let mut ctx = RuntimeContext::new();
        ctx.budget = rite_runtime::ExecutionBudget::unlimited();
        install_defaults(&mut ctx, PermissionSet::allow_all());
        let src = r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/big" ⟦ ^ 200 repeat("x", 5000) ⟧
⟧
"#;
        let _ = run_source("srv.rite", src, &mut ctx).await;
    });
    let addr = {
        let start = std::time::Instant::now();
        loop {
            if let Some(a) = rite_caps::http::last_bound_addr() {
                break a;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(3),
                "server never bound"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    };

    // Over: a 5000-byte body against a 1000-byte ceiling is a catchable err —
    // the remote's size is not the script's bug.
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::parse("net=127.0.0.1").unwrap());
    let mut ctx = RuntimeContext::new();
    ctx.budget.max_string_size = 1000;
    install_defaults(&mut ctx, perms.clone());
    let src = format!(
        "r ← ! @http.get(\"http://{addr}/big\")\n\
         ! @console.println(? is_err(r) ⟦ \"refused\" ⟧ : ⟦ \"accepted\" ⟧)\n"
    );
    run_source("over.rite", &src, &mut ctx).await.expect("run");
    assert!(
        ctx.stdout.join("").contains("refused"),
        "{}",
        ctx.stdout.join("")
    );

    // Under: the same request with room to spare answers normally.
    let mut ctx = RuntimeContext::new();
    ctx.budget.max_string_size = 100_000;
    install_defaults(&mut ctx, perms);
    let src = format!(
        "r ← ! @http.get(\"http://{addr}/big\")?\n\
         ! @console.println(str(count(r.text?)))\n"
    );
    run_source("under.rite", &src, &mut ctx).await.expect("run");
    assert!(ctx.stdout.join("").contains("5000"));

    srv.abort();
    let _ = srv.await;
}
