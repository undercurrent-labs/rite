//! End-to-end: non-blocking example ladder scripts.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rite_bin() -> PathBuf {
    let root = workspace_root();
    let debug = root.join("target/debug/rite");
    if debug.exists() {
        debug
    } else {
        PathBuf::from("rite")
    }
}

fn run_ok(rel: &str) {
    let file = workspace_root().join(rel);
    assert!(file.exists(), "missing {}", file.display());
    let out = Command::new(rite_bin())
        .args(["run", file.to_str().unwrap(), "--allow-all"])
        .current_dir(workspace_root())
        .output()
        .expect("spawn rite");
    assert!(
        out.status.success(),
        "{} failed (status {:?}):\nstderr: {}\nstdout: {}",
        rel,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn ladder_values() {
    run_ok("examples/01-values/main.rite");
}
#[test]
fn ladder_pipelines() {
    run_ok("examples/02-pipelines/main.rite");
}
#[test]
fn ladder_json() {
    run_ok("examples/03-files-and-json/main.rite");
}
#[test]
fn ladder_match() {
    run_ok("examples/04-pattern-matching/main.rite");
}
#[test]
fn ladder_capabilities() {
    run_ok("examples/05-capabilities/main.rite");
}
#[test]
fn ladder_cli() {
    run_ok("examples/06-cli-tool/main.rite");
}
#[test]
fn ladder_modules() {
    run_ok("examples/modules/main.rite");
}
#[test]
fn ladder_hello() {
    run_ok("examples/hello/hello.rite");
}
#[test]
fn ladder_embedded() {
    run_ok("examples/10-embedded-rust/main.rite");
}

#[test]
fn convert_roundtrip_cli() {
    let root = workspace_root();
    let file = root.join("examples/hello/hello.ascii.rite");
    let out = Command::new(rite_bin())
        .args([
            "convert",
            file.to_str().unwrap(),
            "--to",
            "glyph",
            "--stdout",
        ])
        .current_dir(&root)
        .output()
        .expect("convert");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains('←') || text.contains('◆') || text.contains("Aura"));
}

// silence
#[allow(dead_code)]
fn _p(_: &Path) {}
