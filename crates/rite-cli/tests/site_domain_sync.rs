//! Every Rite host mentioned in the tree must be the one `site.toml` declares.
//!
//! The site host was hardcoded in 51 places — bash, YAML, Vue, Rust, Markdown and
//! two `<meta>` tags — and one of them had drifted to `rite.dev`, a domain this
//! project does not own, so every LSP diagnostic offered a help link to a
//! stranger's server. Nobody noticed because nothing compared the strings.
//!
//! Templating the host into all of those at build time is not available: three of
//! them are shipped artifacts (`install.sh`, the release notes, the `<link
//! rel="canonical">`) that have to work with no build step. So the invariant is
//! enforced here instead. The test names every offending file and line, which
//! makes a domain change mechanical.
//!
//! Same shape, and the same reason, as `editor_grammar_sync` and `palette_sync`.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/rite-cli has two ancestors")
        .to_path_buf()
}

struct Manifest {
    primary: String,
    cant: String,
    legacy: Vec<String>,
}

/// Read the three host keys out of `site.toml`.
///
/// Hand-parsed for the same reason `editor_grammar_sync` hand-reads
/// `grammar/keywords.toml`: the workspace has no `toml` dependency, and adding
/// one to check three string keys is not a trade worth making.
fn manifest() -> Manifest {
    let text = std::fs::read_to_string(repo_root().join("site.toml")).expect("site.toml");
    let value = |key: &str| -> Option<String> {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| {
                let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
                rest.strip_prefix('"')?
                    .split('"')
                    .next()
                    .map(str::to_string)
            })
    };
    let legacy_line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("legacy"))
        .unwrap_or("");
    let legacy = legacy_line
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();

    Manifest {
        primary: value("primary").expect("site.toml has a `primary` host"),
        cant: value("cant").expect("site.toml has a `cant` host"),
        legacy,
    }
}

/// Files that write *about* hosts rather than using them, and are exempt.
///
/// `site.toml` declares them. The changelog records what happened — including the
/// `rite.dev` that was never a host anyone owned, which is the whole point of the
/// entry. Rewriting history to satisfy a lint would be the wrong repair, so the
/// exemption covers any host, not only a legacy one.
fn writes_about_hosts(path: &str) -> bool {
    path.ends_with("site.toml")
        || path.ends_with("CHANGELOG.md")
        || path.contains("docs/adr/")
        // This file. It names `rite.dev` three times explaining why naming it is
        // the failure, and a lint that cannot describe its own subject is one
        // nobody can read.
        || path.ends_with("tests/site_domain_sync.rs")
}

fn tracked_files() -> Vec<PathBuf> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| repo_root().join(s))
        .collect()
}

/// Host-shaped strings containing a `rite` label.
///
/// Telling `rite.foo` from `rite.vsix` needs to know which suffixes are top-level
/// domains, so the list below is curated rather than inferred — the alternative,
/// treating any `rite.<word>` as a host, flagged `rite.ebnf`, `rite.tmLanguage.json`
/// and every `rite.vsix` in the packaging scripts.
///
/// It is deliberately wider than the hosts this project uses: the bug it exists to
/// catch was `rite.dev`, a domain nobody here owns, so matching only *known* hosts
/// would have missed it exactly the way review did. Adding a TLD here is how you
/// make the check see a new class of typo.
const TLDS: &[&str] = &[
    "foo", "dev", "sh", "io", "app", "com", "org", "net", "co", "xyz", "run", "page", "site",
];

fn rite_hosts_in(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let mut cursor = 0;
        while let Some(rel) = line[cursor..].find("rite.") {
            let hit = cursor + rel;
            // Expand both ways over host characters, so a subdomain is part of the
            // host: `cant.rite.foo` must be recognized as one name, not as a
            // `rite.foo` with something stuck to the front.
            let mut start = hit;
            while start > 0 && is_host_char(bytes[start - 1]) {
                start -= 1;
            }
            let mut end = hit;
            while end < bytes.len() && is_host_char(bytes[end]) {
                end += 1;
            }
            let host = line[start..end].trim_end_matches('.');
            let labels: Vec<&str> = host.split('.').collect();
            // A `rite` label of its own, and a real TLD. `favorite.foo` has no
            // `rite` label; `rite.vsix` has no TLD.
            if labels.contains(&"rite") && labels.last().is_some_and(|tld| TLDS.contains(tld)) {
                found.push((n + 1, host.to_string()));
            }
            cursor = end.max(hit + 1);
        }
    }
    found
}

fn is_host_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
}

#[test]
fn every_rite_host_in_the_tree_is_one_site_toml_declares() {
    let manifest = manifest();
    let allowed = [manifest.primary.clone(), manifest.cant.clone()];
    let mut offenders: Vec<String> = Vec::new();

    for path in tracked_files() {
        let rel = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        // Binary assets and the lockfile have no prose to check.
        if rel.starts_with("apps/rite-web/public/brand/")
            || rel.ends_with(".png")
            || rel.ends_with(".jpg")
            || rel.ends_with(".gz")
            || rel.ends_with(".zip")
            || rel.ends_with(".vsix")
            || rel == "Cargo.lock"
            || rel == "pnpm-lock.yaml"
        {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // not UTF-8: not prose
        };

        if writes_about_hosts(&rel) {
            continue;
        }
        for (line, host) in rite_hosts_in(&text) {
            if allowed.contains(&host) {
                continue;
            }
            let why = if manifest.legacy.contains(&host) {
                "a legacy host — update it to the canonical one"
            } else {
                "not declared in site.toml — a typo, or a domain nobody owns"
            };
            offenders.push(format!("{rel}:{line}: `{host}` is {why}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "{} reference(s) to a host site.toml does not declare \
         (primary `{}`, cant `{}`):\n  {}",
        offenders.len(),
        manifest.primary,
        manifest.cant,
        offenders.join("\n  ")
    );
}

/// The manifest itself has to be coherent.
#[test]
fn the_cant_host_is_a_subdomain_of_the_primary_one() {
    let manifest = manifest();
    assert!(
        manifest.cant.ends_with(&format!(".{}", manifest.primary)),
        "cant host `{}` is not under `{}` — if that is deliberate, this test is \
         what should change, along with the reasoning in site.toml",
        manifest.cant,
        manifest.primary
    );
    assert!(
        !manifest.legacy.contains(&manifest.primary),
        "`{}` is listed as both primary and legacy",
        manifest.primary
    );
}

/// The three shipped copies of the installer must agree.
///
/// `scripts/install.sh` is the source; `scripts/build-site.sh` copies it to
/// `apps/rite-web/public/install.sh` and `/install`, and both copies are tracked.
/// CI's generation guard catches a divergence only after a full site build, which
/// is minutes; this catches it in milliseconds, and says which file is stale.
#[test]
fn the_installer_copies_match_their_source() {
    let root = repo_root();
    let source = std::fs::read_to_string(root.join("scripts/install.sh")).expect("install.sh");
    for copy in [
        "apps/rite-web/public/install.sh",
        "apps/rite-web/public/install",
    ] {
        let text = std::fs::read_to_string(root.join(copy)).expect(copy);
        assert_eq!(
            text, source,
            "{copy} has drifted from scripts/install.sh — \
             re-run `bash scripts/build-site.sh`, or copy it by hand"
        );
    }
}
