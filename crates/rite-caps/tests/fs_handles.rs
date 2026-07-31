//! Open file handles: `@fs.open` → read/write → `@fs.close`.
//!
//! Every `@fs` read before these was whole-file, so peak memory was the size of
//! the file and nothing could be processed as it arrived. `@fs.lines` was
//! line-by-line as an interface only — it read everything and then split, costing
//! more at its peak than `read` did.
//!
//! The convention is `@tcp`'s (open → opaque handle → close, closing twice is
//! fine), with one deliberate difference: the resource lives on the run's
//! `RuntimeContext` rather than in a process-global, so anything left open closes
//! when the run ends rather than when the process does. That distinction is
//! invisible under `rite run` and load-bearing inside an embedder.

use rite_caps::fs::FsCap;
use rite_caps::{Permission, PermissionSet};
use rite_runtime::{ResultValue, RuntimeContext, Value};
use std::path::Path;

fn perms_for(dir: &Path) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::parse(&format!("fs:read={}", dir.display())).unwrap());
    p.grant(Permission::parse(&format!("fs:write={}", dir.display())).unwrap());
    p
}

async fn call(ctx: &RuntimeContext, perms: &PermissionSet, m: &str, args: Vec<Value>) -> Value {
    FsCap
        .call(m, args, perms, ctx)
        .await
        .unwrap_or_else(|e| panic!("@fs.{m} raised: {e}"))
}

/// Unwrap `ok(v)`; panic with the error otherwise.
fn ok(v: Value) -> Value {
    match v {
        Value::Result(ResultValue::Ok(inner)) => *inner,
        Value::Result(ResultValue::Err(e)) => panic!("expected ok, got err({e})"),
        other => panic!("expected a result, got {other}"),
    }
}

fn is_err(v: &Value) -> bool {
    matches!(v, Value::Result(ResultValue::Err(_)))
}

async fn open(ctx: &RuntimeContext, perms: &PermissionSet, path: &Path, mode: &str) -> Value {
    ok(call(
        ctx,
        perms,
        "open",
        vec![
            Value::string(path.display().to_string()),
            Value::string(mode),
        ],
    )
    .await)
}

#[tokio::test]
async fn writes_then_reads_back_in_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.txt");
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let w = open(&ctx, &perms, &path, "write").await;
    let n = ok(call(
        &ctx,
        &perms,
        "write_chunk",
        vec![w.clone(), Value::string("hello world")],
    )
    .await);
    assert_eq!(n, Value::Int(11), "write_chunk should answer bytes written");
    ok(call(&ctx, &perms, "close", vec![w]).await);

    let r = open(&ctx, &perms, &path, "read").await;
    let first = ok(call(&ctx, &perms, "read_chunk", vec![r.clone(), Value::Int(5)]).await);
    assert_eq!(first, Value::Bytes(b"hello".to_vec().into()));

    // A chunk larger than what is left answers what is left, not an error.
    let rest = ok(call(&ctx, &perms, "read_chunk", vec![r.clone(), Value::Int(100)]).await);
    assert_eq!(rest, Value::Bytes(b" world".to_vec().into()));

    // Empty means the end of the file has been reached.
    let eof = ok(call(&ctx, &perms, "read_chunk", vec![r.clone(), Value::Int(10)]).await);
    assert_eq!(eof, Value::Bytes(Vec::new().into()));
    ok(call(&ctx, &perms, "close", vec![r]).await);
}

/// The whole point: a big file is read without ever holding it whole.
#[tokio::test]
async fn peak_memory_is_the_chunk_rather_than_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.bin");
    std::fs::write(&path, vec![b'x'; 512 * 1024]).unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let r = open(&ctx, &perms, &path, "read").await;
    let mut total = 0usize;
    let mut largest = 0usize;
    loop {
        let chunk = ok(call(
            &ctx,
            &perms,
            "read_chunk",
            vec![r.clone(), Value::Int(4096)],
        )
        .await);
        let Value::Bytes(b) = chunk else {
            panic!("read_chunk should answer bytes")
        };
        if b.is_empty() {
            break;
        }
        largest = largest.max(b.len());
        total += b.len();
    }
    assert_eq!(total, 512 * 1024, "did not read the whole file");
    assert_eq!(largest, 4096, "a chunk larger than asked for came back");
    ok(call(&ctx, &perms, "close", vec![r]).await);
}

/// An empty line is `""`; the end of the file is `none`. They are different, and
/// reporting the end as an empty string would make them indistinguishable.
#[tokio::test]
async fn read_line_separates_an_empty_line_from_the_end_of_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lines.txt");
    std::fs::write(&path, "alpha\n\nbeta\r\n").unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let r = open(&ctx, &perms, &path, "read").await;
    let mut seen = Vec::new();
    loop {
        let line = ok(call(&ctx, &perms, "read_line", vec![r.clone()]).await);
        if line == Value::None {
            break;
        }
        seen.push(format!("{line}"));
    }
    // The `\r` of a CRLF file is stripped with the `\n`, so a script comparing
    // line contents is not defeated by where the file was written.
    assert_eq!(seen, vec!["alpha", "", "beta"]);
    ok(call(&ctx, &perms, "close", vec![r]).await);
}

#[tokio::test]
async fn append_keeps_what_was_there_and_write_truncates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let a = open(&ctx, &perms, &path, "append").await;
    ok(call(
        &ctx,
        &perms,
        "write_chunk",
        vec![a.clone(), Value::string("one\n")],
    )
    .await);
    ok(call(&ctx, &perms, "close", vec![a]).await);

    let a = open(&ctx, &perms, &path, "append").await;
    ok(call(
        &ctx,
        &perms,
        "write_chunk",
        vec![a.clone(), Value::string("two\n")],
    )
    .await);
    ok(call(&ctx, &perms, "close", vec![a]).await);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");

    let w = open(&ctx, &perms, &path, "write").await;
    ok(call(
        &ctx,
        &perms,
        "write_chunk",
        vec![w.clone(), Value::string("three\n")],
    )
    .await);
    ok(call(&ctx, &perms, "close", vec![w]).await);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
}

#[tokio::test]
async fn seek_moves_from_the_start_or_from_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seek.txt");
    std::fs::write(&path, "0123456789").unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let r = open(&ctx, &perms, &path, "read").await;
    assert_eq!(
        ok(call(&ctx, &perms, "seek", vec![r.clone(), Value::Int(4)]).await),
        Value::Int(4)
    );
    assert_eq!(
        ok(call(&ctx, &perms, "read_chunk", vec![r.clone(), Value::Int(2)]).await),
        Value::Bytes(b"45".to_vec().into())
    );
    // Negative counts back from the end, as a negative index does in `slice`.
    assert_eq!(
        ok(call(&ctx, &perms, "seek", vec![r.clone(), Value::Int(-3)]).await),
        Value::Int(7)
    );
    assert_eq!(
        ok(call(&ctx, &perms, "read_chunk", vec![r.clone(), Value::Int(9)]).await),
        Value::Bytes(b"789".to_vec().into())
    );
    ok(call(&ctx, &perms, "close", vec![r]).await);
}

#[tokio::test]
async fn a_handle_is_used_only_for_what_it_was_opened_for() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("modes.txt");
    std::fs::write(&path, "data").unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let r = open(&ctx, &perms, &path, "read").await;
    let refused = call(
        &ctx,
        &perms,
        "write_chunk",
        vec![r.clone(), Value::string("nope")],
    )
    .await;
    assert!(is_err(&refused), "a read handle accepted a write");

    let w = open(&ctx, &perms, &dir.path().join("w.txt"), "write").await;
    let refused = call(&ctx, &perms, "read_chunk", vec![w.clone(), Value::Int(4)]).await;
    assert!(is_err(&refused), "a write handle accepted a read");

    // The file is untouched by the refused write.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "data");
}

#[tokio::test]
async fn closing_twice_is_ok_and_using_a_closed_handle_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.txt");
    std::fs::write(&path, "data").unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();

    let r = open(&ctx, &perms, &path, "read").await;
    ok(call(&ctx, &perms, "close", vec![r.clone()]).await);
    // Closing again is `ok`: a script that closes on both the success and the
    // failure path is being careful, not wrong. `@tcp.close` settled this.
    ok(call(&ctx, &perms, "close", vec![r.clone()]).await);
    // Reading from it is not — that is a mistake, and a silent empty read would
    // look exactly like the end of a file.
    let after = call(&ctx, &perms, "read_chunk", vec![r, Value::Int(4)]).await;
    assert!(is_err(&after), "a closed handle answered a read");
}

/// The mode decides the grant, and it is checked before anything is opened.
#[tokio::test]
async fn opening_for_write_needs_the_write_grant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guarded.txt");
    std::fs::write(&path, "data").unwrap();
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::parse(&format!("fs:read={}", dir.path().display())).unwrap());
    let ctx = RuntimeContext::new();

    // Reading is granted.
    let r = FsCap
        .call(
            "open",
            vec![
                Value::string(path.display().to_string()),
                Value::string("read"),
            ],
            &perms,
            &ctx,
        )
        .await;
    assert!(r.is_ok(), "read grant did not allow #read");

    // Writing is not, and it is refused rather than opened and refused later.
    let w = FsCap
        .call(
            "open",
            vec![
                Value::string(path.display().to_string()),
                Value::string("write"),
            ],
            &perms,
            &ctx,
        )
        .await;
    assert!(
        matches!(w, Err(rite_runtime::EvalError::Permission(_))),
        "opening for write without the grant should be a permission error, got {w:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "data",
        "the refused open truncated the file anyway"
    );
}

#[tokio::test]
async fn an_unknown_mode_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let perms = perms_for(dir.path());
    let ctx = RuntimeContext::new();
    let out = FsCap
        .call(
            "open",
            vec![
                Value::string(dir.path().join("x.txt").display().to_string()),
                Value::string("excl"),
            ],
            &perms,
            &ctx,
        )
        .await;
    assert!(out.is_err(), "an unknown mode was accepted");
}

/// A loop that forgets to close should say so in Rite's words, rather than
/// surfacing the operating system's complaint later from an unrelated call.
#[tokio::test]
async fn the_open_handle_limit_is_reported_as_a_rite_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("many.txt");
    std::fs::write(&path, "x").unwrap();
    let perms = perms_for(dir.path());
    let mut ctx = RuntimeContext::new();
    // A small table, so the test does not open a thousand files to prove a rule.
    ctx.handles = std::sync::Arc::new(rite_runtime::HandleTable::new(2));

    let _a = open(&ctx, &perms, &path, "read").await;
    let _b = open(&ctx, &perms, &path, "read").await;
    let third = FsCap
        .call(
            "open",
            vec![
                Value::string(path.display().to_string()),
                Value::string("read"),
            ],
            &perms,
            &ctx,
        )
        .await;
    match third {
        Err(rite_runtime::EvalError::Message(m)) => {
            assert!(m.contains("too many open file handles"), "{m}");
            assert!(
                m.contains("@fs.close"),
                "the message should say how to fix it: {m}"
            );
        }
        other => panic!("expected a too-many-handles error, got {other:?}"),
    }
}

/// The reason the table lives on the context: an embedder's process does not exit
/// between runs, so a guest that never closed anything must not leak into the next
/// one — or into the host's lifetime.
#[tokio::test]
async fn handles_are_released_when_the_run_ends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("left-open.txt");
    std::fs::write(&path, "data").unwrap();
    let perms = perms_for(dir.path());

    let watch = {
        let ctx = RuntimeContext::new();
        // Deliberately never closed.
        let _ = open(&ctx, &perms, &path, "read").await;
        let _ = open(&ctx, &perms, &path, "read").await;
        assert_eq!(ctx.handles.len(), 2);
        std::sync::Arc::downgrade(&ctx.handles)
    };

    assert!(
        watch.upgrade().is_none(),
        "the handle table outlived the run that made it, so the files are still open"
    );
}
