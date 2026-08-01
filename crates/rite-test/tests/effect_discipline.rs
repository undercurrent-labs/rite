//! Every Rite file the repo ships must satisfy effect discipline.
//!
//! The rules are only worth having if the corpus obeys them: an example that
//! wraps a host call without declaring `◆!` is both a broken example and a sign
//! the rule needs revisiting. Cheaper to catch here than in a reader's editor.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rite_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rite_files(&path, out);
        } else if path.extension().map(|e| e == "rite").unwrap_or(false) {
            out.push(path);
        }
    }
}

/// Does this file sit next to an `expected.exit` declaring a non-zero status?
///
/// Only conformance cases carry one; an example or a test fixture has no sidecar
/// and is therefore always checked.
fn expects_failure(case: &Path) -> bool {
    let Some(dir) = case.parent() else {
        return false;
    };
    std::fs::read_to_string(dir.join("expected.exit"))
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .is_some_and(|code| code != 0)
}

#[test]
fn shipped_rite_files_satisfy_effect_discipline() {
    let root = workspace_root();
    let mut files = Vec::new();
    for dir in ["examples", "conformance", "tests"] {
        rite_files(&root.join(dir), &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "found no .rite files to check");

    let mut offenders = Vec::new();
    for path in &files {
        // A conformance case that declares a non-zero `expected.exit` is *meant* to
        // be rejected — a fixture proving E021 fires is the discipline working, not a
        // violation of it. This check always claimed `expected.exit` covered those
        // and never read it, so the first such fixture failed the sweep that exists
        // to protect the rule it demonstrates.
        if expects_failure(path) {
            continue;
        }
        let (_ir, diags, _sources) = rite_sem::compile_path(path);
        for d in diags.into_vec() {
            let text = format!("{d:?}");
            // Only effect-discipline failures.
            if text.contains("E021") || text.contains("not declared") {
                offenders.push(format!("{}: {text}", path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "effect discipline violations in shipped files:\n  {}",
        offenders.join("\n  ")
    );
}
