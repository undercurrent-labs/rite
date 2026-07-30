//! Dialect conversion and formatter property tests (V1).

use rite_fmt::{convert_source, format_with_dialect, Dialect};
use rite_syntax::{parse_both_equivalent, parse_source};

#[test]
fn convert_ascii_to_glyph_preserves_semantics() {
    let ascii = "def square(n) [[\n  return n * n\n]]\nsquare(3)\n";
    let glyph = convert_source(ascii, Dialect::Glyph).unwrap();
    assert!(glyph.text.contains('◆') || glyph.text.contains("square"));
    assert!(glyph.source_map.is_some());
    parse_both_equivalent(&glyph.text, ascii).unwrap();
}

#[test]
fn convert_glyph_to_ascii_preserves_semantics() {
    let glyph = "◆ square(n) ⟦\n  ^ n * n\n⟧\nsquare(3)\n";
    let ascii = convert_source(glyph, Dialect::Ascii).unwrap();
    assert!(
        ascii.text.contains("def") || ascii.text.contains("return") || ascii.text.contains("[[")
    );
    parse_both_equivalent(glyph, &ascii.text).unwrap();
}

#[test]
fn format_idempotent_both_dialects() {
    let src = "x ← 1 + 2\ny ← [1, 2, 3] → sum\n";
    for d in [Dialect::Glyph, Dialect::Ascii] {
        let once = format_with_dialect(src, d).unwrap().text;
        let twice = format_with_dialect(&once, d).unwrap().text;
        assert_eq!(once, twice, "dialect {:?}", d);
    }
}

#[test]
fn preserve_dialect_returns_source() {
    let src = "weird  spacing";
    // may fail parse — preserve should still return original on parse error path
    let r = convert_source(src, Dialect::Preserve).unwrap();
    assert_eq!(r.text, src);
}

#[test]
fn strings_not_rewritten_by_convert() {
    let src = r#"s ← "use def <- -> return""#;
    let out = convert_source(src, Dialect::Glyph).unwrap().text;
    assert!(
        out.contains("use def <- -> return"),
        "string mutated: {}",
        out
    );
}

#[test]
fn pipeline_roundtrip() {
    let src = "[1, 2, 3] → sum\n";
    let ascii = convert_source(src, Dialect::Ascii).unwrap().text;
    let back = convert_source(&ascii, Dialect::Glyph).unwrap().text;
    let (pa, da, _) = parse_source("a.rite", src);
    let (pb, db, _) = parse_source("b.rite", &back);
    assert!(!da.has_errors() && !db.has_errors());
    assert_eq!(
        format!("{:?}", pa.unwrap().items.len()),
        format!("{:?}", pb.unwrap().items.len())
    );
}

/// The effect marker is part of a declaration's meaning, not decoration.
///
/// `◆! f()` says the function performs host effects, which is what makes callers
/// mark the call. Dropping it while formatting would silently turn a checked
/// effectful function into one anybody may call unmarked — so it has to survive a
/// round trip in both dialects.
#[test]
fn the_declaration_effect_marker_survives_formatting() {
    let glyph = rite_fmt::format_source("◆! f() ⟦ ^ 1 ⟧\n", false).expect("format glyph");
    assert!(glyph.contains("◆!"), "glyph dropped the marker: {glyph}");

    let ascii = rite_fmt::format_source("◆! f() ⟦ ^ 1 ⟧\n", true).expect("format ascii");
    assert!(ascii.contains("def!"), "ascii dropped the marker: {ascii}");

    // And a pure declaration must not grow one.
    let pure = rite_fmt::format_source("◆ g() ⟦ ^ 1 ⟧\n", false).expect("format pure");
    assert!(
        !pure.contains("◆!"),
        "pure function gained a marker: {pure}"
    );
}
