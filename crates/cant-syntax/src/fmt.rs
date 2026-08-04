//! Formatting and ASCII/glyph conversion for Cant.
//!
//! Two operations that look similar and are not:
//!
//! * [`convert`] respells structural operators and changes **nothing else** — not
//!   whitespace, not line breaks, not a single byte of leaf text. It is a
//!   splice of the source at the spans the parser recorded as operators.
//! * [`format`] reprints the program from its AST, choosing layout, and
//!   re-attaches the comments the AST does not carry.
//!
//! Both work from tokens and the parse, never from a regular expression. That is
//! not a stylistic preference: `"a -> b"` is a string, `// -> c` is a comment,
//! and `f([])` is a call. Only the parser knows which `->` is an arrow, which `}`
//! closes a Cant block rather than a Rite closure, and which `*` is scatter
//! rather than multiplication — so both operations consume
//! [`ParseResult::structural`] rather than re-deciding.
//!
//! # The comment guarantee
//!
//! [`format`] re-lexes its own output and refuses — returns `Err` — if the
//! multiset of comment texts changed. A formatter that silently eats a comment
//! is worse than one that fails, and "the layout code handles every case" is not
//! a claim worth trusting without a check. `rite-fmt` does the same thing for the
//! same reason.

use crate::ast::{CantProgramAst, Flow, Leaf, Modifier, Stage, StageKind};
use crate::lexer::lex;
use crate::parser::{parse, StructuralToken};
use crate::token::{CantToken, CantTokenKind as K, Spelling};
use crate::Dialect;
use rite_core::{FileId, SourceFile};

/// How wide a line may be before a construct breaks across lines.
pub const DEFAULT_MAX_WIDTH: usize = 88;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    pub dialect: Dialect,
    pub max_width: usize,
    /// Print the whole program on one line, however long.
    ///
    /// For `-e` output and for embedding a program in a shell command, where a
    /// line break is not free the way it is in a file.
    pub compact: bool,
    pub indent_width: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            dialect: Dialect::Ascii,
            max_width: DEFAULT_MAX_WIDTH,
            compact: false,
            indent_width: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FormatResult {
    pub text: String,
    pub dialect: Dialect,
}

/// What went wrong. Formatting a source with syntax errors is refused rather
/// than attempted: the AST is a recovery, and reprinting a guess as though it
/// were the program is how a formatter loses code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The source did not parse. Reported with the first diagnostic's code.
    Unparseable(String),
    /// The output did not survive the comment check. Always a bug here, never
    /// the caller's fault — reported rather than shipped.
    CommentsLost { before: usize, after: usize },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::Unparseable(code) => {
                write!(f, "cannot format a source with syntax errors ({code})")
            }
            FormatError::CommentsLost { before, after } => write!(
                f,
                "formatting would have changed the comments ({before} before, {after} after) \
                 — this is a bug in the formatter; the file was left alone"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

// ---------------------------------------------------------------- conversion

/// The spelling an operator takes in a dialect.
///
/// Sourced from the manifest rather than written out here, so a glyph change in
/// `grammar/cant/operators.toml` reaches the converter without a code change.
/// `None` means the token has no spelling in that dialect and is left as-is —
/// `$`, `!`, `@`, `;` and `:` are the same character either way.
fn spelling_for(kind: K, dialect: Dialect) -> Option<&'static str> {
    let name = kind.manifest_name()?;
    let op = crate::manifest::manifest().by_token(name)?;
    match dialect {
        Dialect::Ascii => Some(op.ascii.as_str()),
        Dialect::Glyph => op.glyph.as_deref(),
    }
}

/// Respell a source's structural operators, changing nothing else.
///
/// The output is byte-identical to the input everywhere outside the spans the
/// parser recorded as operators, so comments, strings, spacing and leaf text
/// come through exactly as written — including a comment that contains `->`, and
/// a string that contains `?{`.
///
/// A source that does not parse is converted on a best-effort basis using
/// whatever the parser did recognise, because an editor toggling dialects while
/// someone is mid-keystroke should not stop working.
pub fn convert(source: &str, dialect: Dialect) -> String {
    let file = SourceFile::new(FileId(0), "convert.cant", source);
    let parsed = parse(&file);
    splice(source, &parsed.structural, dialect)
}

fn splice(source: &str, structural: &[StructuralToken], dialect: Dialect) -> String {
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for token in structural {
        let start = token.span.start.as_usize().min(source.len());
        let end = token.span.end.as_usize().min(source.len());
        if start < cursor {
            // Overlapping records would mean the parser consumed one byte as two
            // operators. It cannot, but splicing on a bad assumption would
            // corrupt the file rather than fail, so skip instead.
            continue;
        }
        out.push_str(&source[cursor..start]);
        match spelling_for(token.kind, dialect) {
            Some(text) => out.push_str(text),
            None => out.push_str(&source[start..end]),
        }
        cursor = end;
    }
    out.push_str(&source[cursor.min(source.len())..]);
    out
}

// ---------------------------------------------------------------- formatting

/// Format a Cant source.
pub fn format(source: &str, options: FormatOptions) -> Result<FormatResult, FormatError> {
    let file = SourceFile::new(FileId(0), "fmt.cant", source);
    let parsed = parse(&file);
    if parsed.diagnostics.has_errors() {
        let code = parsed
            .diagnostics
            .errors()
            .next()
            .map(|d| d.code.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(FormatError::Unparseable(code));
    }
    let Some(program) = parsed.program else {
        return Err(FormatError::Unparseable("CANT-P001".into()));
    };

    let mut printer = Printer {
        source,
        options,
        comments: comment_tokens(&parsed.tokens),
        emitted: 0,
        out: String::new(),
    };
    printer.program(&program);
    let text = printer.finish();

    let before = comment_texts(source);
    let after = comment_texts(&text);
    if before != after {
        return Err(FormatError::CommentsLost {
            before: before.len(),
            after: after.len(),
        });
    }

    Ok(FormatResult {
        text,
        dialect: options.dialect,
    })
}

/// Every comment in a source, in order, normalized for trailing whitespace.
///
/// Used by the backstop in [`format`]. Comparing texts rather than counts means
/// a formatter that swapped two comments, or truncated one, fails too.
pub fn comment_texts(source: &str) -> Vec<String> {
    let file = SourceFile::new(FileId(0), "comments.cant", source);
    let (tokens, _) = lex(&file);
    tokens
        .iter()
        .filter(|t| t.kind == K::Comment)
        .map(|t| t.text.trim_end().to_string())
        .collect()
}

fn comment_tokens(tokens: &[CantToken]) -> Vec<CantToken> {
    tokens
        .iter()
        .filter(|t| t.kind == K::Comment || t.kind == K::Shebang)
        .cloned()
        .collect()
}

struct Printer<'a> {
    source: &'a str,
    options: FormatOptions,
    /// Comments and the shebang, in source order.
    comments: Vec<CantToken>,
    /// How many of them have been written.
    emitted: usize,
    out: String,
}

impl Printer<'_> {
    fn finish(mut self) -> String {
        // Anything after the last stage — a trailing comment block — still has to
        // be written, at column zero.
        while self.emitted < self.comments.len() {
            let text = self.comments[self.emitted].text.trim_end().to_string();
            self.emitted += 1;
            if !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            self.out.push_str(&text);
        }
        // No trailing newline: this is *the formatted program*, and whether it
        // ends a file (which should end with one) or a `-e` argument (which
        // should not) is the caller's question, not the formatter's. Deciding it
        // here made `--check` report every `-e` expression as unformatted.
        self.out.trim_end().to_string()
    }

    fn program(&mut self, program: &CantProgramAst) {
        // The shebang, if there is one, is trivia at byte 0 and goes first.
        self.flush_comments_before(program.span.start.as_usize(), 0);
        // `use` lines, one per line, before the flow — the only place the
        // grammar allows them. They have no glyph spelling, so both dialects
        // print them identically.
        for import in &program.uses {
            self.out.push_str("use ");
            self.out.push_str(&import.name);
            self.out.push('\n');
        }
        let rendered = self.flow(&program.flow, 0, self.options.indent_width);
        self.out.push_str(&rendered);
    }

    /// Write every comment that starts before `offset`, each on its own line.
    fn flush_comments_before(&mut self, offset: usize, indent: usize) {
        while self.emitted < self.comments.len() {
            let comment = &self.comments[self.emitted];
            if comment.span.start.as_usize() >= offset {
                break;
            }
            let text = comment.text.trim_end().to_string();
            self.emitted += 1;
            if !self.out.is_empty() && !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            self.out.push_str(&" ".repeat(indent));
            self.out.push_str(&text);
            self.out.push('\n');
        }
    }

    // ---- layout

    /// Render a flow, choosing one line or many.
    ///
    /// Returns a string rather than writing directly so that the one-line form
    /// can be measured before it is committed to — which is the whole of the
    /// layout decision, and is why nesting works without a second pass.
    ///
    /// `first_col` is where the first stage lands; `arrow_col` is where each
    /// `->` lands. They differ only at the top level, where a flow's
    /// continuation lines are indented under their first stage to show they
    /// belong to it. Inside a block the body is already visually nested by the
    /// braces, so a second indent would just push it right for no reason:
    ///
    /// ```text
    /// roots              -> ~{
    ///   -> *                  !@fs.read
    ///   -> f                  -> imports
    ///                       }
    /// ```
    fn flow(&mut self, flow: &Flow, first_col: usize, arrow_col: usize) -> String {
        let inline = self.flow_inline(flow);
        if self.options.compact || first_col + inline.chars().count() <= self.options.max_width {
            return inline;
        }

        let arrow_width = self.arrow().chars().count();
        let mut out = String::new();
        for (i, stage) in flow.stages.iter().enumerate() {
            // Comments are re-attached where they were written, so a note above
            // a stage stays above that stage instead of migrating to the end of
            // the program.
            let leading = self.take_comments_before(stage.span.start.as_usize());
            if i == 0 {
                for comment in leading {
                    out.push_str(&comment);
                    out.push('\n');
                    out.push_str(&" ".repeat(first_col));
                }
                out.push_str(&self.stage_at(stage, first_col));
                continue;
            }
            for comment in leading {
                out.push('\n');
                out.push_str(&" ".repeat(arrow_col));
                out.push_str(&comment);
            }
            out.push('\n');
            out.push_str(&" ".repeat(arrow_col));
            out.push_str(self.arrow());
            out.push(' ');
            // The stage body starts after `-> `, and a block it opens hangs from
            // there — which is what makes a modifier line up under its block
            // rather than under the arrow.
            out.push_str(&self.stage_at(stage, arrow_col + arrow_width + 1));
        }
        out
    }

    /// Take the comments that start before `offset`, as trimmed lines.
    fn take_comments_before(&mut self, offset: usize) -> Vec<String> {
        let mut out = Vec::new();
        while self.emitted < self.comments.len() {
            let comment = &self.comments[self.emitted];
            if comment.span.start.as_usize() >= offset {
                break;
            }
            out.push(comment.text.trim_end().to_string());
            self.emitted += 1;
        }
        out
    }

    fn flow_inline(&self, flow: &Flow) -> String {
        flow.stages
            .iter()
            .map(|s| self.stage_inline(s))
            .collect::<Vec<_>>()
            .join(&format!(" {} ", self.arrow()))
    }

    fn stage_inline(&self, stage: &Stage) -> String {
        let body = match &stage.kind {
            StageKind::Leaf(leaf) => self.leaf(leaf),
            StageKind::Scatter => self.op(K::Star).to_string(),
            StageKind::Collect => self.op(K::Collect).to_string(),
            StageKind::Ward { predicate } => format!(
                "{} {} {}",
                self.op(K::WardOpen),
                self.leaf(predicate),
                self.op(K::BlockClose)
            ),
            StageKind::Fork { branches } => format!(
                "{} {} {}",
                self.op(K::ForkOpen),
                branches
                    .iter()
                    .map(|b| self.flow_inline(b))
                    .collect::<Vec<_>>()
                    .join(&format!(" {} ", self.op(K::Semi))),
                self.op(K::BlockClose)
            ),
            StageKind::Orbit { body } => format!(
                "{} {} {}",
                self.op(K::OrbitOpen),
                self.flow_inline(body),
                self.op(K::BlockClose)
            ),
        };
        let mods = stage
            .modifiers
            .iter()
            .map(|m| format!(" {}", self.modifier(m)))
            .collect::<String>();
        format!("{body}{mods}")
    }

    /// Render one stage, breaking it if the inline form does not fit.
    ///
    /// `column` is where the stage's first character lands; `indent` is where a
    /// continuation line starts. They differ for a stage introduced by `-> `.
    fn stage_at(&mut self, stage: &Stage, column: usize) -> String {
        let inline = self.stage_inline(stage);
        if self.options.compact || column + inline.len() <= self.options.max_width {
            return inline;
        }
        let body_indent = column + self.options.indent_width;
        let pad = " ".repeat(column);
        let body_pad = " ".repeat(body_indent);

        let broken = match &stage.kind {
            // A leaf is Rite expression text and is never re-wrapped: Cant does
            // not parse it, so any break would be a guess about a grammar this
            // formatter does not implement.
            StageKind::Leaf(_) | StageKind::Scatter | StageKind::Collect => return inline,
            StageKind::Ward { predicate } => format!(
                "{}\n{}{}\n{}{}",
                self.op(K::WardOpen),
                body_pad,
                self.leaf(predicate),
                pad,
                self.op(K::BlockClose)
            ),
            StageKind::Fork { branches } => {
                let rendered: Vec<String> = branches
                    .iter()
                    .map(|b| self.flow(b, body_indent, body_indent))
                    .collect();
                let joined = rendered
                    .iter()
                    .map(|b| format!("{body_pad}{b}"))
                    .collect::<Vec<_>>()
                    .join(&format!(" {}\n", self.op(K::Semi)));
                format!(
                    "{}\n{}\n{}{}",
                    self.op(K::ForkOpen),
                    joined,
                    pad,
                    self.op(K::BlockClose)
                )
            }
            StageKind::Orbit { body } => format!(
                "{}\n{}{}\n{}{}",
                self.op(K::OrbitOpen),
                body_pad,
                self.flow(body, body_indent, body_indent),
                pad,
                self.op(K::BlockClose)
            ),
        };

        // Modifiers go under the closing brace, one per line: a `:max 4096`
        // trailing a multi-line block reads as part of the block's last line
        // otherwise.
        let mods = stage
            .modifiers
            .iter()
            .map(|m| format!("\n{pad}{}", self.modifier(m)))
            .collect::<String>();
        format!("{broken}{mods}")
    }

    fn modifier(&self, m: &Modifier) -> String {
        format!("{}{} {}", self.op(K::Colon), m.name, m.value.text)
    }

    fn leaf(&self, leaf: &Leaf) -> String {
        // Leaf text is Rite's, and is reproduced exactly. Collapsing its internal
        // whitespace would mean formatting a language this crate does not parse.
        let _ = self.source;
        leaf.text.clone()
    }

    fn arrow(&self) -> &'static str {
        self.op(K::Flow)
    }

    /// An operator's spelling in the target dialect, falling back to ASCII for
    /// the ones with no glyph form.
    fn op(&self, kind: K) -> &'static str {
        spelling_for(kind, self.options.dialect)
            .or_else(|| spelling_for(kind, Dialect::Ascii))
            .unwrap_or("")
    }
}

/// Which spelling a source is written in, by counting structural tokens only.
///
/// A Rite glyph inside a leaf — `⟨a: 1⟩`, `←` — does not make a program "glyph
/// Cant", because it is not a Cant operator. Only the tokens the parser read as
/// operators are counted.
pub fn detect(source: &str) -> Dialect {
    let file = SourceFile::new(FileId(0), "detect.cant", source);
    let parsed = parse(&file);
    let glyphs = parsed
        .structural
        .iter()
        .filter(|t| t.spelling == Spelling::Glyph)
        .count();
    if glyphs > 0 {
        Dialect::Glyph
    } else {
        Dialect::Ascii
    }
}

/// Byte offsets in the converted output for each input offset of interest.
///
/// Conversion only ever changes the length of the operator spans, so a position
/// map is exact rather than approximate: everything between two operators shifts
/// by a running delta. `rite-fmt`'s `LineSourceMap` interpolates within a line
/// because it reprints; this does not have to.
pub fn convert_offset_map(source: &str, dialect: Dialect) -> Vec<(u32, u32)> {
    let file = SourceFile::new(FileId(0), "map.cant", source);
    let parsed = parse(&file);
    let mut pairs = Vec::with_capacity(parsed.structural.len() + 1);
    let mut delta: i64 = 0;
    pairs.push((0u32, 0u32));
    for token in &parsed.structural {
        let start = token.span.start.as_usize();
        let old_len = token.span.len();
        let new_len = spelling_for(token.kind, dialect)
            .map(str::len)
            .unwrap_or(old_len);
        pairs.push((start as u32, (start as i64 + delta).max(0) as u32));
        delta += new_len as i64 - old_len as i64;
        let after = token.span.end.as_usize();
        pairs.push((after as u32, (after as i64 + delta).max(0) as u32));
    }
    pairs.push((
        source.len() as u32,
        (source.len() as i64 + delta).max(0) as u32,
    ));
    pairs
}

/// Map one input byte offset through a conversion.
pub fn map_offset(map: &[(u32, u32)], offset: u32) -> u32 {
    match map.binary_search_by_key(&offset, |(from, _)| *from) {
        Ok(i) => map[i].1,
        Err(0) => 0,
        Err(i) => {
            let (from, to) = map[i - 1];
            to + (offset - from)
        }
    }
}
