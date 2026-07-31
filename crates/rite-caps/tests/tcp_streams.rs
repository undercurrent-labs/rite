//! `@tcp` byte streams: the client/server round trip, the two ways a `recv` can
//! come back without data, and the permission gates on both ends.
//!
//! Every server here binds port `0` and every client dials the address it actually
//! got, so nothing collides with another process over a fixed port.
//!
//! `@tcp.listen` blocks until shutdown, so a test cannot read its bound port out of
//! the return value — it reads it from `tcp::last_bound_addr()`, one process-global
//! side channel shared by every server in the binary. That is why the server tests
//! take `server_lock()`: the observation is global even though nothing in the accept
//! path is.

use rite_caps::{install_defaults, tcp, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext, Value};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

/// Serializes the tests that start a server, because `last_bound_addr` is global.
///
/// Async mutex (not `std`): the guard is held across awaits for the whole test, and
/// a blocking guard there is what `clippy::await_holding_lock` warns about. It also
/// cannot be poisoned, so one failing test does not turn the rest into panics that
/// hide it.
fn server_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ctx_with(perms: PermissionSet) -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, perms);
    ctx
}

/// Loopback is exempt from the *bind* gate but not from the *connect* gate, so a
/// script that talks to itself still needs `net=127.0.0.1`.
fn loopback_perms() -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Net("127.0.0.1".into()));
    p
}

async fn run(perms: PermissionSet, src: &str) -> Value {
    let mut ctx = ctx_with(perms);
    run_source("tcp.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("run failed: {e}"))
}

/// Start a server script in the background and answer the address it bound.
///
/// The caller stops it with `server.abort(); let _ = server.await;` — awaiting the
/// aborted handle is what makes the listener really gone before the next test takes
/// the lock and binds its own.
async fn serve(src: &'static str) -> (tokio::task::JoinHandle<()>, String) {
    tcp::clear_last_bound_addr();
    let handle = tokio::spawn(async move {
        let mut ctx = ctx_with(loopback_perms());
        // The listen call blocks until the task is aborted; a failure before that
        // (a denied bind, a syntax slip) must not be swallowed.
        if let Err(e) = run_source("server.rite", src, &mut ctx).await {
            panic!("server failed: {e}");
        }
    });

    let start = std::time::Instant::now();
    loop {
        if let Some(addr) = tcp::last_bound_addr() {
            return (handle, addr);
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "server never bound"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// An echo server and a client, both in Rite. Proves the whole shape: the block
/// runs per connection with the connection bound to its parameter, `send` and
/// `recv` move bytes, and the client sees what the server wrote back.
#[tokio::test(flavor = "multi_thread")]
async fn loopback_round_trip() {
    let _guard = server_lock().lock().await;

    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  got ← ! @tcp.recv(conn, 1024, 5000)?
  ! @tcp.send(conn, "echo: " + to_text(got)?)?
⟧
"#,
    )
    .await;

    let v = run(
        loopback_perms(),
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
! @tcp.send(conn, "ping ⚡")?
reply ← ! @tcp.recv(conn, 1024, 5000)?
! @tcp.close(conn)?
^ ⟨text: to_text(reply)?, bytes: len(reply)⟩
"#
        ),
    )
    .await;
    server.abort();
    let _ = server.await;

    assert_eq!(v.get_field("text").as_str(), Some("echo: ping ⚡"));
    // "echo: ping " is 11 bytes and ⚡ is 3 — `recv` answers bytes, not characters.
    assert_eq!(v.get_field("bytes").as_int(), Some(14));
}

/// The connection is closed when the block returns — that is the whole lifetime
/// rule — so the client's next read is end-of-stream: **`ok`, zero bytes**. It is
/// emphatically not a timeout, and the test asks for a long timeout so a confusion
/// between the two could not pass by accident.
#[tokio::test(flavor = "multi_thread")]
async fn a_clean_close_answers_zero_bytes() {
    let _guard = server_lock().lock().await;

    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  ! @tcp.send(conn, "bye")?
⟧
"#,
    )
    .await;

    let v = run(
        loopback_perms(),
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
first ← ! @tcp.recv(conn, 1024, 5000)?
// The server's block has returned, so the connection is closed at the other end.
// A 5s ceiling here is not a timing assertion — it is what the test fails *with*
// if end-of-stream is never reported at all.
after ← ! @tcp.recv(conn, 1024, 5000)
! @tcp.close(conn)?
^ ~ after ⟦
  ok data → ⟨kind: "ok", size: len(data), first: to_text(first)?⟩
  err e   → ⟨kind: e.kind, size: -1, first: to_text(first)?⟩
⟧
"#
        ),
    )
    .await;
    server.abort();
    let _ = server.await;

    assert_eq!(v.get_field("first").as_str(), Some("bye"));
    assert_eq!(
        v.get_field("kind").as_str(),
        Some("ok"),
        "a peer that closed cleanly is an answer, not an error"
    );
    assert_eq!(v.get_field("size").as_int(), Some(0));
}

/// A timeout is an ordinary `err` value the program can branch on — the same
/// contract `@udp.recv_from` has. The connection is still open afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn recv_timeout_is_an_err_not_a_raise() {
    let _guard = server_lock().lock().await;

    // A server that accepts and then says nothing for a long time. The client's
    // short `recv` can therefore only time out.
    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  ! @tcp.recv(conn, 1024, 20000)
⟧
"#,
    )
    .await;

    let started = std::time::Instant::now();
    let v = run(
        loopback_perms(),
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
got ← ! @tcp.recv(conn, 1024, 400)
// Still open: a timeout says "not yet", so writing after one must still work.
sent ← ! @tcp.send(conn, "still here")?
! @tcp.close(conn)?
^ ~ got ⟦
  err e → ⟨timed_out: true, kind: e.kind, waited: e.timeout_ms, sent: sent⟩
  ok _  → ⟨timed_out: false, kind: "", waited: 0, sent: sent⟩
⟧
"#
        ),
    )
    .await;
    let elapsed = started.elapsed();
    server.abort();
    let _ = server.await;

    assert_eq!(v.get_field("timed_out"), Value::Bool(true));
    assert_eq!(v.get_field("kind").as_str(), Some("tcp.timeout"));
    assert_eq!(v.get_field("waited").as_int(), Some(400));
    assert_eq!(v.get_field("sent").as_int(), Some(10));

    // Timing is asserted only where the signal is far wider than the noise. The
    // claim is "it waits, then gives up" — so the floor is a generous fraction of
    // the 400ms request (a coarse timer can fire early) and the ceiling is an order
    // of magnitude above it, which a loaded runner still clears. Asserting ~400ms
    // either way would be measuring the scheduler, not the contract.
    assert!(
        elapsed >= Duration::from_millis(200),
        "returned in {elapsed:?}: recv must actually wait for the timeout"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "took {elapsed:?}: recv must give up rather than block forever"
    );
}

/// Bytes go out and come back byte-identical, including sequences that are not
/// valid UTF-8 anywhere — which is the reason payloads are the `bytes` type and
/// `from_hex` exists rather than a `@tcp`-local encoding.
#[tokio::test(flavor = "multi_thread")]
async fn a_binary_payload_survives_intact() {
    let _guard = server_lock().lock().await;

    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  got ← ! @tcp.recv(conn, 1024, 5000)?
  // Straight back out — what a proxy does, and it must not be re-encoded on the way.
  ! @tcp.send(conn, got)?
⟧
"#,
    )
    .await;

    let v = run(
        loopback_perms(),
        &format!(
            r#"
// 0xff 0xfe is a UTF-16 BOM and invalid UTF-8; 0x00 would end a C string.
packet ← from_hex("fffe0001abcdef00")?
conn ← ! @tcp.connect("{addr}")?
! @tcp.send(conn, packet)?
back ← ! @tcp.recv(conn, 1024, 5000)?
! @tcp.close(conn)?
^ ⟨same: back = packet, hex: to_hex(back), size: len(back)⟩
"#
        ),
    )
    .await;
    server.abort();
    let _ = server.await;

    assert_eq!(v.get_field("same"), Value::Bool(true));
    assert_eq!(v.get_field("hex").as_str(), Some("fffe0001abcdef00"));
    assert_eq!(v.get_field("size").as_int(), Some(8));
}

/// The destination is gated per host, exactly as an outbound `@http.get` is — and
/// loopback is *not* exempt, because dialing `127.0.0.1` is how you reach every
/// interesting local service.
#[tokio::test]
async fn connecting_without_a_grant_is_denied() {
    let mut ctx = ctx_with(PermissionSet::default_secure());
    let err = run_source("tcp.rite", r#"! @tcp.connect("127.0.0.1:9")?"#, &mut ctx)
        .await
        .expect_err("no net grant for the destination");
    let s = err.to_string();
    assert!(
        s.contains("net permission denied") && s.contains("127.0.0.1"),
        "error should name the denied host, got: {s}"
    );

    // A grant for a *different* host does not carry over.
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::Net("example.com".into()));
    let mut ctx = ctx_with(perms);
    run_source("tcp.rite", r#"! @tcp.connect("203.0.113.7:9")?"#, &mut ctx)
        .await
        .expect_err("a grant for another host must not cover this one");
}

/// Binding beyond loopback is the `@http.listen` policy, reached through the same
/// function — so `0.0.0.0` is denied by default however you get there.
#[tokio::test]
async fn listening_beyond_loopback_needs_a_grant() {
    let mut ctx = ctx_with(PermissionSet::default_secure());
    let err = run_source(
        "tcp.rite",
        r#"! @tcp.listen "0.0.0.0:0" ⟦ |conn| conn ⟧"#,
        &mut ctx,
    )
    .await
    .expect_err("0.0.0.0 is not loopback");
    let s = err.to_string();
    assert!(
        s.contains("net permission denied") && s.contains("--allow net="),
        "error should say what to do, got: {s}"
    );
}

/// `close` really releases the handle, and closing twice is fine — a script closes
/// on the way out and should not have to track whether it already did.
#[tokio::test(flavor = "multi_thread")]
async fn a_closed_connection_is_gone() {
    let _guard = server_lock().lock().await;

    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  ! @tcp.recv(conn, 16, 3000)
⟧
"#,
    )
    .await;

    let v = run(
        loopback_perms(),
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
! @tcp.close(conn)?
^ ! @tcp.close(conn)?
"#
        ),
    )
    .await;
    assert_eq!(v, Value::None, "closing twice answers ok(none)");

    // Using the handle after `close` is a raise, not a silent no-op.
    let mut ctx = ctx_with(loopback_perms());
    let err = run_source(
        "tcp.rite",
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
! @tcp.close(conn)?
! @tcp.recv(conn, 16, 10)?
"#
        ),
        &mut ctx,
    )
    .await
    .expect_err("recv on a closed connection");
    server.abort();
    let _ = server.await;
    assert!(
        err.to_string().contains("closed or invalid"),
        "unexpected error: {err}"
    );
}

/// A payload is text or bytes. Anything else is a mistake, not a stringification:
/// sending `<bytes len=3>` or a rendered record down the wire is worse than an error.
#[tokio::test(flavor = "multi_thread")]
async fn a_non_payload_is_rejected() {
    let _guard = server_lock().lock().await;

    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  ! @tcp.recv(conn, 16, 3000)
⟧
"#,
    )
    .await;

    let mut ctx = ctx_with(loopback_perms());
    let err = run_source(
        "tcp.rite",
        &format!(
            r#"
conn ← ! @tcp.connect("{addr}")?
! @tcp.send(conn, ⟨a: 1⟩)?
"#
        ),
        &mut ctx,
    )
    .await
    .expect_err("a record is not a payload");
    server.abort();
    let _ = server.await;
    assert!(
        err.to_string().contains("string or bytes"),
        "unexpected error: {err}"
    );
}

/// A handle from another capability is caught by name rather than reaching into
/// whatever connection happens to share its id.
#[tokio::test]
async fn a_udp_socket_is_not_a_tcp_connection() {
    let mut ctx = ctx_with(loopback_perms());
    let err = run_source(
        "tcp.rite",
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
! @tcp.send(sock, "wrong handle")?
"#,
        &mut ctx,
    )
    .await
    .expect_err("a udp socket is not a tcp connection");
    assert!(
        err.to_string().contains("expected handle tcp.conn"),
        "unexpected error: {err}"
    );
}

/// Two clients at once. The accept loop spawns a task per connection, as `@http`
/// does, so a handler that is still waiting must not stop the next one being served.
#[tokio::test(flavor = "multi_thread")]
async fn connections_are_served_concurrently() {
    let _guard = server_lock().lock().await;

    // The handler waits for a line before answering. If connections were serialized,
    // the second client could not be served until the first handler returned — and
    // the first is deliberately made to finish *last*.
    let (server, addr) = serve(
        r#"
! @tcp.listen "127.0.0.1:0" ⟦ |conn|
  got ← ! @tcp.recv(conn, 64, 10000)?
  ! @tcp.send(conn, "saw " + to_text(got)?)?
⟧
"#,
    )
    .await;

    let v = run(
        loopback_perms(),
        &format!(
            r#"
a ← ! @tcp.connect("{addr}")?
b ← ! @tcp.connect("{addr}")?
// `b` is served first even though `a` connected first — impossible if the accept
// loop ran one handler to completion before taking the next connection.
! @tcp.send(b, "second")?
second ← ! @tcp.recv(b, 64, 5000)?
! @tcp.send(a, "first")?
first ← ! @tcp.recv(a, 64, 5000)?
! @tcp.close(a)?
! @tcp.close(b)?
^ ⟨a: to_text(first)?, b: to_text(second)?⟩
"#
        ),
    )
    .await;
    server.abort();
    let _ = server.await;

    assert_eq!(v.get_field("a").as_str(), Some("saw first"));
    assert_eq!(v.get_field("b").as_str(), Some("saw second"));
}
