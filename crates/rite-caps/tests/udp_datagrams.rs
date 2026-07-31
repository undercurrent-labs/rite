//! `@udp` datagram sockets: a loopback round trip, the timeout contract, and the
//! two permission gates (bind address and destination).
//!
//! Everything here binds port 0 and talks to itself, so the tests need no network
//! and cannot collide with another process over a fixed port. They also do not
//! share any process-global state, unlike the `@http` tests — a datagram socket is
//! reachable only through its own handle — so no lock is taken.

use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext, Value};

fn ctx_with(perms: PermissionSet) -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, perms);
    ctx
}

/// Loopback is exempt from the *bind* gate but not from the *destination* gate,
/// so a script that talks to itself still needs `net=127.0.0.1`.
fn loopback_perms() -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Net("127.0.0.1".into()));
    p
}

async fn run(perms: PermissionSet, src: &str) -> Value {
    let mut ctx = ctx_with(perms);
    run_source("udp.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("run failed: {e}"))
}

#[tokio::test]
async fn loopback_round_trip() {
    // The datagram is already sitting in the receiver's buffer before `recv_from`
    // is called, so the 5s ceiling is not a timing assertion — it is the value the
    // test fails *with* if delivery never happens at all.
    let v = run(
        loopback_perms(),
        r#"
listener ← ! @udp.bind("127.0.0.1:0")?
sender   ← ! @udp.bind("127.0.0.1:0")?
where    ← ! @udp.local_addr(listener)?
back     ← ! @udp.local_addr(sender)?
! @udp.send_to(sender, where, "ping ⚡")?
got ← ! @udp.recv_from(listener, 5000)?
! @udp.close(sender)?
! @udp.close(listener)?
^ ⟨text: got.text, bytes: len(got.data), from: got.from, sender: back⟩
"#,
    )
    .await;

    assert_eq!(v.get_field("text").as_str(), Some("ping ⚡"));
    // "ping " is 5 bytes and ⚡ is 3 — `data` is bytes, not characters.
    assert_eq!(v.get_field("bytes").as_int(), Some(8));
    // `from` is the sender's own bound address, which is how a reply is addressed.
    assert_eq!(
        v.get_field("from").as_str(),
        v.get_field("sender").as_str(),
        "the `from` address must be the socket that sent the datagram"
    );
}

/// `@udp.local_addr` is what makes port 0 usable, and `close` really does release
/// the handle: a second call on the same socket must not still work.
#[tokio::test]
async fn a_closed_socket_is_gone() {
    let v = run(
        loopback_perms(),
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
before ← ! @udp.local_addr(sock)?
! @udp.close(sock)?
// Closing twice is fine — a script closes on the way out and should not have to
// track whether it already did.
again ← ! @udp.close(sock)?
^ ⟨addr: before, closed_twice: again⟩
"#,
    )
    .await;
    let addr = v.get_field("addr");
    let addr = addr.as_str().expect("bound address");
    assert!(addr.starts_with("127.0.0.1:"), "unexpected address {addr}");
    assert_ne!(
        addr, "127.0.0.1:0",
        "port 0 must be resolved to a real port"
    );

    // Using the handle after `close` is a raise, not a silent no-op.
    let mut ctx = ctx_with(loopback_perms());
    let err = run_source(
        "udp.rite",
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
! @udp.close(sock)?
! @udp.recv_from(sock, 10)?
"#,
        &mut ctx,
    )
    .await
    .expect_err("recv on a closed socket");
    assert!(
        err.to_string().contains("closed or invalid"),
        "unexpected error: {err}"
    );
}

/// A timeout is an ordinary `err` value: the program keeps running and can branch
/// on it. It is emphatically not a raise.
#[tokio::test]
async fn recv_timeout_is_an_err_not_a_raise() {
    let started = std::time::Instant::now();
    let v = run(
        loopback_perms(),
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
// Nothing is ever sent here, so this can only time out.
got ← ! @udp.recv_from(sock, 400)
! @udp.close(sock)?
^ ~ got ⟦
  err e → ⟨timed_out: true, kind: e.kind, waited: e.timeout_ms⟩
  ok _  → ⟨timed_out: false, kind: "", waited: 0⟩
⟧
"#,
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(v.get_field("timed_out"), Value::Bool(true));
    assert_eq!(v.get_field("kind").as_str(), Some("udp.timeout"));
    assert_eq!(v.get_field("waited").as_int(), Some(400));

    // Timing is asserted only where the signal is far wider than the noise. The
    // claim is "it waits, then gives up" — so the floor is a generous fraction of
    // the 400ms request (a coarse timer can fire early), and the ceiling is an
    // order of magnitude above it, which a loaded runner still clears. Asserting
    // ~400ms either way would be measuring the scheduler, not the contract.
    assert!(
        elapsed >= std::time::Duration::from_millis(200),
        "returned in {elapsed:?}: recv_from must actually wait for the timeout"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "took {elapsed:?}: recv_from must give up rather than block forever"
    );
}

/// Binding beyond loopback is the `@http.listen` policy, reached through the same
/// function — so `0.0.0.0` is denied by default however you get there.
#[tokio::test]
async fn binding_beyond_loopback_needs_a_grant() {
    let mut ctx = ctx_with(PermissionSet::default_secure());
    let err = run_source("udp.rite", r#"! @udp.bind("0.0.0.0:0")?"#, &mut ctx)
        .await
        .expect_err("0.0.0.0 is not loopback");
    let s = err.to_string();
    assert!(
        s.contains("net permission denied") && s.contains("--allow net="),
        "error should say what to do, got: {s}"
    );

    // ...and the grant is what lifts it.
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::Net("0.0.0.0".into()));
    let mut ctx = ctx_with(perms);
    run_source(
        "udp.rite",
        r#"
sock ← ! @udp.bind("0.0.0.0:0")?
! @udp.close(sock)?
"#,
        &mut ctx,
    )
    .await
    .expect("`--allow net=0.0.0.0` should permit the bind");
}

/// The destination is gated per host, exactly as an outbound `@http` request is —
/// binding a socket does not grant the right to send anywhere with it.
#[tokio::test]
async fn sending_to_an_ungranted_host_is_denied() {
    // Loopback binds without a grant, so this run reaches `send_to` with no `net`
    // permission at all: the failure can only come from the destination check.
    let mut ctx = ctx_with(PermissionSet::default_secure());
    let err = run_source(
        "udp.rite",
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
! @udp.send_to(sock, "203.0.113.7:9", "nope")?
"#,
        &mut ctx,
    )
    .await
    .expect_err("no net grant for the destination");
    let s = err.to_string();
    assert!(
        s.contains("net permission denied") && s.contains("203.0.113.7"),
        "error should name the denied host, got: {s}"
    );

    // A grant for a *different* host does not cover it either.
    let mut perms = PermissionSet::default_secure();
    perms.grant(Permission::Net("example.com".into()));
    let mut ctx = ctx_with(perms);
    run_source(
        "udp.rite",
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
! @udp.send_to(sock, "203.0.113.7:9", "nope")?
"#,
        &mut ctx,
    )
    .await
    .expect_err("a grant for another host must not carry over");
}

/// A payload is text or bytes. Bytes come back out of `recv_from` (and out of
/// `@fs.read_bytes`) and go back in unchanged, which is the only way a script can
/// move a non-UTF-8 datagram today.
#[tokio::test]
async fn bytes_survive_a_relay_unchanged() {
    let v = run(
        loopback_perms(),
        r#"
a ← ! @udp.bind("127.0.0.1:0")?
b ← ! @udp.bind("127.0.0.1:0")?
c ← ! @udp.bind("127.0.0.1:0")?
! @udp.send_to(a, ! @udp.local_addr(b)?, "relay me")?
first ← ! @udp.recv_from(b, 5000)?
// `first.data` is a bytes value — feeding it straight back in is what a proxy does.
! @udp.send_to(b, ! @udp.local_addr(c)?, first.data)?
second ← ! @udp.recv_from(c, 5000)?
! @udp.close(a)?
! @udp.close(b)?
! @udp.close(c)?
^ ⟨same: first.data = second.data, text: second.text⟩
"#,
    )
    .await;
    assert_eq!(v.get_field("same"), Value::Bool(true));
    assert_eq!(v.get_field("text").as_str(), Some("relay me"));
}

/// Anything that is not text or bytes is a mistake, not a stringification: sending
/// `<bytes len=3>` or a rendered record down the wire is worse than an error.
#[tokio::test]
async fn a_non_payload_is_rejected() {
    let mut ctx = ctx_with(loopback_perms());
    let err = run_source(
        "udp.rite",
        r#"
sock ← ! @udp.bind("127.0.0.1:0")?
! @udp.send_to(sock, "127.0.0.1:9", ⟨a: 1⟩)?
"#,
        &mut ctx,
    )
    .await
    .expect_err("a record is not a payload");
    assert!(
        err.to_string().contains("string or bytes"),
        "unexpected error: {err}"
    );
}
