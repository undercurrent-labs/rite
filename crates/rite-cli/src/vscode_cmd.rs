//! `rite vscode` — download / install the VS Code extension (.vsix).

use crate::config;
use crate::github;
use anyhow::bail;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub async fn download(out: Option<PathBuf>, version: Option<String>) -> anyhow::Result<ExitCode> {
    let (path, meta) = fetch_vsix(out, version.as_deref()).await?;
    print_meta(&meta, &path);
    println!("downloaded: {}", path.display());
    Ok(ExitCode::SUCCESS)
}

pub async fn install(
    editor: Option<String>,
    download_only: bool,
    out: Option<PathBuf>,
    version: Option<String>,
) -> anyhow::Result<ExitCode> {
    let (path, meta) = fetch_vsix(out, version.as_deref()).await?;
    print_meta(&meta, &path);
    if download_only {
        println!("downloaded (install skipped): {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }

    let editors = resolve_editors(editor.as_deref())?;
    if editors.is_empty() {
        println!("downloaded: {}", path.display());
        println!();
        println!("No `code` / `cursor` / `codium` on PATH.");
        println!("Install manually:");
        println!("  code --install-extension {}", path.display());
        println!("  # or: Extensions → ⋯ → Install from VSIX…");
        return Ok(ExitCode::SUCCESS);
    }

    let mut any_ok = false;
    for ed in &editors {
        print!("installing with `{ed}`… ");
        let status = Command::new(ed)
            .args(["--install-extension"])
            .arg(&path)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("ok");
                any_ok = true;
            }
            Ok(s) => println!("failed (exit {})", s.code().unwrap_or(-1)),
            Err(e) => println!("failed ({e})"),
        }
    }
    if any_ok {
        println!();
        println!("Set absolute paths if the GUI PATH is thin:");
        println!("  rite.lspPath    → path to rite-lsp");
        println!("  rite.binaryPath → path to rite");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("download kept at {}", path.display());
        bail!("editor install failed");
    }
}

pub async fn info(version: Option<String>) -> anyhow::Result<ExitCode> {
    let repo = config::default_repo();
    let release = if let Some(v) = version {
        github::release_by_tag(repo, &v).await.ok()
    } else {
        github::latest_release(repo).await.ok()
    };
    println!("VS Code extension (Rite)");
    println!("  publisher: undercurrent-labs");
    println!("  extension: rite");
    if let Some(r) = &release {
        println!("  release:   {}", r.tag_name);
        let assets: Vec<_> = r
            .assets
            .iter()
            .filter(|a| a.name.ends_with(".vsix") || a.name.contains("vscode"))
            .collect();
        if assets.is_empty() {
            println!("  vsix:      (not published on this release yet)");
        } else {
            for a in assets {
                println!("  vsix:      {} ({} bytes)", a.name, a.size.unwrap_or(0));
                println!("             {}", a.browser_download_url);
            }
        }
    } else {
        println!("  release:   (could not query GitHub)");
    }
    println!("  site:      {}/vscode/rite.vsix", config::site_base());
    println!("  agents:    {}/agents", config::site_base());
    println!();
    println!("Install:");
    println!("  rite vscode install");
    println!("  rite vscode download --out ./rite.vsix");
    Ok(ExitCode::SUCCESS)
}

struct VsixMeta {
    source: String,
    version: String,
    size: u64,
    sha256: String,
}

async fn fetch_vsix(
    out: Option<PathBuf>,
    version: Option<&str>,
) -> anyhow::Result<(PathBuf, VsixMeta)> {
    let repo = config::default_repo();
    let release = if let Some(v) = version {
        github::release_by_tag(repo, v).await.ok()
    } else {
        github::latest_release(repo).await.ok()
    };
    let tag = release
        .as_ref()
        .map(|r| r.tag_name.clone())
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));

    let dest = out.unwrap_or_else(|| {
        config::data_dir()
            .join("vscode")
            .join(format!("rite-{}.vsix", tag.trim_start_matches('v')))
    });
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).ok();
    }

    let candidates = github::vscode_download_candidates(release.as_ref());
    let mut last_err = None;
    for (name, url) in candidates {
        let tmp = dest.with_extension("download");
        match github::download_to(&url, &tmp).await {
            Ok(()) => {
                // basic sanity: vsix is a zip
                let meta = fs::metadata(&tmp)?;
                if meta.len() < 64 {
                    last_err = Some(anyhow::anyhow!("file too small: {name}"));
                    continue;
                }
                fs::rename(&tmp, &dest).or_else(|_| {
                    fs::copy(&tmp, &dest)?;
                    fs::remove_file(&tmp)?;
                    Ok::<(), anyhow::Error>(())
                })?;
                let sha = github::sha256_file(&dest)?;
                return Ok((
                    dest,
                    VsixMeta {
                        source: url,
                        version: tag,
                        size: meta.len(),
                        sha256: sha,
                    },
                ));
            }
            Err(e) => last_err = Some(e),
        }
    }
    bail!(
        "could not download VSIX: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "no assets".into())
    )
}

fn print_meta(meta: &VsixMeta, path: &Path) {
    println!("VS Code extension package");
    println!("  version: {}", meta.version);
    println!("  size:    {} bytes", meta.size);
    println!("  sha256:  {}", meta.sha256);
    println!("  source:  {}", meta.source);
    println!("  file:    {}", path.display());
}

fn resolve_editors(prefer: Option<&str>) -> anyhow::Result<Vec<String>> {
    if let Some(p) = prefer {
        return Ok(vec![p.to_string()]);
    }
    let mut out = Vec::new();
    for name in ["code", "cursor", "codium", "code-insiders"] {
        if which(name) {
            out.push(name.to_string());
        }
    }
    Ok(out)
}

fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
