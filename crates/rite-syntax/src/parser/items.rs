//! Declarations and statements: the top level of a program.

#![allow(clippy::only_used_in_recursion)]

use super::support::pattern_span;
use super::*;
use crate::ast::*;
use crate::token::TokenKind;
use rite_core::Span;

impl Parser {
    pub(super) fn parse_program(&mut self) -> Program {
        let start = self.current_span();
        let mut items = Vec::new();
        while !self.is_eof() {
            // Skip stray semicolons
            if self.check(TokenKind::Semicolon) {
                self.advance();
                continue;
            }
            if let Some(item) = self.parse_item_or_stmt() {
                items.push(item);
            } else {
                // recovery: skip token
                self.advance();
            }
        }
        let end = self.prev_span();
        Program {
            file: self.file,
            items,
            span: start.merge(end),
        }
    }

    pub(super) fn parse_item_or_stmt(&mut self) -> Option<Item> {
        if self.check(TokenKind::Use) {
            return Some(Item::Import(self.parse_import(false)));
        }
        // `pub use path` re-export
        if self.check(TokenKind::Pub) && self.check_nth(1, TokenKind::Use) {
            self.advance(); // pub
            return Some(Item::Import(self.parse_import(true)));
        }
        if self.check(TokenKind::Pub) || self.check(TokenKind::Def) {
            return Some(self.parse_decl_item());
        }
        // Match arms at top level shouldn't appear; statements
        let stmt = self.parse_statement()?;
        Some(Item::Statement(stmt))
    }

    pub(super) fn parse_decl_item(&mut self) -> Item {
        // Taken here, before any of the declaration is consumed: `doc_before` locates
        // the block by what precedes the *current* token, and by the end of this
        // function `self.pos` has moved past the whole body.
        let doc = self.doc_before_current();
        let is_pub = if self.check(TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(TokenKind::Def);
        // `◆! name(…)` / `def! name(…)` — the declaration says the body performs
        // host effects, so callers need a marker. Read straight after the `◆`,
        // mirroring how `!` prefixes an effectful statement.
        let is_effectful = if self.check(TokenKind::Effect) {
            self.advance();
            true
        } else {
            false
        };
        // test decl
        if self.check(TokenKind::Test) {
            self.advance();
            let name_tok = self.expect(TokenKind::String);
            let name = name_tok.text.clone();
            let body = self.parse_block();
            let span = name_tok.span.merge(body.span);
            return Item::Test(TestDecl { name, body, span });
        }
        // event: item/room/world
        if matches!(
            self.peek_kind(),
            TokenKind::Item | TokenKind::Room | TokenKind::World
        ) {
            let kind = match self.advance().kind {
                TokenKind::Item => EventKind::Item,
                TokenKind::Room => EventKind::Room,
                TokenKind::World => EventKind::World,
                _ => unreachable!(),
            };
            let atom = self.parse_atom_lit();
            let body = self.parse_block();
            let span = atom.span.merge(body.span);
            return Item::Event(EventDecl {
                kind,
                atom,
                body,
                span,
            });
        }

        let name = self.parse_ident();
        // data decl: def Name ⟨...⟩ without parens
        if self.check(TokenKind::RecordOpen) {
            let rec = self.parse_record_literal();
            if let Expr::Record(r) = rec {
                return Item::Data(DataDecl {
                    name,
                    fields: r.entries,
                    span: r.span,
                });
            }
        }

        self.expect(TokenKind::LParen);
        let params = self.parse_param_list(TokenKind::RParen);
        self.expect(TokenKind::RParen);
        let return_type = if self.check(TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type_expr())
        } else {
            None
        };
        let body = self.parse_block();
        let span = name.span.merge(body.span);
        Item::Function(FunctionDecl {
            is_pub,
            is_effectful,
            name,
            params,
            return_type,
            body,
            doc,
            span,
        })
    }

    /// The `///` block attached to the declaration at the current token.
    ///
    /// A block qualifies when it sits between the previous piece of code and here —
    /// i.e. nothing else intervenes. That test needs no source text and correctly
    /// rejects a stray block that some other statement has already passed. Blocks are
    /// consumed on use, so two declarations cannot claim the same one.
    ///
    /// Must be called before consuming any of the declaration.
    pub(super) fn doc_before_current(&mut self) -> Option<String> {
        let offset = self.peek().span.start.as_usize();
        // End of the last code token before here (0 at the start of the file).
        let prev_code_end = self
            .pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map(|t| t.span.end.as_usize())
            .unwrap_or(0);
        let idx = self
            .doc_comments
            .iter()
            .rposition(|(start, end, _)| *end <= offset && *start >= prev_code_end)?;
        let (_, _, text) = self.doc_comments.remove(idx);
        Some(text)
    }

    pub(super) fn parse_import(&mut self, is_pub: bool) -> ImportDecl {
        let start = self.advance().span; // use
        let path = self.parse_module_path();
        let alias = if self.check(TokenKind::As) {
            self.advance();
            Some(self.parse_ident())
        } else {
            None
        };
        let end = alias.as_ref().map(|a| a.span).unwrap_or(path.span);
        ImportDecl {
            path,
            alias,
            is_pub,
            span: start.merge(end),
        }
    }

    pub(super) fn parse_module_path(&mut self) -> ModulePath {
        // Relative: `./helpers`, `../lib/util` (slash-separated after ./ or ../)
        if self.check(TokenKind::Dot) {
            let start = self.peek().span;
            self.advance(); // first .
            let parent = if self.check(TokenKind::Dot) {
                self.advance();
                true
            } else {
                false
            };
            if self.check(TokenKind::Slash) {
                self.advance();
            }
            let mut segments = vec![Ident {
                name: if parent { "..".into() } else { ".".into() },
                span: start,
            }];
            segments.push(self.parse_ident());
            while self.check(TokenKind::Slash) || self.check(TokenKind::Dot) {
                self.advance();
                segments.push(self.parse_ident());
            }
            let span = start.merge(segments.last().map(|s| s.span).unwrap_or(start));
            return ModulePath { segments, span };
        }

        let mut segments = vec![self.parse_ident()];
        while self.check(TokenKind::Dot) {
            self.advance();
            segments.push(self.parse_ident());
        }
        let span = segments
            .first()
            .map(|s| s.span)
            .unwrap_or(Span::DUMMY)
            .merge(segments.last().map(|s| s.span).unwrap_or(Span::DUMMY));
        ModulePath { segments, span }
    }

    pub(super) fn parse_statement(&mut self) -> Option<Stmt> {
        if self.check(TokenKind::Return) {
            let start = self.advance().span;
            // Set when the value is several juxtaposed expressions, so the formatter can
            // reprint `^ 200 ⟨…⟩` instead of the list it lowers to.
            let mut juxtaposed = false;
            let value = if self.at_expr_start() {
                // Support `^ 200 ⟨…⟩` / `^ 200 "text"` as multi-value return (status, body, …)
                let first = self.parse_expression();
                if self.at_expr_start()
                    && !self.check(TokenKind::Arrow)
                    && !self.check(TokenKind::Bind)
                    && !self.check(TokenKind::BindMut)
                {
                    let mut elements = vec![first];
                    while self.at_expr_start()
                        && !self.check(TokenKind::Arrow)
                        && !self.check(TokenKind::Semicolon)
                        && !self.check(TokenKind::BlockClose)
                        && !self.check(TokenKind::RBrace)
                    {
                        // Don't consume next statement-like constructs unboundedly
                        let checkpoint = self.pos;
                        let next = self.parse_expression();
                        // Stop if we over-consumed a new binding-like form
                        if self.check(TokenKind::Bind) || self.check(TokenKind::BindMut) {
                            self.pos = checkpoint;
                            break;
                        }
                        elements.push(next);
                        if elements.len() >= 3 {
                            break;
                        }
                    }
                    let span = elements
                        .first()
                        .map(|e| e.span())
                        .unwrap_or(start)
                        .merge(elements.last().map(|e| e.span()).unwrap_or(start));
                    juxtaposed = true;
                    Some(Expr::List(ListExpr { elements, span }))
                } else {
                    Some(first)
                }
            } else {
                None
            };
            let end = value.as_ref().map(|e| e.span()).unwrap_or(start);
            if self.check(TokenKind::Semicolon) {
                self.advance();
            }
            return Some(Stmt::Return(ReturnStmt {
                value,
                juxtaposed,
                span: start.merge(end),
            }));
        }

        // Lookahead for binding: pattern bind_op expr
        // Heuristic: ident/pattern then Bind/BindMut
        let checkpoint = self.pos;
        // Speculative: `⟨…⟩` and `[…]` at statement start might be a destructuring
        // binding or might just be a literal expression. Snapshot the diagnostics too —
        // rewinding `pos` alone left the abandoned pattern attempt's errors behind, so a
        // statement-position literal that is not valid *as a pattern* (`⟨..base, k: v⟩`)
        // reported bogus errors even though the expression parse then succeeded.
        let diag_checkpoint = self.diagnostics.len();
        if let Some(pat) = self.try_parse_binding_pattern() {
            if self.check(TokenKind::Bind) || self.check(TokenKind::BindMut) {
                let mutable = self.check(TokenKind::BindMut);
                self.advance();
                let value = self.parse_expression();
                let span = pattern_span(&pat).merge(value.span());
                if self.check(TokenKind::Semicolon) {
                    self.advance();
                }
                return Some(Stmt::Binding(Binding {
                    pattern: pat,
                    mutable,
                    value,
                    span,
                }));
            }
        }
        self.pos = checkpoint;
        self.diagnostics.rewind(diag_checkpoint);

        // assignment: ident := or op-assign += -= *= /= %=
        if self.check(TokenKind::Ident) && self.pos + 1 < self.tokens.len() {
            let next = self.tokens[self.pos + 1].kind;
            let op = match next {
                TokenKind::Assign => Some(None),
                TokenKind::PlusAssign => Some(Some(BinOp::Add)),
                TokenKind::MinusAssign => Some(Some(BinOp::Sub)),
                TokenKind::StarAssign => Some(Some(BinOp::Mul)),
                TokenKind::SlashAssign => Some(Some(BinOp::Div)),
                TokenKind::PercentAssign => Some(Some(BinOp::Rem)),
                _ => None,
            };
            if let Some(op) = op {
                let name = self.parse_ident();
                self.advance(); // := or op=
                let value = self.parse_expression();
                let span = name.span.merge(value.span());
                if self.check(TokenKind::Semicolon) {
                    self.advance();
                }
                return Some(Stmt::Assign(Assign {
                    name,
                    op,
                    value,
                    span,
                }));
            }
        }

        // for x in xs ⟦ … ⟧  /  ∀ x ∈ xs ⟦ … ⟧
        if self.check(TokenKind::For) || self.check(TokenKind::ForAll) {
            return Some(self.parse_for_in_stmt());
        }

        // say / ¶ expr  →  ! @console.println(expr)
        if self.check(TokenKind::Say) || self.check(TokenKind::Paragraph) {
            let start = self.advance().span;
            let value = self.parse_expression();
            let span = start.merge(value.span());
            if self.check(TokenKind::Semicolon) {
                self.advance();
            }
            return Some(Stmt::Expr(Expr::Unary(UnaryExpr {
                op: UnaryOp::Effect,
                expr: Box::new(Expr::Call(CallExpr {
                    callee: Box::new(Expr::Capability(CapabilityRef {
                        path: vec!["console".into(), "println".into()],
                        span,
                    })),
                    args: vec![value],
                    span,
                })),
                span,
            })));
        }

        // unless / ¿ cond ⟦ … ⟧
        if self.check(TokenKind::Unless) {
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
            let not_cond = Expr::Unary(UnaryExpr {
                op: UnaryOp::Not,
                expr: Box::new(condition),
                span: start,
            });
            return Some(Stmt::Expr(Expr::If(IfExpr {
                condition: Box::new(not_cond),
                then_branch,
                else_branch,
                span: start.merge(end),
            })));
        }

        // while cond ⟦ … ⟧ — desugar to recursive local helper via special form
        if self.check(TokenKind::While) {
            return Some(self.parse_while_stmt());
        }

        // loop n ⟦ … ⟧ — iterate n times
        if self.check(TokenKind::Loop) {
            return Some(self.parse_loop_stmt());
        }

        if self.at_expr_start() {
            let expr = self.parse_expression();
            if self.check(TokenKind::Semicolon) {
                self.advance();
            }
            return Some(Stmt::Expr(expr));
        }
        None
    }

    pub(super) fn parse_for_in_stmt(&mut self) -> Stmt {
        let start = self.advance().span; // for / ∀
        let var = self.parse_ident();
        // in / ∈
        if self.check(TokenKind::In) {
            self.advance();
        } else {
            self.error_expected("in / ∈");
        }
        let prev = self.allow_trailing_block;
        self.allow_trailing_block = false;
        let iter = self.parse_expression();
        self.allow_trailing_block = prev;
        let body = self.parse_block();
        let span = start.merge(body.span);
        // Desugar: iter → each { |var| body }
        let stage = Expr::Call(CallExpr {
            callee: Box::new(Expr::Ident(Ident {
                name: "each".into(),
                span,
            })),
            args: vec![Expr::Block(Block {
                has_param_list: true,
                params: vec![Param {
                    name: var,
                    ty: None,
                    span,
                }],
                body: body.body,
                span: body.span,
            })],
            span,
        });
        Stmt::Expr(Expr::Pipeline(PipelineExpr {
            input: Box::new(iter),
            stages: vec![stage],
            span,
        }))
    }

    pub(super) fn parse_while_stmt(&mut self) -> Stmt {
        // while cond ⟦ body ⟧  →  invoke a small recursive loop via __while sugar:
        // desugar to: (◆ __w() ⟦ ? cond ⟦ body; ^ __w() ⟧ ⟧)()
        let start = self.advance().span;
        let prev = self.allow_trailing_block;
        self.allow_trailing_block = false;
        let cond = self.parse_expression();
        self.allow_trailing_block = prev;
        let body = self.parse_block();
        let span = start.merge(body.span);
        // Represent as Call to builtin-like "while_loop" with cond-closure and body-closure
        // Runtime: while_loop(pred_fn, body_fn)
        Stmt::Expr(Expr::Call(CallExpr {
            callee: Box::new(Expr::Ident(Ident {
                name: "while_loop".into(),
                span,
            })),
            args: vec![
                // Dummy param forces closure (not bare block value).
                Expr::Block(Block {
                    has_param_list: true,
                    params: vec![Param {
                        name: Ident {
                            name: "__".into(),
                            span,
                        },
                        ty: None,
                        span,
                    }],
                    body: vec![Item::Statement(Stmt::Expr(cond))],
                    span,
                }),
                Expr::Block(Block {
                    has_param_list: true,
                    params: vec![Param {
                        name: Ident {
                            name: "__".into(),
                            span,
                        },
                        ty: None,
                        span,
                    }],
                    body: body.body,
                    span: body.span,
                }),
            ],
            span,
        }))
    }

    pub(super) fn parse_loop_stmt(&mut self) -> Stmt {
        // loop n ⟦ body ⟧ → range(0,n) → each { |_| body }
        let start = self.advance().span;
        let n = self.parse_expression();
        let body = self.parse_block();
        let span = start.merge(body.span);
        let range_call = Expr::Call(CallExpr {
            callee: Box::new(Expr::Ident(Ident {
                name: "range".into(),
                span,
            })),
            args: vec![
                Expr::Literal(Literal {
                    kind: LitKind::Int(0),
                    span,
                }),
                n,
            ],
            span,
        });
        let stage = Expr::Call(CallExpr {
            callee: Box::new(Expr::Ident(Ident {
                name: "each".into(),
                span,
            })),
            args: vec![Expr::Block(Block {
                has_param_list: true,
                params: vec![Param {
                    name: Ident {
                        name: "_".into(),
                        span,
                    },
                    ty: None,
                    span,
                }],
                body: body.body,
                span: body.span,
            })],
            span,
        });
        Stmt::Expr(Expr::Pipeline(PipelineExpr {
            input: Box::new(range_call),
            stages: vec![stage],
            span,
        }))
    }

    pub(super) fn try_parse_binding_pattern(&mut self) -> Option<Pattern> {
        // Only simple patterns for binding left-hand side at statement level
        if self.check(TokenKind::Ident) {
            return Some(Pattern::Ident(self.parse_ident()));
        }
        if self.check(TokenKind::LBracket) {
            return Some(self.parse_list_pattern());
        }
        if self.check(TokenKind::RecordOpen) {
            return Some(self.parse_record_pattern());
        }
        None
    }

    // ── Expressions ──────────────────────────────────────────
}
