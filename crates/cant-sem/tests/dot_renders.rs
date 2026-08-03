//! The DOT output must actually render.
//!
//! Structural assertions (`dot.rs`'s unit tests) prove the text has the shape we
//! meant. They cannot prove Graphviz accepts it — a stray unescaped quote, a bad
//! attribute name, or a malformed cluster produces text that looks right and
//! fails, or worse, renders with a warning nobody sees because it went to stderr.
//!
//! So this shells out to `dot` and requires **exit 0 and empty stderr**. The
//! empty-stderr half is the point: Graphviz warns rather than fails on most
//! mistakes, so a test that only checked the status would pass on output that is
//! quietly wrong.
//!
//! # When `dot` is missing
//!
//! The test prints a note and returns. That is a real hole — a machine without
//! Graphviz gets no coverage here — and printing it is how anyone reading a CI
//! log finds out, rather than a green tick implying a check that never ran.
//! Install `graphviz` to close it.

use cant_syntax::parse_source;
use std::io::Write;
use std::process::{Command, Stdio};

/// One of each construct, plus the inputs most likely to break escaping.
const SOURCES: &[&str] = &[
    "a",
    "a -> b -> c",
    "[1, 2, 3] -> * -> ?{ $ % 2 = 0 } -> []",
    "5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []",
    "request -> |{ ?{ $.ok } -> handle ; ~{ children -> * } :max 8 } -> []",
    "roots -> * -> ~{ !@fs.read -> imports -> * -> resolve } :by canonical_path :max 4096 -> []",
    // Escaping: a quote inside leaf text, a backslash, and operators inside a
    // string — all of which reach the DOT label verbatim.
    r#""a \"quoted\" string" -> f"#,
    r#""back\\slash" -> g"#,
    r#"x -> replace($, "->", "|{")"#,
    // Glyphs, which are multi-byte and land in labels.
    "roots → ⋇ → ⟲⟦ !@fs.read → imports ⟧ :max 8 → ⌁",
    // Nested clusters: a fork of forks.
    "x -> |{ |{ a ; b } ; |{ c ; d } }",
    // A label long enough to exercise truncation.
    "some_extremely_long_identifier_that_will_definitely_not_fit_on_one_node -> f",
];

fn dot_available() -> bool {
    Command::new("dot")
        .arg("-V")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn render(dot_source: &str) -> Result<(), String> {
    let mut child = Command::new("dot")
        .args([
            "-Tsvg",
            "-o",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start dot: {e}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("no stdin")?
        .write_all(dot_source.as_bytes())
        .map_err(|e| format!("cannot write to dot: {e}"))?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;

    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("dot exited {:?}: {stderr}", out.status.code()));
    }
    // Graphviz warns rather than failing on most malformed input, so silence is
    // the assertion that matters.
    if !stderr.trim().is_empty() {
        return Err(format!("dot warned: {stderr}"));
    }
    Ok(())
}

#[test]
fn every_construct_renders_without_a_warning() {
    if !dot_available() {
        println!(
            "note: graphviz is not installed, so DOT output was generated but never rendered. \
             `sudo apt install graphviz` (or `brew install graphviz`) closes this gap."
        );
        return;
    }

    let mut failures = Vec::new();
    for source in SOURCES {
        let (parsed, sources) = parse_source("t.cant", source);
        assert!(
            !parsed.has_errors(),
            "{source:?} should parse:\n{}",
            parsed.diagnostics.render_all(&sources)
        );
        let graph = cant_sem::lower(&parsed.program.expect("program"), "t.cant", source.len());
        if let Err(e) = render(&cant_sem::to_dot(&graph)) {
            failures.push(format!("{source:?}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} DOT output(s) Graphviz would not accept:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// The same over every fixture in the tree, so a new one cannot introduce
/// something `dot` chokes on.
#[test]
fn every_fixture_renders_without_a_warning() {
    if !dot_available() {
        println!("note: graphviz is not installed; fixture DOT output was not rendered.");
        return;
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/cant-sem has two ancestors")
        .to_path_buf();

    let mut checked = 0;
    let mut failures = Vec::new();
    for dir in ["conformance/cant/syntax", "examples/cant"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("fixture directory") {
            let case = entry.expect("entry").path();
            if !case.is_dir() {
                continue;
            }
            for name in ["case.cant", "main.cant"] {
                let path = case.join(name);
                if !path.is_file() {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("fixture");
                let (parsed, _) = parse_source("case.cant", &source);
                let Some(ast) = parsed.program else { continue };
                let graph = cant_sem::lower(&ast, "case.cant", source.len());
                checked += 1;
                if let Err(e) = render(&cant_sem::to_dot(&graph)) {
                    failures.push(format!("{}: {e}", case.display()));
                }
            }
        }
    }
    assert!(checked > 10, "only {checked} fixtures rendered");
    assert!(failures.is_empty(), "{}", failures.join("\n  "));
}
