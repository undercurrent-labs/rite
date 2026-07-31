//! `@fs.metadata` reports size, kind, link-ness and modification time.
//!
//! `mtime` and `is_symlink` were added after the record shipped with only
//! `len`/`is_file`/`is_dir`, which made "which files changed since Tuesday" —
//! the most common reason to call `metadata` at all — inexpressible.

use rite_caps::fs::FsCap;
use rite_caps::{Permission, PermissionSet};
use rite_runtime::{AtomInterner, Key, ResultValue, Value};
use std::path::Path;

async fn metadata(perms: &PermissionSet, path: &Path) -> Vec<(Key, Value)> {
    match FsCap
        .call(
            "metadata",
            vec![Value::string(path.display().to_string())],
            perms,
            &AtomInterner::new(),
        )
        .await
    {
        Ok(Value::Result(ResultValue::Ok(inner))) => match *inner {
            Value::Record(fields) => fields.into_iter().collect(),
            other => panic!("expected ok(record), got ok({other})"),
        },
        Ok(other) => panic!("expected ok(record), got {other}"),
        Err(e) => panic!("metadata({}) failed: {e}", path.display()),
    }
}

/// Raw call, for the cases that are supposed to fail.
async fn metadata_raw(perms: &PermissionSet, path: &Path) -> Result<Value, String> {
    FsCap
        .call(
            "metadata",
            vec![Value::string(path.display().to_string())],
            perms,
            &AtomInterner::new(),
        )
        .await
        .map_err(|e| e.to_string())
}

fn field(fields: &[(Key, Value)], name: &str) -> Value {
    fields
        .iter()
        .find(|(k, _)| matches!(k, Key::String(s) if s == name))
        .unwrap_or_else(|| panic!("no `{name}` field in {fields:?}"))
        .1
        .clone()
}

fn perms_read(root: &Path) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    p.grant(Permission::FsRead(root.to_path_buf()));
    p
}

#[tokio::test]
async fn reports_size_kind_and_link_for_a_plain_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let m = metadata(&perms_read(dir.path()), &file).await;
    assert_eq!(field(&m, "len"), Value::Int(6));
    assert_eq!(field(&m, "is_file"), Value::Bool(true));
    assert_eq!(field(&m, "is_dir"), Value::Bool(false));
    assert_eq!(field(&m, "is_symlink"), Value::Bool(false));
}

#[tokio::test]
async fn directories_report_is_dir() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();

    let m = metadata(&perms_read(dir.path()), &sub).await;
    assert_eq!(field(&m, "is_dir"), Value::Bool(true));
    assert_eq!(field(&m, "is_file"), Value::Bool(false));
    assert_eq!(field(&m, "is_symlink"), Value::Bool(false));
}

/// The whole point of the field: `metadata` follows links, so every *other*
/// field describes the target. Before `is_symlink` a script could not tell a
/// link from the file it points at.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_is_flagged_while_the_rest_describes_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, "0123456789").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let m = metadata(&perms_read(dir.path()), &link).await;
    assert_eq!(field(&m, "is_symlink"), Value::Bool(true));
    // Target's size and kind, not the link's — matching `ls -l`.
    assert_eq!(field(&m, "len"), Value::Int(10));
    assert_eq!(field(&m, "is_file"), Value::Bool(true));

    // And the target itself is not flagged, so the field distinguishes the two
    // rather than being true for anything in the neighbourhood.
    let direct = metadata(&perms_read(dir.path()), &target).await;
    assert_eq!(field(&direct, "is_symlink"), Value::Bool(false));
}

/// Documented hole: `metadata` follows the link, so a broken one fails to stat
/// and errors before `is_symlink` can report it. Pinned so the behaviour is a
/// decision rather than a surprise.
#[cfg(unix)]
#[tokio::test]
async fn a_broken_symlink_errors_rather_than_reporting_is_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("dangling.txt");
    std::os::unix::fs::symlink(dir.path().join("nowhere.txt"), &link).unwrap();

    match metadata_raw(&perms_read(dir.path()), &link).await {
        Ok(Value::Result(ResultValue::Err(inner))) => {
            let s = format!("{inner}");
            assert!(s.contains("io.not_found"), "expected not_found, got {s}");
        }
        other => panic!("expected err(...), got {other:?}"),
    }
}

/// `mtime` has to be comparable against `@clock.now`, which is the only reason
/// it is a string. Both render through `to_rfc3339`, and RFC3339 in UTC orders
/// lexicographically — so a file written now must not sort after "now".
#[tokio::test]
async fn mtime_is_rfc3339_and_orders_against_clock_now() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.txt");

    let before = chrono::Utc::now().to_rfc3339();
    std::fs::write(&file, "x").unwrap();
    let m = metadata(&perms_read(dir.path()), &file).await;
    let after = chrono::Utc::now().to_rfc3339();

    let mtime = match field(&m, "mtime") {
        Value::String(s) => s.to_string(),
        other => panic!("expected an mtime string, got {other}"),
    };

    // Parses as the same format `@clock.parse` accepts.
    chrono::DateTime::parse_from_rfc3339(&mtime)
        .unwrap_or_else(|e| panic!("mtime {mtime:?} is not RFC3339: {e}"));

    // Plain string comparison — what a Rite script has — brackets correctly.
    // Filesystem timestamp granularity can be coarse (HFS+ and some network
    // mounts round to a second), so the lower bound is given a second of slack
    // rather than asserting a strictness the filesystem never promised.
    let floor = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    assert!(
        mtime.as_str() >= floor.as_str(),
        "mtime {mtime} sorted before {floor} (from {before})"
    );
    assert!(
        mtime.as_str() <= after.as_str(),
        "mtime {mtime} sorted after {after}"
    );
}

/// An older file must sort below a newer one — the comparison a "changed since"
/// script actually performs.
#[tokio::test]
async fn an_older_file_sorts_below_a_newer_one() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.txt");
    let new = dir.path().join("new.txt");

    std::fs::write(&old, "old").unwrap();
    // Well clear of any filesystem's timestamp granularity — a millisecond gap
    // would pass on ext4 and fail on a filesystem that rounds to the second.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&new, "new").unwrap();

    let perms = perms_read(dir.path());
    let a = metadata(&perms, &old).await;
    let b = metadata(&perms, &new).await;

    let (a, b) = match (field(&a, "mtime"), field(&b, "mtime")) {
        (Value::String(a), Value::String(b)) => (a.to_string(), b.to_string()),
        other => panic!("expected two mtime strings, got {other:?}"),
    };
    assert!(a.as_str() < b.as_str(), "{a} should sort below {b}");
}

/// Metadata is a read, and reads are permission-checked.
#[tokio::test]
async fn metadata_outside_the_granted_root_is_denied() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let file = outside.path().join("secret.txt");
    std::fs::write(&file, "secret").unwrap();

    let err = metadata_raw(&perms_read(dir.path()), &file)
        .await
        .expect_err("metadata outside the granted root should be denied");
    assert!(
        err.contains("permission") || err.contains("denied"),
        "{err}"
    );
}
