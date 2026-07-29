//! `rite skill` — install/update/status for the agent skill bundle.

use crate::config::{self, RiteConfig, SkillState};
use crate::github;
use anyhow::{bail, Context};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTarget {
    Grok,
    Claude,
    Cursor,
    Project,
    CacheOnly,
}

impl SkillTarget {
    pub fn parse_list(s: &str) -> anyhow::Result<Vec<SkillTarget>> {
        let mut out = Vec::new();
        for part in s.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
            out.push(match part {
                "grok" => SkillTarget::Grok,
                "claude" => SkillTarget::Claude,
                "cursor" => SkillTarget::Cursor,
                "project" => SkillTarget::Project,
                "cache" | "cache-only" => SkillTarget::CacheOnly,
                "all" => {
                    return Ok(vec![
                        SkillTarget::Grok,
                        SkillTarget::Claude,
                        SkillTarget::Cursor,
                    ]);
                }
                other => {
                    bail!("unknown skill target `{other}` (try grok,claude,cursor,project,all)")
                }
            });
        }
        if out.is_empty() {
            out.push(SkillTarget::Grok);
        }
        Ok(out)
    }

    fn default_path(self) -> PathBuf {
        match self {
            SkillTarget::Grok => config::home_dir().join(".grok").join("skills").join("rite"),
            SkillTarget::Claude => config::home_dir()
                .join(".claude")
                .join("skills")
                .join("rite"),
            SkillTarget::Cursor => config::home_dir()
                .join(".cursor")
                .join("skills")
                .join("rite"),
            SkillTarget::Project => PathBuf::from(".grok").join("skills").join("rite"),
            SkillTarget::CacheOnly => config::skill_cache_dir(),
        }
    }
}

pub async fn install(
    targets: &str,
    dir: Option<PathBuf>,
    from: Option<String>,
    version: Option<String>,
    force: bool,
) -> anyhow::Result<ExitCode> {
    let targets = if dir.is_some() {
        vec![SkillTarget::CacheOnly] // path override below
    } else {
        SkillTarget::parse_list(targets)?
    };

    let (cache, version_label, source, fingerprint) =
        fetch_skill_to_cache(from.as_deref(), version.as_deref(), force).await?;

    let mut installed = Vec::new();
    if let Some(d) = dir {
        let dest = config::expand_user(&d.to_string_lossy());
        copy_skill_tree(&cache, &dest)?;
        installed.push(dest);
    } else {
        for t in targets {
            let dest = t.default_path();
            if matches!(t, SkillTarget::CacheOnly) {
                installed.push(cache.clone());
                continue;
            }
            copy_skill_tree(&cache, &dest)?;
            installed.push(dest);
        }
    }

    let mut cfg = RiteConfig::load();
    cfg.skill = SkillState {
        installed_at: Some(config::now_iso()),
        version: Some(version_label.clone()),
        fingerprint: Some(fingerprint),
        source: Some(source),
        install_paths: installed.iter().map(|p| p.display().to_string()).collect(),
    };
    cfg.save()?;

    println!("rite skill installed ({version_label})");
    println!("  cache: {}", cache.display());
    for p in &installed {
        println!("  → {}", p.display());
    }
    println!();
    println!("Agents discover skills under ~/.grok/skills, ~/.claude/skills, ~/.cursor/skills.");
    println!("Restart the agent session if it was already open.");
    Ok(ExitCode::SUCCESS)
}

pub async fn update(force: bool) -> anyhow::Result<ExitCode> {
    let cfg = RiteConfig::load();
    let targets = if cfg.skill.install_paths.is_empty() {
        "grok".to_string()
    } else {
        // re-install to previous paths
        return reinstall_to_paths(&cfg.skill.install_paths, force).await;
    };
    install(&targets, None, None, None, force).await
}

async fn reinstall_to_paths(paths: &[String], force: bool) -> anyhow::Result<ExitCode> {
    let (cache, version_label, source, fingerprint) =
        fetch_skill_to_cache(None, None, force).await?;
    let mut installed = Vec::new();
    for p in paths {
        let dest = config::expand_user(p);
        // Skip pure cache path if listed
        if dest == config::skill_cache_dir() {
            installed.push(dest);
            continue;
        }
        copy_skill_tree(&cache, &dest)?;
        installed.push(dest);
    }
    let mut cfg = RiteConfig::load();
    cfg.skill = SkillState {
        installed_at: Some(config::now_iso()),
        version: Some(version_label.clone()),
        fingerprint: Some(fingerprint),
        source: Some(source),
        install_paths: installed.iter().map(|p| p.display().to_string()).collect(),
    };
    cfg.save()?;
    println!("rite skill updated ({version_label})");
    for p in &installed {
        println!("  → {}", p.display());
    }
    Ok(ExitCode::SUCCESS)
}

pub fn status() -> anyhow::Result<ExitCode> {
    let cfg = RiteConfig::load();
    let cache = config::skill_cache_dir();
    println!("skill cache: {}", cache.display());
    println!(
        "  present: {}",
        cache.join("SKILL.md").is_file() || cache.join("skill.md").is_file()
    );
    println!("config: {}", config::config_file().display());
    match &cfg.skill {
        s if s.version.is_none() && s.installed_at.is_none() => {
            println!("  not installed via `rite skill install` yet");
        }
        s => {
            println!("  version:      {}", s.version.as_deref().unwrap_or("—"));
            println!(
                "  installed_at: {}",
                s.installed_at.as_deref().unwrap_or("—")
            );
            println!(
                "  fingerprint:  {}",
                s.fingerprint.as_deref().unwrap_or("—")
            );
            println!("  source:       {}", s.source.as_deref().unwrap_or("—"));
            if s.install_paths.is_empty() {
                println!("  paths: (none recorded)");
            } else {
                println!("  paths:");
                for p in &s.install_paths {
                    let exists = PathBuf::from(p).join("SKILL.md").is_file();
                    println!("    {} {}", if exists { "✓" } else { "✗" }, p);
                }
            }
        }
    }
    if let Some(t) = &cfg.last_update_check {
        println!("last update check: {t}");
        println!(
            "  cli seen:   {}",
            cfg.last_cli_version_seen.as_deref().unwrap_or("—")
        );
        println!(
            "  skill seen: {}",
            cfg.last_skill_version_seen.as_deref().unwrap_or("—")
        );
    }
    Ok(ExitCode::SUCCESS)
}

pub fn print_paths() -> anyhow::Result<ExitCode> {
    println!("cache {}", config::skill_cache_dir().display());
    println!("config {}", config::config_file().display());
    println!("grok {}", SkillTarget::Grok.default_path().display());
    println!("claude {}", SkillTarget::Claude.default_path().display());
    println!("cursor {}", SkillTarget::Cursor.default_path().display());
    println!("project {}", SkillTarget::Project.default_path().display());
    Ok(ExitCode::SUCCESS)
}

async fn fetch_skill_to_cache(
    from: Option<&str>,
    version: Option<&str>,
    force: bool,
) -> anyhow::Result<(PathBuf, String, String, String)> {
    let cache = config::skill_cache_dir();
    if !force && cache.join("SKILL.md").is_file() && from.is_none() && version.is_none() {
        // Still refresh from network for update semantics when version requested;
        // for plain install with existing cache, try network first but keep cache on failure.
    }

    // Local path or URL override
    if let Some(src) = from {
        if src.starts_with("https://") || src.starts_with("http://") {
            let tmp = github::tmp_download_dir()?;
            let dest = tmp.join("skill-download");
            github::download_to(src, &dest).await?;
            let fp = github::sha256_file(&dest)?;
            extract_skill_archive(&dest, &cache)?;
            return Ok((cache, "custom".into(), src.to_string(), fp));
        }
        let p = config::expand_user(src);
        if p.is_dir() {
            copy_skill_tree(&p, &cache)?;
            let fp = fingerprint_dir(&cache)?;
            return Ok((cache, "local".into(), p.display().to_string(), fp));
        }
        if p.is_file() {
            let fp = github::sha256_file(&p)?;
            extract_skill_archive(&p, &cache)?;
            return Ok((cache, "local-archive".into(), p.display().to_string(), fp));
        }
        bail!("skill source not found: {}", p.display());
    }

    // Prefer in-repo skills/rite when developing from a checkout
    let local_repo = PathBuf::from("skills/rite");
    if local_repo.join("SKILL.md").is_file() && version.is_none() {
        copy_skill_tree(&local_repo, &cache)?;
        // Stamp current tool version so status/update checks are meaningful
        let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
        let _ = stamp_skill_version(&cache, env!("CARGO_PKG_VERSION"));
        let fp = fingerprint_dir(&cache)?;
        return Ok((cache, ver, local_repo.display().to_string(), fp));
    }

    let repo = config::default_repo();
    let release = if let Some(tag) = version {
        github::release_by_tag(repo, tag).await.ok()
    } else {
        github::latest_release(repo).await.ok()
    };
    let tag = release
        .as_ref()
        .map(|r| r.tag_name.clone())
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));

    let candidates = github::skill_download_candidates(release.as_ref());
    let tmp = github::tmp_download_dir()?;
    let mut last_err = None;
    for (name, url) in candidates {
        let dest = tmp.join(&name);
        match github::download_to(&url, &dest).await {
            Ok(()) => {
                let fp = github::sha256_file(&dest)?;
                match extract_skill_archive(&dest, &cache) {
                    Ok(()) => {
                        let _ = fs::remove_dir_all(&tmp);
                        return Ok((cache, tag, url, fp));
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            Err(e) => last_err = Some(e),
        }
    }
    if cache.join("SKILL.md").is_file() {
        eprintln!("warning: could not refresh skill from network; using cache");
        let fp = fingerprint_dir(&cache)?;
        let ver = read_skill_version(&cache).unwrap_or(tag);
        return Ok((cache, ver, "cache".into(), fp));
    }
    bail!(
        "failed to download skill bundle: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "no candidates".into())
    )
}

fn copy_skill_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    if !src.join("SKILL.md").is_file() && !src.join("skill.md").is_file() {
        // maybe nested rite/
        let nested = src.join("rite");
        if nested.join("SKILL.md").is_file() {
            return copy_skill_tree(&nested, dest);
        }
        bail!("skill source missing SKILL.md: {}", src.display());
    }
    if dest.exists() {
        fs::remove_dir_all(dest).with_context(|| format!("remove {}", dest.display()))?;
    }
    copy_dir_all(src, dest)?;
    Ok(())
}

fn copy_dir_all(src: &Path, dest: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn extract_skill_archive(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let name = archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let staging = archive
        .parent()
        .unwrap_or(Path::new("."))
        .join("skill-extract");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let file = fs::File::open(archive)?;
        let dec = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(dec);
        archive.unpack(&staging)?;
    } else if name.ends_with(".zip") {
        // Minimal zip: shell out to unzip if available
        let status = std::process::Command::new("unzip")
            .args(["-q", "-o"])
            .arg(archive)
            .arg("-d")
            .arg(&staging)
            .status();
        match status {
            Ok(s) if s.success() => {}
            _ => bail!("need `unzip` to extract skill zip, or use .tar.gz"),
        }
    } else {
        bail!("unknown archive format: {}", archive.display());
    }

    // Find SKILL.md in staging
    let root = find_skill_root(&staging)?;
    copy_skill_tree(&root, dest)?;
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn find_skill_root(dir: &Path) -> anyhow::Result<PathBuf> {
    if dir.join("SKILL.md").is_file() {
        return Ok(dir.to_path_buf());
    }
    if dir.join("rite").join("SKILL.md").is_file() {
        return Ok(dir.join("rite"));
    }
    // one level of children
    for entry in fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() && p.join("SKILL.md").is_file() {
            return Ok(p);
        }
        if p.is_dir() && p.join("rite").join("SKILL.md").is_file() {
            return Ok(p.join("rite"));
        }
    }
    bail!("SKILL.md not found in extracted archive")
}

fn fingerprint_dir(dir: &Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut paths = Vec::new();
    collect_files(dir, &mut paths)?;
    paths.sort();
    for p in paths {
        let rel = p.strip_prefix(dir).unwrap_or(&p);
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update(fs::read(&p)?);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_files(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn read_skill_version(dir: &Path) -> Option<String> {
    let v = dir.join("machine").join("version.json");
    let text = fs::read_to_string(v).ok()?;
    let j: serde_json::Value = serde_json::from_str(&text).ok()?;
    j.get("tag")
        .or_else(|| j.get("version"))
        .or_else(|| j.get("tool_version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn stamp_skill_version(dir: &Path, version: &str) -> anyhow::Result<()> {
    let machine = dir.join("machine");
    fs::create_dir_all(&machine)?;
    let tag = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    };
    let body = serde_json::json!({
        "version": version.trim_start_matches('v'),
        "tag": tag,
        "skill": "rite"
    });
    fs::write(
        machine.join("version.json"),
        serde_json::to_string_pretty(&body)?,
    )?;
    Ok(())
}
