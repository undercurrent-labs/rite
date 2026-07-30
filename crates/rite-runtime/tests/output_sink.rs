//! Output goes to a sink as it is produced, or buffers when there is none.
//!
//! Without a sink, `ctx.stdout` accumulates for the whole run, so a long-running script
//! prints nothing until it exits and a chatty one holds every line in memory. Buffering
//! stays the default because the HTTP host collects a handler's output deliberately and
//! most tests assert on the buffers.

use rite_runtime::{run_source, OutputStream, RuntimeContext};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn a_sink_receives_output_and_the_buffers_stay_empty() {
    let seen: Arc<Mutex<Vec<(OutputStream, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_log = seen.clone();

    let mut ctx = RuntimeContext::new();
    ctx.sink = Some(Arc::new(move |stream, text: &str| {
        sink_log.lock().unwrap().push((stream, text.to_string()));
    }));
    run_source(
        "sink.rite",
        "! @console.println(\"one\")\n! @console.println(\"two\")\n",
        &mut ctx,
    )
    .await
    .expect("run");

    let seen = seen.lock().unwrap();
    let joined: String = seen.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(joined, "one\ntwo\n", "sink saw: {seen:?}");
    assert!(
        seen.iter().all(|(s, _)| *s == OutputStream::Stdout),
        "println must not reach stderr: {seen:?}"
    );
    assert!(
        ctx.stdout.is_empty(),
        "output was buffered as well as streamed: {:?}",
        ctx.stdout
    );
}

#[tokio::test]
async fn without_a_sink_output_still_buffers() {
    let mut ctx = RuntimeContext::new();
    run_source("buf.rite", "! @console.println(\"kept\")\n", &mut ctx)
        .await
        .expect("run");
    assert_eq!(ctx.stdout.join(""), "kept\n");
}

#[tokio::test]
async fn warnings_are_tagged_as_stderr() {
    let seen: Arc<Mutex<Vec<(OutputStream, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let mut ctx = RuntimeContext::new();
    ctx.sink = Some(Arc::new(move |stream, text: &str| {
        log.lock().unwrap().push((stream, text.to_string()));
    }));
    run_source("warn.rite", "! @console.warn(\"careful\")\n", &mut ctx)
        .await
        .expect("run");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert_eq!(seen[0].0, OutputStream::Stderr, "{seen:?}");
    assert!(seen[0].1.contains("careful"));
}

/// Output produced before a failure must reach the sink, not be lost with the error.
#[tokio::test]
async fn output_before_an_error_still_reaches_the_sink() {
    let seen: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let log = seen.clone();
    let mut ctx = RuntimeContext::new();
    ctx.sink = Some(Arc::new(move |_, text: &str| {
        log.lock().unwrap().push_str(text);
    }));
    let err = run_source(
        "err.rite",
        "! @console.println(\"printed\")\nx ← 1 / 0\n",
        &mut ctx,
    )
    .await;
    assert!(err.is_err(), "expected the division to fail");
    assert_eq!(*seen.lock().unwrap(), "printed\n");
}
