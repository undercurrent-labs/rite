use rite_fmt::{comment_texts, format_source, format_with_dialect, Dialect};

#[test]
fn format_idempotent_glyph() {
    let src = "x ← 1 + 2\n";
    let once = format_source(src, false).unwrap();
    let twice = format_source(&once, false).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn format_ascii_roundtrip_parse() {
    let src = "def f(x) [[ return x * 2 ]]\n";
    let glyph = format_source(src, false).unwrap();
    let ascii = format_source(&glyph, true).unwrap();
    assert!(
        ascii.contains("def")
            || ascii.contains("<-")
            || ascii.contains("return")
            || ascii.contains("[[")
    );
    let again = format_source(&ascii, true).unwrap();
    assert_eq!(ascii, again);
}

/// Commented sources must be fixed points too — in every dialect.
#[test]
fn format_idempotent_with_comments() {
    let inputs = [
        "//! module doc\n// leading\nx <- 1 // trailing\n",
        "/// doc\ndef double(n) [[\n  // explain\n  ^ n * 2 // result\n]]\n",
        "#!/usr/bin/env rite\n// after shebang\n! @console.println(\"hi\")\n",
        "/* block */\nx <- 1\n\n// after a blank line\ny <- 2\n",
        "def f() [[\n  // dangling in an otherwise empty body\n]]\n",
        "x <- 1\n// dangling at end of file\n",
    ];
    for src in inputs {
        for d in [
            Dialect::Glyph,
            Dialect::Ascii,
            Dialect::Mixed,
            Dialect::Preserve,
        ] {
            let once = format_with_dialect(src, d).unwrap().text;
            let twice = format_with_dialect(&once, d).unwrap().text;
            assert_eq!(once, twice, "dialect {d:?} not idempotent for:\n{src}");
            assert_eq!(
                comment_texts(src),
                comment_texts(&once),
                "dialect {d:?} changed comments of:\n{src}"
            );
        }
    }
}

/// Round-tripping through the other dialect keeps the comments intact.
#[test]
fn dialect_roundtrip_keeps_comments() {
    let src = "//! doc\n◆ f(n) ⟦\n  // why\n  ^ n // out\n⟧\n";
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    let back = format_with_dialect(&ascii, Dialect::Glyph).unwrap().text;
    assert_eq!(comment_texts(src), comment_texts(&ascii));
    assert_eq!(comment_texts(src), comment_texts(&back));
    assert!(ascii.contains("def f(n) [["), "{ascii}");
    assert!(back.contains("◆ f(n) ⟦"), "{back}");
}

/// The empty `||` of a zero-argument closure has to survive formatting.
///
/// It did not: the printer emitted a parameter list only when there were
/// parameters to name, so `{ || 42 }` came back as `⟦ 42 ⟧` — the formatter
/// turning a function into its own body. Nothing failed at format time; the
/// script failed later, at the call, with `cannot call value of type int`.
#[test]
fn zero_argument_closure_keeps_its_pipes() {
    let src = "f ← ⟦ || 42 ⟧\n";
    let once = format_source(src, false).unwrap();
    assert!(
        once.contains("||"),
        "formatter dropped the empty parameter list: {once:?}"
    );
    assert_eq!(once, format_source(&once, false).unwrap());

    // And through a dialect conversion, where the delimiters change but the
    // parameter list must not.
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(
        ascii.contains("||"),
        "dialect conversion dropped it: {ascii:?}"
    );
}

/// The other half of the same distinction: a block with no `|…|` is a value, and
/// formatting must not invent a parameter list for it either.
#[test]
fn a_bare_block_gains_no_parameter_list() {
    let src = "x ← ⟦ 42 ⟧\n";
    let once = format_source(src, false).unwrap();
    assert!(!once.contains('|'), "formatter added pipes: {once:?}");
}
