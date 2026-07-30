//! GitHub Releases helpers for self-update, skill, and VSIX downloads.

use crate::config;
use anyhow::{bail, Context};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
    pub html_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: Option<u64>,
}

pub async fn latest_release(repo: &str) -> anyhow::Result<Release> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    get_json(&url).await
}

pub async fn release_by_tag(repo: &str, tag: &str) -> anyhow::Result<Release> {
    let tag = normalize_tag(tag);
    let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
    get_json(&url).await
}

async fn get_json<T: for<'de> Deserialize<'de>>(url: &str) -> anyhow::Result<T> {
    let client = http_client()?;
    let res = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header(
            "User-Agent",
            format!("rite-cli/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        bail!("GitHub API {} for {}", res.status(), url);
    }
    Ok(res.json().await?)
}

pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?)
}

pub async fn download_to(url: &str, dest: &Path) -> anyhow::Result<()> {
    let client = http_client()?;
    let res = client
        .get(url)
        .header(
            "User-Agent",
            format!("rite-cli/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .with_context(|| format!("download {url}"))?;
    if !res.status().is_success() {
        bail!("download failed {} for {}", res.status(), url);
    }
    let bytes = res.bytes().await?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dest, &bytes)?;
    Ok(())
}

pub fn normalize_tag(tag: &str) -> String {
    let t = tag.trim();
    if t.starts_with('v') {
        t.to_string()
    } else {
        format!("v{t}")
    }
}

pub fn find_asset<'a>(release: &'a Release, name_substr: &str) -> Option<&'a Asset> {
    release.assets.iter().find(|a| a.name.contains(name_substr))
}

/// Platform archive name fragment matching install.sh / release packaging.
pub fn host_target_triple() -> anyhow::Result<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => bail!("unsupported platform {os}/{arch} for self-update"),
    }
}

pub fn archive_name_for_host() -> anyhow::Result<String> {
    let triple = host_target_triple()?;
    if cfg!(windows) {
        Ok(format!("rite-{triple}.zip"))
    } else {
        Ok(format!("rite-{triple}.tar.gz"))
    }
}

/// Prefer GitHub release asset; fall back to latest/download + site URLs.
pub fn skill_download_candidates(release: Option<&Release>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(r) = release {
        for name in [
            "rite-agent-skill.tar.gz",
            "rite-agent-skill.zip",
            "rite-skill.tar.gz",
            "skill.tar.gz",
        ] {
            if let Some(a) = find_asset(r, name) {
                out.push((a.name.clone(), a.browser_download_url.clone()));
            }
        }
    }
    // Stable GitHub "latest" asset URLs (work even without API asset listing)
    let repo = config::default_repo();
    for name in ["rite-agent-skill.tar.gz", "rite-agent-skill.zip"] {
        out.push((
            name.into(),
            format!("https://github.com/{repo}/releases/latest/download/{name}"),
        ));
    }
    // Site static endpoints (must be real files in dist, not SPA HTML)
    let base = config::site_base();
    out.push((
        "rite-agent-skill.tar.gz".into(),
        format!("{base}/skill/rite-agent-skill.tar.gz"),
    ));
    out.push((
        "rite-agent-skill.zip".into(),
        format!("{base}/skill/rite-agent-skill.zip"),
    ));
    out
}

pub fn vscode_download_candidates(release: Option<&Release>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(r) = release {
        for a in &r.assets {
            if a.name.ends_with(".vsix") || a.name.contains("vscode") {
                out.push((a.name.clone(), a.browser_download_url.clone()));
            }
        }
    }
    let base = config::site_base();
    out.push(("rite.vsix".into(), format!("{base}/vscode/rite.vsix")));
    out
}

pub fn tmp_download_dir() -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("rite-dl-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let hash = Sha256::digest(&data);
    Ok(hex::encode(hash))
}

/// Look up the expected hash for an exact file name in a `SHA256SUMS` body.
///
/// Exact name comparison only: a suffix match would let `evil-rite.tar.gz`
/// satisfy the entry published for `rite.tar.gz`.
pub fn expected_hash_for(sums_text: &str, name: &str) -> Option<String> {
    for line in sums_text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next().unwrap_or("");
        let file = parts.next().unwrap_or("").trim_start_matches('*');
        if file == name {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// Fetch the release `SHA256SUMS` body, if the release publishes one.
pub async fn release_sums(release: Option<&Release>) -> Option<String> {
    let asset = find_asset(release?, "SHA256SUMS")?;
    let client = http_client().ok()?;
    let res = client
        .get(&asset.browser_download_url)
        .header(
            "User-Agent",
            format!("rite-cli/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .ok()?;
    if !res.status().is_success() {
        return None;
    }
    res.text().await.ok()
}

/// Verify a downloaded asset against the release checksums.
///
/// `Ok(true)` verified, `Ok(false)` the release publishes no checksum for this
/// asset (caller decides whether that is acceptable), `Err` mismatch.
pub async fn verify_against_release(
    release: Option<&Release>,
    name: &str,
    path: &Path,
) -> anyhow::Result<bool> {
    let Some(sums) = release_sums(release).await else {
        return Ok(false);
    };
    let Some(expected) = expected_hash_for(&sums, name) else {
        return Ok(false);
    };
    let actual = sha256_file(path)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        bail!("checksum mismatch for {name}: expected {expected}, got {actual}");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::expected_hash_for;

    const SUMS: &str = "\
aaaa1111  rite-x86_64-unknown-linux-gnu.tar.gz
bbbb2222 *rite-agent-skill.tar.gz
# comment line

cccc3333  rite.vsix
";

    #[test]
    fn finds_exact_entries() {
        assert_eq!(
            expected_hash_for(SUMS, "rite-x86_64-unknown-linux-gnu.tar.gz").as_deref(),
            Some("aaaa1111")
        );
        // Leading '*' (binary mode marker) is stripped from the file name.
        assert_eq!(
            expected_hash_for(SUMS, "rite-agent-skill.tar.gz").as_deref(),
            Some("bbbb2222")
        );
        assert_eq!(
            expected_hash_for(SUMS, "rite.vsix").as_deref(),
            Some("cccc3333")
        );
    }

    #[test]
    fn rejects_partial_names() {
        assert_eq!(expected_hash_for(SUMS, "rite.tar.gz"), None);
        assert_eq!(expected_hash_for(SUMS, "evil-rite.vsix"), None);
        assert_eq!(expected_hash_for(SUMS, ""), None);
    }
}
