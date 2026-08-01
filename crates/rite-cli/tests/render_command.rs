//! `rite render` — the command docs and CI use to make pictures headlessly.

use std::path::PathBuf;
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rite_bin() -> PathBuf {
    let root = workspace();
    for rel in ["target/debug/rite", "target/release/rite"] {
        let p = root.join(rel);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("rite")
}

const SAMPLE: &str = "◆! main() ⟦\n  ! @console.println(\"hi\")\n  ^ #ok\n⟧\n";

fn script(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rite_render_{name}.rite"));
    std::fs::write(&p, SAMPLE).unwrap();
    p
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(rite_bin())
        .args(args)
        .output()
        .expect("spawn rite")
}

#[test]
fn svg_goes_to_stdout() {
    let f = script("stdout");
    let out = run(&["render", f.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let svg = String::from_utf8_lossy(&out.stdout);
    assert!(svg.starts_with("<svg"), "{svg:.80}");
    assert!(svg.contains("</svg>"), "the SVG is truncated");
    // The colours are the shared palette's, not something the CLI invented.
    assert!(
        svg.contains("#121821"),
        "no palette background in the output"
    );
}

#[test]
fn a_png_is_a_png() {
    let f = script("png");
    let out_path = std::env::temp_dir().join("rite_render_test.png");
    let _ = std::fs::remove_file(&out_path);
    let out = run(&[
        "render",
        f.to_str().unwrap(),
        "--format",
        "png",
        "--frame",
        "window",
        "--output",
        out_path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let bytes = std::fs::read(&out_path).expect("the png was written");
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "that is not a PNG — the magic bytes are wrong"
    );
    // A picture with text in it is not a handful of bytes. The first version of
    // this rendered an empty frame, which is small and looks like success.
    assert!(
        bytes.len() > 5_000,
        "the png is {} bytes, which is about what an empty frame costs",
        bytes.len()
    );
    let _ = std::fs::remove_file(&out_path);
}

#[test]
fn source_can_come_from_stdin() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(rite_bin())
        .args(["render", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rite");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(SAMPLE.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("<svg"));
}

#[test]
fn the_self_contained_format_carries_the_font() {
    let f = script("selfcontained");
    let plain = run(&["render", f.to_str().unwrap(), "--format", "svg"]);
    let carried = run(&["render", f.to_str().unwrap(), "--format", "svg-font"]);
    assert!(
        carried.status.success(),
        "{}",
        String::from_utf8_lossy(&carried.stderr)
    );

    let plain = String::from_utf8_lossy(&plain.stdout);
    let carried = String::from_utf8_lossy(&carried.stdout);
    assert!(
        !plain.contains("@font-face"),
        "the small format embedded a face"
    );
    assert!(
        carried.contains("@font-face"),
        "no face in the self-contained format"
    );
    assert!(
        carried.len() > plain.len() * 10,
        "the self-contained format is suspiciously small: {} vs {}",
        carried.len(),
        plain.len()
    );
}

/// Every frame renders, and each draws its own chrome.
#[test]
fn every_frame_renders() {
    let f = script("frames");
    for frame in ["text", "box", "window"] {
        let out = run(&["render", f.to_str().unwrap(), "--frame", frame]);
        assert!(out.status.success(), "--frame {frame} failed");
        let svg = String::from_utf8_lossy(&out.stdout);
        assert!(svg.starts_with("<svg"), "--frame {frame} produced no svg");
        assert_eq!(
            svg.contains("<circle"),
            frame == "window",
            "--frame {frame}: window dots in the wrong place"
        );
    }
}

/// A mistake in the flags is a usage error (2), not a crash and not a picture.
#[test]
fn bad_options_are_usage_errors() {
    let f = script("bad");
    for args in [
        vec!["render", f.to_str().unwrap(), "--format", "jpeg"],
        vec!["render", f.to_str().unwrap(), "--frame", "circle"],
        vec!["render", f.to_str().unwrap(), "--font-size", "0"],
        vec!["render", f.to_str().unwrap(), "--scale", "0"],
        vec!["render", "/tmp/definitely-not-here.rite"],
    ] {
        let out = run(&args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "expected a usage error from {args:?}, got {:?}\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "a failed render still wrote something to stdout: {args:?}"
        );
    }
}

/// Source that does not compile still renders — a diagnostics page needs to show
/// the wrong form, and a highlighter that refuses it is no use there.
#[test]
fn broken_source_still_renders() {
    let p = std::env::temp_dir().join("rite_render_broken.rite");
    std::fs::write(&p, "◆ ⟧⟧ ←\n").unwrap();
    let out = run(&["render", p.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "rendering broken source failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("<svg"));
}
