//! `rite update` — check for / install CLI and skill updates.

use crate::config::{self, RiteConfig};
use crate::github;
use crate::skill_cmd;
use anyhow::{bail, Context};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub async fn run(
    check_only: bool,
    force: bool,
    version: Option<String>,
) -> anyhow::Result<ExitCode> {
    let repo = config::default_repo();
    let current = env!("CARGO_PKG_VERSION");
    let release = if let Some(ref v) = version {
        github::release_by_tag(repo, v).await?
    } else {
        github::latest_release(repo).await?
    };
    let latest_tag = release.tag_name.clone();
    let latest_ver = latest_tag.trim_start_matches('v');

    // Skill channel tracks release tags that publish skill assets (or the tag itself).
    let remote_skill = Some(latest_tag.clone());
    let cfg = RiteConfig::load();
    let local_skill = cfg.skill.version.clone();
    let skill_cache_ok = config::skill_cache_dir().join("SKILL.md").is_file();

    let cli_newer = version_is_newer(latest_ver, current);
    let skill_newer = match (&local_skill, &remote_skill) {
        (Some(local), Some(remote)) => {
            local.trim_start_matches('v') != remote.trim_start_matches('v')
        }
        (None, Some(_)) => true, // never installed
        _ => false,
    };

    println!("rite update");
    println!("  installed CLI:  v{current}");
    println!("  latest release: {latest_tag}");
    if let Some(url) = &release.html_url {
        println!("  release notes:  {url}");
    }
    println!();
    println!(
        "  skill installed: {}",
        local_skill.as_deref().unwrap_or("(none)")
    );
    println!(
        "  skill cache:     {}",
        if skill_cache_ok { "present" } else { "missing" }
    );
    println!(
        "  skill on release: {}",
        remote_skill.as_deref().unwrap_or("—")
    );
    if let Some(at) = &cfg.skill.installed_at {
        println!("  skill last pull: {at}");
    }
    println!();

    if cli_newer {
        println!("  CLI:   update available ({current} → {latest_ver})");
    } else {
        println!("  CLI:   up to date");
    }
    if skill_newer {
        println!(
            "  skill: update available ({} → {})",
            local_skill.as_deref().unwrap_or("none"),
            remote_skill.as_deref().unwrap_or("?")
        );
    } else if local_skill.is_some() {
        println!("  skill: up to date");
    } else {
        println!("  skill: not installed — run `rite skill install`");
    }

    // Persist check metadata
    let mut cfg = cfg;
    cfg.last_update_check = Some(config::now_iso());
    cfg.last_cli_version_seen = Some(latest_tag.clone());
    cfg.last_skill_version_seen = remote_skill.clone();
    cfg.save()?;

    if check_only {
        let code = if cli_newer || skill_newer {
            ExitCode::from(1) // non-zero so scripts can detect updates
        } else {
            ExitCode::SUCCESS
        };
        return Ok(code);
    }

    if !cli_newer && !force && version.is_none() {
        // Still refresh skill if needed
        if skill_newer || force {
            println!();
            println!("Updating agent skill…");
            return skill_cmd::update(force).await;
        }
        println!();
        println!("Nothing to do.");
        return Ok(ExitCode::SUCCESS);
    }

    if cli_newer || force || version.is_some() {
        // Refuse before announcing a download we will not perform.
        reject_build_tree_install(&install_dir()?)?;
        println!();
        println!("Downloading CLI {latest_tag}…");
        install_cli_from_release(&release).await?;
        println!("CLI updated to {latest_tag}");
        println!("  binary: {}", current_exe_display());
    }

    if skill_newer || force || local_skill.is_none() {
        println!();
        println!("Updating agent skill…");
        let _ = skill_cmd::update(true).await?;
    }

    Ok(ExitCode::SUCCESS)
}

async fn install_cli_from_release(release: &github::Release) -> anyhow::Result<()> {
    let archive_name = github::archive_name_for_host()?;
    let asset = github::find_asset(release, &archive_name).with_context(|| {
        format!(
            "release {} has no asset matching {archive_name}",
            release.tag_name
        )
    })?;

    // Resolve (and vet) the destination before downloading anything.
    let dest_dir = install_dir()?;
    reject_build_tree_install(&dest_dir)?;

    let tmp = github::tmp_download_dir()?;
    let archive_path = tmp.join(&asset.name);
    github::download_to(&asset.browser_download_url, &archive_path).await?;

    // Checksum verification is mandatory: this replaces the `rite` binary.
    // Every branch below used to fall through to "installed anyway" — a missing
    // SHA256SUMS asset, a failed SHA256SUMS download, or an unlisted archive
    // name all skipped verification silently. scripts/install.sh refuses in the
    // same situations ("Refuse to install without checksums").
    let sums = github::find_asset(release, "SHA256SUMS").with_context(|| {
        format!(
            "release {} publishes no SHA256SUMS asset — refusing to install an \
             unverified binary. Download it manually from {} if you trust it.",
            release.tag_name,
            release
                .html_url
                .clone()
                .unwrap_or_else(|| "the release page".into())
        )
    })?;
    let sums_path = tmp.join("SHA256SUMS");
    github::download_to(&sums.browser_download_url, &sums_path)
        .await
        .with_context(|| {
            format!(
                "could not download SHA256SUMS for {} — refusing to install an \
                 unverified binary",
                release.tag_name
            )
        })?;
    verify_sha256sums(&sums_path, &archive_path, &asset.name)?;

    let extract_dir = tmp.join("extract");
    fs::create_dir_all(&extract_dir)?;
    extract_cli_archive(&archive_path, &extract_dir)?;

    let (rite_bin, lsp_bin) = find_bins(&extract_dir)?;
    fs::create_dir_all(&dest_dir)?;

    let dest_rite = dest_dir.join(if cfg!(windows) { "rite.exe" } else { "rite" });
    let dest_lsp = dest_dir.join(if cfg!(windows) {
        "rite-lsp.exe"
    } else {
        "rite-lsp"
    });

    replace_binary(&rite_bin, &dest_rite)?;
    if lsp_bin.exists() {
        let _ = replace_binary(&lsp_bin, &dest_lsp);
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest_rite)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest_rite, perms)?;
        if dest_lsp.exists() {
            let mut perms = fs::metadata(&dest_lsp)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest_lsp, perms)?;
        }
    }

    let _ = fs::remove_dir_all(&tmp);
    println!("  installed to {}", dest_dir.display());
    if !path_contains(&dest_dir) {
        println!(
            "  note: {} may not be on your PATH — add it or re-open the shell",
            dest_dir.display()
        );
    }
    Ok(())
}

/// Look up `name` in a SHA256SUMS file and compare hashes. Fails closed: an
/// unlisted file is an error, not a warning.
fn verify_sha256sums(sums: &Path, archive: &Path, name: &str) -> anyhow::Result<()> {
    let text = fs::read_to_string(sums)?;
    let actual = github::sha256_file(archive)?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next().unwrap_or("");
        let file = parts.next().unwrap_or("").trim_start_matches('*');
        // Exact file name only. `ends_with` would let `evil-rite-x86_64.tar.gz`
        // satisfy the entry for `rite-x86_64.tar.gz`.
        if file == name {
            if !hash.eq_ignore_ascii_case(&actual) {
                bail!("checksum mismatch for {name}: expected {hash}, got {actual}");
            }
            println!("  checksum ok ({name})");
            return Ok(());
        }
    }
    bail!("{name} is not listed in SHA256SUMS — refusing to install an unverified binary")
}

fn extract_cli_archive(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    // Dispatch on content, not on the asset name: a download that returned an
    // HTML error page must be reported as such.
    crate::archive::extract_any(archive, dest)
}

/// Refuse to overwrite a cargo build artifact.
///
/// `install_dir()` prefers "the directory of the running executable if it
/// already holds a `rite` binary", so `./target/debug/rite update` replaced the
/// local build output with a downloaded release (and left `rite.old` behind).
fn reject_build_tree_install(dest: &Path) -> anyhow::Result<()> {
    if env::var_os("RITE_INSTALL_DIR").is_some() {
        // Explicit destination: the operator asked for it.
        return Ok(());
    }
    if looks_like_build_tree(dest) {
        bail!(
            "refusing to overwrite cargo build output in {}\n  \
             `rite update` installs release binaries; to update a checkout run \
             `git pull && cargo build`\n  \
             to install elsewhere: RITE_INSTALL_DIR=<dir> rite update",
            dest.display()
        );
    }
    Ok(())
}

/// True for paths inside `target/debug`, `target/release`, or a cargo `deps` dir.
fn looks_like_build_tree(path: &Path) -> bool {
    let mut comps: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if comps.last().map(|c| c == "deps").unwrap_or(false) {
        comps.pop();
    }
    for pair in comps.windows(2) {
        if pair[0] == "target" && (pair[1] == "debug" || pair[1] == "release") {
            return true;
        }
        // Custom CARGO_TARGET_DIR keeps the profile dir name.
        if pair[1] == "debug" && pair[0].ends_with("target") {
            return true;
        }
    }
    false
}

fn find_bins(root: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
    let mut rite = None;
    let mut lsp = None;
    fn walk(
        dir: &Path,
        rite: &mut Option<PathBuf>,
        lsp: &mut Option<PathBuf>,
    ) -> anyhow::Result<()> {
        for e in fs::read_dir(dir)? {
            let p = e?.path();
            if p.is_dir() {
                walk(&p, rite, lsp)?;
            } else if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name == "rite" || name == "rite.exe" {
                    *rite = Some(p.clone());
                } else if name == "rite-lsp" || name == "rite-lsp.exe" {
                    *lsp = Some(p.clone());
                }
            }
        }
        Ok(())
    }
    walk(root, &mut rite, &mut lsp)?;
    let rite = rite.with_context(|| format!("rite binary not found in {}", root.display()))?;
    let lsp = lsp.unwrap_or_else(|| root.join("rite-lsp"));
    Ok((rite, lsp))
}

fn install_dir() -> anyhow::Result<PathBuf> {
    if let Ok(d) = env::var("RITE_INSTALL_DIR") {
        return Ok(PathBuf::from(d));
    }
    // Prefer directory of current executable when it looks like a user install
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let name = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == "bin" || parent.ends_with(".local/bin") {
                return Ok(parent.to_path_buf());
            }
            // Also replace in-place if writable
            if parent.join("rite").exists() || parent.join("rite.exe").exists() {
                return Ok(parent.to_path_buf());
            }
        }
    }
    Ok(config::home_dir().join(".local").join("bin"))
}

fn replace_binary(src: &Path, dest: &Path) -> anyhow::Result<()> {
    // On Windows, cannot overwrite running exe easily; write .new then best-effort
    let tmp = dest.with_extension("new");
    fs::copy(src, &tmp).with_context(|| format!("copy to {}", tmp.display()))?;
    let mut backup = None;
    if dest.exists() {
        let bak = dest.with_extension("old");
        let _ = fs::remove_file(&bak);
        if fs::rename(dest, &bak).is_ok() {
            backup = Some(bak);
        }
    }
    fs::rename(&tmp, dest).or_else(|_| {
        fs::copy(&tmp, dest)?;
        fs::remove_file(&tmp)?;
        Ok::<(), anyhow::Error>(())
    })?;
    // Don't leave `rite.old` next to the binary forever. On Windows the old
    // image can still be mapped by the running process, so this is best-effort;
    // it succeeds on the next update if not now.
    if let Some(bak) = backup {
        let _ = fs::remove_file(&bak);
    }
    Ok(())
}

fn path_contains(dir: &Path) -> bool {
    let Ok(path) = env::var("PATH") else {
        return false;
    };
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    env::split_paths(&path).any(|p| p.canonicalize().ok().as_ref() == Some(&dir) || p == dir)
}

fn current_exe_display() -> String {
    env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "rite".into())
}

/// Semver-ish compare: true if `latest` is greater than `current`.
fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let a = parse(latest);
    let b = parse(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x > y {
            return true;
        }
        if x < y {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare() {
        assert!(version_is_newer("0.1.8", "0.1.7"));
        assert!(!version_is_newer("0.1.7", "0.1.7"));
        assert!(!version_is_newer("0.1.6", "0.1.7"));
        assert!(version_is_newer("v0.2.0", "0.1.9"));
    }

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rite_update_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn checksum_verifies_exact_name() {
        let dir = scratch("sums_ok");
        let archive = dir.join("rite-x86_64-unknown-linux-gnu.tar.gz");
        fs::write(&archive, b"payload").unwrap();
        let hash = github::sha256_file(&archive).unwrap();
        let sums = dir.join("SHA256SUMS");
        fs::write(
            &sums,
            format!("{hash}  rite-x86_64-unknown-linux-gnu.tar.gz\n"),
        )
        .unwrap();

        verify_sha256sums(&sums, &archive, "rite-x86_64-unknown-linux-gnu.tar.gz").unwrap();
    }

    #[test]
    fn checksum_mismatch_fails() {
        let dir = scratch("sums_bad");
        let archive = dir.join("rite.tar.gz");
        fs::write(&archive, b"payload").unwrap();
        let sums = dir.join("SHA256SUMS");
        fs::write(&sums, format!("{}  rite.tar.gz\n", "0".repeat(64))).unwrap();

        let err = verify_sha256sums(&sums, &archive, "rite.tar.gz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("checksum mismatch"), "{err}");
    }

    #[test]
    fn unlisted_archive_fails_closed() {
        let dir = scratch("sums_missing");
        let archive = dir.join("rite-aarch64-apple-darwin.tar.gz");
        fs::write(&archive, b"payload").unwrap();
        let sums = dir.join("SHA256SUMS");
        fs::write(&sums, format!("{}  other.tar.gz\n", "0".repeat(64))).unwrap();

        let err = verify_sha256sums(&sums, &archive, "rite-aarch64-apple-darwin.tar.gz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not listed"), "{err}");
    }

    #[test]
    fn suffix_match_no_longer_satisfies_an_entry() {
        // `evil-rite.tar.gz` used to match the `rite.tar.gz` entry via ends_with.
        let dir = scratch("sums_suffix");
        let archive = dir.join("evil-rite.tar.gz");
        fs::write(&archive, b"payload").unwrap();
        let hash = github::sha256_file(&archive).unwrap();
        let sums = dir.join("SHA256SUMS");
        fs::write(&sums, format!("{hash}  rite.tar.gz\n")).unwrap();

        let err = verify_sha256sums(&sums, &archive, "evil-rite.tar.gz")
            .unwrap_err()
            .to_string();
        assert!(err.contains("not listed"), "{err}");
    }

    #[test]
    fn build_tree_destinations_are_rejected() {
        assert!(looks_like_build_tree(Path::new(
            "/home/u/rite/target/debug"
        )));
        assert!(looks_like_build_tree(Path::new(
            "/home/u/rite/target/release"
        )));
        assert!(looks_like_build_tree(Path::new(
            "/home/u/rite/target/debug/deps"
        )));
        assert!(!looks_like_build_tree(Path::new("/home/u/.local/bin")));
        assert!(!looks_like_build_tree(Path::new("/usr/local/bin")));
        // A directory literally named "target" without a profile dir is fine.
        assert!(!looks_like_build_tree(Path::new("/home/u/target")));
    }

    #[test]
    fn reject_build_tree_install_explains_itself() {
        // RITE_INSTALL_DIR is an explicit opt-in and must keep working; the test
        // process may not have it set, so only check the refusing branch here.
        if std::env::var_os("RITE_INSTALL_DIR").is_some() {
            return;
        }
        let err = reject_build_tree_install(Path::new("/tmp/rite/target/debug"))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("refusing to overwrite cargo build output"),
            "{err}"
        );
        assert!(reject_build_tree_install(Path::new("/tmp/somewhere/bin")).is_ok());
    }

    #[test]
    fn old_backup_is_cleaned_up() {
        let dir = scratch("replace");
        let src = dir.join("new-rite");
        fs::write(&src, b"new").unwrap();
        let dest = dir.join("rite");
        fs::write(&dest, b"old").unwrap();

        replace_binary(&src, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        assert!(
            !dir.join("rite.old").exists(),
            "rite.old should not be left behind"
        );
        assert!(!dir.join("rite.new").exists());
    }
}
