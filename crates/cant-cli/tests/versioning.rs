//! **Cant's version is its own.**
//!
//! ADR 0001, Amendment 2. Cant shipped once inside Rite's `0.7.0` archive
//! wearing Rite's number, which claimed seven minor cycles of stability for a v0
//! language whose operator vocabulary can still change. The fix was one line per
//! manifest, and one line per manifest is exactly the kind of thing a later tidy
//! puts back.
//!
//! So these tests hold the two numbers apart, and hold every place that reports
//! one of them to the right source.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-cli has two ancestors")
        .to_path_buf()
}

/// Every crate that makes up Cant.
const CANT_CRATES: &[&str] = &[
    "cant-syntax",
    "cant-sem",
    "cant",
    "cant-cli",
    "cant-wasm",
    "cant-render",
];

fn literal_version(manifest: &str) -> Option<String> {
    let package = manifest.split("[package]").nth(1)?;
    // Stop at the next section, so a `[dependencies]` entry cannot be mistaken
    // for the package's own version.
    let package = package.split("\n[").next().unwrap_or(package);
    for line in package.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("version") {
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('=') else {
                continue;
            };
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn workspace_version(root: &Path) -> String {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let section = manifest
        .split("[workspace.package]")
        .nth(1)
        .expect("[workspace.package]");
    literal_version(&format!("[package]{section}")).expect("a workspace version")
}

/// Every Cant crate states a version of its own.
#[test]
fn no_cant_crate_takes_the_workspace_version() {
    let root = repo_root();
    let mut wrong = Vec::new();
    let mut versions = Vec::new();

    for crate_name in CANT_CRATES {
        let path = root.join("crates").join(crate_name).join("Cargo.toml");
        let manifest = std::fs::read_to_string(&path).expect("a Cant manifest");
        if manifest.contains("version.workspace = true")
            || manifest.contains("version = { workspace = true }")
        {
            wrong.push(format!(
                "{crate_name} takes the workspace version — it would ship as Rite's number"
            ));
            continue;
        }
        match literal_version(&manifest) {
            Some(v) => versions.push((*crate_name, v)),
            None => wrong.push(format!("{crate_name} states no version at all")),
        }
    }

    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    // And they agree with each other: five crates, one language.
    let first = &versions[0].1;
    for (name, version) in &versions {
        assert_eq!(
            version, first,
            "{name} is {version} while {} is {first} — the Cant crates release together",
            versions[0].0
        );
    }
}

/// The two numbers are free to be equal, but nothing may *derive* one from the
/// other. This is the cheap check that they have actually been separated.
#[test]
fn cant_and_rite_carry_different_numbers_today() {
    let root = repo_root();
    let manifest = std::fs::read_to_string(root.join("crates/cant-cli/Cargo.toml")).expect("cant");
    let cant = literal_version(&manifest).expect("a version");
    assert_ne!(
        cant,
        workspace_version(&root),
        "Cant and Rite are on the same number; if that is deliberate, this test \
         needs rewriting to check the *source* of each rather than their values"
    );
}

/// `cant version` reports Rite's real version, not its own with a different label.
#[test]
fn the_reported_rite_version_is_rites() {
    let info = cant::version_info();
    assert_eq!(info.rite, rite_core::VERSION);
    assert_eq!(info.tool, cant::TOOL_VERSION);
    assert_ne!(
        info.tool, info.rite,
        "the tool is reporting one number under two names"
    );
}

/// The binary says the same thing the library does.
#[test]
fn the_binary_reports_both_numbers() {
    let out = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["version"])
        .output()
        .expect("cant binary");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(&format!("cant {}", cant::TOOL_VERSION)),
        "{text}"
    );
    assert!(
        text.contains(&format!("rite: {}", rite_core::VERSION)),
        "{text}"
    );
}

/// The site advertises Cant's number, from Cant's manifest.
///
/// `apps/cant-web/vite.config.ts` used to read `[workspace.package]`, which put
/// Rite's version in the footer under Cant's name — the same mistake in a place
/// no Rust test would ever look.
#[test]
fn the_site_reads_the_cant_manifest_for_its_version() {
    let config = std::fs::read_to_string(repo_root().join("apps/cant-web/vite.config.ts"))
        .expect("vite.config.ts");
    assert!(
        config.contains("crates/cant-cli/Cargo.toml"),
        "the Cant site does not read Cant's manifest for the version it shows"
    );
    // The *read*, not the words: the comment above that code explains what
    // `[workspace.package]` would do wrong, and a lint that cannot survive its
    // own explanation is a lint people delete.
    assert!(
        !config.contains(r#"path.join(repoRoot, "Cargo.toml")"#),
        "the Cant site still opens the workspace manifest — that is Rite's version"
    );
}

/// `cant` refuses to update itself, and names the tool that does it.
#[test]
fn cant_update_points_at_rite_update() {
    let out = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["update"])
        .output()
        .expect("cant binary");
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "a usage error: {text}");
    assert!(text.contains("rite update"), "{text}");
}
