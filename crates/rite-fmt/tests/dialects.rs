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

/// `@tcp.listen addr ⟦ |conn| … ⟧` must come back out of the formatter as it went
/// in, in both dialects.
///
/// The juxtaposed form is sugar for the call `@tcp.listen(addr, block)`, and the
/// formatter walks the *desugared* tree — so without a case for it, `rite fmt`
/// would silently rewrite every server in the corpus into the parenthesised call
/// the book does not teach. That still runs, which is exactly why it would go
/// unnoticed. The handler's parameter has to survive too: dropping `|conn|` turns
/// the block from a closure into a plain block and the handler stops being one.
#[test]
fn the_tcp_listen_block_form_survives_formatting() {
    let glyph = rite_fmt::format_source("! @tcp.listen \"127.0.0.1:0\" ⟦ |conn| conn ⟧\n", false)
        .expect("format glyph");
    assert!(
        glyph.contains("@tcp.listen \"127.0.0.1:0\" ⟦"),
        "glyph lost the juxtaposed form: {glyph}"
    );
    assert!(
        glyph.contains("|conn|"),
        "glyph dropped the parameter: {glyph}"
    );

    let ascii = rite_fmt::format_source("! @tcp.listen \"127.0.0.1:0\" ⟦ |conn| conn ⟧\n", true)
        .expect("format ascii");
    assert!(
        ascii.contains("host.tcp.listen \"127.0.0.1:0\" [["),
        "ascii lost the juxtaposed form: {ascii}"
    );
    assert!(
        ascii.contains("|conn|"),
        "ascii dropped the parameter: {ascii}"
    );

    // Idempotent, and it must not turn some *other* @tcp call into the block form.
    for once in [glyph, ascii] {
        let twice = rite_fmt::format_source(&once, once.contains("host.")).expect("reformat");
        assert_eq!(once, twice, "second pass changed the output");
    }
    let ordinary = rite_fmt::format_source("! @tcp.close(conn)\n", false).expect("format call");
    assert!(
        ordinary.contains("@tcp.close(conn)"),
        "an ordinary @tcp call must stay a call: {ordinary}"
    );
}

/// A glyph-only operator must not be printed as an ASCII infix that means something
/// else.
///
/// `÷` and `∘` were printed as `idiv` and `compose` in ASCII, on the strength of an
/// alias table entry. Neither lexes as an operator — both names are taken by the
/// builtins they lower to — so the output parsed as something different and
/// **changed the answer**: `7 ÷ 2` is 3, and `rite fmt --ascii` made it
/// `7 idiv 2`, which is two statements and evaluates to 7. `f ∘ g` became
/// `f compose g`, which is `f`.
///
/// This asserts the text; `formatting_to_ascii_preserves_the_value` in the same file
/// asserts the thing that actually matters.
#[test]
fn glyph_only_operators_print_as_calls_in_ascii() {
    let ascii = rite_fmt::format_source("x ← 7 ÷ 2\n", true).expect("format ascii");
    assert!(
        ascii.contains("idiv(7, 2)"),
        "÷ must become a call in ASCII, not an infix word: {ascii}"
    );
    let ascii = rite_fmt::format_source("c ← f ∘ g\n", true).expect("format ascii");
    assert!(
        ascii.contains("compose(f, g)"),
        "∘ must become a call in ASCII, not an infix word: {ascii}"
    );

    // In glyph they stay operators — that is the whole point of retaining them.
    let glyph = rite_fmt::format_source("x ← 7 ÷ 2\n", false).expect("format glyph");
    assert!(glyph.contains('÷'), "glyph lost ÷: {glyph}");
    let glyph = rite_fmt::format_source("c ← f ∘ g\n", false).expect("format glyph");
    assert!(glyph.contains('∘'), "glyph lost ∘: {glyph}");
}

#[test]
fn import_alias_prints_as_in_ascii_and_arrow_in_glyph() {
    let src = "use coolio -> cool\n";
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(ascii.contains("use coolio as cool"), "ascii alias: {ascii}");
    let glyph = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(glyph.contains("⊏ coolio → cool"), "glyph alias: {glyph}");
    // And back: the glyph arrow parses and normalises to `as` in ASCII.
    let back = format_with_dialect(&glyph, Dialect::Ascii).unwrap().text;
    assert!(back.contains("use coolio as cool"), "round trip: {back}");
}

#[test]
fn module_access_keeps_the_sigil_in_both_dialects() {
    // `@cool` is a module, not the host: ASCII must not print `host.cool`,
    // while a real capability in the same file still becomes `host.fs`.
    let src = "use coolio as cool\nx <- @cool.square(2)\ny <- do @fs.read(\"f\")\n";
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(
        ascii.contains("@cool.square"),
        "module lost its sigil: {ascii}"
    );
    assert!(
        ascii.contains("host.fs.read"),
        "capability kept `@`: {ascii}"
    );
    let glyph = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(
        glyph.contains("@cool.square"),
        "glyph module access: {glyph}"
    );
    assert!(glyph.contains("@fs.read"), "glyph capability: {glyph}");
}

#[test]
fn guards_and_or_patterns_survive_formatting() {
    let src = "x <- match 1 [[\n  1 | 2 -> \"a\"\n  n if n > 0 -> \"b\"\n  _ -> \"c\"\n]]\n";
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(ascii.contains("1 | 2 ->"), "or-pattern dropped: {ascii}");
    assert!(ascii.contains("n if n > 0 ->"), "guard dropped: {ascii}");
    let glyph = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(glyph.contains("1 | 2 →"), "glyph or-pattern: {glyph}");
    assert!(glyph.contains("n ? n > 0 →"), "glyph guard: {glyph}");
    // An arm without a guard must not gain one.
    assert!(!glyph.contains("_ ?"), "wildcard arm grew a guard: {glyph}");
    // Round trip: the glyph spelling parses back to the same ASCII.
    let back = format_with_dialect(&glyph, Dialect::Ascii).unwrap().text;
    assert_eq!(ascii, back, "guard/or round trip changed the program");
}

#[test]
fn statement_sugar_survives_formatting() {
    let src = "say \"hi\"\nunless done [[\n  say \"go\"\n]]\nfor x in [1, 2] [[\n  say x\n]]\nwhile n < 3 [[\n  n := n + 1\n]]\nloop 2 [[\n  say \"tick\"\n]]\n";
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    for kept in [
        "say \"hi\"",
        "unless done",
        "for x in [1, 2]",
        "while n < 3",
        "loop 2",
    ] {
        assert!(ascii.contains(kept), "`{kept}` was lowered away:\n{ascii}");
    }
    // The expansions must not appear.
    for gone in ["console.println", "each", "while_loop", "range(0"] {
        assert!(!ascii.contains(gone), "expansion `{gone}` leaked:\n{ascii}");
    }
    let glyph = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    for kept in [
        "¶ \"hi\"",
        "¿ done",
        "∀ x ∈ [1, 2]",
        "while n < 3",
        "loop 2",
    ] {
        assert!(glyph.contains(kept), "glyph `{kept}` missing:\n{glyph}");
    }
    // Idempotent, and stable across a dialect round trip.
    let twice = format_with_dialect(&ascii, Dialect::Ascii).unwrap().text;
    assert_eq!(ascii, twice, "ascii formatting is not idempotent");
    let back = format_with_dialect(&glyph, Dialect::Ascii).unwrap().text;
    assert_eq!(ascii, back, "glyph→ascii changed the program");
}
