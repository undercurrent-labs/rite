use rite_test::{differential_source, run_conformance_suite};
use std::path::PathBuf;

#[tokio::test]
async fn conformance_suite_passes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let report = run_conformance_suite(&root).await.expect("run suite");
    for r in &report.results {
        if !r.passed {
            eprintln!("FAIL {}: {}", r.path, r.message);
        }
    }
    assert_eq!(report.failed, 0, "{} conformance failures", report.failed);
    assert!(report.passed > 0, "expected at least one case");
}

#[tokio::test]
async fn differential_basic() {
    differential_source("d.rite", "1 + 2 * 3")
        .await
        .expect("parity");
    differential_source("d.rite", "[1,2,3] → sum")
        .await
        .expect("parity");
    differential_source(
        "d.rite",
        r#"
◆ f(x) ⟦ ^ x + 1 ⟧
f(41)
"#,
    )
    .await
    .expect("parity");
}
