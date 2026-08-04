use rite_core::{FileId, SourceFile};
use rite_syntax::{lex, parse_both_equivalent, parse_source, TokenKind};

#[test]
fn glyph_ascii_equivalent_binding() {
    parse_both_equivalent("x ← 1", "x <- 1").unwrap();
}

#[test]
fn glyph_ascii_equivalent_function() {
    parse_both_equivalent("◆ f(x) ⟦ ^ x ⟧", "def f(x) [[ return x ]]").unwrap();
}

#[test]
fn all_glyphs_lex() {
    let src = "◆ ← ↢ → ^ ? ~ ! @ # ⟦ ⟧ ⟨ ⟩ ∈ ∉ ??";
    let f = SourceFile::new(FileId(0), "t.rite", src);
    let (toks, d) = lex(&f);
    assert!(!d.has_errors());
    assert!(toks.iter().any(|t| t.kind == TokenKind::Def));
    assert!(toks.iter().any(|t| t.kind == TokenKind::Bind));
    assert!(toks.iter().any(|t| t.kind == TokenKind::BindMut));
    assert!(toks.iter().any(|t| t.kind == TokenKind::Arrow));
}

#[test]
fn parse_pipeline() {
    let (p, d, _) = parse_source("t.rite", "[1,2,3] → sum");
    assert!(!d.has_errors());
    assert!(p.is_some());
}

#[test]
fn trailing_block_arg() {
    let (p, d, _) = parse_source("t.rite", "xs → keep { |x| x }");
    assert!(!d.has_errors(), "{:?}", d.into_vec());
    assert!(p.is_some());
}
