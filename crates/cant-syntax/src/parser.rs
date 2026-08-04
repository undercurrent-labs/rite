//! Recursive-descent parser for Cant.
//!
//! The parser is where the ASCII ambiguities the lexer refused to guess at are
//! resolved, all of them from position:
//!
//! * a stage that is nothing but `*` is scatter; a `*` with anything else around
//!   it is multiplication inside a leaf;
//! * a stage that is nothing but `[]` is collect, except as the *first* stage,
//!   where it is the empty list literal;
//! * `:name value` is a modifier only immediately after a structural block's
//!   closing brace — everywhere else `:` is Rite's atom prefix and belongs to the
//!   leaf, which is why `?{ $.level = :error }` parses.
//!
//! Everything the parser cannot see is left alone. A leaf is a run of tokens
//! recorded as source text and a span; whether the names in it exist, whether
//! the call has the right arity, and whether it honours Rite's effect discipline
//! are all Rite's questions, asked after expansion.
//!
//! Recovery is uniform: a construct that cannot be parsed produces a diagnostic
//! and consumes at least one token, so parsing always terminates and a caller
//! always gets every error the source contains rather than only the first.

use crate::ast::*;
use crate::diagnostic::*;
use crate::lexer::lex;
use crate::token::{CantToken, CantTokenKind as K, Spelling};
use rite_core::{FileId, SourceFile, SourceSpan, Span};

/// How deep `?{ |{ ~{ … } } }` may nest.
///
/// The parser recurses once per structural block, so an unbounded nest is a
/// stack overflow — which a fuzzer finds in seconds and which no diagnostic can
/// be produced from. 64 is far past anything readable and far short of the
/// recursion limit. Deeper nesting is also on graph validation's list (spec
/// §7.1); this bound exists so validation gets the chance to run.
pub const MAX_NESTING: usize = 64;

/// A token the parser consumed *as a Cant operator*.
///
/// The lexer cannot know this. A `}` closes a Cant block or a Rite closure, a
/// `*` is scatter or multiplication, a `[]` is collect or an empty list — the
/// answer is positional, and the parser is the only thing that has the position.
///
/// Recording them is what lets the formatter and the ASCII/glyph converter work
/// on tokens without re-deriving that judgement: everything in this list is
/// structural and may be respelled, and everything else is leaf text, a string,
/// or a comment, and must be copied through untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuralToken {
    pub span: Span,
    pub kind: K,
    pub spelling: Spelling,
}

/// The result of parsing a `.cant` source.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// `None` only when there was nothing to parse; a source with errors still
    /// yields the program the parser recovered, so tooling has something to work
    /// with.
    pub program: Option<CantProgramAst>,
    pub diagnostics: CantDiagnostics,
    /// The complete token stream, trivia included, in source order.
    ///
    /// Kept because the formatter and the ASCII/glyph converter need the
    /// comments and the whitespace that the parser drops, and re-lexing to get
    /// them back would let the two views drift.
    pub tokens: Vec<CantToken>,
    /// Every token the parser read as a Cant operator, in source order.
    pub structural: Vec<StructuralToken>,
}

impl ParseResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Parse a Cant source file.
pub fn parse(file: &SourceFile) -> ParseResult {
    let (tokens, diagnostics) = lex(file);
    let significant: Vec<CantToken> = tokens
        .iter()
        .filter(|t| !t.kind.is_trivia())
        .cloned()
        .collect();
    let mut parser = Parser {
        file,
        tokens: significant,
        pos: 0,
        diagnostics,
        nesting: 0,
        structural: Vec::new(),
    };
    let program = parser.parse_program();
    let mut structural = parser.structural;
    // Source order, because recovery can record a block's closer before an
    // enclosing arrow that was consumed later.
    structural.sort_by_key(|t| t.span.start);
    ParseResult {
        program,
        diagnostics: parser.diagnostics,
        tokens,
        structural,
    }
}

struct Parser<'a> {
    file: &'a SourceFile,
    /// Trivia removed. Always ends with an `Eof` token, so `current()` is total.
    tokens: Vec<CantToken>,
    pos: usize,
    diagnostics: CantDiagnostics,
    nesting: usize,
    structural: Vec<StructuralToken>,
}

impl Parser<'_> {
    // ---- token access

    fn current(&self) -> &CantToken {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn at(&self, kind: K) -> bool {
        self.current().kind == kind
    }

    fn at_eof(&self) -> bool {
        self.at(K::Eof)
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    /// Consume the current token, recording it as a Cant operator.
    ///
    /// Every structural token goes through here, so "which spans may the
    /// formatter respell" has exactly one answer and it is produced by the code
    /// that made the decision.
    fn take_structural(&mut self) {
        let token = self.current();
        let recorded = StructuralToken {
            span: token.span,
            kind: token.kind,
            spelling: token.spelling,
        };
        self.structural.push(recorded);
        self.advance();
    }

    /// Record a token as a Cant operator without moving the cursor.
    ///
    /// Scatter and collect are only recognisable *after* a run of tokens has been
    /// consumed and found to be exactly one token long, so they cannot go through
    /// [`Parser::take_structural`] on the way past.
    fn record_structural(&mut self, token: &CantToken) {
        self.structural.push(StructuralToken {
            span: token.span,
            kind: token.kind,
            spelling: token.spelling,
        });
    }

    fn file_id(&self) -> FileId {
        self.file.id
    }

    fn source_span(&self, span: Span) -> SourceSpan {
        SourceSpan::new(self.file_id(), span)
    }

    /// A flow ends where its enclosing construct takes over.
    fn at_flow_terminator(&self) -> bool {
        matches!(self.current().kind, K::Eof | K::BlockClose | K::Semi)
    }

    fn error(&mut self, code: CantCode, title: impl Into<String>, span: Span, label: &str) {
        self.diagnostics
            .push(CantDiagnostic::error(code, title).with_primary(self.source_span(span), label));
    }

    // ---- grammar

    fn parse_program(&mut self) -> Option<CantProgramAst> {
        let uses = self.parse_uses();
        let flow = self.parse_flow();

        // Whatever is left over is one mistake, not many: reporting only the
        // first avoids a cascade in which every following token is "unexpected"
        // because of the same missing brace.
        if !self.at_eof() {
            let tok = self.current().clone();
            match tok.kind {
                K::BlockClose => self.error(
                    CANT_P004_UNEXPECTED_BLOCK_CLOSE,
                    format!("unexpected `{}`", tok.text),
                    tok.span,
                    "there is no ward, fork, or orbit open here",
                ),
                K::Semi => self.error(
                    CANT_P006_UNEXPECTED_SEPARATOR,
                    "unexpected `;`",
                    tok.span,
                    "`;` separates fork branches, and there is no fork here",
                ),
                kind => self.error(
                    CANT_P002_EXPECTED_STAGE,
                    format!("unexpected {kind}"),
                    tok.span,
                    "expected `->` before another stage",
                ),
            }
        }

        if flow.stages.is_empty() {
            let span = if self.tokens.len() > 1 {
                self.tokens[0].span
            } else {
                Span::from_range(0, self.file.len())
            };
            self.diagnostics.push(
                CantDiagnostic::error(CANT_P001_EMPTY_PROGRAM, "this program has no stages")
                    .with_primary(self.source_span(span), "nothing to run")
                    .with_help("a Cant program is a flow, for example `[1, 2, 3] -> * -> []`"),
            );
            return None;
        }
        Some(CantProgramAst {
            span: flow.span,
            uses,
            flow,
        })
    }

    /// Leading `use math` lines, before the flow begins.
    ///
    /// `use` is a Rite keyword, so no leaf can legitimately begin with it —
    /// which is what makes the preamble unambiguous without the parser ever
    /// seeing a newline. The module name is one identifier; resolution is
    /// Rite's entirely.
    fn parse_uses(&mut self) -> Vec<crate::ast::UseDecl> {
        let mut uses = Vec::new();
        while self.at(K::Ident) && self.current().text == "use" {
            let use_span = self.current().span;
            self.advance();
            if self.at(K::Ident) && self.current().text != "use" {
                let name = self.current().text.clone();
                let end = self.current().span.end.as_usize();
                self.advance();
                uses.push(crate::ast::UseDecl {
                    name,
                    span: Span::from_range(use_span.start.as_usize(), end),
                });
            } else {
                self.error(
                    CANT_P002_EXPECTED_STAGE,
                    "expected a module name after `use`",
                    use_span,
                    "`use` imports a Rite module by name: `use math`",
                );
                break;
            }
        }
        uses
    }

    fn parse_flow(&mut self) -> Flow {
        let first_token = self.pos;
        let mut stages: Vec<Stage> = Vec::new();

        loop {
            if self.at_flow_terminator() {
                break;
            }
            if self.at(K::Flow) {
                let span = self.current().span;
                self.error(
                    CANT_P002_EXPECTED_STAGE,
                    "expected a stage before this flow arrow",
                    span,
                    "nothing flows into `->`",
                );
                self.advance();
                continue;
            }

            let before = self.pos;
            match self.parse_stage(stages.is_empty()) {
                Some(stage) => {
                    stages.push(stage);
                    if !self.at(K::Flow) {
                        break;
                    }
                    let arrow = self.current().span;
                    self.take_structural();
                    if self.at_flow_terminator() {
                        self.error(
                            CANT_P005_TRAILING_FLOW,
                            "a flow arrow with nothing after it",
                            arrow,
                            "expected a stage here",
                        );
                        break;
                    }
                }
                None => {
                    // Recovery consumed something, so the loop terminates; carry
                    // on so the rest of the flow is still parsed and reported.
                    if self.pos == before {
                        break;
                    }
                    if self.at(K::Flow) {
                        self.advance();
                    }
                }
            }
        }

        Flow {
            span: self.span_from(first_token),
            stages,
        }
    }

    fn parse_stage(&mut self, is_first: bool) -> Option<Stage> {
        if self.current().kind.opens_block() {
            return self.parse_block_stage();
        }
        if self.at_modifier() {
            return self.reject_orphan_modifier();
        }

        let run_start = self.pos;
        let _ = self.consume_leaf_run(&[K::Flow, K::Semi, K::BlockClose], true);
        let run = self.tokens[run_start..self.pos].to_vec();

        if run.is_empty() {
            let span = self.current().span;
            self.error(
                CANT_P002_EXPECTED_STAGE,
                format!("expected a stage, found {}", self.current().kind),
                span,
                "a stage is a value, a call, or a ward, fork, or orbit",
            );
            return None;
        }

        let span = crate::lexer::span_of(&run);
        let kind = match run.as_slice() {
            [only] if only.kind == K::Star => {
                self.record_structural(only);
                StageKind::Scatter
            }
            // `[]` opening a program is an empty list, not a collect: there are
            // no emissions to gather yet. The glyph `⌁` has no second meaning,
            // so it stays collect wherever it appears and graph validation gets
            // to say why that is wrong.
            [only] if only.kind == K::Collect && is_first && only.spelling == Spelling::Ascii => {
                StageKind::Leaf(self.leaf_from(&run))
            }
            [only] if only.kind == K::Collect => {
                self.record_structural(only);
                StageKind::Collect
            }
            _ => {
                self.report_glyph_only_operators_in_leaf(&run);
                StageKind::Leaf(self.leaf_from(&run))
            }
        };
        Some(Stage {
            kind,
            span,
            modifiers: Vec::new(),
        })
    }

    fn parse_block_stage(&mut self) -> Option<Stage> {
        let opener = self.current().clone();

        if self.nesting >= MAX_NESTING {
            self.diagnostics.push(
                CantDiagnostic::error(
                    CANT_P013_NESTING_TOO_DEEP,
                    format!("blocks nested more than {MAX_NESTING} deep"),
                )
                .with_primary(
                    self.source_span(opener.span),
                    "this block is too deeply nested",
                )
                .with_help("flatten the flow, or lift part of it into its own program"),
            );
            self.skip_to_matching_close();
            return None;
        }

        self.take_structural();
        self.nesting += 1;
        let kind = match opener.kind {
            K::WardOpen => StageKind::Ward {
                predicate: self.parse_ward_predicate(&opener),
            },
            K::ForkOpen => StageKind::Fork {
                branches: self.parse_fork_branches(),
            },
            _ => StageKind::Orbit {
                body: self.parse_flow(),
            },
        };
        self.nesting -= 1;

        let close = self.expect_block_close(&opener);
        let mut stage = Stage {
            kind,
            span: opener.span.merge(close),
            modifiers: Vec::new(),
        };
        self.parse_modifiers(&mut stage);
        Some(stage)
    }

    /// A ward's predicate is one expression, not a flow — `?{ a -> b }` is a
    /// mistake with a clear meaning (the author wanted a chain) and a clear
    /// answer (put the chain after the ward).
    fn parse_ward_predicate(&mut self, opener: &CantToken) -> Leaf {
        let run_start = self.pos;
        let arrow = self.consume_leaf_run(&[K::Semi, K::BlockClose], false);
        let run = self.tokens[run_start..self.pos].to_vec();

        if let Some(arrow) = arrow {
            self.diagnostics.push(
                CantDiagnostic::error(
                    CANT_P012_WARD_IS_NOT_A_FLOW,
                    "a ward predicate is one expression, not a flow",
                )
                .with_primary(self.source_span(arrow), "`->` cannot appear inside `?{ }`")
                .with_secondary(self.source_span(opener.span), "this ward")
                .with_help("close the ward and continue the flow after it: `?{ p } -> stage`"),
            );
        }
        if run.is_empty() {
            let span = self.current().span.merge(opener.span);
            self.error(
                CANT_P002_EXPECTED_STAGE,
                "this ward has no predicate",
                span,
                "expected an expression such as `$ > 0`",
            );
            return Leaf {
                text: String::new(),
                span: opener.span,
                has_effect_marker: false,
                has_placeholder: false,
            };
        }
        self.report_glyph_only_operators_in_leaf(&run);
        self.leaf_from(&run)
    }

    fn parse_fork_branches(&mut self) -> Vec<Flow> {
        let mut branches = Vec::new();
        loop {
            let branch = self.parse_flow();
            if branch.stages.is_empty() {
                let span = self.current().span;
                self.error(
                    CANT_P011_EMPTY_FORK_BRANCH,
                    "this fork branch is empty",
                    span,
                    "every branch needs at least one stage",
                );
            }
            branches.push(branch);
            if self.at(K::Semi) {
                self.take_structural();
                continue;
            }
            break;
        }
        branches
    }

    fn expect_block_close(&mut self, opener: &CantToken) -> Span {
        if self.at(K::BlockClose) {
            let span = self.current().span;
            self.take_structural();
            return span;
        }
        if self.at(K::Semi) {
            let span = self.current().span;
            self.error(
                CANT_P006_UNEXPECTED_SEPARATOR,
                "unexpected `;`",
                span,
                "`;` separates fork branches; this is not a fork",
            );
            self.skip_to_matching_close();
            return span;
        }
        self.diagnostics.push(
            CantDiagnostic::error(
                CANT_P003_UNCLOSED_BLOCK,
                format!("unclosed `{}`", opener.text),
            )
            .with_primary(self.source_span(opener.span), "opened here, never closed")
            .with_secondary(self.source_span(self.current().span), "reached this")
            .with_help("close it with `}`, or `⟧` if the block was opened with a glyph"),
        );
        self.current().span
    }

    // ---- modifiers

    /// Is the cursor on `:name`, with no space between the two?
    ///
    /// The adjacency requirement is what keeps `:` usable as Rite's atom prefix:
    /// `:max` is a modifier, `= :error` is an atom in a comparison, and neither
    /// reading depends on what came before.
    fn at_modifier(&self) -> bool {
        if !self.at(K::Colon) {
            return false;
        }
        match self.tokens.get(self.pos + 1) {
            Some(next) => next.kind == K::Ident && next.span.start == self.current().span.end,
            None => false,
        }
    }

    fn parse_modifiers(&mut self, stage: &mut Stage) {
        while self.at_modifier() {
            let colon = self.current().clone();
            self.take_structural();
            let name_token = self.current().clone();
            self.advance();

            let value_start = self.pos;
            let _ = self.consume_leaf_run(&[K::Flow, K::Semi, K::BlockClose, K::Colon], true);
            let run = self.tokens[value_start..self.pos].to_vec();

            let name_span = colon.span.merge(name_token.span);
            if run.is_empty() {
                self.error(
                    CANT_P010_MODIFIER_NEEDS_VALUE,
                    format!("`:{}` needs a value", name_token.text),
                    name_span,
                    "expected a value after the modifier name",
                );
                continue;
            }
            let value = self.leaf_from(&run);
            stage.modifiers.push(Modifier {
                name: name_token.text.clone(),
                span: name_span.merge(value.span),
                name_span,
                value,
            });
        }
        // After a block, a `:` can only have been meant as a modifier — there
        // is no expression for it to be an atom prefix in.
        if self.at(K::Colon) {
            let span = self.current().span;
            self.error(
                CANT_P009_MODIFIER_NEEDS_NAME,
                "expected a modifier name after `:`",
                span,
                "write it as `:max 1024`, with no space after the colon",
            );
            self.advance();
        }
    }

    /// `-> :max 4` — a modifier written as though it were its own stage.
    fn reject_orphan_modifier(&mut self) -> Option<Stage> {
        let colon = self.current().clone();
        let name = self.tokens[self.pos + 1].clone();
        self.advance();
        self.advance();
        let value_start = self.pos;
        let _ = self.consume_leaf_run(&[K::Flow, K::Semi, K::BlockClose, K::Colon], true);
        let span = colon
            .span
            .merge(name.span)
            .merge(crate::lexer::span_of(&self.tokens[value_start..self.pos]));
        self.diagnostics.push(
            CantDiagnostic::error(
                CANT_P008_MODIFIER_WITHOUT_FORM,
                format!("`:{}` does not follow a ward, fork, or orbit", name.text),
            )
            .with_primary(
                self.source_span(span),
                "a modifier configures the form to its left",
            )
            .with_help(format!(
                "attach it directly, with no arrow: `~{{ … }} :{} …`",
                name.text
            )),
        );
        None
    }

    // ---- leaf runs

    /// Consume tokens until one of `stops` is reached at leaf-depth zero.
    ///
    /// Depth counts `(`, `[`, `{` and their glyph equivalents against `)`, `]`
    /// and `}`, so the `}` closing a Rite closure inside a leaf
    /// (`keep { |n| n > 0 }`) is not mistaken for the `}` closing the Cant block
    /// the leaf sits in. A structural opener counts as depth too: it is brace-like
    /// and is closed by the same `}`.
    ///
    /// Returns the span of the first depth-zero `->` seen when `stop_at_flow` is
    /// false — the caller wanted a single expression and got a flow.
    fn consume_leaf_run(&mut self, stops: &[K], stop_at_flow: bool) -> Option<Span> {
        let start = self.pos;
        let mut depth = 0usize;
        let mut flow_seen = None;
        while !self.at_eof() {
            let kind = self.current().kind;
            if depth == 0 {
                if stops.contains(&kind) {
                    break;
                }
                if kind == K::Flow {
                    if stop_at_flow {
                        break;
                    }
                    flow_seen.get_or_insert(self.current().span);
                }
                // A block opener can only *start* a stage, never continue one:
                // `f ?{ p }` is a missing arrow, not a leaf.
                if kind.opens_block() && self.pos > start {
                    break;
                }
            }
            if kind.opens_block() || kind.opens_depth() {
                depth += 1;
            } else if kind.closes_depth() {
                depth = depth.saturating_sub(1);
            }
            self.advance();
        }
        flow_seen
    }

    /// Skip forward to the `}` that closes the block currently being parsed, and
    /// consume it. Used only for recovery.
    fn skip_to_matching_close(&mut self) {
        let mut depth = 0usize;
        while !self.at_eof() {
            let kind = self.current().kind;
            if kind.opens_block() || kind.opens_depth() {
                depth += 1;
            } else if kind.closes_depth() {
                if depth == 0 {
                    self.advance();
                    return;
                }
                depth -= 1;
            }
            self.advance();
        }
    }

    fn leaf_from(&self, run: &[CantToken]) -> Leaf {
        let span = crate::lexer::span_of(run);
        Leaf {
            // Sliced from the source rather than rebuilt from token text, so a
            // comment or an unusual spacing inside a leaf survives into
            // generated Rite exactly as written.
            text: self.file.slice(span).trim().to_string(),
            span,
            has_effect_marker: run.iter().any(|t| t.kind == K::Bang),
            has_placeholder: run.iter().any(|t| t.kind == K::Dollar),
        }
    }

    /// `⋇` and `⌁` are glyph-only: they have no second meaning the way `*` and
    /// `[]` do, so seeing one inside a leaf is always a mistake worth naming.
    fn report_glyph_only_operators_in_leaf(&mut self, run: &[CantToken]) {
        for token in run {
            if token.spelling != Spelling::Glyph {
                continue;
            }
            let (name, ascii) = match token.kind {
                K::Star => ("scatter", "*"),
                K::Collect => ("collect", "[]"),
                _ => continue,
            };
            self.diagnostics.push(
                CantDiagnostic::error(
                    CANT_P007_GLYPH_ONLY_OPERATOR_IN_LEAF,
                    format!("`{}` is the {name} operator, and is not part of an expression", token.text),
                )
                .with_primary(
                    self.source_span(token.span),
                    format!("{name} has to be a stage of its own"),
                )
                .with_help(format!(
                    "write `-> {ascii} ->` to {name}, or use the ASCII spelling if you meant an operator"
                )),
            );
        }
    }

    fn span_from(&self, first_token: usize) -> Span {
        if first_token >= self.pos {
            return Span::DUMMY;
        }
        crate::lexer::span_of(&self.tokens[first_token..self.pos])
    }
}
