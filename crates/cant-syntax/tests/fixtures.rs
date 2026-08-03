//! Walks `conformance/cant/` and `examples/cant/`.
//!
//! A fixture that cannot be read is a failure, not a skip — the same rule Rite's
//! conformance runner keeps, and for the same reason: a fixture that silently
//! disappears is worse than no fixture, because the suite still reports green.

use cant_syntax::{parse_source, structure};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-syntax has two ancestors")
        .to_path_buf()
}

/// Immediate subdirectories of `dir`, sorted, so failures are reported in a
/// stable order however the filesystem feels about it.
fn cases(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no fixtures under {}", dir.display());
    out
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[test]
fn every_syntax_fixture_parses_cleanly() {
    for case in cases(&repo_root().join("conformance/cant/syntax")) {
        let path = case.join("case.cant");
        let source = read(&path);
        let (result, sources) = parse_source(&path.display().to_string(), &source);
        assert!(
            !result.has_errors(),
            "{} should parse:\n{}",
            case.display(),
            result.diagnostics.render_all(&sources)
        );
        assert!(
            result.program.is_some(),
            "{} produced no program",
            case.display()
        );
    }
}

#[test]
fn every_dialect_pair_produces_the_same_program() {
    for case in cases(&repo_root().join("conformance/cant/dialect")) {
        let ascii_path = case.join("ascii.cant");
        let glyph_path = case.join("glyph.cant");
        let ascii = read(&ascii_path);
        let glyph = read(&glyph_path);

        let (a, a_sources) = parse_source(&ascii_path.display().to_string(), &ascii);
        let (g, g_sources) = parse_source(&glyph_path.display().to_string(), &glyph);
        assert!(
            !a.has_errors(),
            "{}:\n{}",
            ascii_path.display(),
            a.diagnostics.render_all(&a_sources)
        );
        assert!(
            !g.has_errors(),
            "{}:\n{}",
            glyph_path.display(),
            g.diagnostics.render_all(&g_sources)
        );
        assert_eq!(
            structure(&a.program.expect("ascii program")),
            structure(&g.program.expect("glyph program")),
            "{} — the two spellings are not the same program",
            case.display()
        );
    }
}

/// A graph fixture must be syntactically fine — otherwise it is testing the
/// parser, not the validator, and would keep passing if validation were deleted.
#[test]
fn every_execution_fixture_parses_cleanly() {
    for case in cases(&repo_root().join("conformance/cant/execution")) {
        let path = case.join("case.cant");
        let source = read(&path);
        let (result, sources) = parse_source(&path.display().to_string(), &source);
        assert!(
            !result.has_errors(),
            "{} must parse:\n{}",
            case.display(),
            result.diagnostics.render_all(&sources)
        );
        assert!(
            case.join("expected.exit").is_file(),
            "{} has no expected.exit",
            case.display()
        );
    }
}

#[test]
fn every_lowering_fixture_parses_cleanly() {
    for case in cases(&repo_root().join("conformance/cant/lowering")) {
        let path = case.join("case.cant");
        let source = read(&path);
        let (result, sources) = parse_source(&path.display().to_string(), &source);
        assert!(
            !result.has_errors(),
            "{} must parse:\n{}",
            case.display(),
            result.diagnostics.render_all(&sources)
        );
        assert!(
            case.join("expected.rite").is_file(),
            "{} has no golden expansion",
            case.display()
        );
    }
}

#[test]
fn every_graph_fixture_parses_cleanly() {
    for case in cases(&repo_root().join("conformance/cant/graph")) {
        let path = case.join("case.cant");
        let source = read(&path);
        let (result, sources) = parse_source(&path.display().to_string(), &source);
        assert!(
            !result.has_errors(),
            "{} must parse — a graph fixture tests validation, not parsing:\n{}",
            case.display(),
            result.diagnostics.render_all(&sources)
        );
    }
}

#[test]
fn every_diagnostic_fixture_reports_the_code_it_expects() {
    for case in cases(&repo_root().join("conformance/cant/diagnostics")) {
        let path = case.join("case.cant");
        let source = read(&path);
        let expected = read(&case.join("expected.code")).trim().to_string();

        let (result, _) = parse_source(&path.display().to_string(), &source);
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

#[test]
fn every_example_parses_cleanly() {
    for case in cases(&repo_root().join("examples/cant")) {
        let path = case.join("main.cant");
        let source = read(&path);
        let (result, sources) = parse_source(&path.display().to_string(), &source);
        assert!(
            !result.has_errors(),
            "{} should parse:\n{}",
            case.display(),
            result.diagnostics.render_all(&sources)
        );
        assert!(
            case.join("README.md").is_file(),
            "{} has no README explaining it",
            case.display()
        );
    }
}

/// A fixture directory nobody walks is a fixture nobody runs.
///
/// The Rite tree has been bitten by generation writing somewhere no test looked;
/// this catches the fixture equivalent, where a new category is added under
/// `conformance/cant/` and quietly never runs.
#[test]
fn every_fixture_directory_is_reachable() {
    // `lowering` and `execution` are empty until Phases 4 and 5 and are listed
    // here so that filling them without wiring a runner fails loudly.
    // Nothing is pending any more: every category has a runner.
    let pending: [&str; 0] = [];
    // `graph` is consumed by `cant-sem/tests/fixtures.rs`, `lowering` by
    // `cant/tests/expand.rs`, and `execution` by
    // `cant-cli/tests/differential.rs` — all needing crates this one cannot
    // depend on. They are parsed here anyway, so a fixture cannot become a
    // parser test by accident.
    let walked = [
        "syntax",
        "dialect",
        "diagnostics",
        "graph",
        "lowering",
        "execution",
    ];

    let root = repo_root().join("conformance/cant");
    let mut found: Vec<String> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .map(|e| e.expect("directory entry"))
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();

    for name in &found {
        assert!(
            walked.contains(&name.as_str()) || pending.contains(&name.as_str()),
            "conformance/cant/{name} is walked by no test — wire it up or remove it"
        );
    }
    for name in walked {
        assert!(
            found.contains(&name.to_string()),
            "conformance/cant/{name} is walked by a test but does not exist"
        );
    }
    for name in pending {
        let dir = root.join(name);
        let has_cases = std::fs::read_dir(&dir)
            .map(|entries| entries.filter_map(Result::ok).any(|e| e.path().is_dir()))
            .unwrap_or(false);
        assert!(
            !has_cases,
            "conformance/cant/{name} has fixtures but no runner — add one in the phase that fills it"
        );
    }
}
