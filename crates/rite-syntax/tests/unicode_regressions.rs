//! Lexer regressions: multi-byte characters used to panic the whole process,
//! multiline de-indentation byte-sliced at a character offset, `\{` could not
//! escape interpolation, and an unterminated `` ` `` was silently accepted.

use rite_core::{
    Diagnostics, FileId, SourceFile, E003_UNTERMINATED_STRING, E004_UNTERMINATED_COMMENT,
};
use rite_syntax::{lex, Token, TokenKind};

fn lex_src(src: &str) -> (Vec<Token>, Diagnostics) {
    let f = SourceFile::new(FileId(0), "t.rite", src);
    lex(&f)
}

fn kinds(src: &str) -> Vec<TokenKind> {
    let (toks, _) = lex_src(src);
    toks.into_iter()
        .filter(|t| !t.kind.is_trivia() && t.kind != TokenKind::Eof)
        .map(|t| t.kind)
        .collect()
}

fn texts_of(src: &str, kind: TokenKind) -> Vec<String> {
    let (toks, _) = lex_src(src);
    toks.into_iter()
        .filter(|t| t.kind == kind)
        .map(|t| t.text)
        .collect()
}

fn has_code(d: &Diagnostics, code: rite_core::ErrorCode) -> bool {
    d.iter().any(|x| x.code == code)
}

/// Every token boundary must land on a character boundary of the source, and
/// tokens must advance monotonically through it. A byte-at-a-time scan inside a
/// comment or multiline string broke this and panicked on the next slice.
fn assert_boundaries(src: &str) {
    let (toks, _) = lex_src(src);
    let mut prev_end = 0usize;
    for t in &toks {
        let (start, end) = (t.span.start.as_usize(), t.span.end.as_usize());
        assert!(start <= end, "inverted span {start}..{end} in {src:?}");
        assert!(end <= src.len(), "span {start}..{end} past end of {src:?}");
        assert!(
            src.is_char_boundary(start) && src.is_char_boundary(end),
            "token {:?} span {start}..{end} is not on a char boundary in {src:?}",
            t.kind
        );
        assert!(
            start >= prev_end,
            "token {:?} at {start} overlaps previous token ending at {prev_end} in {src:?}",
            t.kind
        );
        prev_end = end;
    }
    assert_eq!(toks.last().map(|t| t.kind), Some(TokenKind::Eof));
}

// ---------------------------------------------------------------- BUG 1 ------

#[test]
fn block_comment_with_multibyte_chars() {
    let src = "/* résumé of the algorithm */\n! @console.println(\"ok\")\n";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors(), "unexpected diagnostics: {:?}", d.len());
    assert_eq!(
        texts_of(src, TokenKind::Comment),
        vec!["/* résumé of the algorithm */"]
    );
    assert_eq!(
        kinds(src),
        vec![
            TokenKind::Effect,
            TokenKind::Host,
            TokenKind::Ident,
            TokenKind::Dot,
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::String,
            TokenKind::RParen,
        ]
    );
    assert_boundaries(src);
}

#[test]
fn block_comment_with_astral_char() {
    let src = "/* ship it 🚀 */\n1\n";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors());
    assert_eq!(kinds(src), vec![TokenKind::Int]);
    assert_boundaries(src);
}

#[test]
fn nested_block_comment_with_multibyte_chars() {
    let src = "1 /* é /* 🚀 ✓ */ ø */ 2";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors());
    assert_eq!(kinds(src), vec![TokenKind::Int, TokenKind::Int]);
    assert_boundaries(src);
}

#[test]
fn unterminated_block_comment_with_multibyte_chars() {
    let src = "/* résumé 🚀";
    let (_, d) = lex_src(src);
    assert!(has_code(&d, E004_UNTERMINATED_COMMENT));
    assert_boundaries(src);
}

#[test]
fn multiline_string_with_multibyte_chars() {
    let src = "x <- \"\"\"\nCafé menu 🚀\n\"\"\"\n";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors());
    assert_eq!(
        texts_of(src, TokenKind::MultilineString),
        vec!["Café menu 🚀"]
    );
    assert_boundaries(src);
}

#[test]
fn unterminated_multiline_string_with_multibyte_chars() {
    let src = "x <- \"\"\"\nCafé menu";
    let (_, d) = lex_src(src);
    assert!(has_code(&d, E003_UNTERMINATED_STRING));
    assert_boundaries(src);
}

#[test]
fn multibyte_chars_in_every_raw_text_context() {
    // Each of these scans raw text and must survive multi-byte input, closed or
    // not, without panicking or leaving `pos` mid-character.
    for src in [
        "// é 🚀\n1",
        "/* é 🚀 */ 1",
        "/* é /* 🚀 */ */ 1",
        "/* é 🚀",
        "\"\"\"é 🚀\"\"\"",
        "\"\"\"é 🚀",
        "r\"é 🚀\"",
        "r\"é 🚀",
        "\"é 🚀\"",
        "\"é 🚀",
        "`é 🚀`",
        "`é 🚀",
        "◆ é 🚀 ⟦⟧",
        "\"\"\"\n\u{a0}é\n\"\"\"",
    ] {
        assert_boundaries(src);
        let _ = rite_syntax::parse_source("t.rite", src);
    }
}

#[test]
fn whole_program_with_unicode_comment_parses() {
    let (program, d, _) = rite_syntax::parse_source(
        "t.rite",
        "/* résumé of the algorithm */\n! @console.println(\"ok\")\n",
    );
    assert!(program.is_some());
    assert!(!d.has_errors());
}

// ---------------------------------------------------------------- BUG 2 ------

#[test]
fn multiline_deindent_is_char_safe() {
    // `min_indent` is 1 (from " a"); the middle line is a single two-byte NBSP,
    // which a byte slice at offset 1 used to split in half and panic on.
    let src = "x <- \"\"\"\n a\n\u{a0}\n b\n\"\"\"\n";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors());
    assert_eq!(
        texts_of(src, TokenKind::MultilineString),
        vec!["a\n\u{a0}\nb"]
    );
    assert_boundaries(src);
}

#[test]
fn multiline_deindent_strips_only_leading_whitespace() {
    let src = "\"\"\"\n    é one\n      two\n    ✓ three\n\"\"\"";
    assert_eq!(
        texts_of(src, TokenKind::MultilineString),
        vec!["é one\n  two\n✓ three"]
    );
}

#[test]
fn multiline_deindent_keeps_shorter_indent_lines_blank() {
    // A line with less indentation than the common indent loses the whitespace
    // it has and nothing else.
    let src = "\"\"\"\n    a\n \n    b\n\"\"\"";
    assert_eq!(texts_of(src, TokenKind::MultilineString), vec!["a\n\nb"]);
}

// ---------------------------------------------------------------- BUG 3 ------
//
// Token text uses the doubled-brace convention: `{{`/`}}` is a literal brace,
// a single `{ … }` pair is an interpolation hole. `rite-sem`'s
// `desugar_interpolation` decodes it (see the rite-sem interpolation tests for
// the end-to-end behaviour).

#[test]
fn escaped_brace_is_distinguishable_from_interpolation() {
    // \{name} must not interpolate; {name} must.
    assert_eq!(
        texts_of("\"literal: \\{name}\"", TokenKind::String),
        vec!["literal: {{name}"]
    );
    assert_eq!(
        texts_of("\"hi {name}\"", TokenKind::String),
        vec!["hi {name}"]
    );
    assert_eq!(
        texts_of("\"{a} and \\{a}\"", TokenKind::String),
        vec!["{a} and {{a}"]
    );
}

#[test]
fn escaped_closing_brace_is_doubled_too() {
    assert_eq!(
        texts_of("\"\\{x\\}\"", TokenKind::String),
        vec!["{{x}}"] // both braces escaped
    );
}

#[test]
fn escaped_brace_without_a_pair_is_decoded_by_the_lexer() {
    // Text with no `{`+`}` pair never reaches the desugarer, so the doubling is
    // folded here instead — the two paths must agree.
    assert_eq!(texts_of("\"\\{\"", TokenKind::String), vec!["{"]);
    assert_eq!(texts_of("\"\\}\"", TokenKind::String), vec!["}"]);
    assert_eq!(texts_of("\"a \\{ b\"", TokenKind::String), vec!["a { b"]);
    assert_eq!(texts_of("\"a \\} b\"", TokenKind::String), vec!["a } b"]);
}

#[test]
fn escaped_braces_do_not_disturb_other_escapes() {
    assert_eq!(
        texts_of("\"tab\\there \\u{41} \\\\ \\\"q\\\"\"", TokenKind::String),
        vec!["tab\there A \\ \"q\""]
    );
}

#[test]
fn multiline_strings_use_doubled_braces() {
    // A multiline string interpolates but has no escape processing, so `{{`/`}}`
    // is the literal-brace spelling and reaches the desugarer verbatim.
    assert_eq!(
        texts_of("\"\"\"{{ mustache }}\"\"\"", TokenKind::MultilineString),
        vec!["{{ mustache }}"]
    );
    // …and the lexer folds the doubling itself when there is no pair for the
    // desugarer to split on, exactly as for escaped strings.
    assert_eq!(
        texts_of("\"\"\"{{\"\"\"", TokenKind::MultilineString),
        vec!["{"]
    );
}

#[test]
fn raw_strings_are_fully_literal() {
    // Every brace is escaped, so the desugarer cannot read `r"{x}"` as a hole:
    // the token text decodes back to exactly what the source spelled.
    assert_eq!(texts_of("r\"{x}\"", TokenKind::RawString), vec!["{{x}}"]);
    assert_eq!(
        texts_of("r\"hi {name} there\"", TokenKind::RawString),
        vec!["hi {{name}} there"]
    );
    // A doubled brace in a raw string is two braces, not one.
    assert_eq!(
        texts_of("r\"{{x}}\"", TokenKind::RawString),
        vec!["{{{{x}}}}"]
    );
    // With no pair to split on the encoding is folded straight back.
    assert_eq!(texts_of("r\"{\"", TokenKind::RawString), vec!["{"]);
    assert_eq!(texts_of("r\"}\"", TokenKind::RawString), vec!["}"]);
    assert_eq!(texts_of("r\"a\\b{\"", TokenKind::RawString), vec!["a\\b{"]);
}

#[test]
fn unescape_braces_is_the_shared_decoder() {
    // rite-sem decodes non-interpolated literals with this; keep it in step with
    // what the lexer encodes.
    assert_eq!(rite_syntax::unescape_braces("{{x}}"), "{x}");
    assert_eq!(rite_syntax::unescape_braces("{{{{x}}}}"), "{{x}}");
    assert_eq!(rite_syntax::unescape_braces("{x}"), "{x}");
    assert_eq!(rite_syntax::unescape_braces("é {{🚀}}"), "é {🚀}");
}

#[test]
fn a_backslash_before_an_interpolation_stays_a_backslash() {
    // `\\` is a literal backslash; the following brace is still a hole.
    assert_eq!(
        texts_of("\"C:\\\\{dir}\"", TokenKind::String),
        vec!["C:\\{dir}"]
    );
}

// ---------------------------------------------------------------- BUG 4 ------

#[test]
fn unterminated_escaped_ident_is_reported() {
    let src = "z <- `unterminated";
    let (toks, d) = lex_src(src);
    assert!(
        has_code(&d, E003_UNTERMINATED_STRING),
        "expected an unterminated-quote diagnostic, got {:?}",
        d.iter().map(|x| x.code.as_str()).collect::<Vec<_>>()
    );
    assert!(toks.iter().any(|t| t.kind == TokenKind::Ident));
    assert_boundaries(src);
}

#[test]
fn invalid_utf8_is_rejected_before_lexing() {
    // The lexer only ever sees valid UTF-8 (`SourceFile` holds a `String`), so
    // raw bytes are screened here instead.
    let d = rite_syntax::lexer::validate_utf8(&[0xff, 0xfe], FileId(0))
        .expect_err("invalid UTF-8 accepted");
    assert!(has_code(&d, rite_core::E001_INVALID_UTF8));
    let ok = rite_syntax::lexer::validate_utf8("é 🚀".as_bytes(), FileId(0))
        .expect("valid UTF-8 rejected");
    assert_eq!(ok, "é 🚀");
}

#[test]
fn terminated_escaped_ident_is_clean() {
    let src = "`spaced name` <- 1";
    let (_, d) = lex_src(src);
    assert!(!d.has_errors());
    assert_eq!(texts_of(src, TokenKind::Ident), vec!["spaced name"]);
}
