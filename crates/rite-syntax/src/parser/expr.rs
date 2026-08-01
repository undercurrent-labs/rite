//! Expressions, from the pipeline operator down to primaries.

#![allow(clippy::only_used_in_recursion)]

use super::support::{is_callable_expr, pattern_span};
use super::*;
use crate::ast::*;
use crate::token::TokenKind;
use rite_core::{
    simple_error, E010_UNEXPECTED_TOKEN, E012_UNCLOSED_DELIMITER, E015_PIPELINE_RESULT_OPERAND,
    E016_TRY_ON_PIPELINE_STAGE,
};

impl Parser {
    pub fn parse_expression(&mut self) -> Expr {
        self.parse_conditional()
    }

    /// `input → stage → stage`.
    ///
    /// Binds **looser than every binary operator**, so the input side reads as it is
    /// written and a completed value moves to the next stage:
    ///
    /// ```text
    /// a + b → str   is   (a + b) → str
    /// ```
    ///
    /// Each stage is still parsed at postfix level — a name, a call, or a
    /// trailing-block call, never a bare operator expression.
    ///
    /// # Why an operator *after* a pipeline is rejected
    ///
    /// An infix operator cannot be looser than `+` on its left and tighter than `+` on
    /// its right. Reaching the input side costs the result side: with `→` at the
    /// bottom of the chain there is no level left to attach a trailing operator to.
    ///
    /// This has been arranged all three possible ways, and only one of them is honest:
    ///
    /// * **Loose, stages as full expressions** — the original. The stage swallowed the
    ///   operator, so `xs → count > 2` meant `xs → (count > 2)` and failed at runtime
    ///   with "cannot call value of type bool". Every operator after a stage was
    ///   affected, and none of it was visible in the source.
    /// * **Tight, stages at postfix** — the replacement. `xs → count > 2` worked, at
    ///   the price of `a + b → str` silently meaning `a + (b → str)`.
    /// * **Loose, stages at postfix, trailing operator rejected** — this one. The input
    ///   side is what it looks like, and the case the tight arrangement bought is a
    ///   *parse error* naming the parenthesised form rather than a wrong answer.
    ///
    /// So `xs → count > 2` no longer parses; write `(xs → count) > 2`. That is the same
    /// trade F#, Elixir and Elm make with `|>`, where the equivalent is a type error.
    pub(super) fn parse_pipeline(&mut self) -> Expr {
        let expr = self.parse_coalesce();
        let mut stages = Vec::new();
        let start = expr.span();
        while self.check(TokenKind::Arrow) {
            // Disambiguate match arms: if we're inside match block, arms use Arrow
            // At expression level, Arrow is pipeline
            self.advance();
            let stage = self.parse_pipeline_stage();
            stages.push(stage);
        }
        if stages.is_empty() {
            return expr;
        }
        let end = stages.last().map(|s| s.span()).unwrap_or(start);
        let span = start.merge(end);
        // Nothing below this level will consume a trailing operator, so saying so here
        // is the difference between a diagnostic and a dangling token reported
        // somewhere unrelated.
        if let Some(op) = self.binary_operator_text() {
            self.diagnostics.push(
                simple_error(
                    E015_PIPELINE_RESULT_OPERAND,
                    format!("a pipeline's result cannot be an operand of `{op}`"),
                    self.file,
                    span,
                    "this pipeline is followed by an operator",
                )
                .with_help("parenthesise the pipeline to use its result: `(… → …) <op> x`"),
            );
        }
        Expr::Pipeline(PipelineExpr {
            input: Box::new(expr),
            stages,
            span,
        })
    }

    /// The operator sitting at the cursor, if it is a binary one.
    ///
    /// Every token any level of the ladder below `parse_pipeline` would consume as an
    /// infix operator. Kept as one list so a new operator cannot be added to the ladder
    /// and silently escape the check.
    fn binary_operator_text(&self) -> Option<&'static str> {
        Some(match self.peek_kind() {
            TokenKind::Coalesce => "??",
            TokenKind::Or => "or",
            TokenKind::Xor => "xor",
            TokenKind::And => "and",
            TokenKind::Eq => "=",
            TokenKind::NotEq => "!=",
            TokenKind::Lt => "<",
            TokenKind::LtEq => "<=",
            TokenKind::Gt => ">",
            TokenKind::GtEq => ">=",
            TokenKind::In => "in",
            TokenKind::NotIn => "not in",
            TokenKind::Rest => "..",
            TokenKind::RangeIncl => "..=",
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::Idiv => "//",
            TokenKind::Power => "**",
            TokenKind::Compose => "∘",
            _ => return None,
        })
    }

    pub(super) fn parse_pipeline_stage(&mut self) -> Expr {
        // member projection: .name
        if self.check(TokenKind::Dot) {
            let start = self.advance().span;
            let field = self.parse_ident();
            let field_span = field.span;
            // Represent as special member on placeholder
            return Expr::Member(MemberExpr {
                object: Box::new(Expr::Placeholder(Placeholder { span: start })),
                field,
                span: start.merge(field_span),
            });
        }
        // Postfix level: an identifier, a call, a trailing-block call (`map { … }`), a
        // closure, a field access. Deliberately not a full expression — that is what let
        // a stage absorb the operator that followed the pipeline.
        let stage = self.parse_postfix();
        // `xs → f(a)?` binds the `?` to the stage, which describes nothing anyone
        // means. The evaluator read it as "call `f(a)` — *without* the piped value —
        // unwrap that, then call the result with `xs`", and the compiler backend
        // refused to lower it at all, so the two execution paths disagreed about a
        // program that compiled. Nothing in the corpus, the book or the fixtures used
        // it. Rejecting it is cheaper than choosing which of the two wrong readings to
        // canonise; `?` belongs on the pipeline's result, where it applies to the value
        // that comes out.
        if let Expr::Try(t) = &stage {
            self.diagnostics.push(
                simple_error(
                    E016_TRY_ON_PIPELINE_STAGE,
                    "`?` cannot be applied to a pipeline stage",
                    self.file,
                    t.span,
                    "this unwraps the stage, not the value flowing through it",
                )
                .with_help("put it on the result instead: `(… → …)?`"),
            );
        }
        stage
    }

    pub(super) fn parse_conditional(&mut self) -> Expr {
        if self.check(TokenKind::If) {
            let start = self.advance().span;
            let prev = self.allow_trailing_block;
            self.allow_trailing_block = false;
            let condition = self.parse_expression();
            self.allow_trailing_block = prev;
            let then_branch = self.parse_block();
            let else_branch = if self.check(TokenKind::Colon) || self.check(TokenKind::Else) {
                self.advance();
                Some(self.parse_block())
            } else {
                None
            };
            let end = else_branch
                .as_ref()
                .map(|b| b.span)
                .unwrap_or(then_branch.span);
            return Expr::If(IfExpr {
                condition: Box::new(condition),
                then_branch,
                else_branch,
                span: start.merge(end),
            });
        }
        if self.check(TokenKind::Match) {
            return self.parse_match();
        }
        self.parse_pipeline()
    }

    pub(super) fn parse_match(&mut self) -> Expr {
        let start = self.advance().span;
        // Scrutinee must not swallow the match arms block as a trailing call arg.
        let prev = self.allow_trailing_block;
        self.allow_trailing_block = false;
        let scrutinee = self.parse_expression();
        self.allow_trailing_block = prev;
        // Match body is a block of arms: pattern → expr
        let block_start = self.current_span();
        let open = self.peek_kind();
        if open != TokenKind::BlockOpen && open != TokenKind::LBrace {
            self.error_expected("match block");
            return Expr::Match(MatchExpr {
                scrutinee: Box::new(scrutinee),
                arms: vec![],
                span: start,
            });
        }
        self.advance();
        let mut arms = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::BlockClose) && !self.check(TokenKind::RBrace)
        {
            if self.check(TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            let arm = self.parse_match_arm();
            arms.push(arm);
        }
        if self.check(TokenKind::BlockClose) || self.check(TokenKind::RBrace) {
            self.advance();
        } else {
            self.diagnostics.push(simple_error(
                E012_UNCLOSED_DELIMITER,
                "unclosed match block",
                self.file,
                block_start,
                "expected ⟧ or ]]",
            ));
        }
        let end = self.prev_span();
        Expr::Match(MatchExpr {
            scrutinee: Box::new(scrutinee),
            arms,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_match_arm(&mut self) -> MatchArm {
        let pattern = self.parse_pattern();
        self.expect(TokenKind::Arrow);
        let body = self.parse_expression();
        let span = pattern_span(&pattern).merge(body.span());
        MatchArm {
            pattern,
            body,
            span,
        }
    }

    pub(super) fn parse_coalesce(&mut self) -> Expr {
        let mut left = self.parse_or();
        while self.check(TokenKind::Coalesce) {
            self.advance();
            let right = self.parse_or();
            let span = left.span().merge(right.span());
            left = Expr::Coalesce(CoalesceExpr {
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_or(&mut self) -> Expr {
        let mut left = self.parse_xor();
        while self.check(TokenKind::Or) {
            self.advance();
            let right = self.parse_xor();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_xor(&mut self) -> Expr {
        let mut left = self.parse_and();
        while self.check(TokenKind::Xor) {
            self.advance();
            let right = self.parse_and();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op: BinOp::Xor,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_and(&mut self) -> Expr {
        let mut left = self.parse_equality();
        while self.check(TokenKind::And) {
            self.advance();
            let right = self.parse_equality();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_equality(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        while matches!(self.peek_kind(), TokenKind::Eq | TokenKind::NotEq) {
            let op = match self.advance().kind {
                TokenKind::Eq => BinOp::Eq,
                TokenKind::NotEq => BinOp::NotEq,
                _ => unreachable!(),
            };
            let right = self.parse_comparison();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_range();
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::GtEq => BinOp::GtEq,
                TokenKind::In => BinOp::In,
                TokenKind::NotIn => BinOp::NotIn,
                _ => break,
            };
            self.advance();
            let right = self.parse_range();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_range(&mut self) -> Expr {
        let left = self.parse_term();
        if self.check(TokenKind::Rest) {
            // a..b exclusive
            let start = left.span();
            self.advance();
            let right = self.parse_term();
            let span = start.merge(right.span());
            return Expr::Call(CallExpr {
                callee: Box::new(Expr::Ident(Ident {
                    name: "range".into(),
                    span,
                })),
                args: vec![left, right],
                span,
            });
        }
        if self.check(TokenKind::RangeIncl) {
            // a..=b or a‥b inclusive
            let start = left.span();
            self.advance();
            let right = self.parse_term();
            let span = start.merge(right.span());
            return Expr::Call(CallExpr {
                callee: Box::new(Expr::Ident(Ident {
                    name: "range_incl".into(),
                    span,
                })),
                args: vec![left, right],
                span,
            });
        }
        left
    }

    pub(super) fn parse_term(&mut self) -> Expr {
        let mut left = self.parse_factor();
        while matches!(self.peek_kind(), TokenKind::Plus | TokenKind::Minus) {
            let op = match self.advance().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => unreachable!(),
            };
            let right = self.parse_factor();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_factor(&mut self) -> Expr {
        let mut left = self.parse_power();
        while matches!(
            self.peek_kind(),
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent | TokenKind::Idiv
        ) {
            let op = match self.advance().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Idiv => BinOp::Idiv,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => unreachable!(),
            };
            let right = self.parse_power();
            let span = left.span().merge(right.span());
            left = Expr::Binary(BinaryExpr {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            });
        }
        left
    }

    pub(super) fn parse_power(&mut self) -> Expr {
        let left = self.parse_compose();
        if self.check(TokenKind::Power) {
            let start = left.span();
            self.advance();
            let right = self.parse_power(); // right-associative
            let span = start.merge(right.span());
            return Expr::Call(CallExpr {
                callee: Box::new(Expr::Ident(Ident {
                    name: "pow".into(),
                    span,
                })),
                args: vec![left, right],
                span,
            });
        }
        left
    }

    pub(super) fn parse_compose(&mut self) -> Expr {
        let mut left = self.parse_unary();
        while self.check(TokenKind::Compose) {
            self.advance();
            let right = self.parse_unary();
            let span = left.span().merge(right.span());
            // f ∘ g  →  { |x| f(g(x)) }  represented as call compose(f, g)
            left = Expr::Call(CallExpr {
                callee: Box::new(Expr::Ident(Ident {
                    name: "compose".into(),
                    span,
                })),
                args: vec![left, right],
                span,
            });
        }
        left
    }

    pub(super) fn parse_unary(&mut self) -> Expr {
        if self.check(TokenKind::Minus) {
            let start = self.advance().span;
            let expr = self.parse_unary();
            let span = start.merge(expr.span());
            return Expr::Unary(UnaryExpr {
                op: UnaryOp::Neg,
                expr: Box::new(expr),
                span,
            });
        }
        if self.check(TokenKind::Not) {
            let start = self.advance().span;
            let expr = self.parse_unary();
            let span = start.merge(expr.span());
            return Expr::Unary(UnaryExpr {
                op: UnaryOp::Not,
                expr: Box::new(expr),
                span,
            });
        }
        if self.check(TokenKind::Effect) {
            let start = self.advance().span;
            let expr = self.parse_unary();
            let span = start.merge(expr.span());
            return Expr::Unary(UnaryExpr {
                op: UnaryOp::Effect,
                expr: Box::new(expr),
                span,
            });
        }
        self.parse_postfix()
    }

    pub(super) fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            // A `(` or `[` that opens a line starts a new statement; it is not a call or
            // index applied to the previous one. Rite has no statement terminator, so the
            // line break is the only separator. Without this,
            //
            //     a ← 1
            //     [9]
            //
            // parsed as `a ← 1[9]` — indexing an int, which yields `none`, so `a` was
            // silently bound to nothing at all. `.field`, `?` and the operators keep
            // crossing lines, which is what makes multi-line pipelines and chains work.
            if self.peek().starts_line
                && (self.check(TokenKind::LParen) || self.check(TokenKind::LBracket))
            {
                break;
            }
            // Only treat `(` as call on callable forms — not after record/list
            // literals, so a newline + `(x + y).z` does not glue onto `⟨…⟩`.
            if self.check(TokenKind::LParen) && is_callable_expr(&expr) {
                let start = expr.span();
                self.advance();
                let args = self.parse_arg_list(TokenKind::RParen);
                let end_tok = self.expect(TokenKind::RParen);
                expr = Expr::Call(CallExpr {
                    callee: Box::new(expr),
                    args,
                    span: start.merge(end_tok.span),
                });
            } else if self.check(TokenKind::Dot) {
                let start = expr.span();
                self.advance();
                let field = self.parse_ident();
                let field_span = field.span;
                expr = Expr::Member(MemberExpr {
                    object: Box::new(expr),
                    field,
                    span: start.merge(field_span),
                });
            } else if self.check(TokenKind::LBracket) {
                let start = expr.span();
                self.advance();
                let index = self.parse_expression();
                let end_tok = self.expect(TokenKind::RBracket);
                expr = Expr::Index(IndexExpr {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span: start.merge(end_tok.span),
                });
            } else if self.check(TokenKind::If) {
                // Postfix `?` is try-unwrap. The token kind for `?` is also prefix if.
                // If this `?` starts a new conditional (`? cond ⟦…⟧` on the next line),
                // do not attach it as try on the previous expression.
                if self.looks_like_prefix_if() {
                    break;
                }
                let start = expr.span();
                self.advance();
                expr = Expr::Try(TryExpr {
                    expr: Box::new(expr),
                    span: start.merge(self.prev_span()),
                });
            } else if self.allow_trailing_block
                && (self.check(TokenKind::LBrace) || self.check(TokenKind::BlockOpen))
                && is_callable_expr(&expr)
            {
                // Trailing block argument: keep { |x| ... } / map ⟦ ... ⟧
                // Only after callables so `? cond ⟦…⟧` is not treated as a call.
                // Disabled while parsing match scrutinees so `~ x ⟦ arms ⟧` works.
                let start = expr.span();
                let block = self.parse_block();
                expr = Expr::Call(CallExpr {
                    callee: Box::new(expr),
                    args: vec![Expr::Block(block.clone())],
                    span: start.merge(block.span),
                });
            } else {
                break;
            }
        }
        expr
    }

    pub(super) fn parse_primary(&mut self) -> Expr {
        match self.peek_kind() {
            TokenKind::True => {
                let t = self.advance();
                Expr::Literal(Literal {
                    kind: LitKind::Bool(true),
                    span: t.span,
                })
            }
            TokenKind::False => {
                let t = self.advance();
                Expr::Literal(Literal {
                    kind: LitKind::Bool(false),
                    span: t.span,
                })
            }
            TokenKind::None => {
                let t = self.advance();
                Expr::Literal(Literal {
                    kind: LitKind::None,
                    span: t.span,
                })
            }
            TokenKind::Int => {
                let t = self.advance();
                let n = parse_int_literal(&t.text);
                Expr::Literal(Literal {
                    kind: LitKind::Int(n),
                    span: t.span,
                })
            }
            TokenKind::Float => {
                let t = self.advance();
                let n = t.text.replace('_', "").parse::<f64>().unwrap_or(0.0);
                Expr::Literal(Literal {
                    kind: LitKind::Float(n),
                    span: t.span,
                })
            }
            TokenKind::String | TokenKind::MultilineString | TokenKind::RawString => {
                let t = self.advance();
                Expr::Literal(Literal {
                    kind: LitKind::String(t.text.clone()),
                    span: t.span,
                })
            }
            TokenKind::Atom => Expr::Atom(self.parse_atom_lit()),
            TokenKind::OkMark | TokenKind::Ok => {
                let start = self.advance().span;
                if self.check(TokenKind::LParen) {
                    self.advance();
                    let inner = self.parse_expression();
                    let end = self.expect(TokenKind::RParen).span;
                    let span = start.merge(end);
                    Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "ok".into(),
                            span,
                        })),
                        args: vec![inner],
                        span,
                    })
                } else if matches!(self.peek_kind(), TokenKind::OkMark | TokenKind::Ok) {
                    // bare
                    Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "ok".into(),
                            span: start,
                        })),
                        args: vec![Expr::Literal(Literal {
                            kind: LitKind::None,
                            span: start,
                        })],
                        span: start,
                    })
                } else if self.at_expr_start()
                    && !self.check(TokenKind::BlockOpen)
                    && !self.check(TokenKind::LBrace)
                {
                    let inner = self.parse_unary();
                    let span = start.merge(inner.span());
                    Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "ok".into(),
                            span,
                        })),
                        args: vec![inner],
                        span,
                    })
                } else {
                    Expr::Ident(Ident {
                        name: "ok".into(),
                        span: start,
                    })
                }
            }
            TokenKind::ErrMark | TokenKind::Err => {
                let start = self.advance().span;
                if self.check(TokenKind::LParen) {
                    self.advance();
                    let inner = self.parse_expression();
                    let end = self.expect(TokenKind::RParen).span;
                    let span = start.merge(end);
                    Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "err".into(),
                            span,
                        })),
                        args: vec![inner],
                        span,
                    })
                } else if self.at_expr_start()
                    && !self.check(TokenKind::BlockOpen)
                    && !self.check(TokenKind::LBrace)
                {
                    let inner = self.parse_unary();
                    let span = start.merge(inner.span());
                    Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "err".into(),
                            span,
                        })),
                        args: vec![inner],
                        span,
                    })
                } else {
                    Expr::Ident(Ident {
                        name: "err".into(),
                        span: start,
                    })
                }
            }
            // Contextual keywords used as plain names. `item`, `room` and `world`
            // introduce game declarations and `test` a test, but only after `◆`/`def`
            // at item level — which is parsed before we ever get here. In expression
            // position they are ordinary identifiers, and must resolve to whatever
            // binding is in scope rather than quietly yielding `none`.
            // `some` is pattern-only syntax (`~ v ⟦ some x → … ⟧`), so unlike `ok`
            // and `err` it has no constructor form to preserve here.
            TokenKind::Item
            | TokenKind::Room
            | TokenKind::World
            | TokenKind::Test
            | TokenKind::Some => {
                let t = self.advance();
                Expr::Ident(Ident {
                    name: t.text.clone(),
                    span: t.span,
                })
            }
            TokenKind::Ident => {
                // HTTP method routes only inside blocks — treat as ident here
                // But GET "/path" is route
                if self.is_http_method()
                    && self.pos + 1 < self.tokens.len()
                    && matches!(
                        self.tokens[self.pos + 1].kind,
                        TokenKind::String | TokenKind::MultilineString | TokenKind::RawString
                    )
                {
                    return Expr::Route(self.parse_route());
                }
                // also token kinds Get, Post, etc.
                Expr::Ident(self.parse_ident())
            }
            TokenKind::Get
            | TokenKind::Post
            | TokenKind::Put
            | TokenKind::Patch
            | TokenKind::Delete
            | TokenKind::Head
            | TokenKind::Options => {
                if self.pos + 1 < self.tokens.len()
                    && matches!(
                        self.tokens[self.pos + 1].kind,
                        TokenKind::String | TokenKind::MultilineString | TokenKind::RawString
                    )
                {
                    Expr::Route(self.parse_route())
                } else {
                    // treat as ident-like
                    let t = self.advance();
                    Expr::Ident(Ident {
                        name: t.text.clone(),
                        span: t.span,
                    })
                }
            }
            TokenKind::Dollar => {
                let t = self.advance();
                Expr::Placeholder(Placeholder { span: t.span })
            }
            TokenKind::Host => self.parse_capability_or_http(),
            TokenKind::LBracket => self.parse_list_literal(),
            TokenKind::RecordOpen => self.parse_record_literal(),
            TokenKind::BlockOpen | TokenKind::LBrace => Expr::Block(self.parse_block()),
            TokenKind::LParen => {
                let start = self.advance().span;
                let expr = self.parse_expression();
                let end = self.expect(TokenKind::RParen).span;
                Expr::Group(GroupExpr {
                    expr: Box::new(expr),
                    span: start.merge(end),
                })
            }
            TokenKind::Underscore => {
                let t = self.advance();
                Expr::Ident(Ident {
                    name: "_".into(),
                    span: t.span,
                })
            }
            _ => {
                let span = self.current_span();
                self.diagnostics.push(simple_error(
                    E010_UNEXPECTED_TOKEN,
                    format!("unexpected token {}", self.peek_kind()),
                    self.file,
                    span,
                    "expected expression",
                ));
                if !self.is_eof() {
                    self.advance();
                }
                Expr::Literal(Literal {
                    kind: LitKind::None,
                    span,
                })
            }
        }
    }

    pub(super) fn parse_capability_or_http(&mut self) -> Expr {
        let start = self.advance().span; // @ or host.
        let mut path = Vec::new();
        // first segment
        if self.check(TokenKind::Ident)
            || matches!(
                self.peek_kind(),
                TokenKind::Item | TokenKind::Test | TokenKind::Get
            )
        {
            path.push(self.advance().text.clone());
        } else {
            self.error_expected("capability name");
            return Expr::Capability(CapabilityRef {
                path: vec!["?".into()],
                span: start,
            });
        }
        while self.check(TokenKind::Dot) {
            self.advance();
            if self.check(TokenKind::Ident) || self.is_keyword_as_ident() {
                path.push(self.advance().text.clone());
            } else {
                break;
            }
        }

        // @http.listen addr block
        if path.len() >= 2 && path[0] == "http" && path[1] == "listen" {
            let addr = self.parse_listen_addr();
            let body = self.parse_block();
            let span = start.merge(body.span);
            return Expr::HttpListen(HttpListenExpr {
                addr: Box::new(addr),
                body,
                span,
            });
        }

        // `@tcp.listen addr ⟦ |conn| … ⟧` — the same juxtaposed shape, but it is
        // *only* sugar: it becomes the ordinary call `@tcp.listen(addr, ⟦|conn| …⟧)`,
        // whose block argument desugars to a closure like any other. No new AST node,
        // no new IR, and effect discipline still applies — the call needs its `!`,
        // unlike `@http.listen`, which bypasses the check by not being a call at all.
        if path.len() >= 2 && path[0] == "tcp" && path[1] == "listen" {
            let addr = self.parse_listen_addr();
            let block = self.parse_block();
            let span = start.merge(block.span);
            return Expr::Call(CallExpr {
                callee: Box::new(Expr::Capability(CapabilityRef {
                    path,
                    span: start.merge(self.prev_span()),
                })),
                args: vec![addr, Expr::Block(block)],
                span,
            });
        }

        let end = self.prev_span();
        Expr::Capability(CapabilityRef {
            path,
            span: start.merge(end),
        })
    }

    /// The address in `@http.listen addr ⟦…⟧` / `@tcp.listen addr ⟦…⟧`.
    ///
    /// The handler block belongs to `listen`, never to the address — so
    /// trailing-block call sugar is switched off while the address is read. It is
    /// only inert for a string literal, which is not callable; with the address in a
    /// binding (`where ← "127.0.0.1:9000"`) the sugar would otherwise read
    /// `where ⟦…⟧` as a *call to `where`* and leave `listen` with no block at all.
    /// Match scrutinees turn it off for exactly the same reason.
    fn parse_listen_addr(&mut self) -> Expr {
        let prev = self.allow_trailing_block;
        self.allow_trailing_block = false;
        let addr = self.parse_expression();
        self.allow_trailing_block = prev;
        addr
    }

    pub(super) fn parse_route(&mut self) -> RouteExpr {
        let method_tok = self.advance();
        let method = match method_tok.kind {
            TokenKind::Get => HttpMethod::Get,
            TokenKind::Post => HttpMethod::Post,
            TokenKind::Put => HttpMethod::Put,
            TokenKind::Patch => HttpMethod::Patch,
            TokenKind::Delete => HttpMethod::Delete,
            TokenKind::Head => HttpMethod::Head,
            TokenKind::Options => HttpMethod::Options,
            TokenKind::Ident => match method_tok.text.as_str() {
                "GET" => HttpMethod::Get,
                "POST" => HttpMethod::Post,
                "PUT" => HttpMethod::Put,
                "PATCH" => HttpMethod::Patch,
                "DELETE" => HttpMethod::Delete,
                "HEAD" => HttpMethod::Head,
                "OPTIONS" => HttpMethod::Options,
                _ => HttpMethod::Get,
            },
            _ => HttpMethod::Get,
        };
        let path_tok = self.advance();
        let path = path_tok.text.clone();
        let params = if self.check(TokenKind::Pipe) {
            self.parse_block_params()
        } else {
            vec![]
        };
        let body = self.parse_block();
        let span = method_tok.span.merge(body.span);
        RouteExpr {
            method,
            path,
            params,
            body,
            span,
        }
    }

    pub(super) fn parse_list_literal(&mut self) -> Expr {
        let start = self.advance().span;
        let mut elements = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::RBracket) {
            elements.push(self.parse_expression());
            if self.check(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.expect(TokenKind::RBracket).span;
        Expr::List(ListExpr {
            elements,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_record_literal(&mut self) -> Expr {
        let start = self.advance().span; // ⟨ or <<
        let mut entries = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::RecordClose) {
            // `⟨..base, k: v⟩` — spread `base` in, then let later entries win.
            // Sugar for record merge: see the fold in rite-sem's `Expr::Record`.
            // `..` is canonical (grammar/sigils.toml) and dialect-neutral; `...` is
            // accepted as a synonym and normalised to `..` by the formatter.
            if self.check(TokenKind::Rest) || self.check(TokenKind::Spread) {
                let open = self.advance().span;
                let value = self.parse_expression();
                let span = open.merge(value.span());
                entries.push(RecordEntry {
                    key: RecordKey::Spread,
                    value,
                    span,
                });
                if self.check(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            let key = self.parse_record_key();
            self.expect(TokenKind::Colon);
            let value = self.parse_expression();
            let span = match &key {
                RecordKey::Ident(i) => i.span,
                RecordKey::Atom(a) => a.span,
                RecordKey::String(_) => self.prev_span(),
                RecordKey::Spread => self.prev_span(),
            }
            .merge(value.span());
            entries.push(RecordEntry { key, value, span });
            if self.check(TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let end = self.expect(TokenKind::RecordClose).span;
        Expr::Record(RecordExpr {
            entries,
            span: start.merge(end),
        })
    }

    pub(super) fn parse_record_key(&mut self) -> RecordKey {
        if self.check(TokenKind::Atom) {
            RecordKey::Atom(self.parse_atom_lit())
        } else if matches!(
            self.peek_kind(),
            TokenKind::String | TokenKind::MultilineString | TokenKind::RawString
        ) {
            let t = self.advance();
            RecordKey::String(t.text.clone())
        } else {
            RecordKey::Ident(self.parse_ident())
        }
    }

    pub(super) fn parse_block(&mut self) -> Block {
        let start = self.current_span();
        let open = self.peek_kind();
        if open != TokenKind::BlockOpen && open != TokenKind::LBrace {
            self.error_expected("block");
            return Block {
                params: vec![],
                has_param_list: false,
                body: vec![],
                span: start,
            };
        }
        self.advance();
        // Whether the `|…|` was written, not whether it named anything: `{ || 42 }`
        // is a function of no arguments, and `⟦ 42 ⟧` is a block that evaluates to
        // 42. Both have an empty `params`, so the distinction has to be recorded
        // here or it is gone by the time desugar has to make it.
        let has_param_list = self.check(TokenKind::Pipe);
        let params = if has_param_list {
            self.parse_block_params()
        } else {
            vec![]
        };

        let mut body = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::BlockClose) && !self.check(TokenKind::RBrace)
        {
            if self.check(TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            // middleware: use / ⊏ expr  (`use @http.log` or glyph `⊏ @http.log`)
            if self.check(TokenKind::Use) {
                // Could be import or middleware — in block after http.listen, it's middleware
                // Heuristic: if next is Host or LBrace or Ident path without only module path...
                // Spec: `use @http.log` / `⊏ @http.log` or `use { |req, next| ... }`
                let checkpoint = self.pos;
                self.advance(); // use / ⊏
                if self.check(TokenKind::Host)
                    || self.check(TokenKind::LBrace)
                    || self.check(TokenKind::BlockOpen)
                {
                    let expr = self.parse_expression();
                    body.push(Item::Statement(Stmt::Expr(Expr::Call(CallExpr {
                        callee: Box::new(Expr::Ident(Ident {
                            name: "__middleware_use".into(),
                            span: expr.span(),
                        })),
                        args: vec![expr],
                        span: start,
                    }))));
                    continue;
                }
                self.pos = checkpoint;
            }

            // match arms inside match already handled; routes as expressions
            if let Some(item) = self.parse_item_or_stmt() {
                body.push(item);
            } else {
                self.advance();
            }
        }

        if self.check(TokenKind::BlockClose) || self.check(TokenKind::RBrace) {
            self.advance();
        } else {
            self.diagnostics.push(simple_error(
                E012_UNCLOSED_DELIMITER,
                "unclosed block",
                self.file,
                start,
                "expected ⟧, ]], or }",
            ));
        }
        let end = self.prev_span();
        Block {
            params,
            has_param_list,
            body,
            span: start.merge(end),
        }
    }

    pub(super) fn parse_block_params(&mut self) -> Vec<Param> {
        self.expect(TokenKind::Pipe);
        let params = self.parse_param_list(TokenKind::Pipe);
        self.expect(TokenKind::Pipe);
        params
    }

    pub(super) fn parse_param_list(&mut self, closer: TokenKind) -> Vec<Param> {
        let mut params = Vec::new();
        if self.check(closer) {
            return params;
        }
        loop {
            let name = self.parse_ident();
            let ty = if self.check(TokenKind::Colon) {
                self.advance();
                Some(self.parse_type_expr())
            } else {
                None
            };
            let span = name.span;
            params.push(Param { name, ty, span });
            if self.check(TokenKind::Comma) {
                self.advance();
                if self.check(closer) {
                    break;
                }
            } else {
                break;
            }
        }
        params
    }

    pub(super) fn parse_arg_list(&mut self, closer: TokenKind) -> Vec<Expr> {
        let mut args = Vec::new();
        if self.check(closer) {
            return args;
        }
        loop {
            args.push(self.parse_expression());
            if self.check(TokenKind::Comma) {
                self.advance();
                if self.check(closer) {
                    break;
                }
            } else {
                break;
            }
        }
        args
    }

    // ── Patterns ─────────────────────────────────────────────
}
