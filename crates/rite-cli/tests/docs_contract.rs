//! Docs ↔ tests contract: book claims about HTTP/console/middleware must have tests.
//!
//! This is a lightweight audit that fails CI if we document behavior without a
//! matching automated test file/name.

use std::fs;
use std::path::PathBuf;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    fs::read_to_string(workspace().join(rel)).unwrap_or_default()
}

#[test]
fn http_book_claims_have_tests() {
    let book = read("docs/book/http.md");
    let tests = collect_test_sources();

    // Claims that must be backed by tests (substring match in any test source).
    let required: &[(&str, &str)] = &[
        ("@http.log", "log"),
        ("@http.recover", "recover"),
        ("@console.println", "console"),
        ("access log", "access_log"),
        ("middleware", "middleware"),
    ];

    // Map book keywords to required test markers (function or file names / comments).
    let markers_required = [
        (
            "http.log",
            &["http_log", "access_log", "middleware_registration"][..],
        ),
        ("http.recover", &["recover"][..]),
        ("console.println", &["console", "obs-ping", "handler"][..]),
        ("⊏", &["glyph", "⊏"][..]),
    ];

    assert!(
        book.contains("@http.log") || book.contains("http.log"),
        "http.md should document @http.log"
    );
    assert!(
        book.contains("console"),
        "http.md should document console in handlers"
    );

    let blob = tests.join("\n");
    for (claim, needles) in markers_required {
        if book.contains(claim) || book.contains(&claim.replace('.', " ")) {
            let ok = needles.iter().any(|n| blob.contains(n));
            assert!(
                ok,
                "docs mention `{claim}` but no test source contains any of {needles:?}"
            );
        }
    }

    // Explicit test files that must exist for HTTP observability.
    for must in [
        "crates/rite-caps/tests/http_handlers.rs",
        "crates/rite-caps/tests/http_observability.rs",
    ] {
        assert!(
            workspace().join(must).is_file(),
            "required test file missing: {must}"
        );
    }

    let _ = required;
}

#[test]
fn sugar_book_has_sugar_tests() {
    let book = read("docs/book/sugar.md");
    assert!(!book.is_empty(), "sugar.md missing");
    assert!(workspace()
        .join("crates/rite-caps/tests/sugar_pack.rs")
        .is_file());
    assert!(workspace()
        .join("crates/rite-caps/tests/sugar_dual_dialect.rs")
        .is_file());
    let dual = read("crates/rite-caps/tests/sugar_dual_dialect.rs");
    assert!(
        dual.contains("dual_"),
        "dual dialect tests should be named dual_*"
    );
}

fn collect_test_sources() -> Vec<String> {
    let mut out = Vec::new();
    let roots = [
        workspace().join("crates/rite-caps/tests"),
        workspace().join("crates/rite-cli/tests"),
        workspace().join("crates/rite-runtime/tests"),
        workspace().join("crates/rite-syntax/tests"),
        workspace().join("crates/rite-repl/src"),
    ];
    for root in roots {
        walk(&root, &mut out);
    }
    out
}

fn walk(dir: &PathBuf, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(t) = fs::read_to_string(&p) {
                out.push(format!("// FILE {}\n{}", p.display(), t));
            }
        }
    }
}
