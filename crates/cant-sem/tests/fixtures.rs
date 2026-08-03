//! `conformance/cant/graph/` — sources that parse but whose graph is wrong.
//!
//! Kept here rather than in `cant-syntax` because only this crate can run the
//! validator. The sibling test over there asserts each of these *parses*, so a
//! fixture cannot quietly become a parser test.

use cant_sem::analyze;
use cant_syntax::parse_source;
use rite_core::FileId;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-sem has two ancestors")
        .to_path_buf()
}

#[test]
fn every_graph_fixture_reports_the_code_it_expects() {
    let root = repo_root().join("conformance/cant/graph");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no graph fixtures");

    for case in cases {
        let source = std::fs::read_to_string(case.join("case.cant")).expect("case.cant");
        let expected = std::fs::read_to_string(case.join("expected.code"))
            .expect("expected.code")
            .trim()
            .to_string();

        let (parsed, _) = parse_source("case.cant", &source);
        let ast = parsed.program.expect("a graph fixture parses");
        let result = analyze(&ast, FileId(0), "case.cant", source.len());
        let got: Vec<String> = result
            .diagnostics
            .errors()
            .map(|d| d.code.to_string())
            .collect();
        assert_eq!(
            got.first().map(String::as_str),
            Some(expected.as_str()),
            "{} expected {expected}, got {got:?}",
            case.display()
        );
    }
}
