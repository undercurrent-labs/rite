//! Recursive-descent parser.
//!
//! Split by grammar layer rather than by size: [`items`] handles declarations and
//! statements, [`expr`] the precedence chain, [`pattern`] destructuring, and [`support`]
//! shared plumbing (types, identifiers, lookahead predicates, token access). Every
//! submodule adds methods to the one [`Parser`] via its own `impl` block, so the layers
//! are separable to read without changing how they call each other.

mod expr;
mod items;
mod pattern;
mod support;

use crate::ast::*;
use crate::token::{Token, TokenKind};
use rite_core::{Diagnostics, FileId};

pub struct Parser {
    file: FileId,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Diagnostics,
    /// When false, `status ⟦…⟧` is not treated as a call (needed for `~ status ⟦…⟧`).
    allow_trailing_block: bool,
    /// `///` blocks kept out of the token stream, for attaching to declarations.
    doc_comments: Vec<(usize, usize, String)>,
}

pub fn parse(file: FileId, tokens: &[Token]) -> (Option<Program>, Diagnostics) {
    let doc_comments = collect_doc_comments(tokens);
    let filtered: Vec<Token> = tokens
        .iter()
        .filter(|t| !t.kind.is_trivia())
        .cloned()
        .collect();
    let mut p = Parser {
        file,
        tokens: filtered,
        pos: 0,
        diagnostics: Diagnostics::new(),
        allow_trailing_block: true,
        doc_comments,
    };
    let program = p.parse_program();
    (Some(program), p.diagnostics)
}

pub fn parse_expression(file: FileId, tokens: &[Token]) -> (Option<Expr>, Diagnostics) {
    let filtered: Vec<Token> = tokens
        .iter()
        .filter(|t| !t.kind.is_trivia())
        .cloned()
        .collect();
    let mut p = Parser {
        file,
        tokens: filtered,
        pos: 0,
        diagnostics: Diagnostics::new(),
        allow_trailing_block: true,
        doc_comments: Vec::new(),
    };
    let expr = p.parse_expression();
    (Some(expr), p.diagnostics)
}

/// `///` runs in source order, as (start offset, end offset, text).
///
/// Kept aside because the parser drops trivia before it starts, which is why
/// `FunctionDecl.doc` was always `None` and nothing ever harvested doc comments.
/// Consecutive lines are merged so a multi-line doc block is one string.
fn collect_doc_comments(tokens: &[Token]) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for tok in tokens {
        if tok.kind != TokenKind::DocComment {
            continue;
        }
        let body = tok
            .text
            .trim_start()
            .trim_start_matches("///")
            .trim()
            .to_string();
        let (start, end) = (tok.span.start.as_usize(), tok.span.end.as_usize());
        // Merge with the previous line when only whitespace separates them.
        match out.last_mut() {
            Some((_, prev_end, text)) if *prev_end < start && start - *prev_end <= 2 => {
                text.push('\n');
                text.push_str(&body);
                *prev_end = end;
            }
            _ => out.push((start, end, body)),
        }
    }
    out
}

fn parse_int_literal(text: &str) -> i64 {
    let clean = text.replace('_', "");
    if let Some(hex) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).unwrap_or(0)
    } else if let Some(bin) = clean
        .strip_prefix("0b")
        .or_else(|| clean.strip_prefix("0B"))
    {
        i64::from_str_radix(bin, 2).unwrap_or(0)
    } else {
        clean.parse().unwrap_or(0)
    }
}

// Fix: TokenKind::Type doesn't exist - remove from match
// The is_keyword_as_ident has TokenKind::Type which won't compile - need to fix

#[cfg(test)]
mod doc_comments {
    use super::*;
    use rite_core::{FileId, SourceFile};

    fn docs_of(src: &str) -> Vec<(String, Option<String>)> {
        let file = SourceFile::new(FileId(0), "t.rite", src);
        let (toks, _) = crate::lex(&file);
        let (prog, _) = parse(FileId(0), &toks);
        prog.expect("parse")
            .items
            .into_iter()
            .filter_map(|i| match i {
                Item::Function(f) => Some((f.name.name, f.doc)),
                _ => None,
            })
            .collect()
    }

    /// `FunctionDecl.doc` was always `None`: the parser drops trivia before it runs, so
    /// nothing could ever harvest `///` from real sources.
    #[test]
    fn doc_comment_attaches_to_the_following_function() {
        let got = docs_of("/// Squares a value.\ndef square(n) [[ ^ n * n ]]\n");
        assert_eq!(
            got,
            vec![("square".into(), Some("Squares a value.".into()))]
        );
    }

    #[test]
    fn consecutive_doc_lines_merge_into_one_block() {
        let got = docs_of("/// First.\n/// Second.\ndef f(n) [[ ^ n ]]\n");
        assert_eq!(got[0].1.as_deref(), Some("First.\nSecond."));
    }

    #[test]
    fn a_function_without_a_doc_gets_none() {
        let got = docs_of("/// For f only.\ndef f(n) [[ ^ n ]]\ndef g(n) [[ ^ n ]]\n");
        assert_eq!(got[0].1.as_deref(), Some("For f only."));
        assert_eq!(got[1].1, None, "doc leaked to the next function");
    }

    #[test]
    fn a_doc_block_is_claimed_by_the_nearest_declaration_below_it() {
        let got = docs_of("/// One.\ndef a(n) [[ ^ n ]]\n/// Two.\ndef b(n) [[ ^ n ]]\n");
        assert_eq!(got[0].1.as_deref(), Some("One."));
        assert_eq!(got[1].1.as_deref(), Some("Two."));
    }

    #[test]
    fn code_between_the_doc_and_the_declaration_detaches_it() {
        // The block documents nothing: a statement intervenes.
        let got = docs_of("/// Stray.\nx <- 1\ndef f(n) [[ ^ n ]]\n");
        assert_eq!(got[0].1, None);
    }

    #[test]
    fn pub_functions_keep_their_doc() {
        let got = docs_of("/// Exported.\npub def f(n) [[ ^ n ]]\n");
        assert_eq!(got[0].1.as_deref(), Some("Exported."));
    }
}
