//! Patterns: bindings, list and record destructuring, match arms.

#![allow(clippy::only_used_in_recursion)]

use super::support::pattern_span;
use super::*;
use crate::ast::*;
use crate::token::TokenKind;

impl Parser {
    /// `1 | 2 | 3` — alternatives at the top level of a match arm only.
    /// Bindings and nested positions use `parse_pattern` directly, where a
    /// refutable alternative would have nowhere to fall through to.
    pub(super) fn parse_or_pattern(&mut self) -> Pattern {
        let first = self.parse_pattern();
        if !self.check(TokenKind::Pipe) {
            return first;
        }
        let start = pattern_span(&first);
        let mut alternatives = vec![first];
        while self.check(TokenKind::Pipe) {
            self.advance();
            alternatives.push(self.parse_pattern());
        }
        let end = alternatives.last().map(pattern_span).unwrap_or(start);
        Pattern::Or(OrPattern {
            alternatives,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_pattern(&mut self) -> Pattern {
        if self.check(TokenKind::Underscore) {
            let t = self.advance();
            return Pattern::Wildcard(t.span);
        }
        if self.check(TokenKind::Ok)
            || self.check(TokenKind::Err)
            || self.check(TokenKind::Some)
            || self.check(TokenKind::None)
        {
            let t = self.advance();
            let kind = match t.kind {
                TokenKind::Ok => ResultPatKind::Ok,
                TokenKind::Err => ResultPatKind::Err,
                TokenKind::Some => ResultPatKind::Some,
                TokenKind::None => ResultPatKind::None,
                _ => unreachable!(),
            };
            let binding = if self.at_pattern_start() && !self.check(TokenKind::Arrow) {
                Some(Box::new(self.parse_pattern()))
            } else {
                None
            };
            let end = binding.as_ref().map(|b| pattern_span(b)).unwrap_or(t.span);
            return Pattern::Result(ResultPattern {
                kind,
                binding,
                span: t.span.merge(end),
            });
        }
        if self.check(TokenKind::Atom) {
            return Pattern::Atom(self.parse_atom_lit());
        }
        if self.check(TokenKind::LBracket) {
            return self.parse_list_pattern();
        }
        if self.check(TokenKind::RecordOpen) {
            return self.parse_record_pattern();
        }
        if matches!(
            self.peek_kind(),
            TokenKind::Int
                | TokenKind::Float
                | TokenKind::String
                | TokenKind::True
                | TokenKind::False
                | TokenKind::None
                | TokenKind::MultilineString
                | TokenKind::RawString
        ) {
            if let Expr::Literal(lit) = self.parse_primary() {
                return Pattern::Literal(lit);
            }
        }
        if self.check(TokenKind::Ident) {
            let ident = self.parse_ident();
            if self.check(TokenKind::Colon) {
                // typed pattern: name: type — for now treat as typed wrapper
                // only if used in params; in match, colon might be else
            }
            return Pattern::Ident(ident);
        }
        let span = self.current_span();
        self.error_expected("pattern");
        Pattern::Wildcard(span)
    }

    pub(super) fn parse_list_pattern(&mut self) -> Pattern {
        let start = self.advance().span;
        let mut elements = Vec::new();
        let mut rest = None;
        while !self.is_eof() && !self.check(TokenKind::RBracket) {
            if self.check(TokenKind::Rest) {
                self.advance();
                rest = Some(Box::new(self.parse_pattern()));
                break;
            }
            elements.push(self.parse_pattern());
            if self.check(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.expect(TokenKind::RBracket).span;
        Pattern::List(ListPattern {
            elements,
            rest,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_record_pattern(&mut self) -> Pattern {
        let start = self.advance().span;
        let mut fields = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::RecordClose) {
            let name = self.parse_ident();
            let pattern = if self.check(TokenKind::Colon) {
                self.advance();
                Some(self.parse_pattern())
            } else {
                None
            };
            let span = name.span;
            fields.push(FieldPattern {
                name,
                pattern,
                span,
            });
            if self.check(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.expect(TokenKind::RecordClose).span;
        Pattern::Record(RecordPattern {
            fields,
            span: start.merge(end),
        })
    }
}
