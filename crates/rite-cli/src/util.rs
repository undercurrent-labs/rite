//! Shared CLI helpers: checkout discovery, browser opening, source-tree walking.

use std::path::{Component, Path, PathBuf};

/// Directories never walked when collecting `.rite` sources.
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules", ".rite", ".jj", ".hg"];

/// Hard depth cap: a second guard behind the symlink check below.
const MAX_DEPTH: usize = 64;

/// Find the nearest ancestor directory (of `start`) that contains `rel`.
fn find_up(start: &Path, rel: &str) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(rel).exists() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Locate a Rite checkout that contains `rel` (e.g. `docs/book`).
///
/// Repo-relative paths cannot be hardcoded: `rite docs build` run from `/tmp`
/// used to create `./skills/rite` wherever the user happened to stand. We look
/// in the current directory and its ancestors, then above the running binary so
/// `target/debug/rite` still finds the checkout it was built in.
pub fn checkout_containing(rel: &str) -> Option<PathBuf> {
    if let Ok(root) = std::env::var("RITE_REPO_ROOT") {
        let root = PathBuf::from(root);
        if root.join(rel).exists() {
            return Some(root);
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(hit) = find_up(&cwd, rel) {
            return Some(hit);
        }
    }
    let exe = std::env::current_exe().ok()?;
    find_up(exe.parent()?, rel)
}

/// Resolve a repo-relative input path, or explain where we looked.
pub fn require_checkout_path(rel: &str, what: &str) -> anyhow::Result<PathBuf> {
    match checkout_containing(rel) {
        Some(root) => Ok(root.join(rel)),
        None => anyhow::bail!(
            "cannot find {what} (`{rel}`) — this is a repository maintenance command.\n  \
             run it from a Rite checkout, pass an explicit path, or set RITE_REPO_ROOT=<checkout>"
        ),
    }
}

/// Open `target` (URL or file path) in the platform browser / handler.
///
/// `xdg-open` only exists on Linux; the project ships macOS and Windows
/// binaries, so dispatch per platform and report failure to the caller
/// instead of pretending it worked.
pub fn open_in_browser(target: &str) -> anyhow::Result<()> {
    use std::process::{Command, Stdio};

    let mut cmd = match std::env::consts::OS {
        "macos" => {
            let mut c = Command::new("open");
            c.arg(target);
            c
        }
        "windows" => {
            // `start` is a cmd builtin; the empty string is the window title,
            // without it a quoted target is treated as the title.
            let mut c = Command::new("cmd");
            c.args(["/C", "start", ""]).arg(target);
            c
        }
        _ => {
            let mut c = Command::new("xdg-open");
            c.arg(target);
            c
        }
    };
    let status = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("could not launch a browser ({e})"))?;
    if !status.success() {
        anyhow::bail!("browser command exited with {status}");
    }
    Ok(())
}

/// Collect `.rite` files from a file or directory argument.
pub fn collect_rite_files(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.exists() {
        anyhow::bail!("no such file or directory: {}", path.display());
    }
    let mut out = Vec::new();
    walk_rite(path, 0, &mut out)?;
    out.sort();
    Ok(out)
}

/// Recurse without following symlinked directories.
///
/// The previous implementation used `Path::is_dir()`, which follows symlinks: a
/// `dir -> ..` link made `rite fmt .` recurse until it ran out of stack.
fn walk_rite(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_rite(&entry.path(), depth + 1, out)?;
        } else if name.ends_with(".rite") {
            out.push(entry.path());
        }
    }
    Ok(())
}

/// Join `rel` under `root`, rejecting escapes (`..`, absolute paths, symlinks).
///
/// Used by the docs static server: a request path must not be able to read
/// outside the served root.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel.trim_start_matches('/'));
    let mut out = root.to_path_buf();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let root_real = root.canonicalize().ok()?;
    let out_real = out.canonicalize().ok()?;
    if out_real.starts_with(&root_real) {
        Some(out_real)
    } else {
        None
    }
}

/// Minimal extension → content type map (no `mime_guess` dependency).
pub fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "md" => "text/markdown; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/vnd.microsoft.icon",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" | "rite" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rite_util_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn collect_skips_ignored_dirs() {
        let root = tmpdir("skip");
        std::fs::write(root.join("a.rite"), "1\n").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/b.rite"), "1\n").unwrap();
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::write(root.join("node_modules/c.rite"), "1\n").unwrap();

        let files = collect_rite_files(&root).unwrap();
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(files[0].ends_with("a.rite"));
    }

    #[cfg(unix)]
    #[test]
    fn collect_survives_symlink_loop() {
        let root = tmpdir("loop");
        std::fs::write(root.join("a.rite"), "1\n").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        // sub/loop -> root : infinite recursion for a follow-symlinks walker
        std::os::unix::fs::symlink(&root, root.join("sub/loop")).unwrap();

        let files = collect_rite_files(&root).unwrap();
        assert_eq!(files.len(), 1, "{files:?}");
    }

    #[test]
    fn safe_join_rejects_escapes() {
        let root = tmpdir("safe");
        std::fs::write(root.join("ok.txt"), "x").unwrap();
        assert!(safe_join(&root, "ok.txt").is_some());
        assert!(safe_join(&root, "../ok.txt").is_none());
        assert!(safe_join(&root, "/etc/passwd").is_none());
        assert!(safe_join(&root, "sub/../../etc/passwd").is_none());
    }

    #[test]
    fn content_types() {
        assert_eq!(
            content_type_for(Path::new("a/b.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(content_type_for(Path::new("x.json")), "application/json");
        assert_eq!(
            content_type_for(Path::new("x.unknown")),
            "application/octet-stream"
        );
    }
}
