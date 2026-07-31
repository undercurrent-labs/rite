//! Security and permission enforcement tests.

use rite_caps::permissions::{Permission, PermissionSet};
use std::path::PathBuf;

#[test]
fn default_denies_fs_env_process() {
    let p = PermissionSet::default_secure();
    assert!(p.check_console().is_ok());
    assert!(p.check_clock().is_ok());
    assert!(p.check_random().is_ok());
    assert!(p.check_process().is_err());
    assert!(p.check_env("HOME").is_err());
    assert!(p
        .check_fs_read(std::path::Path::new("/etc/passwd"))
        .is_err());
    assert!(p.check_fs_write(std::path::Path::new("/tmp/x")).is_err());
}

#[test]
fn allow_all_grants_everything() {
    let p = PermissionSet::allow_all();
    assert!(p.check_process().is_ok());
    assert!(p.check_env("PATH").is_ok());
    assert!(p.check_fs_read(std::path::Path::new(".")).is_ok());
}

#[test]
fn fs_read_root_blocks_traversal() {
    let mut p = PermissionSet::default_secure();
    let root = std::env::temp_dir().join("rite_perm_root");
    let _ = std::fs::create_dir_all(&root);
    p.grant(Permission::FsRead(root.clone()));

    // Inside root — ok once file exists or parent is under root
    let inside = root.join("ok.txt");
    let _ = std::fs::write(&inside, "hi");
    assert!(p.check_fs_read(&inside).is_ok());

    // Outside root should fail
    let outside = std::env::temp_dir().join("rite_outside_secret.txt");
    let _ = std::fs::write(&outside, "secret");
    assert!(p.check_fs_read(&outside).is_err());
}

#[test]
fn fs_write_does_not_widen_read_outside() {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::FsWrite(PathBuf::from("./output")));
    // write root may allow read under same root, but not arbitrary paths
    assert!(p
        .check_fs_read(std::path::Path::new("/etc/passwd"))
        .is_err());
}

#[test]
fn env_allowlist() {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Env("APP_MODE".into()));
    assert!(p.check_env("APP_MODE").is_ok());
    assert!(p.check_env("SECRET").is_err());
}

#[test]
fn parse_permission_specs() {
    assert!(matches!(Permission::parse("all").unwrap(), Permission::All));
    assert!(matches!(
        Permission::parse("fs:read=./data").unwrap(),
        Permission::FsRead(_)
    ));
    assert!(matches!(
        Permission::parse("net=api.example.com").unwrap(),
        Permission::Net(_)
    ));
    assert!(Permission::parse("not-a-perm").is_err());
}

#[test]
fn deny_narrows() {
    let mut p = PermissionSet::allow_all();
    p.deny(Permission::Process);
    assert!(p.check_process().is_err());
    // allow_all flag may still be true depending on implementation —
    // after deny process, process must fail
}

#[tokio::test]
async fn runtime_fs_denied_without_permission() {
    use rite_caps::install_defaults;
    use rite_runtime::{run_source, RuntimeContext};

    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let result = run_source("t.rite", r#"! @fs.read("/etc/passwd")"#, &mut ctx).await;
    // Either permission error at capability or result err — must not return ok file contents
    match result {
        Err(e) => {
            let s = e.to_string();
            assert!(
                s.contains("permission") || s.contains("denied") || s.contains("fs"),
                "unexpected err: {}",
                s
            );
        }
        Ok(v) => {
            // ok(...) with not_found is fine; ok with password content is not
            let s = format!("{}", v);
            assert!(!s.contains("root:"), "leaked file contents: {}", s);
        }
    }
}

/// `--deny console` must actually deny.
///
/// Console calls bypass `CapabilityHost` — they need `&mut RuntimeContext` to reach the
/// output buffer or sink, which the trait cannot provide — so `ConsoleCap`'s
/// `check_console()` was unreachable and a denied script printed anyway. The grant is
/// mirrored onto the context, where the evaluator can see it.
#[tokio::test]
async fn console_denied_is_enforced() {
    use rite_caps::install_defaults;
    use rite_runtime::{run_source, RuntimeContext};

    let mut denied = PermissionSet::default_secure();
    denied.deny(Permission::Console);
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, denied);
    let err = run_source("d.rite", "! @console.println(\"nope\")\n", &mut ctx)
        .await
        .expect_err("console must be denied");
    assert!(
        err.to_string().to_lowercase().contains("console"),
        "unexpected error: {err}"
    );
    assert!(ctx.stdout.is_empty(), "denied output still buffered");
}

#[tokio::test]
async fn console_allowed_by_default() {
    use rite_caps::install_defaults;
    use rite_runtime::{run_source, RuntimeContext};

    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    run_source("a.rite", "! @console.println(\"fine\")\n", &mut ctx)
        .await
        .expect("console is allowed under the default policy");
    assert_eq!(ctx.stdout.join(""), "fine\n");
}

/// `--allow env=PATH,HOME` reads as two names, and used to be stored as the single
/// variable `"PATH,HOME"` — a name no environment can have. The grant was accepted
/// and granted nothing, which is the worst shape for a permission bug: it looks
/// applied and denies at the point of use.
#[test]
fn env_grants_accept_a_comma_separated_list() {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Env("PATH,HOME".into()));
    assert!(p.check_env("PATH").is_ok(), "PATH should be granted");
    assert!(p.check_env("HOME").is_ok(), "HOME should be granted");
    assert!(
        p.check_env("SECRET").is_err(),
        "only the listed names are granted"
    );
    assert!(
        p.check_env("PATH,HOME").is_err(),
        "the literal joined string must not become a variable name"
    );
}

/// Whitespace around the separator is what a person types, so it must not decide
/// whether the grant works.
#[test]
fn env_grant_list_tolerates_spaces() {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Env("PATH, HOME".into()));
    assert!(p.check_env("PATH").is_ok());
    assert!(p.check_env("HOME").is_ok());
}

/// Denial takes the same list form, or `--deny` could not undo what `--allow` did.
#[test]
fn env_denial_accepts_the_same_list() {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::Env("PATH,HOME,LANG".into()));
    p.deny(Permission::Env("HOME,LANG".into()));
    assert!(p.check_env("PATH").is_ok());
    assert!(p.check_env("HOME").is_err());
    assert!(p.check_env("LANG").is_err());
}

/// Creating a directory two levels deep is the ordinary case for `@fs.mkdir`, and it
/// was refused: only one missing path component was resolved, so `a/b` with neither
/// present stayed relative and matched no granted root. A grant of the working
/// directory would not let a script create a directory inside the working directory.
#[test]
fn a_path_several_levels_from_anything_that_exists_is_still_under_its_root() {
    let dir = std::env::temp_dir().join("rite_perm_deep_root");
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::FsWrite(dir.clone()));

    for rel in ["one", "one/two", "one/two/three"] {
        assert!(
            p.check_fs_write(&dir.join(rel)).is_ok(),
            "`{rel}` is inside the granted root and must be writable"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Resolving the missing tail must not hand `..` to a textual prefix check: a path
/// that walks up through components which do not exist still escapes the root, and
/// `granted/missing/../..` must not read as "starts with granted".
#[test]
fn a_missing_path_cannot_climb_out_of_its_root() {
    let dir = std::env::temp_dir().join("rite_perm_deep_escape");
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::FsWrite(dir.clone()));

    for rel in [
        "missing/../../escaped.txt",
        "../escaped.txt",
        "a/b/../../../x",
    ] {
        assert!(
            p.check_fs_write(&dir.join(rel)).is_err(),
            "`{rel}` leaves the granted root and must be denied"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
