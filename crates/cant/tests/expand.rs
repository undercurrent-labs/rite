//! Expansion: the generated Rite, and what Rite makes of it.
//!
//! The load-bearing test here is [`every_expansion_passes_rite_check`] — the
//! specification's Phase 4 acceptance, and the only thing that proves generated
//! Rite is *Rite* rather than something that merely looks like it. Everything
//! else is a property of the text.
//!
//! Executing the generated Rite is Phase 5's job and lives in the differential
//! harness; this file stops at "Rite accepts it".

use cant::{check, expand};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant has two ancestors")
        .to_path_buf()
}

fn rite_of(source: &str) -> String {
    let (expansion, analysis) = expand("t.cant", source);
    expansion
        .unwrap_or_else(|| panic!("{source:?} should expand:\n{}", analysis.render()))
        .rite
}

/// Programs that must be *whole* — resolvable, not merely parseable.
///
/// `conformance/cant/syntax` is deliberately excluded. Those fixtures exist to
/// prove the parser handles a shape, and several use names like `square` and
/// `resolve` that no module defines; requiring them to resolve would conflate
/// "does this parse" with "does this run", and the answer to the second is what
/// `conformance/cant/lowering` is for.
fn corpus() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = EXTRA
        .iter()
        .map(|s| ("<inline>".to_string(), s.to_string()))
        .collect();
    for dir in ["conformance/cant/lowering", "examples/cant"] {
        let root = repo_root().join(dir);
        for entry in std::fs::read_dir(&root).expect("fixture directory") {
            let case = entry.expect("entry").path();
            if !case.is_dir() {
                continue;
            }
            for name in ["case.cant", "main.cant"] {
                let path = case.join(name);
                if path.is_file() {
                    out.push((
                        path.display().to_string(),
                        std::fs::read_to_string(&path).expect("fixture"),
                    ));
                }
            }
        }
    }
    assert!(
        out.len() > 15,
        "corpus is suspiciously small: {}",
        out.len()
    );
    out
}

/// The generated header names the tool that wrote the file, and that line is the
/// one part of an expansion a release changes without anyone touching the
/// lowering.
///
/// Normalized rather than removed from the output: a reader who finds one of
/// these files in a build directory needs to know which `cant` produced it. But
/// comparing it would mean regenerating every golden on every version bump, and a
/// bulk regeneration is exactly how a real lowering change slips through
/// unreviewed.
fn without_tool_version(rite: &str) -> String {
    rite.lines()
        .map(|line| {
            if line.starts_with("// Generated from ") {
                match line.split_once(" by cant ") {
                    Some((head, _)) => format!("{head} by cant <version>. Do not edit."),
                    None => line.to_string(),
                }
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The golden expansions in `conformance/cant/lowering/*/expected.rite`.
///
/// A golden file is worth the maintenance here: expansion is deterministic, and
/// a diff of these is the clearest possible record of what a change to the
/// lowering actually did.
#[test]
fn every_lowering_fixture_matches_its_golden_expansion() {
    let root = repo_root().join("conformance/cant/lowering");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("lowering fixtures")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no lowering fixtures");

    for case in cases {
        let path = case.join("case.cant");
        let source = std::fs::read_to_string(&path).expect("case.cant");
        let expected = std::fs::read_to_string(case.join("expected.rite")).expect("expected.rite");
        // The *logical* name, not the path: the generated header and the runtime
        // messages embed it, and a golden that depended on where the repository
        // happens to live would fail on every other machine.
        let (expansion, analysis) = expand("case.cant", &source);
        let expansion = expansion
            .unwrap_or_else(|| panic!("{} should expand:\n{}", case.display(), analysis.render()));
        assert_eq!(
            without_tool_version(&expansion.rite),
            without_tool_version(&expected),
            "{} expanded differently — if that was intended, regenerate with\n  \
             (cd {} && cant expand case.cant > expected.rite)",
            case.display(),
            case.display()
        );
        let _ = &path;
    }
}

const EXTRA: &[&str] = &[
    "3",
    "3 -> $ + 1",
    "[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []",
    r#""-" -> join(["a", "b"], $)"#,
    "5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []",
    "[ [1, 2], [3, 4], [5] ] -> * -> sum -> []",
    "[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :max 64 -> []",
    "4 -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []",
    r#""p" -> !@fs.read"#,
    r#"lines("a\nbb") -> * -> ?{ count($) > 1 } -> upper -> []"#,
];

// ---- the acceptance criterion

/// **Phase 4's acceptance.** Every generated program is valid Rite: it parses,
/// it resolves, and its effect declarations are honest.
#[test]
fn every_expansion_passes_rite_check() {
    let mut failures = Vec::new();
    for (name, source) in corpus() {
        let result = check(&name, &source);
        if result.has_errors() {
            failures.push(format!("{name}: {}", result.render()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} program(s) whose expansion Rite rejected:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---- properties of the generated text

#[test]
fn expansion_is_deterministic() {
    for (_, source) in corpus() {
        assert_eq!(rite_of(&source), rite_of(&source));
    }
}

#[test]
fn names_are_hygienic_and_differ_between_programs() {
    let a = expand("t.cant", "a -> b").0.expect("expand");
    let b = expand("t.cant", "a -> c").0.expect("expand");
    assert_ne!(
        a.prefix, b.prefix,
        "two programs generated the same helper names"
    );
    assert!(a.prefix.starts_with("cant_"));
    // Prefix, source hash, node number — a prefix alone is not enough.
    assert!(a.rite.contains(&format!("{}_n0", a.prefix)));
    assert!(a.rite.contains(&format!("{}_main", a.prefix)));
}

/// A user identifier that happens to look generated must not collide.
#[test]
fn a_user_name_cannot_collide_with_a_generated_one() {
    let source = "cant_deadbeef_n0 -> upper";
    let expansion = expand("t.cant", source).0.expect("expand");
    // The user's name survives verbatim as the source stage's expression, and
    // the generated ones carry *this* source's hash, which is not `deadbeef`.
    assert!(
        expansion.rite.contains("^ [ cant_deadbeef_n0 ]"),
        "{}",
        expansion.rite
    );
    assert_ne!(expansion.prefix, "cant_deadbeef");
    assert!(!expansion.rite.contains("def cant_deadbeef_n0("));
}

/// ADR 0002's central claim, checked on the generated text.
#[test]
fn effectful_calls_stay_structurally_visible() {
    let rite = rite_of(r#""p" -> !@fs.read"#);
    // The call is in a function body, marked, as a direct call.
    assert!(rite.contains("!@fs.read(__e)"), "{rite}");
    assert!(
        rite.contains("def! "),
        "the holder is declared effectful: {rite}"
    );
    // Nothing is handed to a higher-order helper.
    assert!(!rite.contains("each("), "{rite}");
}

#[test]
fn effect_ness_propagates_out_of_a_fork_or_orbit() {
    for source in [
        r#"x -> |{ a ; !@fs.read }"#,
        r#"x -> ~{ !@fs.read } :max 4"#,
        r#"x -> |{ a ; ~{ !@fs.read } :max 4 }"#,
    ] {
        let rite = rite_of(source);
        // The enclosing generated function and `main` must both be `def!`, or
        // Rite's resolver rejects the expansion — which is what
        // `every_expansion_passes_rite_check` would catch. This asserts the
        // intent directly.
        assert!(
            rite.contains("def! "),
            "{source:?} produced no `def!`:\n{rite}"
        );
        assert!(
            rite.contains("_main()\n") && rite.contains("! "),
            "{source:?} did not mark its call sites:\n{rite}"
        );
    }
}

#[test]
fn a_pure_program_declares_nothing_effectful() {
    let rite = rite_of("[1, 2] -> * -> ?{ $ > 1 } -> []");
    assert!(!rite.contains("def!"), "{rite}");
}

/// Orbit's bound is in the generated code, not merely in the graph.
#[test]
fn the_orbit_limit_reaches_the_generated_rite() {
    let rite = rite_of("r -> ~{ d } :max 7");
    assert!(rite.contains("__accepted > 7"), "{rite}");
    assert!(
        rite.contains("panic("),
        "reaching the limit must fail: {rite}"
    );
    // And the default when none is written.
    let rite = rite_of("r -> ~{ d }");
    assert!(rite.contains("__accepted > 1024"), "{rite}");
}

#[test]
fn scatter_checks_its_input_so_the_failure_names_the_operator() {
    let rite = rite_of("xs -> *");
    assert!(rite.contains("type_of(__e) != \"list\""), "{rite}");
    assert!(rite.contains("scatter expected a list"), "{rite}");
}

#[test]
fn the_program_boundary_normalizes_zero_one_and_many() {
    let rite = rite_of("a -> b");
    assert!(rite.contains("if (__n = 0) [[ ^ none ]]"), "{rite}");
    assert!(rite.contains("if (__n = 1) [[ ^ first(__in) ]]"), "{rite}");
}

// ---- source maps

#[test]
fn every_leaf_maps_precisely() {
    let source = "[1, 2] -> * -> ?{ $ > 1 } -> square -> []";
    let expansion = expand("t.cant", source).0.expect("expand");
    let precise: Vec<_> = expansion
        .map
        .mappings()
        .iter()
        .filter(|m| m.precise)
        .collect();
    // Source, ward predicate, and the `square` stage.
    assert_eq!(precise.len(), 3, "{precise:#?}");
    for mapping in precise {
        let text = &source[mapping.cant.start.as_usize()..mapping.cant.end.as_usize()];
        assert!(
            !text.is_empty(),
            "a precise mapping points at nothing: {mapping:?}"
        );
    }
}

#[test]
fn source_map_spans_are_monotonic_and_in_bounds() {
    for (_, source) in corpus() {
        let Some(expansion) = expand("t.cant", &source).0 else {
            continue;
        };
        for mapping in expansion.map.mappings() {
            assert!(
                mapping.cant.end.as_usize() <= source.len(),
                "cant span out of bounds in {source:?}"
            );
            assert!(
                mapping.rite.end.as_usize() <= expansion.rite.len(),
                "rite span out of bounds in {source:?}"
            );
            assert!(mapping.cant.start <= mapping.cant.end);
            assert!(mapping.rite.start <= mapping.rite.end);
        }
        // Structural regions are emitted in node order, so their generated spans
        // never run backwards.
        let structural: Vec<_> = expansion
            .map
            .mappings()
            .iter()
            .filter(|m| !m.precise)
            .collect();
        for pair in structural.windows(2) {
            assert!(
                pair[0].rite.start <= pair[1].rite.start,
                "generated regions are out of order in {source:?}"
            );
        }
    }
}

#[test]
fn a_generated_position_resolves_back_to_the_cant_that_produced_it() {
    let source = "[1, 2] -> * -> ?{ $ > 1 } -> []";
    let expansion = expand("t.cant", source).0.expect("expand");
    // The ward's condition in the generated text.
    let at = expansion.rite.find("__e > 1").expect("the ward condition");
    let mapping = expansion
        .map
        .to_cant(rite_core::Span::from_range(at, at + 7))
        .expect("a mapping");
    assert!(mapping.precise);
    assert_eq!(
        &source[mapping.cant.start.as_usize()..mapping.cant.end.as_usize()],
        "$ > 1"
    );
}

// ---- diagnostics from Rite point at Cant

#[test]
fn an_unmarked_host_call_is_reported_once_at_the_cant_source() {
    let result = check("t.cant", r#""data.json" -> @fs.read"#);
    assert!(result.has_errors());
    let errors: Vec<_> = result.diagnostics.errors().collect();
    assert_eq!(
        errors.len(),
        1,
        "the cascade through generated functions should collapse: {:?}",
        errors.iter().map(|d| &d.title).collect::<Vec<_>>()
    );
    let diagnostic = errors[0];
    assert_eq!(diagnostic.code.to_string(), "CANT-S001");
    // Points at the user's text, not at a generated identifier.
    let rendered = result.render();
    assert!(rendered.contains("@fs.read"), "{rendered}");
    assert!(
        !rendered.contains("cant_"),
        "a generated name leaked: {rendered}"
    );
    // And the Rite code travels with it.
    let origin = diagnostic.rite.as_ref().expect("rite origin");
    assert_eq!(origin.code, "E021");
}

#[test]
fn an_undefined_name_points_at_the_stage_that_used_it() {
    let result = check("t.cant", "3 -> square");
    assert!(result.has_errors());
    let rendered = result.render();
    assert!(rendered.contains("CANT-S002"), "{rendered}");
    assert!(rendered.contains("square"), "{rendered}");
    assert_eq!(result.exit_code(), 4);
}

/// A leaf Cant accepts and Rite cannot parse is the user's problem, reported
/// against their leaf — not a Cant bug, and not a syntax error in the Cant.
#[test]
fn a_leaf_that_is_not_valid_rite_is_reported_against_the_leaf() {
    let result = check("t.cant", "[[1, 2], [3]] -> * -> sum -> []");
    assert!(result.has_errors());
    let rendered = result.render();
    assert!(rendered.contains("CANT-S004"), "{rendered}");
    assert_eq!(result.exit_code(), 4, "semantic, not a parse failure");
}

#[test]
fn a_program_rejected_before_expansion_is_not_expanded() {
    // Expanding a program Cant has already refused would print a guess as
    // though it were the program.
    let (expansion, analysis) = expand("t.cant", "rows -> ?{ !@fs.exists($) }");
    assert!(analysis.has_errors());
    assert!(expansion.is_none());
}

// ---- json diagnostics carry the Rite metadata

#[test]
fn json_diagnostics_preserve_the_underlying_rite_code_and_span() {
    let result = check("t.cant", r#""data.json" -> @fs.read"#);
    let json = result.diagnostics.to_json();
    let first = &json[0];
    assert_eq!(first["code"], serde_json::json!("CANT-S001"));
    assert_eq!(first["rite"]["code"], serde_json::json!("E021"));
    assert!(first["rite"]["span"].is_object(), "{first}");
    assert!(first["labels"][0]["span"].is_object(), "{first}");
}

// ---- the golden comparison's one exemption

/// `without_tool_version` normalizes the header line and nothing else.
///
/// The risk it introduces is that it hides a difference it was not meant to
/// hide, so: the version really is what changes, and a changed body still shows.
#[test]
fn only_the_tool_version_is_normalized_away() {
    let a = "// Generated from case.cant by cant 0.6.2. Do not edit.\n^ 1\n";
    let b = "// Generated from case.cant by cant 9.9.9. Do not edit.\n^ 1\n";
    assert_ne!(a, b, "the version is embedded, or this test proves nothing");
    assert_eq!(without_tool_version(a), without_tool_version(b));

    let changed_body = "// Generated from case.cant by cant 9.9.9. Do not edit.\n^ 2\n";
    assert_ne!(
        without_tool_version(a),
        without_tool_version(changed_body),
        "a real lowering change must still fail the comparison"
    );

    // And the header the tool actually writes is the shape being matched — a
    // reworded header would silently stop being normalized, which is fine, but a
    // reworded header that no longer *contains* the version would not be.
    let (expansion, _) = expand("case.cant", "1 -> $ + 1");
    let header = expansion.expect("expands").rite;
    let header = header.lines().next().expect("a header line");
    assert!(header.starts_with("// Generated from "), "{header}");
    assert!(header.contains(env!("CARGO_PKG_VERSION")), "{header}");
}
