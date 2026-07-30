use rite_doc::run_doctests;
use std::path::Path;

#[tokio::test]
async fn book_doctests_mostly_pass() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = run_doctests(&[&root.join("docs/book"), &root.join("docs/diagnostics")]).await;
    // Allow empty if no fences; fail only on hard failures
    for r in &report.results {
        if !r.ok {
            eprintln!(
                "doctest fail {}:{} {} — {}",
                r.file, r.line, r.mode, r.message
            );
        }
    }
    assert_eq!(report.failed, 0, "{} doctest failures", report.failed);
}

/// `rite docs agent --output skills/rite` reads its SKILL.md from that same path.
/// Regenerating in place must leave the hand-written file alone: a failed read falls
/// back to a stub, which would otherwise replace real content with a placeholder.
#[test]
fn regenerating_the_bundle_in_place_keeps_skill_md() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let skill = root.join("skills/rite/SKILL.md");
    if !skill.is_file() {
        return; // not a full checkout
    }
    let before = std::fs::read_to_string(&skill).expect("read SKILL.md");
    assert!(
        !before.trim().is_empty(),
        "SKILL.md is empty before the run"
    );

    // generate_agent_bundle resolves its source relative to the cwd.
    let prev = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&root).expect("chdir to root");
    let result = rite_doc::generate_agent_bundle(std::path::Path::new("skills/rite"));
    std::env::set_current_dir(prev).expect("restore cwd");
    result.expect("generate bundle");

    let after = std::fs::read_to_string(&skill).expect("read SKILL.md after");
    assert_eq!(before, after, "regenerating in place rewrote SKILL.md");
}
