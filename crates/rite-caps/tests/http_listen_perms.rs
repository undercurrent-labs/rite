//! `@http.listen` bind-address policy (docs/book/http.md):
//! loopback by default, every other interface needs `net`.
//!
//! Regression for a verified escape: `@http.listen "0.0.0.0:18099"` served the
//! whole network with zero `--allow` flags.

use rite_caps::http::check_listen_perm;
use rite_caps::{install_defaults, Permission, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};

fn with_net(spec: &str) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::parse(spec).expect("permission spec"));
    p
}

#[test]
fn loopback_allowed_by_default() {
    let p = PermissionSet::default_secure();
    for addr in [
        "127.0.0.1:0",
        "127.0.0.1:8080",
        "127.0.0.2:8080",
        "127.1.2.3:8080",
        "localhost:8080",
        "LOCALHOST:8080",
        "[::1]:8080",
    ] {
        assert!(
            check_listen_perm(addr, &p).is_ok(),
            "`{addr}` is loopback and must bind under the default posture"
        );
    }
}

#[test]
fn wildcard_and_lan_denied_by_default() {
    let p = PermissionSet::default_secure();
    for addr in [
        "0.0.0.0:18099",
        "0.0.0.0:0",
        "[::]:8080",
        "192.168.1.10:8080",
        "10.0.0.5:80",
        "8.8.8.8:80",
    ] {
        let err = check_listen_perm(addr, &p)
            .expect_err(&format!("`{addr}` is not loopback and must need net"))
            .to_string();
        assert!(
            err.contains("net permission denied") && err.contains("--allow net="),
            "error should say what to do, got: {err}"
        );
    }
}

#[test]
fn deceptive_hostname_is_not_loopback() {
    let p = PermissionSet::default_secure();
    // Substring matching used to accept all of these.
    for addr in [
        "evil-127.0.0.1.example.com:80",
        "127.0.0.1.evil.example.com:80",
        "localhost.evil.example.com:80",
        "0.0.0.0.evil.example.com:80",
        "notlocalhost:8080",
    ] {
        assert!(
            check_listen_perm(addr, &p).is_err(),
            "`{addr}` merely contains a loopback-looking string"
        );
    }
}

#[test]
fn explicit_net_grant_allows_wildcard() {
    assert!(check_listen_perm("0.0.0.0:18099", &with_net("net=0.0.0.0")).is_ok());
    assert!(check_listen_perm("0.0.0.0:18099", &with_net("net=*")).is_ok());
    assert!(check_listen_perm("[::]:18099", &with_net("net=::")).is_ok());
    assert!(check_listen_perm("192.168.1.10:80", &with_net("net=192.168.1.10")).is_ok());
    assert!(check_listen_perm("0.0.0.0:18099", &PermissionSet::allow_all()).is_ok());
    // A grant for one host does not open another.
    assert!(check_listen_perm("0.0.0.0:18099", &with_net("net=192.168.1.10")).is_err());
    // `--allow net=localhost` must not be a wildcard grant either.
    assert!(check_listen_perm("0.0.0.0:18099", &with_net("net=localhost")).is_err());
}

#[tokio::test]
async fn listen_on_wildcard_refuses_to_bind_without_net() {
    // End to end: the script must fail before a socket exists.
    std::env::set_var("RITE_HTTP_TEST", "1");
    std::env::set_var("RITE_HTTP_TEST_SECS", "2");
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let err = run_source(
        "listen.rite",
        r#"
@http.listen "0.0.0.0:18099" ⟦
  GET "/" ⟦ ^ 200 "pwned" ⟧
⟧
"#,
        &mut ctx,
    )
    .await
    .expect_err("binding 0.0.0.0 with no permissions must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("net permission denied"),
        "unexpected error: {msg}"
    );
    // Nothing should be listening on the port the script asked for.
    assert!(
        std::net::TcpStream::connect("127.0.0.1:18099").is_err(),
        "a server was started despite the denial"
    );
}
