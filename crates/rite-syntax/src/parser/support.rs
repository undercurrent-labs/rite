//! Shared plumbing: types, identifiers, lookahead predicates, token access.

#![allow(clippy::only_used_in_recursion)]

use super::*;
use crate::ast::*;
use crate::token::{Token, TokenKind};
use rite_core::{simple_error, Span, E011_EXPECTED_TOKEN, E013_INVALID_SYNTAX};

impl Parser {
    pub(super) fn parse_type_expr(&mut self) -> TypeExpr {
        if self.check(TokenKind::LBracket) {
            self.advance();
            let inner = self.parse_type_expr();
            self.expect(TokenKind::RBracket);
            return TypeExpr::List(Box::new(inner));
        }
        if self.check(TokenKind::RecordOpen) {
            self.advance();
            let mut fields = Vec::new();
            while !self.check(TokenKind::RecordClose) && !self.is_eof() {
                let name = self.parse_ident();
                self.expect(TokenKind::Colon);
                let ty = self.parse_type_expr();
                fields.push((name, ty));
                if self.check(TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RecordClose);
            return TypeExpr::Record(fields);
        }
        let name = self.parse_ident();
        if name.name == "result" && self.check(TokenKind::Lt) {
            // result<T> — optional; we may not have Lt as generic
        }
        if name.name == "any" {
            return TypeExpr::Any(name.span);
        }
        // result name with following type in brackets handled simply as Named
        TypeExpr::Named(name)
    }

    pub(super) fn parse_atom_lit(&mut self) -> AtomLit {
        let t = self.expect(TokenKind::Atom);
        let parts: Vec<String> = t.text.split('.').map(|s| s.to_string()).collect();
        AtomLit {
            parts,
            span: t.span,
        }
    }

    pub(super) fn parse_ident(&mut self) -> Ident {
        if self.check(TokenKind::Ident) || self.is_keyword_as_ident() {
            let t = self.advance();
            Ident {
                name: t.text.clone(),
                span: t.span,
            }
        } else {
            let span = self.current_span();
            self.error_expected("identifier");
            Ident {
                name: "_".into(),
                span,
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────

    pub(super) fn at_expr_start(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Ident
                | TokenKind::Int
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::MultilineString
                | TokenKind::RawString
                | TokenKind::True
                | TokenKind::False
                | TokenKind::None
                | TokenKind::Atom
                | TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::BlockOpen
                | TokenKind::RecordOpen
                | TokenKind::Host
                | TokenKind::If
                | TokenKind::Match
                | TokenKind::Effect
                | TokenKind::Minus
                | TokenKind::Not
                | TokenKind::Dollar
                | TokenKind::Return
                | TokenKind::Get
                | TokenKind::Post
                | TokenKind::Put
                | TokenKind::Patch
                | TokenKind::Delete
                | TokenKind::Head
                | TokenKind::Options
                | TokenKind::OkMark
                | TokenKind::ErrMark
                // Contextual keywords. `is_keyword_as_ident` already lets these be
                // *bound* — `◆ f(item)` names its parameter `item` — so leaving them
                // out here meant `^ item` saw no expression, returned `none`, and the
                // parameter silently read as nothing. Every read has to parse as an
                // expression wherever the binding is allowed.
                | TokenKind::Item
                | TokenKind::Room
                | TokenKind::World
                | TokenKind::Test
                | TokenKind::Ok
                | TokenKind::Err
                | TokenKind::Some
                | TokenKind::Say
                | TokenKind::Paragraph
                | TokenKind::For
                | TokenKind::ForAll
                | TokenKind::Unless
                | TokenKind::While
                | TokenKind::Loop
        )
    }

    pub(super) fn at_pattern_start(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Ident
                | TokenKind::Atom
                | TokenKind::Underscore
                | TokenKind::LBracket
                | TokenKind::RecordOpen
                | TokenKind::Ok
                | TokenKind::Err
                | TokenKind::Some
                | TokenKind::None
                | TokenKind::Int
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::True
                | TokenKind::False
        )
    }

    /// True when the current `?` token starts prefix if (`? cond ⟦…⟧`) rather than
    /// postfix try (`expr?`). Used so a following-line conditional is not glued onto
    /// the previous expression (e.g. `[] → last` then `? x = none ⟦…⟧`).
    pub(super) fn looks_like_prefix_if(&self) -> bool {
        if self.peek_kind() != TokenKind::If {
            return false;
        }
        // `expr?` followed by another `? cond ⟦…⟧`: the first `?` is try.
        if self
            .tokens
            .get(self.pos + 1)
            .map(|t| t.kind == TokenKind::If)
            .unwrap_or(false)
        {
            return false;
        }
        let mut i = self.pos + 1;
        let mut depth_paren = 0i32;
        let mut depth_bracket = 0i32;
        let mut depth_brace = 0i32;
        let mut saw_expr = false;
        let limit = (self.pos + 48).min(self.tokens.len());
        while i < limit {
            let kind = self.tokens[i].kind;
            let at_top = depth_paren == 0 && depth_bracket == 0 && depth_brace == 0;
            match kind {
                TokenKind::Eof => return false,
                // `? <condition> ⟦` — prefix if
                TokenKind::BlockOpen | TokenKind::LBrace if at_top && saw_expr => {
                    return true;
                }
                // Next statement is a binding/assign — this `?` was postfix try
                TokenKind::Bind | TokenKind::BindMut | TokenKind::Assign | TokenKind::Semicolon
                    if at_top =>
                {
                    return false;
                }
                // Cannot appear in an if-condition; stop (postfix try or unrelated)
                TokenKind::Return | TokenKind::Def | TokenKind::Match | TokenKind::Effect
                    if at_top =>
                {
                    return false;
                }
                // Closed the enclosing block without seeing if-body
                TokenKind::BlockClose | TokenKind::RBrace if at_top => {
                    return false;
                }
                // A closing delimiter while already at depth zero means the scan has
                // left the group the `?` sits inside — `id(x?)`, where the very next
                // token closes the call. There is no `⟦` body at this level, so the
                // `?` was postfix try.
                //
                // These used to `saturating_sub`, which saturates at `i32::MIN`, not at
                // zero: the depth went to -1, the *next* `(` brought it back to 0, and a
                // lambda's `{` on the following statement then looked top-level. So
                // `r ← id(@json.decode(raw)?)` followed by `each(r, { |x| x })` read the
                // `?` as a prefix `if` and failed to parse — pointing at the previous
                // line, which is a good way to lose an afternoon.
                TokenKind::LParen => {
                    depth_paren += 1;
                    saw_expr = true;
                }
                TokenKind::RParen => {
                    if depth_paren == 0 {
                        return false;
                    }
                    depth_paren -= 1;
                    saw_expr = true;
                }
                TokenKind::LBracket => {
                    depth_bracket += 1;
                    saw_expr = true;
                }
                TokenKind::RBracket => {
                    if depth_bracket == 0 {
                        return false;
                    }
                    depth_bracket -= 1;
                    saw_expr = true;
                }
                TokenKind::RecordOpen => {
                    depth_brace += 1;
                    saw_expr = true;
                }
                TokenKind::RecordClose => {
                    if depth_brace == 0 {
                        return false;
                    }
                    depth_brace -= 1;
                    saw_expr = true;
                }
                _ => saw_expr = true,
            }
            i += 1;
        }
        false
    }

    pub(super) fn is_http_method(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Get
                | TokenKind::Post
                | TokenKind::Put
                | TokenKind::Patch
                | TokenKind::Delete
                | TokenKind::Head
                | TokenKind::Options
        ) || (self.check(TokenKind::Ident)
            && matches!(
                self.tokens.get(self.pos).map(|t| t.text.as_str()),
                Some("GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS")
            ))
    }

    /// Keywords that are still ordinary names after a `.`.
    ///
    /// A capability method shares its spelling with whatever the lexer happens to
    /// have promoted to a keyword, and the two do not otherwise interact: `@game.say`
    /// is a call, not the `say` statement. Leaving `Say` out of this set made that
    /// call unwritable — it parsed as `@game.` followed by a keyword and failed at
    /// runtime with `unknown @game.`, so one capability function could not be reached
    /// from Rite at all.
    pub(super) fn is_keyword_as_ident(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Item
                | TokenKind::Room
                | TokenKind::World
                | TokenKind::Test
                | TokenKind::Ok
                | TokenKind::Err
                | TokenKind::Some
                | TokenKind::Say
                | TokenKind::Get
                | TokenKind::Post
                | TokenKind::Put
                | TokenKind::Patch
                | TokenKind::Delete
                | TokenKind::Head
                | TokenKind::Options
        )
    }

    pub(super) fn check(&self, kind: TokenKind) -> bool {
        self.peek_kind() == kind
    }

    pub(super) fn check_nth(&self, n: usize, kind: TokenKind) -> bool {
        self.tokens
            .get(self.pos + n)
            .map(|t| t.kind == kind)
            .unwrap_or(false)
    }

    pub(super) fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or_else(|| Token {
            kind: TokenKind::Eof,
            span: Span::DUMMY,
            file: self.file,
            text: String::new(),
            starts_line: false,
        })
    }

    pub(super) fn peek_kind(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof)
    }

    pub(super) fn is_eof(&self) -> bool {
        self.peek_kind() == TokenKind::Eof || self.pos >= self.tokens.len()
    }

    pub(super) fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            t
        } else {
            self.tokens.last().cloned().unwrap_or_else(|| Token {
                kind: TokenKind::Eof,
                span: Span::DUMMY,
                file: self.file,
                text: String::new(),
                starts_line: false,
            })
        }
    }

    pub(super) fn expect(&mut self, kind: TokenKind) -> Token {
        if self.check(kind) {
            self.advance()
        } else {
            let span = self.current_span();
            self.diagnostics.push(simple_error(
                E011_EXPECTED_TOKEN,
                format!("expected {}", kind),
                self.file,
                span,
                format!("found {}", self.peek_kind()),
            ));
            self.tokens
                .get(self.pos.saturating_sub(1))
                .cloned()
                .unwrap_or_else(|| Token {
                    kind: TokenKind::Eof,
                    span: Span::DUMMY,
                    file: self.file,
                    text: String::new(),
                    starts_line: false,
                })
        }
    }

    pub(super) fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::DUMMY)
    }

    pub(super) fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            Span::DUMMY
        }
    }

    pub(super) fn error_expected(&mut self, what: &str) {
        let span = self.current_span();
        self.diagnostics.push(simple_error(
            E013_INVALID_SYNTAX,
            format!("expected {}", what),
            self.file,
            span,
            format!("found {}", self.peek_kind()),
        ));
    }
}

pub(super) fn is_callable_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident(_) | Expr::Member(_) | Expr::Call(_) | Expr::Capability(_) | Expr::Group(_)
    )
}

pub(super) fn pattern_span(p: &Pattern) -> Span {
    match p {
        Pattern::Ident(i) => i.span,
        Pattern::Atom(a) => a.span,
        Pattern::Literal(l) => l.span,
        Pattern::Wildcard(s) => *s,
        Pattern::List(l) => l.span,
        Pattern::Record(r) => r.span,
        Pattern::Result(r) => r.span,
        Pattern::Typed(t) => t.span,
    }
}
