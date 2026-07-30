//! `@fs.glob` must respect the granted read roots.
//!
//! Regression: with `--allow fs:read=.` the old implementation permission-checked
//! `.` and then returned unfiltered matches for any pattern, so
//! `@fs.glob("/etc/ssh/*")` and `@fs.glob("/home/me/.ssh/*")` listed private keys.

use rite_caps::fs::FsCap;
use rite_caps::{Permission, PermissionSet};
use rite_runtime::{ResultValue, Value};
use std::path::Path;

/// Raw `@fs.glob` call: `Ok(paths)` on success, `Err(message)` on a capability error.
async fn glob(perms: &PermissionSet, pattern: &str) -> Result<Vec<String>, String> {
    match FsCap
        .call(
            "glob",
            vec![Value::string(pattern)],
            perms,
            &rite_runtime::AtomInterner::new(),
        )
        .await
    {
        Ok(Value::Result(ResultValue::Ok(inner))) => match *inner {
            Value::List(xs) => Ok(xs.iter().map(|x| format!("{x}")).collect()),
            other => panic!("expected ok(list), got ok({other})"),
        },
        Ok(other) => panic!("expected ok(list), got {other}"),
        Err(e) => Err(e.to_string()),
    }
}

async fn glob_paths(perms: &PermissionSet, pattern: &str) -> Vec<String> {
    glob(perms, pattern)
        .await
        .unwrap_or_else(|e| panic!("glob({pattern}) denied: {e}"))
}

fn perms_read(root: &Path) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::FsRead(root.to_path_buf()));
    p
}

#[tokio::test]
async fn pattern_inside_root_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/c.txt"), "c").unwrap();
    let perms = perms_read(dir.path());

    let flat = glob_paths(&perms, &format!("{}/*.txt", dir.path().display())).await;
    assert_eq!(flat.len(), 2, "expected a.txt and b.txt, got {flat:?}");
    assert!(flat.iter().any(|p| p.ends_with("a.txt")), "{flat:?}");

    let deep = glob_paths(&perms, &format!("{}/**/*.txt", dir.path().display())).await;
    // Compare with `/` on every platform: results carry native separators, so on Windows
    // this read `sub\c.txt` and the POSIX spelling failed for a correct result.
    assert!(
        deep.iter()
            .any(|p| p.replace('\\', "/").ends_with("sub/c.txt")),
        "recursive glob inside the root should still work: {deep:?}"
    );
}

#[tokio::test]
async fn pattern_outside_root_leaks_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let perms = perms_read(dir.path());

    for pattern in ["/etc/ssh/*", "/etc/*", "/etc/passwd", "/root/.ssh/*"] {
        let err = glob(&perms, pattern)
            .await
            .expect_err(&format!("`{pattern}` points outside the granted root"));
        assert!(
            err.contains("permission denied") || err.contains("fs:read"),
            "expected a permission error for {pattern}, got {err}"
        );
    }
}

#[tokio::test]
async fn traversal_pattern_is_denied() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(dir.path().join("secret.txt"), "s3cret").unwrap();
    let perms = perms_read(&root);

    let err = glob(&perms, &format!("{}/../*", root.display()))
        .await
        .expect_err("`../` escapes the granted root");
    assert!(
        err.contains("permission denied") || err.contains("fs:read"),
        "unexpected error: {err}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_out_of_root_is_dropped_from_matches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("ok.txt"), "fine").unwrap();
    let outside = dir.path().join("secret.txt");
    std::fs::write(&outside, "s3cret").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("escape.txt")).unwrap();

    let perms = perms_read(&root);
    let hits = glob_paths(&perms, &format!("{}/*.txt", root.display())).await;
    assert!(hits.iter().any(|p| p.ends_with("ok.txt")), "{hits:?}");
    assert!(
        !hits.iter().any(|p| p.ends_with("escape.txt")),
        "a symlink to outside the root must be filtered out: {hits:?}"
    );
}

#[tokio::test]
async fn no_read_permission_denies_everything() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let perms = PermissionSet::default_secure();
    assert!(glob(&perms, &format!("{}/*", dir.path().display()))
        .await
        .is_err());
    assert!(glob(&perms, "*").await.is_err());
}

#[tokio::test]
async fn allow_all_globs_anywhere() {
    let perms = PermissionSet::allow_all();
    // Not asserting contents — only that the check does not reject.
    assert!(glob(&perms, "/etc/*").await.is_ok());
}
