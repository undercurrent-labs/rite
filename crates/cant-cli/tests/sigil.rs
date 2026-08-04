//! `cant sigil`, end to end.
//!
//! The library is tested in `rite-sigil`; what is checked here is the seam a
//! user actually touches — the four input forms, the default output path, the
//! flag surface, and the exit statuses — because every one of those is a
//! contract and none of them is exercised by a unit test.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-cli has two ancestors")
        .to_path_buf()
}

fn cant(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("cant runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

const EXAMPLE: &str = "examples/sigil/basic-flow.cant";

#[test]
fn a_file_renders_to_svg_on_stdout() {
    let out = cant(&["sigil", EXAMPLE, "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let svg = stdout(&out);
    assert!(svg.starts_with("<svg"), "{svg}");
    assert!(svg.trim_end().ends_with("</svg>"));
}

#[test]
fn an_expression_renders() {
    let out = cant(&["sigil", "-e", "[1, 2, 3] -> * -> $ * $ -> []", "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).starts_with("<svg"));
}

/// The pipe from §17.2, which is also the proof that the adapter is a real
/// boundary: this path never parses a `.cant` file.
#[test]
fn graph_json_renders_without_any_source() {
    let graph = cant(&["graph", EXAMPLE, "--format", "json"]);
    assert_eq!(code(&graph), 0, "{}", stderr(&graph));

    let path = std::env::temp_dir().join("cant-sigil-graph-test.json");
    std::fs::write(&path, stdout(&graph)).expect("write graph");
    let out = cant(&[
        "sigil",
        "--graph",
        path.to_str().expect("utf-8 path"),
        "-o",
        "-",
    ]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).starts_with("<svg"));
}

/// §17.3: a file input writes `<basename>.sigil.svg` beside it.
#[test]
fn a_file_input_writes_a_sibling_artifact_by_default() {
    let dir = std::env::temp_dir().join("cant-sigil-default-out");
    let _ = std::fs::create_dir_all(&dir);
    let source = dir.join("program.cant");
    std::fs::write(&source, "[1, 2] -> * -> []\n").expect("write source");

    let out = cant(&["sigil", source.to_str().expect("utf-8 path")]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let expected = dir.join("program.sigil.svg");
    assert!(
        expected.exists(),
        "no artifact at {}; stderr: {}",
        expected.display(),
        stderr(&out)
    );
    let svg = std::fs::read_to_string(&expected).expect("read artifact");
    assert!(svg.starts_with("<svg"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// An expression writes to stdout rather than inventing a filename in whatever
/// directory someone happened to be in.
#[test]
fn an_expression_without_output_goes_to_stdout() {
    let out = cant(&["sigil", "-e", "[1] -> []"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).starts_with("<svg"));
}

#[test]
fn stdin_is_a_source() {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["sigil", "-", "-o", "-"])
        .current_dir(repo_root())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"[1, 2] -> * -> []\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).starts_with("<svg"));
}

#[test]
fn scene_json_is_a_format() {
    let out = cant(&["sigil", EXAMPLE, "--format", "scene-json", "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let value: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(value["schema"], serde_json::json!("rite.sigil.scene"));
}

/// The defaults §17.3 specifies, observed through the fingerprint rather than
/// asserted about the code.
#[test]
fn the_defaults_are_svg_neon_ritual_veiled_safe_and_the_graph_seed() {
    let out = cant(&["sigil", EXAMPLE, "--check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let line = stdout(&out);
    for expected in [
        "theme=neon-ritual@1",
        "mode=veiled",
        "metadata=safe",
        "format=svg",
    ] {
        assert!(line.contains(expected), "{expected} missing from {line}");
    }
}

/// `--check` renders and reports without writing, so a build can assert an
/// artifact is producible without producing one.
#[test]
fn check_reports_a_fingerprint_and_writes_nothing() {
    let dir = std::env::temp_dir().join("cant-sigil-check");
    let _ = std::fs::create_dir_all(&dir);
    let source = dir.join("p.cant");
    std::fs::write(&source, "[1] -> []\n").expect("write");

    let out = cant(&["sigil", source.to_str().expect("utf-8"), "--check"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).starts_with("sigil/"));
    assert!(
        !dir.join("p.sigil.svg").exists(),
        "--check wrote an artifact"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every theme and every mode is accepted, and each theme produces a different
/// artifact.
#[test]
fn every_theme_and_mode_is_accepted() {
    let mut seen: Vec<String> = Vec::new();
    for theme in ["neon-ritual", "void", "parchment"] {
        let out = cant(&["sigil", EXAMPLE, "--theme", theme, "-o", "-"]);
        assert_eq!(code(&out), 0, "{theme}: {}", stderr(&out));
        let svg = stdout(&out);
        assert!(!seen.contains(&svg), "{theme} renders like another theme");
        seen.push(svg);
    }
    for mode in ["veiled", "inscribed", "revealed"] {
        let out = cant(&["sigil", EXAMPLE, "--mode", mode, "-o", "-"]);
        assert_eq!(code(&out), 0, "{mode}: {}", stderr(&out));
    }
    for metadata in ["full", "safe", "minimal", "none"] {
        let out = cant(&["sigil", EXAMPLE, "--metadata", metadata, "-o", "-"]);
        assert_eq!(code(&out), 0, "{metadata}: {}", stderr(&out));
    }
}

/// `--canonical` is documented as reproducible, so two runs are the same bytes
/// — and it differs from the seeded default, or it would not be worth having.
#[test]
fn canonical_output_is_reproducible_and_differs_from_the_default() {
    let a = cant(&["sigil", EXAMPLE, "--canonical", "-o", "-"]);
    let b = cant(&["sigil", EXAMPLE, "--canonical", "-o", "-"]);
    assert_eq!(code(&a), 0, "{}", stderr(&a));
    assert_eq!(stdout(&a), stdout(&b));

    let seeded = cant(&["sigil", EXAMPLE, "-o", "-"]);
    assert_ne!(
        stdout(&a),
        stdout(&seeded),
        "--canonical produced the seeded orientation"
    );
}

/// The same seed is the same picture, whoever asks for it.
#[test]
fn an_explicit_seed_is_reproducible() {
    let a = cant(&["sigil", EXAMPLE, "--seed", "42", "-o", "-"]);
    let b = cant(&["sigil", EXAMPLE, "--seed", "42", "-o", "-"]);
    assert_eq!(code(&a), 0, "{}", stderr(&a));
    assert_eq!(stdout(&a), stdout(&b));
    let other = cant(&["sigil", EXAMPLE, "--seed", "43", "-o", "-"]);
    assert_ne!(stdout(&a), stdout(&other), "the seed changed nothing");
}

/// Veiled is the default, and the default draws no text.
#[test]
fn the_default_artifact_contains_no_visible_text() {
    let out = cant(&["sigil", EXAMPLE, "-o", "-"]);
    assert!(!stdout(&out).contains("<text"));
}

#[test]
fn a_transparent_background_omits_the_backing_rectangle() {
    let out = cant(&["sigil", EXAMPLE, "--background", "transparent", "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(!stdout(&out).contains("<rect"));
}

#[test]
fn a_hex_background_is_used_and_a_bad_one_is_refused() {
    let out = cant(&["sigil", EXAMPLE, "--background", "#101820", "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("fill=\"#101820\""));

    let bad = cant(&["sigil", EXAMPLE, "--background", "puce", "-o", "-"]);
    assert_eq!(code(&bad), 2, "a bad colour must be a usage error");
}

/// Every bad flag is a usage error (2), not a crash and not a silent default.
#[test]
fn unknown_option_values_are_usage_errors() {
    for (flag, value) in [
        ("--theme", "chartreuse"),
        ("--mode", "naked"),
        ("--metadata", "everything"),
        ("--seed", "soon"),
        ("--format", "pdf"),
    ] {
        let out = cant(&["sigil", EXAMPLE, flag, value, "-o", "-"]);
        assert_eq!(code(&out), 2, "{flag} {value}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(value),
            "{flag}'s error does not name the bad value: {}",
            stderr(&out)
        );
    }
}

/// A flag typo is reported before a file is read, so nobody waits for a parse to
/// discover they misspelled a theme.
#[test]
fn a_bad_flag_is_reported_even_when_the_source_does_not_exist() {
    let out = cant(&[
        "sigil",
        "no-such-file.cant",
        "--theme",
        "chartreuse",
        "-o",
        "-",
    ]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("chartreuse"), "{}", stderr(&out));
}

#[test]
fn a_missing_source_is_a_usage_error() {
    let out = cant(&["sigil", "-o", "-"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("no source"), "{}", stderr(&out));
}

#[test]
fn a_source_and_an_expression_together_are_a_usage_error() {
    let out = cant(&["sigil", EXAMPLE, "-e", "[1] -> []", "-o", "-"]);
    assert_eq!(code(&out), 2);
}

/// A program that does not parse fails with Cant's own exit status — a syntax
/// error is a syntax error whichever command found it.
#[test]
fn an_unparseable_program_fails_with_cants_parse_status() {
    let out = cant(&["sigil", "-e", "[1] -> |{", "-o", "-"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(stderr(&out).contains("CANT-"), "{}", stderr(&out));
}

/// A graph over the cap is refused with a diagnostic that names a way out.
#[test]
fn a_graph_over_the_node_cap_is_refused() {
    let out = cant(&["sigil", EXAMPLE, "--max-nodes", "1", "-o", "-"]);
    assert_eq!(code(&out), 4, "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("SIGIL-S001"), "{err}");
    assert!(
        err.contains("cant graph") || err.contains("simplify"),
        "{err}"
    );
}

/// A graph document that is not a Cant graph is refused by name.
#[test]
fn a_foreign_graph_document_is_refused() {
    let path = std::env::temp_dir().join("cant-sigil-foreign.json");
    std::fs::write(&path, r#"{"schema":"something.else","version":"1"}"#).expect("write");
    let out = cant(&["sigil", "--graph", path.to_str().expect("utf-8"), "-o", "-"]);
    let _ = std::fs::remove_file(&path);
    assert_eq!(code(&out), 4, "{}", stderr(&out));
    assert!(stderr(&out).contains("SIGIL-V001"), "{}", stderr(&out));
}

/// `--simplify` renders, and produces a smaller artifact than the full form.
#[test]
fn simplify_produces_a_smaller_artifact() {
    let full = cant(&["sigil", "examples/sigil/complex.cant", "-o", "-"]);
    let simple = cant(&[
        "sigil",
        "examples/sigil/complex.cant",
        "--simplify",
        "-o",
        "-",
    ]);
    assert_eq!(code(&simple), 0, "{}", stderr(&simple));
    assert!(
        stdout(&simple).len() < stdout(&full).len(),
        "--simplify did not simplify anything"
    );
}

/// PNG and HTML are formats now, and each is what it claims to be.
#[test]
fn png_and_html_are_formats() {
    let dir = std::env::temp_dir().join("cant-sigil-formats");
    let _ = std::fs::create_dir_all(&dir);

    let png = dir.join("a.png");
    let out = cant(&[
        "sigil",
        EXAMPLE,
        "--format",
        "png",
        "-o",
        png.to_str().expect("utf-8"),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let bytes = std::fs::read(&png).expect("read png");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");

    let out = cant(&["sigil", EXAMPLE, "--format", "html", "-o", "-"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let page = stdout(&out);
    assert!(
        page.starts_with("<!doctype html>"),
        "{}",
        &page[..60.min(page.len())]
    );
    assert!(
        page.contains("id=\"sigil-codex\""),
        "no Codex in the export"
    );
    assert!(page.contains("<svg"), "no inline SVG in the export");
    let _ = std::fs::remove_dir_all(&dir);
}

/// §16.3: the interactive export works offline. Nothing to fetch.
#[test]
fn an_html_export_references_nothing_remote() {
    let out = cant(&["sigil", EXAMPLE, "--format", "html", "-o", "-"]);
    let page = stdout(&out).to_lowercase();
    let without_ns = page.replace("xmlns=\"http://www.w3.org/2000/svg\"", "");
    for banned in ["http://", "https://", "<link", "@import", "fetch("] {
        assert!(!without_ns.contains(banned), "remote reference `{banned}`");
    }
}

/// A veiled HTML export is a veiled picture with a decodable Codex beside it —
/// §13.4's web default — so the canvas draws nothing and the panel decodes.
#[test]
fn a_veiled_html_export_still_has_a_populated_codex() {
    let out = cant(&["sigil", EXAMPLE, "--format", "html", "-o", "-"]);
    let page = stdout(&out);
    assert!(!page.contains("<text"), "the veiled canvas drew text");
    assert!(page.contains("class=\"kind\""), "the Codex is empty");
}

/// `--ornament` is accepted at every level and changes how much is drawn.
#[test]
fn every_ornament_level_is_accepted_and_changes_the_density() {
    let mut sizes = Vec::new();
    for level in ["none", "sparse", "ritual", "maximal"] {
        let out = cant(&["sigil", EXAMPLE, "--ornament", level, "-o", "-"]);
        assert_eq!(code(&out), 0, "{level}: {}", stderr(&out));
        // The class *attribute*, not the stylesheet rule — `.sigil-ornament{…}`
        // is emitted whatever the level, and counting it would make `none`
        // look like it drew one.
        sizes.push(stdout(&out).matches("class=\"sigil-ornament\"").count());
    }
    assert_eq!(sizes[0], 0, "`none` drew ornament");
    assert!(
        sizes[1] < sizes[2] && sizes[2] < sizes[3],
        "levels do not increase: {sizes:?}"
    );

    let bad = cant(&["sigil", EXAMPLE, "--ornament", "baroque", "-o", "-"]);
    assert_eq!(code(&bad), 2);
}

/// The contradictory pair warns rather than silently resolving.
#[test]
fn metadata_none_with_a_revealing_mode_warns() {
    let out = cant(&[
        "sigil",
        EXAMPLE,
        "--mode",
        "revealed",
        "--metadata",
        "none",
        "-o",
        "-",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("SIGIL-C001"), "{err}");
    assert!(
        err.contains("veiled"),
        "the warning must say what to do: {err}"
    );

    // And the sensible pairing is silent.
    let quiet = cant(&["sigil", EXAMPLE, "--metadata", "none", "-o", "-"]);
    assert!(!stderr(&quiet).contains("SIGIL-C001"), "{}", stderr(&quiet));
}

/// The command is advertised in `--help`. A subcommand nobody can discover is a
/// subcommand nobody uses.
#[test]
fn sigil_is_advertised_in_help() {
    let out = cant(&["--help"]);
    assert!(stdout(&out).contains("sigil"), "{}", stdout(&out));
}
