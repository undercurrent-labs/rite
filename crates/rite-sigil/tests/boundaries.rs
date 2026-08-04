//! The dependency direction is the architecture, so it is a test.
//!
//! Three ADRs reduce to statements about one file — `crates/rite-sigil/Cargo.toml` —
//! and a comment saying "do not add a runtime here" is not a mechanism. Every
//! one of these has a failure mode that would be discovered late and expensively:
//!
//! * a `rite-runtime` dependency means the renderer can execute a program, and
//!   the browser build acquires an evaluator (ADR 0003);
//! * a `cant-sem` dependency drags a parser into every build that only wanted to
//!   draw a JSON file, and the untrusted-input boundary stops being anywhere in
//!   particular (ADR 0006);
//! * a Rite crate depending on Sigil inverts the one-way edge that lets either
//!   be deleted without the other (ADR 0001's rule, extended).
//!
//! These read manifests and sources as text rather than inspecting a build
//! graph, on the same reasoning `crates/cant-cli/tests/boundaries.rs` gives: the
//! text is what a person edits, and a test that fails on the edit points at the
//! line that caused it.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/rite-sigil has two ancestors")
        .to_path_buf()
}

fn manifest(crate_name: &str) -> String {
    let path = repo_root()
        .join("crates")
        .join(crate_name)
        .join("Cargo.toml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The dependency table, without the comments — so a crate *named in a comment
/// explaining why it is absent* does not read as present.
fn dependency_lines(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .map(|line| line.split('#').next().unwrap_or("").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn declares(manifest: &str, dependency: &str) -> bool {
    dependency_lines(manifest).iter().any(|line| {
        line.starts_with(dependency) && line[dependency.len()..].starts_with([' ', '='])
    })
}

/// ADR 0003. A renderer that can execute is not a renderer.
#[test]
fn rite_sigil_cannot_execute_anything() {
    let m = manifest("rite-sigil");
    for forbidden in [
        "rite-runtime",
        "rite-caps",
        "rite-compiler",
        "rite-repl",
        "rite-lsp",
        "tokio",
        "axum",
        "hyper",
        "reqwest",
    ] {
        assert!(
            !declares(&m, forbidden),
            "rite-sigil declares `{forbidden}`; ADR 0003 — Sigil is a renderer, not a runtime"
        );
    }
}

/// ADR 0006. The renderer takes a normalized graph; Cant is adapted into it.
#[test]
fn rite_sigil_does_not_know_what_cant_is() {
    let m = manifest("rite-sigil");
    for forbidden in ["cant-syntax", "cant-sem", "cant", "cant-wasm"] {
        assert!(
            !declares(&m, forbidden),
            "rite-sigil declares `{forbidden}`; ADR 0006 — the adapter lives on the Cant side"
        );
    }
    // And the same rule from Rite's side: the renderer does not parse or resolve
    // a language either.
    for forbidden in ["rite-syntax", "rite-sem", "rite-fmt", "rite-analysis"] {
        assert!(
            !declares(&m, forbidden),
            "rite-sigil declares `{forbidden}`; it consumes a graph, not a program"
        );
    }
}

/// Not a single source file names a Cant type, whatever the manifest says. A
/// `serde` shape mirroring `CantProgram` would be an undeclared structural
/// dependency — the manifest would stay clean while the crate broke the first
/// time Cant changed its JSON.
#[test]
fn no_rite_sigil_source_mentions_cant() {
    let src = repo_root().join("crates/rite-sigil/src");
    // Assembled rather than written out, so this file does not itself contain
    // the strings it forbids — `crates/cant-cli/tests/boundaries.rs` scans every
    // `rite-*` source for exactly these, and a test whose needle trips its own
    // sibling test is a false positive nobody enjoys diagnosing.
    let needles = [
        concat!("cant", "_syntax"),
        concat!("cant", "_sem"),
        concat!("cant", "::"),
        concat!("Cant", "Program"),
    ];
    let mut offenders = Vec::new();
    visit(&src, &mut |path, text| {
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            if needles.iter().any(|needle| code.contains(needle)) {
                offenders.push(format!("{}:{}", path.display(), n + 1));
            }
        }
    });
    assert!(
        offenders.is_empty(),
        "rite-sigil source referencing Cant: {offenders:?}\n\
         ADR 0006: the renderer's input model is its own"
    );
}

/// ADR 0001's rule, extended to Sigil: Rite core crates do not depend on the
/// renderer. Deleting `crates/rite-sigil*` and `apps/sigil-web` must leave a
/// Rite that builds.
#[test]
fn no_rite_crate_depends_on_sigil() {
    let crates_dir = repo_root().join("crates");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&crates_dir).expect("crates/") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        // Sigil's own crates, and Cant's, which are allowed to know: the edge is
        // `cant-* -> rite-sigil`, never the reverse.
        if name.starts_with("rite-sigil") || name.starts_with("cant-") || name == "cant" {
            continue;
        }
        let path = entry.path().join("Cargo.toml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if declares(&text, "rite-sigil") {
            offenders.push(name);
        }
    }
    assert!(
        offenders.is_empty(),
        "Rite crates depending on Sigil: {offenders:?}\n\
         the edge is one-way; Rite must build with the renderer deleted"
    );
}

/// The crate is a workspace member, so `cargo test --workspace` runs its tests.
/// A crate outside the members list is a crate CI does not check.
#[test]
fn rite_sigil_is_a_workspace_member() {
    let text = std::fs::read_to_string(repo_root().join("Cargo.toml")).expect("workspace manifest");
    assert!(
        text.contains("\"crates/rite-sigil\""),
        "crates/rite-sigil is not a workspace member"
    );
}

/// ADR 0004: the normalized graph carries no geometry. Asserted against the
/// *type* here rather than only a serialized instance, because a field that is
/// `skip_serializing_if` empty would pass an instance check while still existing.
#[test]
fn the_graph_model_declares_no_coordinate_fields() {
    let text = std::fs::read_to_string(repo_root().join("crates/rite-sigil/src/graph.rs"))
        .expect("graph.rs");
    let mut offenders = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("").trim();
        if !code.starts_with("pub ") || !code.contains(':') {
            continue;
        }
        let field = code
            .trim_start_matches("pub ")
            .split(':')
            .next()
            .unwrap_or("")
            .trim();
        if matches!(
            field,
            "x" | "y"
                | "cx"
                | "cy"
                | "width"
                | "height"
                | "radius"
                | "angle"
                | "rotation"
                | "layout"
        ) {
            offenders.push(format!("graph.rs:{}: `{field}`", n + 1));
        }
    }
    assert!(
        offenders.is_empty(),
        "the normalized graph gained geometry: {offenders:?}\n\
         ADR 0004: coordinates live in the scene, computed from topology"
    );
}

fn visit(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                f(&path, &text);
            }
        }
    }
}
