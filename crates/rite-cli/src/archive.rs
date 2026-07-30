//! Archive extraction + format sniffing for downloaded release assets.

use anyhow::{bail, Context};
use std::path::Path;
use std::process::{Command, Stdio};

/// What a downloaded file actually is, regardless of its name.
///
/// The site once served SPA HTML at `/skill/rite-agent-skill.tar.gz`; sniffing
/// magic bytes turns "unknown archive error" into an honest diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Gzip,
    Zip,
    Html,
    Unknown,
}

pub fn sniff(path: &Path) -> anyhow::Result<ArchiveKind> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sniff_bytes(&data))
}

pub fn sniff_bytes(data: &[u8]) -> ArchiveKind {
    if data.starts_with(&[0x1f, 0x8b]) {
        return ArchiveKind::Gzip;
    }
    if data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06") {
        return ArchiveKind::Zip;
    }
    let head = String::from_utf8_lossy(&data[..data.len().min(512)]).to_ascii_lowercase();
    if head.contains("<!doctype html") || head.contains("<html") {
        return ArchiveKind::Html;
    }
    ArchiveKind::Unknown
}

pub fn extract_tar_gz(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let dec = flate2::read::GzDecoder::new(file);
    tar::Archive::new(dec)
        .unpack(dest)
        .with_context(|| format!("unpack {}", archive.display()))?;
    Ok(())
}

/// Extract a `.zip` by shelling out, trying every extractor a platform is
/// likely to have.
///
/// There is no zip crate in the dependency tree, and Windows release assets are
/// `.zip`: relying on `unzip` alone left `rite update` broken on Windows (no
/// `unzip` in a default install). Windows 10+ ships `tar.exe` (bsdtar, reads
/// zip) and PowerShell's `Expand-Archive`; both are tried before failing with an
/// actionable message.
pub fn extract_zip(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dest)?;
    let mut attempted: Vec<String> = Vec::new();

    for tool in zip_extractors() {
        let mut cmd = Command::new(tool.program);
        for a in tool.args {
            cmd.arg(a);
        }
        match tool.layout {
            ArgLayout::UnzipStyle => {
                cmd.arg(archive).arg("-d").arg(dest);
            }
            ArgLayout::TarStyle => {
                cmd.arg("-xf").arg(archive).arg("-C").arg(dest);
            }
            ArgLayout::PowerShell => {
                cmd.arg(format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    dest.display()
                ));
            }
        }
        match cmd.stdout(Stdio::null()).stderr(Stdio::null()).status() {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => attempted.push(format!(
                "{} (exit {})",
                tool.program,
                s.code().unwrap_or(-1)
            )),
            Err(e) => attempted.push(format!("{} ({e})", tool.program)),
        }
    }

    bail!(
        "no working zip extractor found for {}\n  tried: {}\n  \
         install `unzip` (Linux/macOS) or extract the archive manually and copy \
         `rite`/`rite-lsp` onto your PATH",
        archive.display(),
        if attempted.is_empty() {
            "(none)".to_string()
        } else {
            attempted.join(", ")
        }
    )
}

enum ArgLayout {
    UnzipStyle,
    TarStyle,
    PowerShell,
}

struct Extractor {
    program: &'static str,
    args: &'static [&'static str],
    layout: ArgLayout,
}

fn zip_extractors() -> Vec<Extractor> {
    let unzip = Extractor {
        program: "unzip",
        args: &["-q", "-o"],
        layout: ArgLayout::UnzipStyle,
    };
    // bsdtar/GNU tar 1.32+ read zip archives.
    let tar = Extractor {
        program: "tar",
        args: &[],
        layout: ArgLayout::TarStyle,
    };
    if cfg!(windows) {
        vec![
            tar,
            Extractor {
                program: "powershell",
                args: &["-NoProfile", "-NonInteractive", "-Command"],
                layout: ArgLayout::PowerShell,
            },
            unzip,
        ]
    } else {
        vec![unzip, tar]
    }
}

/// Extract by sniffed content, not by file name.
pub fn extract_any(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    match sniff(archive)? {
        ArchiveKind::Gzip => extract_tar_gz(archive, dest),
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::Html => bail!(
            "{} is HTML, not an archive — the download URL returned a web page \
             (wrong URL, or a single-page app fallback)",
            archive.display()
        ),
        ArchiveKind::Unknown => bail!(
            "{} is not a recognized archive (expected .tar.gz or .zip)",
            archive.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff_bytes(&[0x1f, 0x8b, 0x08, 0x00]), ArchiveKind::Gzip);
        assert_eq!(sniff_bytes(b"PK\x03\x04rest"), ArchiveKind::Zip);
        assert_eq!(
            sniff_bytes(b"<!DOCTYPE html><html><body>nope"),
            ArchiveKind::Html
        );
        assert_eq!(sniff_bytes(b"just text"), ArchiveKind::Unknown);
        assert_eq!(sniff_bytes(b""), ArchiveKind::Unknown);
    }

    #[test]
    fn extract_any_rejects_html_download() {
        let p = std::env::temp_dir().join(format!("rite_arch_html_{}.tar.gz", std::process::id()));
        std::fs::write(&p, b"<!doctype html><html>SPA</html>").unwrap();
        let dest = std::env::temp_dir().join(format!("rite_arch_out_{}", std::process::id()));
        let err = extract_any(&p, &dest).unwrap_err().to_string();
        assert!(err.contains("HTML"), "{err}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tar_gz_roundtrip() {
        let dir = std::env::temp_dir().join(format!("rite_arch_tgz_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/hello.txt"), "hi").unwrap();

        let archive = dir.join("a.tar.gz");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            b.append_dir_all("src", dir.join("src")).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        assert_eq!(sniff(&archive).unwrap(), ArchiveKind::Gzip);
        let out = dir.join("out");
        extract_any(&archive, &out).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("src/hello.txt")).unwrap(),
            "hi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
