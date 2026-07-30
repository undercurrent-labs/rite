//! Documentation extraction and site generation.
//!
//! Deliberately a separate binary from `doctest_smoke.rs`: that one has to `chdir` to
//! resolve `skills/rite/SKILL.md`, and cwd is process-global. Everything here uses
//! absolute paths, so nothing in this file depends on where it was run from.

use rite_doc::{document_script, generate, parse_doc_comment};
use std::path::Path;

// ---------------------------------------------------------------- doc comment parsing

#[test]
fn tags_are_pulled_out_of_the_prose() {
    let d = parse_doc_comment(&[
        "/// Reads a file from disk.",
        "/// Second line of prose.",
        "/// @param path Where to read from.",
        "/// @param mode How to open it.",
        "/// @returns The file text.",
        "/// @effects fs:read",
        "/// @permission fs:read=./data",
    ]);

    assert_eq!(d.text, "Reads a file from disk.\nSecond line of prose.");
    assert_eq!(
        d.params,
        vec![
            ("path".to_string(), "Where to read from.".to_string()),
            ("mode".to_string(), "How to open it.".to_string()),
        ],
        "params keep source order"
    );
    assert_eq!(d.returns.as_deref(), Some("The file text."));
    assert_eq!(d.effects, vec!["fs:read", "permission:fs:read=./data"]);
}

#[test]
fn a_module_sigil_is_stripped_like_a_declaration_sigil() {
    let d = parse_doc_comment(&["//! Module level.", "//! @returns nothing really"]);
    assert_eq!(
        d.text, "Module level.",
        "`//!` must not survive into the rendered text"
    );
    assert_eq!(d.returns.as_deref(), Some("nothing really"));
}

#[test]
fn already_stripped_lines_parse_the_same_as_raw_ones() {
    // The parser hands back doc text with the sigil already removed; the same function
    // has to read both, or wiring it to the AST would silently mangle every line.
    let raw = parse_doc_comment(&["/// Prose.", "/// @returns a value"]);
    let stripped = parse_doc_comment(&["Prose.", "@returns a value"]);
    assert_eq!(raw.text, stripped.text);
    assert_eq!(raw.returns, stripped.returns);
}

#[test]
fn fenced_examples_are_collected() {
    let d = parse_doc_comment(&[
        "/// Adds.",
        "/// ```",
        "/// add(1, 2)",
        "/// ```",
        "/// And another:",
        "/// ```",
        "/// add(3, 4)",
        "/// ```",
    ]);
    assert_eq!(d.examples, vec!["add(1, 2)\n", "add(3, 4)\n"]);
    assert_eq!(
        d.text, "Adds.\nAnd another:",
        "fences stay out of the prose"
    );
}

#[test]
fn an_unterminated_example_is_kept_not_dropped() {
    // The buffer used to fall off the end of the loop, losing the whole example. A
    // missing back-fence is a typo, and silently publishing nothing is the worst answer.
    let d = parse_doc_comment(&["/// Adds.", "/// ```", "/// add(1, 2)"]);
    assert_eq!(d.examples, vec!["add(1, 2)\n"]);
}

#[test]
fn a_comment_with_no_tags_is_all_prose() {
    let d = parse_doc_comment(&["/// Just prose."]);
    assert_eq!(d.text, "Just prose.");
    assert!(d.params.is_empty() && d.returns.is_none() && d.examples.is_empty());
}

// -------------------------------------------------------------------- script scraping

const GEO: &str = r#"//! Geometry helpers.
//! Second line.

/// Area of a circle.
/// @param radius Centre to edge.
/// @returns Area as a float.
pub ◆ circle_area(radius) ⟦
  ^ 3.14159 * radius * radius
⟧

/// Internal helper.
◆ double(x) ⟦ ^ x * 2 ⟧

◆ undocumented(x) ⟦ ^ x ⟧
"#;

#[test]
fn documented_declarations_are_extracted_with_their_tags() {
    let doc = document_script("geo.rite", GEO);

    assert_eq!(doc.path, "geo.rite");
    assert_eq!(
        doc.module_doc.as_deref(),
        Some("Geometry helpers.\nSecond line.")
    );
    assert_eq!(
        doc.functions.len(),
        2,
        "the undocumented function is not listed"
    );

    let area = &doc.functions[0];
    assert_eq!(area.name, "circle_area");
    assert!(area.is_pub);
    assert_eq!(area.signature, "circle_area(radius)");
    assert_eq!(area.params, vec!["radius"]);
    assert_eq!(area.docs, "Area of a circle.");
    assert_eq!(area.returns.as_deref(), Some("Area as a float."));
    assert_eq!(
        area.param_docs,
        vec![("radius".to_string(), "Centre to edge.".to_string())]
    );

    assert!(!doc.functions[1].is_pub, "`◆` without `pub` is private");
}

#[test]
fn the_ascii_dialect_documents_identically() {
    let ascii = r#"//! Geometry helpers.
//! Second line.

/// Area of a circle.
/// @param radius Centre to edge.
/// @returns Area as a float.
pub def circle_area(radius) [[
  return 3.14159 * radius * radius
]]

/// Internal helper.
def double(x) [[ return x * 2 ]]

def undocumented(x) [[ return x ]]
"#;
    let glyph = document_script("geo.rite", GEO);
    let plain = document_script("geo.rite", ascii);

    assert_eq!(glyph.module_doc, plain.module_doc);
    assert_eq!(glyph.functions.len(), plain.functions.len());
    for (a, b) in glyph.functions.iter().zip(&plain.functions) {
        assert_eq!(a.signature, b.signature);
        assert_eq!(a.docs, b.docs);
        assert_eq!(a.is_pub, b.is_pub);
        assert_eq!(a.param_docs, b.param_docs);
    }
}

#[test]
fn a_shebang_does_not_hide_the_module_doc() {
    let src = "#!/usr/bin/env rite\n//! A script.\n\n◆ f() ⟦ ^ 1 ⟧\n";
    assert_eq!(
        document_script("s.rite", src).module_doc.as_deref(),
        Some("A script.")
    );
}

#[test]
fn a_file_with_no_docs_yields_nothing_to_document() {
    let doc = document_script("bare.rite", "◆ f(x) ⟦ ^ x ⟧\n");
    assert!(doc.module_doc.is_none());
    assert!(doc.functions.is_empty());
}

#[test]
fn a_comment_that_is_not_a_doc_comment_is_not_documentation() {
    let doc = document_script("c.rite", "// ordinary comment\n◆ f(x) ⟦ ^ x ⟧\n");
    assert!(doc.functions.is_empty(), "`//` is not `///`");
    assert!(doc.module_doc.is_none());
}

#[test]
fn a_broken_file_does_not_take_down_the_doc_build() {
    // One unparseable script in a tree must not abort documenting the rest, so this
    // returns whatever survived rather than erroring.
    let doc = document_script("broken.rite", "◆ f( ⟦ ^^^ ⟧⟧⟧ ((");
    assert_eq!(doc.path, "broken.rite");
    let _ = doc.functions; // no panic is the assertion
}

#[test]
fn multi_parameter_signatures_render_in_order() {
    let doc = document_script(
        "m.rite",
        "/// Sums three.\n◆ add3(a, b, c) ⟦ ^ a + b + c ⟧\n",
    );
    assert_eq!(doc.functions[0].signature, "add3(a, b, c)");
    assert_eq!(doc.functions[0].params, vec!["a", "b", "c"]);
}

// ------------------------------------------------------------------- site generation

fn generated(path: Option<&Path>) -> tempfile::TempDir {
    let out = tempfile::tempdir().expect("tempdir");
    generate(path, out.path()).expect("generate docs");
    out
}

fn read(dir: &tempfile::TempDir, rel: &str) -> String {
    std::fs::read_to_string(dir.path().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[test]
fn the_reference_build_writes_every_expected_artifact() {
    let out = generated(None);
    for f in [
        "index.json",
        "search.json",
        "reference.md",
        "capabilities.md",
        "html/index.html",
    ] {
        assert!(out.path().join(f).is_file(), "missing {f}");
    }
    assert!(
        !out.path().join("scripts.md").exists(),
        "no path argument means no script reference"
    );
}

#[test]
fn the_index_lists_every_host_capability_function() {
    // Guards against the doc site and the host drifting apart: the generator must report
    // exactly what `HostCapabilities` exposes, not a hand-kept copy.
    let out = generated(None);
    let index: serde_json::Value = serde_json::from_str(&read(&out, "index.json")).expect("json");

    let host = rite_caps::HostCapabilities::with_defaults(rite_caps::PermissionSet::allow_all());
    let expected: usize = host.all_descriptors().iter().map(|(_, d)| d.len()).sum();
    let documented: usize = index["capabilities"]
        .as_array()
        .expect("capabilities array")
        .iter()
        .map(|c| c["functions"].as_array().map_or(0, |f| f.len()))
        .sum();

    assert_eq!(documented, expected, "every host function is documented");
    assert!(expected > 0, "the host exposes something to document");

    let caps_md = read(&out, "capabilities.md");
    for (name, descs) in host.all_descriptors() {
        assert!(caps_md.contains(&format!("## @{name}")), "missing @{name}");
        for d in descs {
            assert!(
                caps_md.contains(&format!("### {}", d.name)),
                "@{}.{} missing from capabilities.md",
                name,
                d.name
            );
        }
    }
}

#[test]
fn the_index_reports_the_tool_version() {
    let out = generated(None);
    let index: serde_json::Value = serde_json::from_str(&read(&out, "index.json")).expect("json");
    assert_eq!(index["version"], env!("CARGO_PKG_VERSION"));
    assert!(read(&out, "reference.md").contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn every_book_section_becomes_its_own_page_and_a_search_entry() {
    let out = generated(None);
    let index: serde_json::Value = serde_json::from_str(&read(&out, "index.json")).expect("json");
    let sections = index["sections"].as_array().expect("sections");
    assert!(sections.len() > 5, "the book has sections");

    let search = read(&out, "search.json");
    for s in sections {
        let id = s["id"].as_str().expect("id");
        assert!(
            out.path().join(format!("{id}.md")).is_file(),
            "no page for section {id}"
        );
        assert!(
            search.contains(&format!("{id}.md")),
            "{id} is not searchable"
        );
    }
}

#[test]
fn a_path_argument_documents_the_scripts_under_it() {
    let src = tempfile::tempdir().expect("srcdir");
    std::fs::write(src.path().join("geo.rite"), GEO).expect("write");
    std::fs::create_dir_all(src.path().join("nested")).expect("mkdir");
    std::fs::write(
        src.path().join("nested/more.rite"),
        "/// Nested helper.\npub ◆ helper() ⟦ ^ 1 ⟧\n",
    )
    .expect("write nested");
    // Not Rite, and must be ignored rather than parsed.
    std::fs::write(src.path().join("notes.txt"), "not rite").expect("write txt");

    let out = generated(Some(src.path()));

    let scripts = read(&out, "scripts.md");
    assert!(scripts.contains("circle_area(radius)"), "{scripts}");
    assert!(scripts.contains("Centre to edge."), "param docs render");
    assert!(scripts.contains("Returns: Area as a float."));
    assert!(scripts.contains("Nested helper."), "recurses into subdirs");
    assert!(!scripts.contains("not rite"), "non-Rite files are skipped");
    assert!(
        !scripts.contains("undocumented"),
        "undocumented functions stay out"
    );

    // and it is reachable from search and the JSON index
    assert!(read(&out, "search.json").contains("circle_area(radius)"));
    let index: serde_json::Value = serde_json::from_str(&read(&out, "index.json")).expect("json");
    assert_eq!(index["scripts"].as_array().expect("scripts").len(), 2);
    assert!(read(&out, "html/index.html").contains("Script reference"));
}

#[test]
fn a_single_file_path_documents_just_that_file() {
    let src = tempfile::tempdir().expect("srcdir");
    let file = src.path().join("geo.rite");
    std::fs::write(&file, GEO).expect("write");

    let out = generated(Some(&file));
    assert!(read(&out, "scripts.md").contains("circle_area(radius)"));
}

#[test]
fn a_path_with_nothing_documented_writes_no_script_page() {
    let src = tempfile::tempdir().expect("srcdir");
    std::fs::write(src.path().join("bare.rite"), "◆ f(x) ⟦ ^ x ⟧\n").expect("write");
    let out = generated(Some(src.path()));
    assert!(
        !out.path().join("scripts.md").exists(),
        "an empty script reference is not worth a page"
    );
}

#[test]
fn script_prose_cannot_inject_html() {
    // Doc text is author-controlled, and once it reaches the generated site it is
    // markup. Anything angle-bracketed or quoted has to arrive escaped.
    let src = tempfile::tempdir().expect("srcdir");
    std::fs::write(
        src.path().join("evil.rite"),
        "/// Pwn <script>alert(\"x\")</script> & \"quoted\".\npub ◆ f() ⟦ ^ 1 ⟧\n",
    )
    .expect("write");

    let html = read(&generated(Some(src.path())), "html/index.html");
    assert!(
        !html.contains("<script>alert"),
        "raw script tag reached the page:\n{html}"
    );
    assert!(html.contains("&lt;script&gt;"), "tag is escaped");
    assert!(html.contains("&amp;"), "ampersand is escaped");
    assert!(html.contains("&quot;"), "quote is escaped");
}

#[test]
fn generating_twice_into_one_directory_is_stable() {
    // `rite doc` is routinely re-run over its own output; the second pass must produce
    // the same bytes rather than appending or half-updating.
    let src = tempfile::tempdir().expect("srcdir");
    std::fs::write(src.path().join("geo.rite"), GEO).expect("write");
    let out = tempfile::tempdir().expect("out");

    generate(Some(src.path()), out.path()).expect("first");
    let first = read(&out, "reference.md");
    let first_scripts = read(&out, "scripts.md");

    generate(Some(src.path()), out.path()).expect("second");
    assert_eq!(first, read(&out, "reference.md"));
    assert_eq!(first_scripts, read(&out, "scripts.md"));
}

#[test]
fn output_ordering_does_not_depend_on_directory_order() {
    // Two files, generated twice: filesystem read order is not sorted, so without an
    // explicit sort the script reference would shuffle between builds.
    let src = tempfile::tempdir().expect("srcdir");
    for name in ["a.rite", "b.rite", "c.rite", "d.rite"] {
        std::fs::write(
            src.path().join(name),
            format!("/// Doc for {name}.\npub ◆ f_{}() ⟦ ^ 1 ⟧\n", &name[..1]),
        )
        .expect("write");
    }
    let one = read(&generated(Some(src.path())), "scripts.md");
    let two = read(&generated(Some(src.path())), "scripts.md");
    assert_eq!(one, two, "script order is deterministic");

    let order: Vec<usize> = ["a.rite", "b.rite", "c.rite", "d.rite"]
        .iter()
        .map(|n| one.find(n).unwrap_or_else(|| panic!("{n} missing")))
        .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted, "files appear in sorted order");
}

#[test]
fn a_missing_path_is_not_an_error() {
    let out = tempfile::tempdir().expect("out");
    generate(Some(Path::new("/definitely/not/here")), out.path())
        .expect("a missing source path degrades to the plain reference");
    assert!(out.path().join("reference.md").is_file());
}

// -------------------------------------------------------------- the committed bundle

/// The checked-in agent bundle must match what this binary would emit.
///
/// `scripts/package-skill.sh` used to keep these in step by regenerating them during
/// packaging, with whichever `rite` it could find and every error swallowed — so a stale
/// `target/release/rite` silently rewrote tracked files with its own older output. That is
/// gone; drift is now something to report rather than something a release script papers
/// over with unknown content.
#[test]
fn the_committed_capability_manifest_matches_the_host() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/rite/machine/capabilities.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return; // not a full checkout
    };
    let committed: serde_json::Value = serde_json::from_str(&text).expect("valid json");

    let host = rite_caps::HostCapabilities::with_defaults(rite_caps::PermissionSet::allow_all());
    for (name, descs) in host.all_descriptors() {
        let listed = committed
            .get(name)
            .and_then(|v| v.as_array())
            .unwrap_or_else(|| {
                panic!("@{name} missing from the committed manifest — run `rite docs build`")
            });
        assert_eq!(
            listed.len(),
            descs.len(),
            "@{name} has {} functions in the manifest and {} in the host — run `rite docs build`",
            listed.len(),
            descs.len()
        );
        for d in descs {
            assert!(
                listed
                    .iter()
                    .any(|f| f.get("name").and_then(|n| n.as_str()) == Some(d.name)),
                "@{}.{} missing from the committed manifest — run `rite docs build`",
                name,
                d.name
            );
        }
    }
}

#[test]
fn the_committed_version_manifest_matches_this_build() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/rite/machine/version.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(
        v["tool_version"].as_str(),
        Some(env!("CARGO_PKG_VERSION")),
        "the committed bundle was generated by a different build — run `rite docs build`"
    );
}
