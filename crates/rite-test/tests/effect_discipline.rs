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
        let (_ir, diags, _sources) = rite_sem::compile_path(path);
        for d in diags.into_vec() {
            let text = format!("{d:?}");
            // Only effect-discipline failures; a fixture may fail to compile on
            // purpose (`expected.exit` covers those).
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
