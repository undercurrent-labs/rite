//! Packaging gates: skill archives + VS Code VSIX.
//!
//! These catch CI-only failures (relative OUT paths, broken npm lockfiles)
//! before a slow Release matrix.
//!
//! Skill packaging always runs. VSIX packaging runs when `node`/`npm` are
//! available (set `RITE_SKIP_VSIX=1` to skip).

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_ok(cmd: &mut Command) {
    let out = cmd.output().expect("spawn");
    if !out.status.success() {
        panic!(
            "command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
            cmd,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn package_skill_with_relative_out_path() {
    // Relative OUT is what broke Release: zip ran after cd into stage/.
    let root = workspace();
    let out = root.join("target/packaging-test/skill-rel");
    let _ = std::fs::remove_dir_all(&out);

    run_ok(
        Command::new("bash")
            .arg("scripts/package-skill.sh")
            .arg("target/packaging-test/skill-rel")
            .current_dir(&root),
    );

    assert!(
        out.join("rite-agent-skill.tar.gz").is_file(),
        "missing tar.gz in {}",
        out.display()
    );
    assert!(
        out.join("rite-agent-skill.zip").is_file(),
        "missing zip in {}",
        out.display()
    );
    assert!(
        out.join("SHA256SUMS").is_file(),
        "missing SHA256SUMS in {}",
        out.display()
    );

    // Tar must contain rite/SKILL.md
    let tar_list = Command::new("tar")
        .args(["-tzf"])
        .arg(out.join("rite-agent-skill.tar.gz"))
        .output()
        .expect("tar -tzf");
    let listing = String::from_utf8_lossy(&tar_list.stdout);
    assert!(
        listing.lines().any(|l| l.ends_with("SKILL.md")),
        "SKILL.md not in tar:\n{listing}"
    );
}

#[test]
fn package_vsix_clean() {
    if std::env::var_os("RITE_SKIP_VSIX").is_some() {
        eprintln!("skipping vsix packaging (RITE_SKIP_VSIX set)");
        return;
    }
    if Command::new("node").arg("--version").output().is_err()
        || Command::new("npm").arg("--version").output().is_err()
    {
        eprintln!("skipping vsix packaging (node/npm not available)");
        return;
    }

    let root = workspace();
    let out = root.join("target/packaging-test/rite.vsix");
    let _ = std::fs::remove_file(&out);

    run_ok(
        Command::new("bash")
            .arg("scripts/package-vsix.sh")
            .arg("target/packaging-test/rite.vsix")
            .current_dir(&root),
    );

    assert!(out.is_file(), "vsix missing at {}", out.display());
    let meta = std::fs::metadata(&out).unwrap();
    assert!(
        meta.len() > 1000,
        "vsix too small: {} bytes",
        meta.len()
    );
}

#[test]
fn check_packaging_script_skill_section() {
    // Ensure the gate script exists and package-skill is invocable as CI does.
    let root = workspace();
    assert!(root.join("scripts/check-packaging.sh").is_file());
    assert!(root.join("scripts/package-skill.sh").is_file());
    assert!(root.join("scripts/package-vsix.sh").is_file());
    assert!(root.join("skills/rite/SKILL.md").is_file());
    assert!(
        root.join("editors/vscode/package.json").is_file(),
        "vscode package.json missing"
    );
    // Lockfile must not reference monorepo pnpm store paths
    let lock = root.join("editors/vscode/package-lock.json");
    if lock.is_file() {
        let text = std::fs::read_to_string(&lock).unwrap();
        assert!(
            !text.contains("node_modules/.pnpm") && !text.contains("../../node_modules"),
            "editors/vscode/package-lock.json is pnpm-linked; regenerate with npm in editors/vscode"
        );
    }
}

/// Helper used by docs: paths that must stay absolute in packaging scripts.
#[test]
fn package_skill_script_resolves_absolute_out() {
    let root = workspace();
    let script = std::fs::read_to_string(root.join("scripts/package-skill.sh")).unwrap();
    assert!(
        script.contains("OUT=\"$(cd \"$OUT_IN\" && pwd)\"")
            || script.contains("cd \"$OUT_IN\" && pwd"),
        "package-skill.sh must canonicalize OUT to an absolute path"
    );
}

fn _ensure_path(p: &Path) {
    let _ = p;
}
