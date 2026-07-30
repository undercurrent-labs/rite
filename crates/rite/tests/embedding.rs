//! The public embedding API.
//!
//! `RiteEngine` is what `docs/book/embedding.md` tells Rust hosts to use, and it had no
//! tests at all — so nothing checked that the permission builder actually restricts what
//! an embedded script can do, which is the whole point of embedding a sandboxed language.

use rite::{caps::PermissionSet, RiteEngine};
use rite_runtime::Value;

fn engine_allow_all() -> RiteEngine {
    RiteEngine::builder().allow_all().build().expect("build")
}

#[tokio::test]
async fn runs_a_pure_expression_and_returns_its_value() {
    let out = engine_allow_all()
        .run_source("t.rite", "1 + 2 * 3")
        .await
        .expect("run");
    assert_eq!(out, Value::Int(7));
}

#[tokio::test]
async fn returns_the_final_expression_of_a_script() {
    let out = engine_allow_all()
        .run_source("t.rite", "◆ sq(n) ⟦ ^ n * n ⟧\nsq(12)\n")
        .await
        .expect("run");
    assert_eq!(out, Value::Int(144));
}

#[tokio::test]
async fn ascii_dialect_runs_identically() {
    let glyph = engine_allow_all()
        .run_source("g.rite", "◆ f(n) ⟦ ^ n + 1 ⟧\nf(1)\n")
        .await
        .expect("glyph");
    let ascii = engine_allow_all()
        .run_source("a.rite", "def f(n) [[ return n + 1 ]]\nf(1)\n")
        .await
        .expect("ascii");
    assert_eq!(glyph, ascii);
}

/// The default engine is sandboxed. An embedder that forgets to grant anything must not
/// hand the script the filesystem.
#[tokio::test]
async fn the_default_engine_denies_the_filesystem() {
    let engine = RiteEngine::builder().build().expect("build");
    let err = engine
        .run_source("t.rite", "! @fs.read(\"/etc/passwd\")?\n")
        .await
        .expect_err("default must deny fs");
    assert!(
        err.to_string().to_lowercase().contains("permission"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn a_scoped_grant_allows_only_its_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let inside = dir.path().join("ok.txt");
    std::fs::write(&inside, "hello").expect("write");

    let engine = RiteEngine::builder()
        .allow(&format!("fs:read={}", dir.path().display()))
        .expect("grant")
        .build()
        .expect("build");

    let got = engine
        .run_source(
            "t.rite",
            &format!("! @fs.read(\"{}\")?\n", inside.display()),
        )
        .await
        .expect("read inside the granted root");
    assert_eq!(got, Value::string("hello"));

    // Outside the root, still denied.
    let err = engine
        .run_source("t.rite", "! @fs.read(\"/etc/hostname\")?\n")
        .await
        .expect_err("outside the root must be denied");
    assert!(
        err.to_string().to_lowercase().contains("permission"),
        "{err}"
    );
}

#[tokio::test]
async fn an_explicit_permission_set_is_honoured() {
    let engine = RiteEngine::builder()
        .with_permissions(PermissionSet::default_secure())
        .build()
        .expect("build");
    // Console is allowed under the default-secure policy.
    engine
        .run_source("t.rite", "! @console.println(\"ok\")\n")
        .await
        .expect("console allowed");
    // Process is not.
    let err = engine
        .run_source("t.rite", "! @process.run(\"echo\", [\"x\"])?\n")
        .await
        .expect_err("process must be denied");
    assert!(
        err.to_string().to_lowercase().contains("permission"),
        "{err}"
    );
}

#[tokio::test]
async fn a_budget_stops_a_runaway_script() {
    let engine = RiteEngine::builder()
        .allow_all()
        .with_budget(rite_runtime::ExecutionBudget::new().with_max_steps(50))
        .build()
        .expect("build");
    let err = engine
        .run_source(
            "t.rite",
            "n ↢ 0\n(1..100000) → each { |i| n := n + i }\nn\n",
        )
        .await
        .expect_err("the budget must stop this");
    assert!(
        err.to_string().contains("budget"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn run_path_reads_a_file_from_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("s.rite");
    std::fs::write(&script, "6 * 7\n").expect("write");
    let out = engine_allow_all()
        .run_path(&script)
        .await
        .expect("run_path");
    assert_eq!(out, Value::Int(42));
}

#[tokio::test]
async fn run_path_reports_a_missing_file_rather_than_panicking() {
    let err = engine_allow_all()
        .run_path("/nonexistent/definitely-not-here.rite")
        .await
        .expect_err("missing file");
    assert!(!err.to_string().is_empty());
}

#[test]
fn check_source_reports_diagnostics_without_running() {
    let engine = RiteEngine::builder().build().expect("build");
    assert!(!engine.check_source("ok.rite", "1 + 1\n").has_errors());
    let bad = engine.check_source("bad.rite", "x ← undefined_name\n");
    assert!(bad.has_errors(), "expected an undefined-name error");
}

/// `check_source` must not execute — an embedder uses it to validate untrusted input.
#[tokio::test]
async fn check_source_does_not_execute_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let marker = dir.path().join("written.txt");
    let engine = RiteEngine::builder().allow_all().build().expect("build");
    let src = format!("! @fs.write(\"{}\", \"x\")?\n", marker.display());
    let _ = engine.check_source("t.rite", &src);
    assert!(!marker.exists(), "check_source executed the script");
}

#[test]
fn compile_ir_succeeds_and_serialises() {
    let engine = RiteEngine::builder().build().expect("build");
    let ir = engine
        .compile_ir("t.rite", "◆ f() ⟦ ^ 1 ⟧\nf()\n")
        .expect("compile");
    let json = rite::ir_json(&ir);
    assert!(json.is_object(), "ir_json should be an object");
    assert!(
        engine.compile_ir("t.rite", "x ← undefined_name\n").is_err(),
        "a resolve error must fail compile_ir"
    );
}

#[test]
fn parse_returns_a_program_or_diagnostics() {
    let engine = RiteEngine::builder().build().expect("build");
    let p = engine.parse("t.rite", "◆ f() ⟦ ^ 1 ⟧\n").expect("parse");
    assert!(!p.items.is_empty());
    assert!(
        engine.parse("t.rite", "◆ f( ⟦\n").is_err(),
        "unclosed params"
    );
}

#[test]
fn format_source_round_trips_both_dialects() {
    let src = "// keep me\n◆ f(n) ⟦\n  ^ n\n⟧\n";
    let glyph = rite::format_source(src, false).expect("glyph");
    assert!(glyph.contains("// keep me"), "comment lost: {glyph}");
    assert!(glyph.contains('◆'), "{glyph}");
    let ascii = rite::format_source(src, true).expect("ascii");
    assert!(ascii.contains("def "), "{ascii}");
    assert!(ascii.contains("// keep me"), "comment lost: {ascii}");
}

#[tokio::test]
async fn run_file_allow_all_is_a_working_shortcut() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("s.rite");
    std::fs::write(&script, "! @console.println(\"hi\")\n21 * 2\n").expect("write");
    let out = rite::run_file_allow_all(&script).await.expect("run");
    assert_eq!(out, Value::Int(42));
}
