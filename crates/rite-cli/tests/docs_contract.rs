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

/// The tutorial list exists twice — `TUTORIALS` in the site registry drives the
/// index cards and the sidebar, `docs/tutorials/README.md` is what a reader sees
/// on GitHub — and nothing but this test makes them agree.
///
/// The book already demonstrates the failure: its chapter order lives in both
/// `DOC_CHAPTERS` and `docs/book/README.md`, they drifted, and the site showed two
/// different numberings on one screen. Tutorials get the guard the book never had.
#[test]
fn the_tutorial_list_matches_the_readme() {
    let registry = read("apps/rite-web/src/lib/tutorials.ts");
    let readme = read("docs/tutorials/README.md");
    assert!(
        !registry.is_empty() && !readme.is_empty(),
        "tutorial registry or README missing"
    );

    // `slug: "json-pipeline"` from the registry, in declaration order.
    let slugs: Vec<String> = registry
        .lines()
        .filter_map(|l| l.trim().strip_prefix("slug: \"")?.split('"').next())
        .map(str::to_string)
        .collect();
    assert!(!slugs.is_empty(), "no slugs parsed from the registry");

    // `[Title](json-pipeline.md)` from the README table, in row order.
    let linked: Vec<String> = readme
        .lines()
        .filter(|l| l.starts_with('|'))
        .filter_map(|l| {
            let start = l.find("](")? + 2;
            let rest = &l[start..];
            Some(rest.split(".md").next()?.to_string())
        })
        .collect();

    assert_eq!(
        slugs, linked,
        "docs/tutorials/README.md and TUTORIALS disagree — same order, same slugs"
    );

    // Every listed tutorial must actually exist, or the site links to a 404.
    for slug in &slugs {
        let path = workspace().join(format!("docs/tutorials/{slug}.md"));
        assert!(path.is_file(), "missing tutorial file {}", path.display());
    }
}

/// The book's chapter order lives in two places for the same reason the tutorial
/// list does: `DOC_CHAPTERS` renders the sidebar, `docs/book/README.md` renders the
/// `/docs` index. They have drifted before — the index said chapter 11 while the
/// sidebar said 16, on one screen — and the fix was always "check it by eye", which
/// is what let it drift again. This is that check, mechanised.
#[test]
fn the_chapter_list_matches_the_readme() {
    let registry = read("apps/rite-web/src/lib/docs.ts");
    let readme = read("docs/book/README.md");

    // Only the DOC_CHAPTERS array: REFERENCE_PAGES below it has the same shape.
    let body = registry
        .split_once("DOC_CHAPTERS: DocChapter[] = [")
        .expect("DOC_CHAPTERS not found")
        .1
        .split_once("];")
        .expect("unterminated DOC_CHAPTERS")
        .0;
    let slugs: Vec<String> = body
        .lines()
        .filter_map(|l| l.trim().strip_prefix("{ slug: \"")?.split('"').next())
        .map(str::to_string)
        .collect();
    assert!(!slugs.is_empty(), "no chapters parsed from DOC_CHAPTERS");

    // `12. [Effects and capabilities](effects.md) — …` — the numbered list only, so
    // the capability index and the API-reference links below it are not picked up.
    let linked: Vec<String> = readme
        .lines()
        .filter(|l| {
            l.split_once(". [")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|l| {
            let start = l.find("](")? + 2;
            Some(l[start..].split(".md").next()?.to_string())
        })
        .collect();

    assert_eq!(
        slugs, linked,
        "docs/book/README.md and DOC_CHAPTERS disagree — same order, same slugs"
    );

    for slug in &slugs {
        let path = workspace().join(format!("docs/book/{slug}.md"));
        assert!(path.is_file(), "missing chapter file {}", path.display());
    }
}
