//! End-to-end: non-server example ladder scripts run successfully.

use std::path::PathBuf;
use std::process::Command;

fn rite_bin() -> PathBuf {
    // Prefer workspace target
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // tests/e2e is under workspace root when run as integration from a crate;
    // this file lives at repo tests/e2e — use CARGO_TARGET_DIR or relative.
    let candidates = [
        PathBuf::from("target/debug/rite"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/rite"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../target/debug/rite"),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    PathBuf::from("rite")
}

fn run_example(rel: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // When this test is under a crate, adjust; for workspace we place it in rite-test
    let _ = root;
    let path = PathBuf::from(rel);
    assert!(
        path.exists() || PathBuf::from("..").join(rel).exists() || PathBuf::from("../..").join(rel).exists(),
        "missing {}",
        rel
    );
    let file = if path.exists() {
        path
    } else if PathBuf::from("..").join(rel).exists() {
        PathBuf::from("..").join(rel)
    } else {
        PathBuf::from("../..").join(rel)
    };
    let out = Command::new(rite_bin())
        .args(["run", file.to_str().unwrap(), "--allow-all"])
        .output()
        .expect("spawn rite");
    assert!(
        out.status.success(),
        "{} failed: {}\n{}",
        rel,
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

// This test module is intended to live in rite-test crate
