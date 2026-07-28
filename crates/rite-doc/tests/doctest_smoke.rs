use rite_doc::run_doctests;
use std::path::Path;

#[tokio::test]
async fn book_doctests_mostly_pass() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = run_doctests(&[
        &root.join("docs/book"),
        &root.join("docs/diagnostics"),
    ])
    .await;
    // Allow empty if no fences; fail only on hard failures
    for r in &report.results {
        if !r.ok {
            eprintln!("doctest fail {}:{} {} — {}", r.file, r.line, r.mode, r.message);
        }
    }
    assert_eq!(report.failed, 0, "{} doctest failures", report.failed);
}
