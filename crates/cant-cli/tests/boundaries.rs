//! ADR 0001's central claim, checked instead of asserted in prose.
//!
//! **Rite crates never depend on Cant crates.** Deleting `crates/cant-*`,
//! `grammar/cant/`, `docs/cant/` and the four workspace member entries must
//! leave a Rite that builds and behaves identically. A `use cant_syntax::…`
//! anywhere under `crates/rite-*` would quietly end that, and it would be found
//! at the worst possible moment — when someone tried to remove Cant.
//!
//! Lives in `cant-cli` because it is the leaf of the Cant graph: a test crate
//! that no Cant crate depends on, so it can never be the thing that breaks the
//! rule it checks.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-cli has two ancestors")
        .to_path_buf()
}

fn crate_dirs(prefix: &str) -> Vec<PathBuf> {
    let crates = repo_root().join("crates");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&crates)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", crates.display()))
        .map(|e| e.expect("directory entry").path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == prefix.trim_end_matches('-') || n.starts_with(prefix))
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no crates matching `{prefix}*`");
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_rite_crate_manifest_names_a_cant_crate() {
    for dir in crate_dirs("rite") {
        let manifest = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            assert!(
                !line.starts_with("cant") && !line.contains("crates/cant"),
                "{} depends on a Cant crate: `{line}`\n\
                 ADR 0001: the dependency edge is one-way, cant-* -> rite-*",
                manifest.display()
            );
        }
    }
}

#[test]
fn no_rite_source_file_imports_a_cant_crate() {
    let mut offenders = Vec::new();
    for dir in crate_dirs("rite") {
        let mut files = Vec::new();
        rust_files(&dir, &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            if text.contains("cant_syntax") || text.contains("cant_sem") || text.contains("cant::")
            {
                offenders.push(file);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Rite sources referencing Cant: {offenders:?}\n\
         ADR 0001: the dependency edge is one-way, cant-* -> rite-*"
    );
}

/// The workspace lists all four Cant crates, so `cargo test --workspace` runs
/// their tests. A crate outside the members list is a crate CI does not check.
#[test]
fn every_cant_crate_is_a_workspace_member() {
    let root = repo_root().join("Cargo.toml");
    let text = std::fs::read_to_string(&root).expect("workspace Cargo.toml");
    for name in ["cant-syntax", "cant-sem", "cant", "cant-cli"] {
        assert!(
            text.contains(&format!("\"crates/{name}\"")),
            "crates/{name} is not a workspace member"
        );
    }
}

/// Cant does not add itself to Rite's grammar, dialect enum, or alias table.
#[test]
fn cant_does_not_appear_in_rites_language_surface() {
    let root = repo_root();
    for file in [
        "grammar/aliases.json",
        "grammar/rite.ebnf",
        "grammar/keywords.toml",
        "grammar/glyphs.toml",
    ] {
        let text = std::fs::read_to_string(root.join(file))
            .unwrap_or_else(|e| panic!("cannot read {file}: {e}"));
        assert!(
            !text.to_lowercase().contains("cant"),
            "{file} mentions Cant; Rite's language surface is unchanged by ADR 0001"
        );
    }
    let fmt = std::fs::read_to_string(root.join("crates/rite-fmt/src/lib.rs"))
        .expect("crates/rite-fmt/src/lib.rs");
    let dialect = fmt
        .split("pub enum Dialect {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("rite_fmt::Dialect");
    assert!(
        !dialect.contains("Cant"),
        "rite_fmt::Dialect gained a Cant variant; ADR 0001 prohibits it"
    );
}
