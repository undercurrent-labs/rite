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

/// The shipped `SKILL.md` must be whole.
///
/// This is the check that was missing. Its neighbour below asserts the file is *unchanged*
/// by a regeneration, which a file that was already truncated passes happily — and that is
/// how twelve commits shipped a `SKILL.md` cut off mid-table at exactly 4096 bytes with a
/// green suite. Invariance is not integrity.
#[test]
fn the_shipped_skill_file_is_not_truncated() {
    let Some(text) = read_tracked_skill() else {
        return; // not a full checkout
    };
    assert!(
        text.ends_with('\n'),
        "SKILL.md does not end with a newline, so it was cut short"
    );
    // The last section, and a line from it: a truncation loses the tail first.
    for marker in ["## When stuck", "machine/diagnostics.json"] {
        assert!(
            text.contains(marker),
            "SKILL.md is missing {marker:?} — truncated? ({} bytes)",
            text.len()
        );
    }
    // A page-boundary size with an unterminated table row is the exact signature.
    let last = text.lines().last().unwrap_or_default();
    assert!(
        !last.starts_with('|') || last.ends_with('|'),
        "SKILL.md ends mid-table row: {last:?}"
    );
}

/// `rite docs agent --output skills/rite` reads its SKILL.md from that same path.
/// Regenerating in place must leave the hand-written file alone: a failed read falls
/// back to a stub, which would otherwise replace real content with a placeholder.
///
/// Runs against a **copy**. Pointing it at the tracked tree meant a test mutated a
/// committed file on every `cargo test`, and this test also `chdir`s — which is
/// process-global, so it raced the doctest above for the cwd. Whether that is what
/// truncated the shipped file could not be reproduced on demand; either way a test has no
/// business writing into the working tree, and on a copy it cannot.
#[test]
fn regenerating_the_bundle_in_place_keeps_skill_md() {
    let Some(before) = read_tracked_skill() else {
        return;
    };
    let stage = tempfile::tempdir().expect("tempdir");
    let bundle = stage.path().join("skills/rite");
    std::fs::create_dir_all(&bundle).expect("create bundle dir");
    std::fs::write(bundle.join("SKILL.md"), &before).expect("seed the copy");
    // Inputs are anchored to the root argument; a missing one is an error, so
    // the grammar files are staged too.
    std::fs::create_dir_all(stage.path().join("grammar")).expect("grammar dir");
    std::fs::write(stage.path().join("grammar/aliases.json"), "{}").expect("aliases");
    std::fs::write(stage.path().join("grammar/rite.ebnf"), "(* test *)\n").expect("ebnf");

    rite_doc::generate_agent_bundle(stage.path(), &bundle).expect("generate bundle");

    let after = std::fs::read_to_string(bundle.join("SKILL.md")).expect("read after");
    assert_eq!(before, after, "regenerating in place rewrote SKILL.md");
    assert!(
        bundle.join("machine/capabilities.json").is_file(),
        "the rest of the bundle was still generated"
    );
}

/// The tracked `SKILL.md`, or `None` outside a full checkout.
fn read_tracked_skill() -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/rite/SKILL.md");
    std::fs::read_to_string(path)
        .ok()
        .filter(|t| !t.trim().is_empty())
}
