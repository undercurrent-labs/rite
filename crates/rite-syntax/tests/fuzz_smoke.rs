//! Property-based fuzzing of the lexer and parser: no input may panic the host
//! process, and the lexer must always make character-aligned progress.
//!
//! The hand-written sample list this file used to be is kept below as explicit
//! regressions; the generators are what catch the next one.

use proptest::prelude::*;
use rite_core::{FileId, SourceFile};
use rite_syntax::{lex, TokenKind};

/// Lex `src` and assert the invariants that hold for *every* input.
fn check_lex(src: &str) {
    let f = SourceFile::new(FileId(0), "fuzz.rite", src);
    let (toks, _) = lex(&f);

    // Always terminated by exactly one Eof, and never empty.
    assert_eq!(
        toks.last().map(|t| t.kind),
        Some(TokenKind::Eof),
        "no Eof for {src:?}"
    );
    assert_eq!(
        toks.iter().filter(|t| t.kind == TokenKind::Eof).count(),
        1,
        "more than one Eof for {src:?}"
    );

    let mut prev_end = 0usize;
    for t in &toks {
        let (start, end) = (t.span.start.as_usize(), t.span.end.as_usize());
        assert!(start <= end && end <= src.len(), "bad span in {src:?}");
        // The root cause of the multi-byte panics: a scan that stopped inside a
        // character. Spans are byte offsets, so they must be char boundaries.
        assert!(
            src.is_char_boundary(start) && src.is_char_boundary(end),
            "token {:?} span {start}..{end} splits a character in {src:?}",
            t.kind
        );
        assert!(start >= prev_end, "tokens went backwards in {src:?}");
        prev_end = end;
    }
    // Progress: only trivia/Eof may be zero-width, so a non-empty source must
    // yield at least one token besides Eof.
    if !src.trim().is_empty() {
        assert!(toks.len() > 1, "no tokens for {src:?}");
    }
}

fn check_parse(src: &str) {
    let _ = rite_syntax::parse_source("fuzz.rite", src);
}

/// Arbitrary Unicode text, biased towards the shapes that hurt: multi-byte
/// characters, delimiters, escapes and newlines.
fn text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        // Arbitrary characters, including astral planes and combining marks.
        any::<String>(),
        // A palette of characters that interact with the scanners.
        proptest::collection::vec(
            prop_oneof![
                Just("é"),
                Just("🚀"),
                Just("✓"),
                Just("\u{a0}"),
                Just("\u{200b}"),
                Just("ß"),
                Just("日"),
                Just("\\"),
                Just("\""),
                Just("\'"),
                Just("`"),
                Just("{"),
                Just("}"),
                Just("*"),
                Just("/"),
                Just("\n"),
                Just(" "),
                Just("\t"),
                Just("◆"),
                Just("⟦"),
                Just("\\u{"),
                Just("\\{"),
                Just("\"\"\""),
            ],
            0..24,
        )
        .prop_map(|v| v.concat()),
    ];
    proptest::collection::vec(piece, 1..3).prop_map(|v| v.concat())
}

/// Wrap arbitrary text in each lexical context that scans raw text, both
/// properly closed and truncated at end of input.
fn wrapped_text() -> impl Strategy<Value = String> {
    (text(), 0usize..14).prop_map(|(s, ctx)| match ctx {
        0 => format!("/* {s} */"),
        1 => format!("/* outer /* {s} */ still outer */"),
        2 => format!("/* {s}"),
        3 => format!("// {s}\n1"),
        4 => format!("/// {s}\n1"),
        5 => format!("\"\"\"\n{s}\n\"\"\""),
        6 => format!("\"\"\"{s}"),
        7 => format!("r\"{s}\""),
        8 => format!("r\"{s}"),
        9 => format!("\"{s}\""),
        10 => format!("\"{s}"),
        11 => format!("`{s}`"),
        12 => format!("`{s}"),
        _ => format!("x <- \"{s}\" // {s}\n/* {s} */"),
    })
}

/// Random soup of real lexical fragments — exercises the token dispatch rather
/// than the raw-text scanners.
fn fragment_soup() -> impl Strategy<Value = String> {
    let fragment = prop_oneof![
        Just("◆"),
        Just("def"),
        Just("<-"),
        Just("←"),
        Just("→"),
        Just("⟦"),
        Just("⟧"),
        Just("⟨"),
        Just("⟩"),
        Just("<<"),
        Just(">>"),
        Just("~"),
        Just("?"),
        Just("!"),
        Just("@fs.read"),
        Just("host."),
        Just("#ok"),
        Just(":error"),
        Just("not in"),
        Just("0x"),
        Just("0b1"),
        Just("1_0.5e-3"),
        Just("..."),
        Just("..="),
        Just("\"s {x} \\{y}\""),
        Just("\"\"\"\né\n\"\"\""),
        Just("r\"raw é\""),
        Just("`quoted é`"),
        Just("/* é */"),
        Just("// é\n"),
        Just("é"),
        Just("🚀"),
        Just("$"),
        Just("_"),
        Just("|"),
        Just("("),
        Just(")"),
        Just("{"),
        Just("}"),
        Just(" "),
        Just("\n"),
    ];
    proptest::collection::vec(fragment, 0..40).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(384))]

    #[test]
    fn lexer_never_panics_on_arbitrary_text(s in text()) {
        check_lex(&s);
        check_parse(&s);
    }

    #[test]
    fn lexer_never_panics_inside_any_lexical_context(s in wrapped_text()) {
        check_lex(&s);
        check_parse(&s);
    }

    #[test]
    fn lexer_never_panics_on_fragment_soup(s in fragment_soup()) {
        check_lex(&s);
        check_parse(&s);
    }

    /// Truncating a valid-ish program anywhere — including mid-character — must
    /// still lex and parse without panicking.
    #[test]
    fn prefixes_of_unicode_programs_never_panic(cut in 0usize..80) {
        let src = "◆ greet(name) → \"hi {name} é 🚀\"\n/* résumé 🚀 */\nx <- \"\"\"\n  Café\n  \"\"\"\n! @console.println(greet(\"Zed\"))\n";
        let mut end = cut.min(src.len());
        while end > 0 && !src.is_char_boundary(end) {
            end -= 1;
        }
        check_lex(&src[..end]);
        check_parse(&src[..end]);
    }
}

#[test]
fn lexer_explicit_samples_no_panic() {
    let samples = [
        "",
        "\u{feff}",
        "<<<>>>",
        "def x <- ",
        "◆←→?~!@#⟦⟧⟨⟩∈∉",
        "\"unterminated",
        "/* unclosed",
        "0x",
        "0b",
        "host.",
        "((((((",
        // BUG 1: multi-byte characters in raw-text contexts.
        "/* résumé of the algorithm */",
        "/* ship it 🚀 */",
        "/* a /* 🚀 */ é */",
        "x <- \"\"\"\nCafé menu\n\"\"\"",
        "\"\"\"🚀",
        "// é 🚀",
        "r\"é 🚀\"",
        "`é 🚀`",
        // BUG 2: de-indent at a multi-byte leading character.
        "\"\"\"\n a\n\u{a0}\n b\n\"\"\"",
        "\"\"\"\n\u{a0}\n\"\"\"",
        // BUG 3: escaped vs. interpolated braces.
        "\"\\{name}\"",
        "\"{name}\"",
        "\"\\{\"",
        "\"\\}\"",
        "\"{{}}\"",
        // BUG 4: unterminated escaped identifier.
        "`unterminated",
    ];
    for s in samples {
        check_lex(s);
        check_parse(s);
    }
}

#[test]
fn parser_garbage_no_panic() {
    let samples = [
        "",
        "????",
        "⟦⟦⟦",
        "def",
        "→→→",
        "match",
        "@",
        "1 2 3 4 5",
        "{ |x|",
        "⟨a:",
        "◆ é(🚀) → ⟦",
        "~ é ⟦ #ok → ⟧",
    ];
    for s in samples {
        check_parse(s);
    }
}
