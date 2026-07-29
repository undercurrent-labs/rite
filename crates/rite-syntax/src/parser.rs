use crate::ast::*;
use crate::token::{Token, TokenKind};
use rite_core::{
    simple_error, Diagnostics, FileId, Span, E010_UNEXPECTED_TOKEN, E011_EXPECTED_TOKEN,
    E012_UNCLOSED_DELIMITER, E013_INVALID_SYNTAX,
};

pub struct Parser {
    file: FileId,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Diagnostics,
    /// When false, `status ⟦…⟧` is not treated as a call (needed for `~ status ⟦…⟧`).
    allow_trailing_block: bool,
}

pub fn parse(file: FileId, tokens: &[Token]) -> (Option<Program>, Diagnostics) {
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
    };
    let expr = p.parse_expression();
    (Some(expr), p.diagnostics)
}

impl Parser {
    fn parse_program(&mut self) -> Program {
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

    fn parse_item_or_stmt(&mut self) -> Option<Item> {
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

    fn parse_decl_item(&mut self) -> Item {
        let is_pub = if self.check(TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        self.expect(TokenKind::Def);
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
            name,
            params,
            return_type,
            body,
            doc: None,
            span,
        })
    }

    fn parse_import(&mut self, is_pub: bool) -> ImportDecl {
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

    fn parse_module_path(&mut self) -> ModulePath {
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

    fn parse_statement(&mut self) -> Option<Stmt> {
        if self.check(TokenKind::Return) {
            let start = self.advance().span;
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
                span: start.merge(end),
            }));
        }

        // Lookahead for binding: pattern bind_op expr
        // Heuristic: ident/pattern then Bind/BindMut
        let checkpoint = self.pos;
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

    fn parse_for_in_stmt(&mut self) -> Stmt {
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

    fn parse_while_stmt(&mut self) -> Stmt {
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

    fn parse_loop_stmt(&mut self) -> Stmt {
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

    fn try_parse_binding_pattern(&mut self) -> Option<Pattern> {
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

    pub fn parse_expression(&mut self) -> Expr {
        self.parse_pipeline()
    }

    fn parse_pipeline(&mut self) -> Expr {
        let expr = self.parse_conditional();
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
            expr
        } else {
            let end = stages.last().map(|s| s.span()).unwrap_or(start);
            Expr::Pipeline(PipelineExpr {
                input: Box::new(expr),
                stages,
                span: start.merge(end),
            })
        }
    }

    fn parse_pipeline_stage(&mut self) -> Expr {
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
        self.parse_conditional()
    }

    fn parse_conditional(&mut self) -> Expr {
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
        self.parse_coalesce()
    }

    fn parse_match(&mut self) -> Expr {
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

    fn parse_match_arm(&mut self) -> MatchArm {
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

    fn parse_coalesce(&mut self) -> Expr {
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

    fn parse_or(&mut self) -> Expr {
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

    fn parse_xor(&mut self) -> Expr {
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

    fn parse_and(&mut self) -> Expr {
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

    fn parse_equality(&mut self) -> Expr {
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

    fn parse_comparison(&mut self) -> Expr {
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

    fn parse_range(&mut self) -> Expr {
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

    fn parse_term(&mut self) -> Expr {
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

    fn parse_factor(&mut self) -> Expr {
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

    fn parse_power(&mut self) -> Expr {
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

    fn parse_compose(&mut self) -> Expr {
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

    fn parse_unary(&mut self) -> Expr {
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

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
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

    fn parse_primary(&mut self) -> Expr {
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
            TokenKind::Ident => {
                // HTTP method routes only inside blocks — treat as ident here
                // But GET "/path" is route
                if self.is_http_method() && self.pos + 1 < self.tokens.len() {
                    if matches!(
                        self.tokens[self.pos + 1].kind,
                        TokenKind::String | TokenKind::MultilineString | TokenKind::RawString
                    ) {
                        return Expr::Route(self.parse_route());
                    }
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

    fn parse_capability_or_http(&mut self) -> Expr {
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
            let addr = self.parse_expression();
            let body = self.parse_block();
            let span = start.merge(body.span);
            return Expr::HttpListen(HttpListenExpr {
                addr: Box::new(addr),
                body,
                span,
            });
        }

        let end = self.prev_span();
        Expr::Capability(CapabilityRef {
            path,
            span: start.merge(end),
        })
    }

    fn parse_route(&mut self) -> RouteExpr {
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

    fn parse_list_literal(&mut self) -> Expr {
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

    fn parse_record_literal(&mut self) -> Expr {
        let start = self.advance().span; // ⟨ or <<
        let mut entries = Vec::new();
        while !self.is_eof() && !self.check(TokenKind::RecordClose) {
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

    fn parse_record_key(&mut self) -> RecordKey {
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

    fn parse_block(&mut self) -> Block {
        let start = self.current_span();
        let open = self.peek_kind();
        if open != TokenKind::BlockOpen && open != TokenKind::LBrace {
            self.error_expected("block");
            return Block {
                params: vec![],
                body: vec![],
                span: start,
            };
        }
        self.advance();
        let params = if self.check(TokenKind::Pipe) {
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
            body,
            span: start.merge(end),
        }
    }

    fn parse_block_params(&mut self) -> Vec<Param> {
        self.expect(TokenKind::Pipe);
        let params = self.parse_param_list(TokenKind::Pipe);
        self.expect(TokenKind::Pipe);
        params
    }

    fn parse_param_list(&mut self, closer: TokenKind) -> Vec<Param> {
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

    fn parse_arg_list(&mut self, closer: TokenKind) -> Vec<Expr> {
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

    fn parse_pattern(&mut self) -> Pattern {
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

    fn parse_list_pattern(&mut self) -> Pattern {
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

    fn parse_record_pattern(&mut self) -> Pattern {
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

    fn parse_type_expr(&mut self) -> TypeExpr {
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

    fn parse_atom_lit(&mut self) -> AtomLit {
        let t = self.expect(TokenKind::Atom);
        let parts: Vec<String> = t.text.split('.').map(|s| s.to_string()).collect();
        AtomLit {
            parts,
            span: t.span,
        }
    }

    fn parse_ident(&mut self) -> Ident {
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

    fn at_expr_start(&self) -> bool {
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
                | TokenKind::Say
                | TokenKind::Paragraph
                | TokenKind::For
                | TokenKind::ForAll
                | TokenKind::Unless
                | TokenKind::While
                | TokenKind::Loop
        )
    }

    fn at_pattern_start(&self) -> bool {
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
    fn looks_like_prefix_if(&self) -> bool {
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
                TokenKind::LParen => {
                    depth_paren += 1;
                    saw_expr = true;
                }
                TokenKind::RParen => {
                    depth_paren = depth_paren.saturating_sub(1);
                    saw_expr = true;
                }
                TokenKind::LBracket => {
                    depth_bracket += 1;
                    saw_expr = true;
                }
                TokenKind::RBracket => {
                    depth_bracket = depth_bracket.saturating_sub(1);
                    saw_expr = true;
                }
                TokenKind::RecordOpen => {
                    depth_brace += 1;
                    saw_expr = true;
                }
                TokenKind::RecordClose => {
                    depth_brace = depth_brace.saturating_sub(1);
                    saw_expr = true;
                }
                _ => saw_expr = true,
            }
            i += 1;
        }
        false
    }

    fn is_http_method(&self) -> bool {
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

    fn is_keyword_as_ident(&self) -> bool {
        matches!(
            self.peek_kind(),
            TokenKind::Item
                | TokenKind::Room
                | TokenKind::World
                | TokenKind::Test
                | TokenKind::Ok
                | TokenKind::Err
                | TokenKind::Some
                | TokenKind::Get
                | TokenKind::Post
                | TokenKind::Put
                | TokenKind::Patch
                | TokenKind::Delete
                | TokenKind::Head
                | TokenKind::Options
        )
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn check_nth(&self, n: usize, kind: TokenKind) -> bool {
        self.tokens
            .get(self.pos + n)
            .map(|t| t.kind == kind)
            .unwrap_or(false)
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or_else(|| Token {
            kind: TokenKind::Eof,
            span: Span::DUMMY,
            file: self.file,
            text: String::new(),
        })
    }

    fn peek_kind(&self) -> TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| t.kind)
            .unwrap_or(TokenKind::Eof)
    }

    fn is_eof(&self) -> bool {
        self.peek_kind() == TokenKind::Eof || self.pos >= self.tokens.len()
    }

    fn advance(&mut self) -> Token {
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
            })
        }
    }

    fn expect(&mut self, kind: TokenKind) -> Token {
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
                })
        }
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.pos)
            .map(|t| t.span)
            .unwrap_or(Span::DUMMY)
    }

    fn prev_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            Span::DUMMY
        }
    }

    fn error_expected(&mut self, what: &str) {
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

fn is_callable_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Ident(_) | Expr::Member(_) | Expr::Call(_) | Expr::Capability(_) | Expr::Group(_)
    )
}

fn pattern_span(p: &Pattern) -> Span {
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
