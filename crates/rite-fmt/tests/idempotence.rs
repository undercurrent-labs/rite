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
