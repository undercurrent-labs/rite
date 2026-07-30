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
