//! Formatter and converter: the layout rules, and the properties the
//! specification requires of both.
//!
//! The properties (spec §11.2) are the point of this file. Layout is taste and
//! can be argued about; "formatting twice is the same as formatting once" and
//! "converting cannot change what the program means" are not negotiable, and are
//! checked over the fixture corpus and a generator rather than over examples
//! someone happened to think of.

use cant_syntax::{
    convert, detect, format, parse_source, structure, Dialect, FormatError, FormatOptions,
};
use proptest::prelude::*;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-syntax has two ancestors")
        .to_path_buf()
}

fn opts(width: usize) -> FormatOptions {
    FormatOptions {
        max_width: width,
        ..Default::default()
    }
}

fn fmt(source: &str, width: usize) -> String {
    format(source, opts(width))
        .unwrap_or_else(|e| panic!("{source:?} should format: {e}"))
        .text
}

fn shape(source: &str) -> serde_json::Value {
    let (result, sources) = parse_source("t.cant", source);
    assert!(
        !result.has_errors(),
        "{source:?} should parse:\n{}",
        result.diagnostics.render_all(&sources)
    );
    structure(&result.program.expect("program"))
}

/// Every valid Cant source the repository has, plus the ones below.
fn corpus() -> Vec<String> {
    let mut out: Vec<String> = EXTRA.iter().map(|s| s.to_string()).collect();
    for dir in ["conformance/cant/syntax", "examples/cant"] {
        let root = repo_root().join(dir);
        for entry in std::fs::read_dir(&root).expect("fixture directory") {
            let case = entry.expect("entry").path();
            if !case.is_dir() {
                continue;
            }
            for name in ["case.cant", "main.cant"] {
                let path = case.join(name);
                if path.is_file() {
                    out.push(std::fs::read_to_string(&path).expect("fixture"));
                }
            }
        }
    }
    for dir in ["conformance/cant/dialect"] {
        let root = repo_root().join(dir);
        for entry in std::fs::read_dir(&root).expect("fixture directory") {
            let case = entry.expect("entry").path();
            for name in ["ascii.cant", "glyph.cant"] {
                let path = case.join(name);
                if path.is_file() {
                    out.push(std::fs::read_to_string(&path).expect("fixture"));
                }
            }
        }
    }
    assert!(
        out.len() > 20,
        "corpus is suspiciously small: {}",
        out.len()
    );
    out
}

const EXTRA: &[&str] = &[
    "a",
    "a -> b",
    "[1, 2, 3] -> * -> ?{ $ % 2 = 0 } -> square -> []",
    "5 -> |{ $ + 1 ; $ * 2 ; square } -> []",
    "5 -> |{ $ + 1 ; $ * 2 ; square } :par -> []",
    "roots -> * -> ~{ deps -> * } :by canonical :max 1024 -> []",
    "request -> |{ ?{ $.ok } -> handle ; ~{ children -> * } :max 8 } -> []",
    "// a comment\nx -> f",
    "x -> f // trailing\n-> g",
    "\"a -> b\" -> replace($, \"->\", \"|{\")",
    "xs -> ?{ any($, { |n| n > 0 }) } -> []",
    "rows -> * -> ?{ $.level = :error } -> .message -> []",
    "clean:{ trim -> ?{ count($) > 0 } }\n[\"a\"] -> * -> clean -> []",
    "a:{ trim }\nb:{ upper }\n[\"x\"] -> * -> a -> b -> []",
    "// above the definition\nclean:{ trim }\n[\"a\"] -> clean",
];

// ---- layout

#[test]
fn a_short_flow_stays_on_one_line() {
    assert_eq!(fmt("[1,2,3]->*->[]", 88), "[1,2,3] -> * -> []");
}

#[test]
fn a_long_flow_breaks_at_its_arrows() {
    let out = fmt("alpha -> beta -> gamma -> delta -> epsilon -> zeta", 20);
    assert_eq!(
        out,
        "alpha\n  -> beta\n  -> gamma\n  -> delta\n  -> epsilon\n  -> zeta"
    );
}

/// A stage that fits stays whole even when the flow around it does not: only
/// the flow breaks, not everything inside it.
#[test]
fn a_block_that_fits_is_not_broken_just_because_the_flow_was() {
    let out = fmt(
        "roots -> ~{ deps -> * } :max 8 -> collect_them -> finally -> []",
        30,
    );
    assert_eq!(
        out,
        "roots\n  -> ~{ deps -> * } :max 8\n  -> collect_them\n  -> finally\n  -> []"
    );
}

/// The canonical multi-line shape from the specification, character for
/// character: body hanging from the opener, closer and modifiers under it.
#[test]
fn a_broken_orbit_matches_the_canonical_layout() {
    let out = fmt(
        "roots -> * -> ~{ !@fs.read -> imports -> * -> resolve } :by canonical_path :max 4096 -> []",
        40,
    );
    assert_eq!(
        out,
        concat!(
            "roots\n",
            "  -> *\n",
            "  -> ~{\n",
            "       !@fs.read\n",
            "       -> imports\n",
            "       -> *\n",
            "       -> resolve\n",
            "     }\n",
            "     :by canonical_path\n",
            "     :max 4096\n",
            "  -> []"
        )
    );
}

#[test]
fn a_broken_fork_puts_each_branch_on_its_own_line() {
    let out = fmt(
        "request -> |{ authenticate ; audit_request ; handle_it }",
        30,
    );
    assert_eq!(
        out,
        "request\n  -> |{\n       authenticate ;\n       audit_request ;\n       handle_it\n     }"
    );
}

#[test]
fn compact_ignores_the_width() {
    let source =
        "roots -> * -> ~{ !@fs.read -> imports -> * -> resolve } :by canonical_path :max 4096 -> []";
    let out = format(
        source,
        FormatOptions {
            compact: true,
            max_width: 10,
            ..Default::default()
        },
    )
    .expect("format")
    .text;
    assert!(!out.contains('\n'), "{out}");
    assert_eq!(shape(&out), shape(source));
}

/// Leaf text is Rite's, and Cant does not parse it — so it is reproduced exactly
/// rather than re-spaced by a formatter that does not know the grammar.
#[test]
fn leaf_text_is_never_rewritten() {
    assert_eq!(fmt("xs -> f( 1,2 ,  3 )", 88), "xs -> f( 1,2 ,  3 )");
    assert_eq!(fmt("[1,2,3] -> []", 88), "[1,2,3] -> []");
}

#[test]
fn a_source_with_syntax_errors_is_refused_rather_than_guessed_at() {
    let err = format("roots -> ~{ deps", FormatOptions::default()).expect_err("unparseable");
    assert!(
        matches!(err, FormatError::Unparseable(ref c) if c == "CANT-P003"),
        "{err:?}"
    );
}

// ---- comments

#[test]
fn comments_stay_where_they_were_written() {
    let out = fmt(
        "// leading\nroots\n  -> * \n  // about the ward\n  -> ?{ $ > 0 }\n",
        20,
    );
    assert_eq!(
        out,
        "// leading\nroots\n  -> *\n  // about the ward\n  -> ?{ $ > 0 }"
    );
}

#[test]
fn no_comment_is_ever_lost() {
    for source in corpus() {
        for width in [20usize, 40, 88] {
            let Ok(result) = format(&source, opts(width)) else {
                continue;
            };
            assert_eq!(
                cant_syntax::fmt::comment_texts(&result.text),
                cant_syntax::fmt::comment_texts(&source),
                "width {width} changed the comments of {source:?}"
            );
        }
    }
}

// ---- conversion

#[test]
fn conversion_touches_only_structural_operators() {
    let source = "// a -> comment\n\"a -> string ?{ }\" -> f([]) -> ?{ $ > 0 } -> []";
    let glyph = convert(source, Dialect::Glyph);
    assert!(glyph.starts_with("// a -> comment\n"), "{glyph}");
    assert!(glyph.contains("\"a -> string ?{ }\""), "{glyph}");
    assert!(
        glyph.contains("f([])"),
        "the `[]` inside a call is not collect: {glyph}"
    );
    assert!(
        glyph.contains('→') && glyph.contains('⊣') && glyph.contains('⌁'),
        "{glyph}"
    );
}

/// Converting is idempotent: the second pass has nothing left to respell.
///
/// Stated this way rather than as "converting to the dialect a source already
/// uses changes nothing", which is false for a *mixed* source — and mixed input
/// is explicitly accepted (`conformance/cant/dialect/mixed`). Converting mixed
/// input normalizes it, which is the point.
#[test]
fn conversion_is_idempotent() {
    for source in corpus() {
        for dialect in [Dialect::Ascii, Dialect::Glyph] {
            let once = convert(&source, dialect);
            assert_eq!(
                convert(&once, dialect),
                once,
                "converting {source:?} to {dialect:?} twice differs from once"
            );
        }
    }
}

/// A source already wholly in one spelling is left byte-identical.
#[test]
fn converting_a_pure_source_to_its_own_dialect_changes_nothing() {
    for source in corpus() {
        let dialect = detect(&source);
        let normalized = convert(&source, dialect);
        if normalized != source {
            // Mixed input: normalizing is correct, so this source has nothing to
            // say about the property. `mixed/glyph.cant` is deliberately mixed.
            continue;
        }
        assert_eq!(convert(&normalized, dialect), normalized);
    }
}

#[test]
fn an_unparseable_source_still_converts_what_it_can() {
    // An editor toggling spellings mid-keystroke must not stop working.
    let out = convert("roots -> * -> ~{ deps", Dialect::Glyph);
    assert!(out.starts_with("roots → ⋇ →"), "{out}");
}

// ---- properties (spec §11.2)

#[test]
fn format_is_idempotent() {
    for source in corpus() {
        for width in [20usize, 40, 88, 200] {
            let Ok(once) = format(&source, opts(width)) else {
                continue;
            };
            let twice = format(&once.text, opts(width)).expect("formatted output re-formats");
            assert_eq!(
                twice.text, once.text,
                "width {width} is not idempotent for {source:?}"
            );
        }
    }
}

#[test]
fn format_preserves_the_program() {
    for source in corpus() {
        for width in [20usize, 88] {
            let Ok(result) = format(&source, opts(width)) else {
                continue;
            };
            assert_eq!(
                shape(&result.text),
                shape(&source),
                "width {width} changed the program of {source:?}"
            );
        }
    }
}

#[test]
fn conversion_preserves_the_program_in_both_directions() {
    for source in corpus() {
        if parse_source("t.cant", &source).0.has_errors() {
            continue;
        }
        for dialect in [Dialect::Ascii, Dialect::Glyph] {
            assert_eq!(
                shape(&convert(&source, dialect)),
                shape(&source),
                "converting {source:?} to {dialect:?} changed the program"
            );
        }
    }
}

/// `ascii(glyph(ascii(x))) == ascii(x)` — the round trip the specification names.
#[test]
fn the_ascii_glyph_round_trip_returns_to_the_same_bytes() {
    for source in corpus() {
        let ascii = convert(&source, Dialect::Ascii);
        let there_and_back = convert(&convert(&ascii, Dialect::Glyph), Dialect::Ascii);
        assert_eq!(there_and_back, ascii, "round trip changed {source:?}");
    }
}

#[test]
fn format_and_convert_commute_on_the_program() {
    // Formatting to glyphs and converting to glyphs are different operations
    // that must agree about what the program is.
    for source in corpus() {
        let Ok(formatted) = format(
            &source,
            FormatOptions {
                dialect: Dialect::Glyph,
                ..Default::default()
            },
        ) else {
            continue;
        };
        assert_eq!(
            shape(&formatted.text),
            shape(&convert(&source, Dialect::Glyph)),
            "for {source:?}"
        );
    }
}

// ---- source maps

#[test]
fn a_conversion_offset_map_is_monotonic_and_in_bounds() {
    for source in corpus() {
        for dialect in [Dialect::Ascii, Dialect::Glyph] {
            let converted = convert(&source, dialect);
            let map = cant_syntax::fmt::convert_offset_map(&source, dialect);
            let mut last_from = 0u32;
            let mut last_to = 0u32;
            for (from, to) in &map {
                assert!(
                    *from >= last_from,
                    "input offsets went backwards in {source:?}"
                );
                assert!(
                    *to >= last_to,
                    "output offsets went backwards in {source:?}"
                );
                assert!(*from as usize <= source.len(), "input offset out of bounds");
                assert!(
                    *to as usize <= converted.len(),
                    "output offset out of bounds"
                );
                last_from = *from;
                last_to = *to;
            }
            // The end of the input maps to the end of the output.
            assert_eq!(
                cant_syntax::fmt::map_offset(&map, source.len() as u32),
                converted.len() as u32,
                "end offset for {source:?}"
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The properties again, over generated programs rather than written ones.
    #[test]
    fn generated_programs_format_idempotently(
        source in proptest::collection::vec(
            proptest::sample::select(vec![
                "a", "b", "f(1)", "$ + 1", "$ * 2", r#""s""#, "!@fs.read", "*", "[]",
                "?{ $ > 0 }", "|{ a ; b }", "~{ c -> * }", ".field", "1",
            ]),
            1..8,
        ).prop_map(|parts| parts.join(" -> "))
    ) {
        if let Ok(once) = format(&source, opts(40)) {
            let twice = format(&once.text, opts(40)).expect("re-format");
            prop_assert_eq!(&twice.text, &once.text);
            // And the program is unchanged.
            let before = parse_source("a.cant", &source);
            let after = parse_source("b.cant", &once.text);
            if !before.0.has_errors() {
                prop_assert!(!after.0.has_errors());
                prop_assert_eq!(
                    structure(&before.0.program.unwrap()),
                    structure(&after.0.program.unwrap())
                );
            }
        }
    }

    #[test]
    fn generated_programs_survive_the_dialect_round_trip(
        source in proptest::collection::vec(
            proptest::sample::select(vec![
                "a", "f(1)", "$ % 2 = 0", "*", "[]", "?{ $ > 0 }", "|{ a ; b }", "~{ c }",
            ]),
            1..8,
        ).prop_map(|parts| parts.join(" -> "))
    ) {
        let ascii = convert(&source, Dialect::Ascii);
        let round = convert(&convert(&ascii, Dialect::Glyph), Dialect::Ascii);
        prop_assert_eq!(round, ascii);
    }
}

#[test]
fn use_lines_survive_formatting_in_both_dialects() {
    let source = "use mathy\n[1, 2] -> * -> mathy.square($) -> []";
    let ascii = cant_syntax::fmt::format(
        source,
        FormatOptions {
            dialect: Dialect::Ascii,
            ..Default::default()
        },
    )
    .expect("formats");
    assert!(
        ascii.text.starts_with("use mathy\n"),
        "the formatter dropped the import:\n{}",
        ascii.text
    );
    let glyph = cant_syntax::fmt::format(
        source,
        FormatOptions {
            dialect: Dialect::Glyph,
            ..Default::default()
        },
    )
    .expect("formats");
    assert!(glyph.text.starts_with("use mathy\n"), "{}", glyph.text);
    // Idempotent: formatting the formatted output changes nothing.
    let again = cant_syntax::fmt::format(
        &ascii.text,
        FormatOptions {
            dialect: Dialect::Ascii,
            ..Default::default()
        },
    )
    .expect("formats");
    assert_eq!(again.text, ascii.text);
}

// ---- definitions

#[test]
fn a_definition_is_one_line_with_the_name_against_the_brace() {
    assert_eq!(
        fmt("clean:{trim->upper}\n[\"a\"]->clean", 88),
        "clean:{ trim -> upper }\n[\"a\"] -> clean"
    );
}

#[test]
fn a_long_definition_breaks_like_any_other_block() {
    let out = fmt("clean:{ alpha -> beta -> gamma -> delta }\nx -> clean", 24);
    assert_eq!(
        out,
        "clean:{\n  alpha\n  -> beta\n  -> gamma\n  -> delta\n}\nx -> clean"
    );
    // And what came out still parses as the program that went in.
    assert_eq!(
        shape(&out),
        shape("clean:{ alpha -> beta -> gamma -> delta }\nx -> clean")
    );
}

/// Compact output is one line, which the braces make possible: nothing about a
/// definition depends on where the line breaks are.
#[test]
fn compact_puts_a_definition_and_the_flow_on_one_line() {
    let out = format(
        "clean:{ trim }\n[\"a\"] -> clean",
        FormatOptions {
            compact: true,
            ..Default::default()
        },
    )
    .expect("formats")
    .text;
    assert_eq!(out, "clean:{ trim } [\"a\"] -> clean");
    assert_eq!(shape(&out), shape("clean:{ trim }\n[\"a\"] -> clean"));
}

#[test]
fn converting_a_definition_respells_only_its_operators() {
    let source = "clean:{ trim -> upper }\n// :{ in a comment\n[\"a\"] -> clean";
    let glyph = convert(source, Dialect::Glyph);
    assert!(glyph.contains("clean≔⟦ trim → upper ⟧"), "{glyph}");
    assert!(glyph.contains("// :{ in a comment"), "{glyph}");
    assert_eq!(convert(&glyph, Dialect::Ascii), source);
}

/// A `:{` the parser read as a Rite record field is not an operator, so the
/// converter must not touch it.
#[test]
fn a_record_field_holding_a_block_survives_conversion() {
    let source = "[1] -> map($, { |n| << f:{ n } >> })";
    assert_eq!(
        convert(&convert(source, Dialect::Glyph), Dialect::Ascii),
        source
    );
    assert!(!convert(source, Dialect::Glyph).contains('≔'));
}
