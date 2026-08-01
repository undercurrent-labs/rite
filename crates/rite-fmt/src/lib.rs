//! Idempotent Rite formatter and dialect converter (V1).
//!
//! # Comment preservation
//!
//! The formatter prints from the AST, but comments are not part of the AST. To
//! avoid destroying them it re-uses the trivia tokens the lexer already emits
//! (`Comment`, `DocComment`, `ModuleDocComment`, `Shebang`) and interleaves them
//! with the printed nodes by byte span:
//!
//! * before printing a node, every comment that starts before it is flushed on
//!   its own line at the node's indentation (this covers `//!`, `///` and
//!   own-line `//` / `/* … */` comments, plus the shebang, which is trivia at
//!   byte 0 and therefore always emitted first);
//! * after printing a node, a comment that starts on the same source line as the
//!   node's end is re-attached as a trailing comment;
//! * anything left over inside a construct is flushed at the next statement
//!   boundary, so it may move to its own line but is never dropped.
//!
//! Blank-line grouping is preserved (capped at one blank line) by comparing
//! source line numbers, which also makes the output idempotent.
//!
//! As a backstop, [`format_with_dialect`] re-lexes its own output and refuses
//! (returns `Err`) if the set of comments changed, so `rite fmt` can never
//! silently delete source.

use rite_core::{SourceMap, Span};
use rite_syntax::{
    lex, parse, BinOp, Block, Expr, Item, LitKind, Pattern, Program, Stmt, Token, TokenKind,
    UnaryOp,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    #[default]
    Glyph,
    Ascii,
    Mixed,
    Preserve,
}

#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    pub dialect: Dialect,
    pub indent_width: usize,
    pub max_width: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            dialect: Dialect::Glyph,
            indent_width: 2,
            max_width: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FormatResult {
    pub text: String,
    pub dialect: Dialect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_map: Option<LineSourceMap>,
}

/// Approximate source map for cursor preservation across format/convert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineSourceMap {
    /// For each byte in the output (sampled per line start + identity within line),
    /// maps to an input byte. Full length == out.len() when built densely.
    pub out_to_in: Vec<u32>,
}

/// Build a monotonic byte map by aligning lines (good enough for dialect toggles).
pub fn build_line_source_map(input: &str, output: &str) -> LineSourceMap {
    let in_lines: Vec<&str> = input.split_inclusive('\n').collect();
    let out_lines: Vec<&str> = output.split_inclusive('\n').collect();
    let mut out_to_in = Vec::with_capacity(output.len());
    let mut in_off = 0u32;
    let mut out_off = 0usize;
    let n = out_lines.len().max(in_lines.len());
    for i in 0..n {
        let ol = out_lines.get(i).copied().unwrap_or("");
        let il = in_lines.get(i).copied().unwrap_or("");
        let in_start = in_off;
        for j in 0..ol.len() {
            // Map proportionally within the line
            let ratio = if ol.is_empty() {
                0.0
            } else {
                j as f64 / ol.len() as f64
            };
            let mapped = in_start + ((il.len() as f64) * ratio) as u32;
            out_to_in.push(mapped.min(input.len() as u32));
            out_off += 1;
        }
        in_off = in_off.saturating_add(il.len() as u32);
    }
    // Pad if needed
    while out_to_in.len() < output.len() {
        out_to_in.push(input.len().saturating_sub(1) as u32);
    }
    if out_to_in.len() > output.len() {
        out_to_in.truncate(output.len());
    }
    let _ = out_off;
    LineSourceMap { out_to_in }
}

/// Map (line, utf16_col) 0-based through a line source map.
pub fn map_cursor(
    map: &LineSourceMap,
    old_source: &str,
    new_source: &str,
    line: u32,
    character: u32,
) -> (u32, u32) {
    let old_byte = pos_to_byte(old_source, line, character);
    if map.out_to_in.is_empty() {
        return (line, character);
    }
    let mut best = 0usize;
    let mut best_dist = u32::MAX;
    for (i, &m) in map.out_to_in.iter().enumerate() {
        let d = m.abs_diff(old_byte as u32);
        if d < best_dist {
            best_dist = d;
            best = i;
        }
    }
    byte_to_pos(new_source, best)
}

fn pos_to_byte(text: &str, line: u32, character: u32) -> usize {
    let mut cur_line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if cur_line == line && col >= character {
            return i;
        }
        if ch == '\n' {
            cur_line += 1;
            col = 0;
            if cur_line > line {
                return i;
            }
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    text.len()
}

fn byte_to_pos(text: &str, byte: usize) -> (u32, u32) {
    let byte = byte.min(text.len());
    let mut line = 0u32;
    let mut col = 0u32;
    let mut i = 0usize;
    for ch in text.chars() {
        let bl = ch.len_utf8();
        if i + bl > byte {
            break;
        }
        i += bl;
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    (line, col)
}

pub fn format_source(source: &str, ascii: bool) -> Result<String, String> {
    let dialect = if ascii {
        Dialect::Ascii
    } else {
        Dialect::Glyph
    };
    format_with_dialect(source, dialect).map(|r| r.text)
}

pub fn format_with_dialect(source: &str, dialect: Dialect) -> Result<FormatResult, String> {
    // `Preserve` keeps whatever dialect the file is already written in.
    let resolved = match dialect {
        Dialect::Preserve => detect_dialect(source),
        other => other,
    };
    let mut sources = SourceMap::new();
    let id = sources.add_file("fmt.rite", source);
    let file = sources
        .get(id)
        .ok_or_else(|| "no source file".to_string())?
        .clone();
    let (tokens, mut diags) = lex(&file);
    let (program, parse_diags) = parse(id, &tokens);
    diags.extend(parse_diags.into_vec());
    if diags.has_errors() {
        // Never rewrite a file we could not fully understand.
        if dialect == Dialect::Preserve {
            let text = source.to_string();
            let source_map = Some(build_line_source_map(source, &text));
            return Ok(FormatResult {
                text,
                dialect,
                source_map,
            });
        }
        return Err(format!("parse errors: {}", diags.len()));
    }
    let program = program.ok_or_else(|| "no program".to_string())?;
    let text = format_program_with_trivia(
        &program,
        source,
        &tokens,
        &FormatOptions {
            dialect: resolved,
            ..Default::default()
        },
    );
    // Fail-safe: refuse rather than destroy. If the round-trip changed the
    // comments in any way, the caller gets an error and the file is untouched.
    check_comments_preserved(source, &text)?;
    check_reparses(&text)?;
    check_no_new_diagnostics(source, &text)?;
    let source_map = Some(build_line_source_map(source, &text));
    Ok(FormatResult {
        text,
        dialect: resolved,
        source_map,
    })
}

/// Guess the dialect a source file is written in, so formatting can keep it.
///
/// Returns [`Dialect::Glyph`] or [`Dialect::Ascii`] — whichever spelling is used
/// for more dialect-sensitive tokens (ties, and files with no such tokens, go to
/// the project default `Glyph`).
pub fn detect_dialect(source: &str) -> Dialect {
    let mut sources = SourceMap::new();
    let id = sources.add_file("detect.rite", source);
    let Some(file) = sources.get(id) else {
        return Dialect::Glyph;
    };
    let (tokens, _) = lex(file);
    let mut glyph = 0usize;
    let mut ascii = 0usize;
    for t in &tokens {
        if let Some(g) = glyph_spelling(t.kind) {
            if t.text == g {
                glyph += 1;
            } else {
                ascii += 1;
            }
        }
    }
    if ascii > glyph {
        Dialect::Ascii
    } else {
        Dialect::Glyph
    }
}

/// The glyphic spelling of a dialect-sensitive token, if it has one.
fn glyph_spelling(kind: TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::Def => "◆",
        TokenKind::Bind => "←",
        TokenKind::BindMut => "↢",
        TokenKind::Arrow => "→",
        TokenKind::Return => "^",
        TokenKind::If => "?",
        TokenKind::Match => "~",
        TokenKind::Effect => "!",
        TokenKind::Host => "@",
        TokenKind::BlockOpen => "⟦",
        TokenKind::BlockClose => "⟧",
        TokenKind::RecordOpen => "⟨",
        TokenKind::RecordClose => "⟩",
        TokenKind::In => "∈",
        TokenKind::NotIn => "∉",
        TokenKind::Use => "⊏",
        TokenKind::Xor => "⊻",
        TokenKind::Compose => "∘",
        TokenKind::RangeIncl => "‥",
        TokenKind::AtomPrefix => "#",
        _ => return None,
    })
}

/// Every comment (and the shebang) in `source`, in order, normalized the same
/// way the formatter emits them. Used by the fail-safe check and by tests.
pub fn comment_texts(source: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let id = sources.add_file("comments.rite", source);
    let Some(file) = sources.get(id) else {
        return Vec::new();
    };
    let (tokens, _) = lex(file);
    tokens
        .iter()
        .filter(|t| is_comment_kind(t.kind))
        .map(|t| normalize_comment(&t.text))
        .collect()
}

fn is_comment_kind(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Comment
            | TokenKind::DocComment
            | TokenKind::ModuleDocComment
            | TokenKind::Shebang
    )
}

/// Trailing whitespace is the only thing we are allowed to change in a comment.
fn normalize_comment(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    out
}

/// Fail-safe: compare the comments of input and output byte-for-byte.
fn check_comments_preserved(input: &str, output: &str) -> Result<(), String> {
    let before = comment_texts(input);
    let after = comment_texts(output);
    if before == after {
        return Ok(());
    }
    let missing: Vec<&str> = before
        .iter()
        .filter(|c| !after.contains(c))
        .map(|c| c.as_str())
        .collect();
    let detail = if missing.is_empty() {
        format!(
            "comment order or text changed ({} before, {} after)",
            before.len(),
            after.len()
        )
    } else {
        format!(
            "{} comment(s) would be lost, first: {}",
            missing.len(),
            missing[0]
        )
    };
    Err(format!(
        "refusing to format: {detail}. This is a rite-fmt bug; the file was left unchanged."
    ))
}

/// Fail-safe: formatting must not introduce diagnostics the input did not have.
///
/// Re-parsing is not enough. The printer works from the desugared AST, so it can
/// emit a program that still parses but no longer *resolves* — dropping a route's
/// `|req|` parameter list produced `undefined name `req`` at request time. Compare
/// the semantic diagnostics of input and output span-free (severity + code + title,
/// since spans legitimately move) and refuse if the output gained any. Diagnostics
/// the input already had cancel out, so this never blocks formatting a file that
/// was already broken.
fn check_no_new_diagnostics(input: &str, output: &str) -> Result<(), String> {
    let before = semantic_diagnostics(input);
    let after = semantic_diagnostics(output);
    if before == after {
        return Ok(());
    }
    let mut unmatched = before;
    let mut gained = Vec::new();
    for d in after {
        match unmatched.iter().position(|b| *b == d) {
            Some(i) => {
                unmatched.remove(i);
            }
            None => gained.push(d),
        }
    }
    if gained.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to format: the formatted output introduces {} new diagnostic(s), first: {}. \
         This is a rite-fmt bug; the file was left unchanged.",
        gained.len(),
        gained[0]
    ))
}

/// Span-free fingerprints of the semantic diagnostics for `source`, sorted.
fn semantic_diagnostics(source: &str) -> Vec<String> {
    let mut sources = SourceMap::new();
    let id = sources.add_file("fmt-sem.rite", source);
    let Some(file) = sources.get(id).cloned() else {
        return Vec::new();
    };
    let (_, diags) = rite_sem::compile_to_ir(&file);
    let mut out: Vec<String> = diags
        .into_vec()
        .iter()
        .map(|d| format!("{:?} {:?}: {}", d.severity, d.code, d.title))
        .collect();
    out.sort();
    out
}

/// Fail-safe: a file that parsed before formatting must still parse after.
fn check_reparses(output: &str) -> Result<(), String> {
    let mut sources = SourceMap::new();
    let id = sources.add_file("fmt-check.rite", output);
    let Some(file) = sources.get(id) else {
        return Ok(());
    };
    let (tokens, mut diags) = lex(file);
    let (_, parse_diags) = parse(id, &tokens);
    diags.extend(parse_diags.into_vec());
    if diags.has_errors() {
        return Err(format!(
            "refusing to format: the formatted output no longer parses ({} diagnostic(s)). \
             This is a rite-fmt bug; the file was left unchanged.",
            diags.len()
        ));
    }
    Ok(())
}

/// A comment or shebang recovered from the token stream.
#[derive(Debug, Clone)]
struct Trivia {
    /// Normalized comment text, verbatim apart from trailing whitespace.
    text: String,
    start: usize,
    start_line: usize,
    end_line: usize,
    /// True when only whitespace precedes the comment on its line.
    own_line: bool,
    /// True for `//`-style comments, which swallow the rest of their line.
    line_comment: bool,
}

fn line_starts_of(source: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn line_at(line_starts: &[usize], byte: usize) -> usize {
    line_starts
        .partition_point(|&s| s <= byte)
        .saturating_sub(1)
}

fn collect_trivia(tokens: &[Token], source: &str, line_starts: &[usize]) -> Vec<Trivia> {
    tokens
        .iter()
        .filter(|t| is_comment_kind(t.kind))
        .map(|t| {
            let start = t.span.start.as_usize();
            let end = t.span.end.as_usize().min(source.len());
            let start_line = line_at(line_starts, start);
            let bol = line_starts[start_line];
            let own_line = source[bol..start.min(source.len())]
                .chars()
                .all(char::is_whitespace);
            Trivia {
                text: normalize_comment(&t.text),
                start,
                start_line,
                end_line: line_at(line_starts, end.saturating_sub(1).max(start)),
                own_line,
                line_comment: t.text.starts_with("//") || t.text.starts_with("#!"),
            }
        })
        .collect()
}

/// Convert aliases between dialects without regex rewrites of strings/comments.
pub fn convert_source(source: &str, to: Dialect) -> Result<FormatResult, String> {
    match to {
        Dialect::Preserve => {
            let text = source.to_string();
            let source_map = Some(build_line_source_map(source, &text));
            Ok(FormatResult {
                text,
                dialect: Dialect::Preserve,
                source_map,
            })
        }
        Dialect::Glyph | Dialect::Ascii | Dialect::Mixed => {
            // Mixed prefers glyph for core constructs
            let d = if matches!(to, Dialect::Mixed) {
                Dialect::Glyph
            } else {
                to
            };
            format_with_dialect(source, d)
        }
    }
}

/// Format an AST without its source text.
///
/// Comments live in the token stream, not the AST, so this **cannot** preserve
/// them. Prefer [`format_with_dialect`] or [`format_program_with_source`] for
/// anything that writes a user's file back to disk.
pub fn format_program(program: &Program, opts: &FormatOptions) -> String {
    let mut f = Formatter::new(opts, &[], &[0]);
    f.program(program);
    f.out
}

/// Format an AST, re-attaching the comments found in `source`.
pub fn format_program_with_source(program: &Program, source: &str, opts: &FormatOptions) -> String {
    let mut sources = SourceMap::new();
    let id = sources.add_file("fmt.rite", source);
    match sources.get(id) {
        Some(file) => {
            let (tokens, _) = lex(file);
            format_program_with_trivia(program, source, &tokens, opts)
        }
        None => format_program(program, opts),
    }
}

fn format_program_with_trivia(
    program: &Program,
    source: &str,
    tokens: &[Token],
    opts: &FormatOptions,
) -> String {
    let line_starts = line_starts_of(source);
    let trivia = collect_trivia(tokens, source, &line_starts);
    let mut f = Formatter::new(opts, &trivia, &line_starts);
    f.program(program);
    f.out
}

struct Formatter<'a> {
    opts: FormatOptions,
    out: String,
    indent: usize,
    trivia: &'a [Trivia],
    /// Index of the next comment that has not been emitted yet.
    next_trivia: usize,
    line_starts: &'a [usize],
    /// Source line of the last construct emitted at a statement boundary.
    last_line: Option<usize>,
}

impl<'a> Formatter<'a> {
    fn new(opts: &FormatOptions, trivia: &'a [Trivia], line_starts: &'a [usize]) -> Self {
        Self {
            opts: *opts,
            out: String::new(),
            indent: 0,
            trivia,
            next_trivia: 0,
            line_starts,
            last_line: None,
        }
    }

    fn program(&mut self, program: &Program) {
        for item in &program.items {
            self.boundary_item(item);
        }
        // Comments after the last item (or a file that is only comments).
        self.flush_leading(usize::MAX);
    }

    /// Emit one item at a statement boundary: leading comments, the item itself,
    /// then any trailing comment on the same line.
    fn boundary_item(&mut self, item: &Item) {
        let span = item_span(item);
        let start = span.start.as_usize();
        let end = span.end.as_usize();
        self.flush_leading(start);
        self.newline();
        self.gap_before(self.line_at(start));
        self.pad();
        self.item(item);
        self.last_line = Some(self.line_at(end.saturating_sub(1).max(start)));
        self.flush_trailing(end);
        self.out.push('\n');
    }

    fn line_at(&self, byte: usize) -> usize {
        line_at(self.line_starts, byte)
    }

    /// Did the author write this construct across more than one line?
    ///
    /// Collection literals are reprinted the way they were laid out: a record or list
    /// broken over several lines stays broken, a one-liner stays a one-liner. Collapsing
    /// everything produced single lines hundreds of characters long out of deliberately
    /// tabulated data. Idempotent by construction — the reprinted form spans the same
    /// number of lines, so a second pass makes the same decision.
    fn spans_lines(&self, span: rite_core::Span) -> bool {
        let start = span.start.as_usize();
        let end = span.end.as_usize().saturating_sub(1).max(start);
        self.line_at(start) != self.line_at(end)
    }

    fn at_line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn newline(&mut self) {
        if !self.at_line_start() {
            self.out.push('\n');
        }
    }

    /// Ensure exactly one blank line before the next content.
    fn ensure_blank(&mut self) {
        if self.out.is_empty() {
            return;
        }
        self.newline();
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// Preserve (at most one) blank line where the source had one.
    fn gap_before(&mut self, start_line: usize) {
        if let Some(prev) = self.last_line {
            if start_line > prev + 1 {
                self.ensure_blank();
            }
        }
    }

    /// Emit every pending comment that starts before `before_byte`, each on its
    /// own line at the current indentation.
    fn flush_leading(&mut self, before_byte: usize) {
        while self
            .trivia
            .get(self.next_trivia)
            .is_some_and(|t| t.start < before_byte)
        {
            let t = self.trivia[self.next_trivia].clone();
            self.next_trivia += 1;
            self.newline();
            self.gap_before(t.start_line);
            self.pad();
            self.push_comment(&t.text);
            self.out.push('\n');
            self.last_line = Some(t.end_line);
        }
    }

    /// Re-attach comments that sat on the same source line as the construct that
    /// just ended at `end_byte`.
    fn flush_trailing(&mut self, end_byte: usize) {
        let mut line = self.line_at(end_byte.saturating_sub(1));
        loop {
            let Some(t) = self.trivia.get(self.next_trivia) else {
                return;
            };
            if t.own_line || t.start < end_byte || t.start_line != line {
                return;
            }
            let t = t.clone();
            self.next_trivia += 1;
            self.out.push(' ');
            self.push_comment(&t.text);
            line = t.end_line;
            self.last_line = Some(line);
            if t.line_comment {
                // Nothing may follow a `//` comment on its line.
                return;
            }
        }
    }

    /// Write a comment: first line at the current indent, interior lines of a
    /// block comment verbatim so re-formatting is a fixed point.
    fn push_comment(&mut self, text: &str) {
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.out.push('\n');
            }
            self.out.push_str(line);
        }
    }

    fn pad(&mut self) {
        for _ in 0..self.indent * self.opts.indent_width {
            self.out.push(' ');
        }
    }

    fn ascii_mode(&self) -> bool {
        matches!(self.opts.dialect, Dialect::Ascii)
    }

    fn sigil(&self, glyph: &str, ascii: &str) -> String {
        if self.ascii_mode() {
            ascii.to_string()
        } else {
            glyph.to_string()
        }
    }

    fn item(&mut self, item: &Item) {
        match item {
            Item::Function(func) => {
                if func.is_pub {
                    self.out.push_str("pub ");
                }
                self.out.push_str(&self.sigil("◆", "def"));
                // The effect marker is part of the declaration's meaning, not
                // decoration: dropping it here would silently turn a checked
                // effectful function into one callers may call unmarked.
                if func.is_effectful {
                    self.out.push_str(&self.sigil("!", "!"));
                }
                self.out.push(' ');
                self.out.push_str(&func.name.name);
                self.out.push('(');
                for (i, p) in func.params.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.out.push_str(&p.name.name);
                    if let Some(ty) = &p.ty {
                        self.out.push_str(": ");
                        self.type_expr(ty);
                    }
                }
                self.out.push(')');
                if let Some(ret) = &func.return_type {
                    self.out.push(' ');
                    self.out.push_str(&self.sigil("→", "->"));
                    self.out.push(' ');
                    self.type_expr(ret);
                }
                self.out.push(' ');
                self.block(&func.body);
            }
            Item::Import(imp) => {
                if imp.is_pub {
                    self.out.push_str("pub ");
                }
                self.out.push_str(&self.sigil("⊏ ", "use "));
                // Relative imports (`./lib/helpers`) keep their slashes; only
                // module paths are dot-separated.
                let segments = &imp.path.segments;
                let relative = segments
                    .first()
                    .is_some_and(|s| s.name == "." || s.name == "..");
                for (i, s) in segments.iter().enumerate() {
                    if i > 0 {
                        self.out.push(if relative { '/' } else { '.' });
                    }
                    self.out.push_str(&s.name);
                }
                if let Some(a) = &imp.alias {
                    self.out.push_str(" as ");
                    self.out.push_str(&a.name);
                }
            }
            Item::Test(t) => {
                self.out.push_str(&self.sigil("◆", "def"));
                self.out.push_str(" test ");
                self.out.push('"');
                self.out.push_str(&t.name);
                self.out.push('"');
                self.out.push(' ');
                self.block(&t.body);
            }
            Item::Event(e) => {
                self.out.push_str(&self.sigil("◆", "def"));
                self.out.push(' ');
                self.out.push_str(match e.kind {
                    rite_syntax::EventKind::Item => "item",
                    rite_syntax::EventKind::Room => "room",
                    rite_syntax::EventKind::World => "world",
                });
                self.out.push(' ');
                self.out.push_str(&self.sigil("#", ":"));
                self.out.push_str(&e.atom.parts.join("."));
                self.out.push(' ');
                self.block(&e.body);
            }
            Item::Data(d) => {
                self.out.push_str(&self.sigil("◆", "def"));
                self.out.push(' ');
                self.out.push_str(&d.name.name);
                self.out.push(' ');
                self.out.push_str(&self.sigil("⟨", "<<"));
                for (i, e) in d.fields.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.record_key(&e.key);
                    self.out.push_str(": ");
                    self.expr(&e.value);
                }
                self.out.push_str(&self.sigil("⟩", ">>"));
            }
            Item::Statement(s) => self.stmt(s),
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Binding(b) => {
                self.pattern(&b.pattern);
                self.out.push(' ');
                self.out.push_str(&if b.mutable {
                    self.sigil("↢", "<~")
                } else {
                    self.sigil("←", "<-")
                });
                self.out.push(' ');
                self.expr(&b.value);
            }
            Stmt::Assign(a) => {
                self.out.push_str(&a.name.name);
                match a.op {
                    None => self.out.push_str(" := "),
                    Some(BinOp::Add) => self.out.push_str(" += "),
                    Some(BinOp::Sub) => self.out.push_str(" -= "),
                    Some(BinOp::Mul) => self.out.push_str(" *= "),
                    Some(BinOp::Div) => self.out.push_str(" /= "),
                    Some(BinOp::Rem) => self.out.push_str(" %= "),
                    Some(_) => self.out.push_str(" := "),
                }
                self.expr(&a.value);
            }
            Stmt::Return(r) => {
                self.out.push_str(&self.sigil("^", "return"));
                match &r.value {
                    // `^ 200 ⟨…⟩` — juxtaposition, not a list literal. Printing the
                    // lowered `^ [200, ⟨…⟩]` reworded the central HTTP handler idiom.
                    Some(Expr::List(l)) if r.juxtaposed => {
                        for e in &l.elements {
                            self.out.push(' ');
                            self.expr(e);
                        }
                    }
                    Some(v) => {
                        self.out.push(' ');
                        self.expr(v);
                    }
                    None => {}
                }
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn block(&mut self, block: &Block) {
        self.out.push_str(&self.sigil("⟦", "[["));
        // The empty `||` of a zero-argument closure has to survive formatting.
        // Printing on `!params.is_empty()` dropped it, and `{ || 42 }` came back as
        // `⟦ 42 ⟧` — a formatter quietly turning a function into its own body, which
        // then failed at the call site with `cannot call value of type int`.
        if block.has_param_list {
            self.out.push_str(" |");
            for (i, p) in block.params.iter().enumerate() {
                if i > 0 {
                    self.out.push_str(", ");
                }
                self.out.push_str(&p.name.name);
            }
            self.out.push('|');
        }
        // Everything before the closing delimiter belongs to this block.
        let inner_end = block.span.end.as_usize().saturating_sub(1);
        let holds_comments = self
            .trivia
            .get(self.next_trivia)
            .is_some_and(|t| t.start < inner_end);
        if block.body.is_empty() && !holds_comments {
            self.out.push(' ');
            self.out.push_str(&self.sigil("⟧", "]]"));
            return;
        }
        // A block the author wrote on one line stays on one line — `◆ sq(n) ⟦ ^ n * n ⟧`
        // and inline `? c ⟦ a ⟧ : ⟦ b ⟧` are idiomatic and were being expanded to four
        // lines each. Only for a single statement, and never when comments are involved,
        // since those need lines of their own. Idempotent: the reprinted form occupies
        // one line too, so a second pass decides the same way.
        if block.body.len() == 1 && !holds_comments && !self.spans_lines(block.span) {
            self.out.push(' ');
            let before = self.out.len();
            self.item(&block.body[0]);
            // Anything that broke a line anyway (a nested multi-line literal) falls
            // back to the block layout rather than emitting a ragged half-inline form.
            if self.out[before..].contains('\n') {
                self.out.truncate(before);
            } else {
                self.out.push(' ');
                self.out.push_str(&self.sigil("⟧", "]]"));
                return;
            }
        }
        self.out.push('\n');
        self.indent += 1;
        self.last_line = Some(self.line_at(block.span.start.as_usize()));
        for item in &block.body {
            self.boundary_item(item);
        }
        // Comments dangling at the end of the block body.
        self.flush_leading(inner_end);
        self.indent -= 1;
        self.pad();
        self.out.push_str(&self.sigil("⟧", "]]"));
    }

    fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(l) => match &l.kind {
                LitKind::None => self.out.push_str("none"),
                LitKind::Bool(b) => self.out.push_str(if *b { "true" } else { "false" }),
                LitKind::Int(n) => self.out.push_str(&n.to_string()),
                LitKind::Float(n) => self.out.push_str(&n.to_string()),
                LitKind::String(s) => {
                    self.out.push('"');
                    for c in s.chars() {
                        match c {
                            '"' => self.out.push_str("\\\""),
                            '\\' => self.out.push_str("\\\\"),
                            '\n' => self.out.push_str("\\n"),
                            '\t' => self.out.push_str("\\t"),
                            c => self.out.push(c),
                        }
                    }
                    self.out.push('"');
                }
            },
            Expr::Ident(i) => self.out.push_str(&i.name),
            Expr::Atom(a) => {
                self.out.push_str(&self.sigil("#", ":"));
                self.out.push_str(&a.parts.join("."));
            }
            Expr::List(l) => {
                let multi = !l.elements.is_empty() && self.spans_lines(l.span);
                self.out.push('[');
                if multi {
                    self.indent += 1;
                }
                for (i, e) in l.elements.iter().enumerate() {
                    if multi {
                        if i > 0 {
                            self.out.push(',');
                        }
                        self.out.push('\n');
                        self.pad();
                    } else if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.expr(e);
                }
                if multi {
                    self.indent -= 1;
                    self.out.push('\n');
                    self.pad();
                }
                self.out.push(']');
            }
            Expr::Record(r) => {
                let multi = !r.entries.is_empty() && self.spans_lines(r.span);
                self.out.push_str(&self.sigil("⟨", "<<"));
                if multi {
                    self.indent += 1;
                }
                for (i, e) in r.entries.iter().enumerate() {
                    if multi {
                        if i > 0 {
                            self.out.push(',');
                        }
                        self.out.push('\n');
                        self.pad();
                    } else if i > 0 {
                        self.out.push_str(", ");
                    }
                    if matches!(e.key, rite_syntax::RecordKey::Spread) {
                        self.out.push_str("..");
                        self.expr(&e.value);
                    } else {
                        self.record_key(&e.key);
                        self.out.push_str(": ");
                        self.expr(&e.value);
                    }
                }
                if multi {
                    self.indent -= 1;
                    self.out.push('\n');
                    self.pad();
                }
                self.out.push_str(&self.sigil("⟩", ">>"));
            }
            Expr::Binary(b) => {
                // `÷` and `∘` are glyph-only. Their "ASCII spellings" — `idiv` and
                // `compose` — do not lex as operators, because both names are taken
                // by the builtins they lower to, so a keyword would collide with
                // `idiv(7, 2)` and `compose(f, g)`.
                //
                // Printing them infix in ASCII changed the answer. `x ← 7 ÷ 2` is 3;
                // `rite fmt --ascii` wrote `x <- 7 idiv 2`, which parses — as two
                // statements — and is **7**. `f ∘ g` became `f compose g`, which is
                // `f`. The call form is the only ASCII rendering that means the same
                // thing, and it round-trips.
                if self.ascii_mode() {
                    let call = match b.op {
                        BinOp::Idiv => Some("idiv"),
                        BinOp::Compose => Some("compose"),
                        _ => None,
                    };
                    if let Some(name) = call {
                        self.out.push_str(name);
                        self.out.push('(');
                        self.expr(&b.left);
                        self.out.push_str(", ");
                        self.expr(&b.right);
                        self.out.push(')');
                        return;
                    }
                }
                self.expr(&b.left);
                self.out.push(' ');
                match b.op {
                    BinOp::Add => self.out.push('+'),
                    BinOp::Sub => self.out.push('-'),
                    BinOp::Mul => self.out.push('*'),
                    BinOp::Div => self.out.push('/'),
                    BinOp::Rem => self.out.push('%'),
                    BinOp::Eq => self.out.push('='),
                    BinOp::NotEq => self.out.push_str("!="),
                    BinOp::Lt => self.out.push('<'),
                    BinOp::LtEq => self.out.push_str("<="),
                    BinOp::Gt => self.out.push('>'),
                    BinOp::GtEq => self.out.push_str(">="),
                    BinOp::And => self.out.push_str("and"),
                    BinOp::Or => self.out.push_str("or"),
                    BinOp::In => {
                        self.out.push_str(&self.sigil("∈", "in"));
                        self.out.push(' ');
                        self.expr(&b.right);
                        return;
                    }
                    BinOp::NotIn => {
                        self.out.push_str(&self.sigil("∉", "not in"));
                        self.out.push(' ');
                        self.expr(&b.right);
                        return;
                    }
                    BinOp::Xor => self.out.push_str(&self.sigil("⊻", "xor")),
                    BinOp::Power => self.out.push_str("**"),
                    BinOp::Idiv => self.out.push_str(&self.sigil("÷", "idiv")),
                    BinOp::Range => self.out.push_str(".."),
                    BinOp::RangeIncl => self.out.push_str(&self.sigil("‥", "..=")),
                    BinOp::Compose => self.out.push_str(&self.sigil("∘", "compose")),
                }
                self.out.push(' ');
                self.expr(&b.right);
            }
            Expr::Unary(u) => {
                match u.op {
                    UnaryOp::Neg => self.out.push('-'),
                    UnaryOp::Not => self.out.push_str("not "),
                    UnaryOp::Effect => {
                        self.out.push_str(&self.sigil("!", "do"));
                        self.out.push(' ');
                    }
                    UnaryOp::Spread => self.out.push_str(".."),
                }
                self.expr(&u.expr);
            }
            Expr::Call(c) => {
                // `use @http.log` / `⊏ { |req, next| … }` is parsed into a call to the
                // internal `__middleware_use`. Print the source form back: emitting the
                // internal name rewrote hand-written middleware into a symbol users are
                // not supposed to see (and `__`-prefixed names are reserved for desugar,
                // so nobody can have written this call themselves).
                if let Expr::Ident(callee) = c.callee.as_ref() {
                    if callee.name == "__middleware_use" && c.args.len() == 1 {
                        self.out.push_str(&self.sigil("⊏", "use"));
                        self.out.push(' ');
                        self.expr(&c.args[0]);
                        return;
                    }
                }
                // `@tcp.listen addr ⟦ |conn| … ⟧` is parsed as this call. Printing it
                // as `@tcp.listen(addr, ⟦…⟧)` would still run, but `rite fmt` would
                // rewrite every server in the corpus into a shape the book does not
                // teach — so the sugar is printed back the way it was written.
                if let Expr::Capability(cap) = c.callee.as_ref() {
                    if cap.path.len() >= 2
                        && cap.path[0] == "tcp"
                        && cap.path[1] == "listen"
                        && c.args.len() == 2
                        && matches!(c.args[1], Expr::Block(_))
                    {
                        self.expr(&c.callee);
                        self.out.push(' ');
                        self.expr(&c.args[0]);
                        self.out.push(' ');
                        self.expr(&c.args[1]);
                        return;
                    }
                }
                // `keep ⟦ |n| … ⟧` — a single trailing block argument, printed back
                // the way it was written. The AST records that the source used the
                // sugar; without that flag it is indistinguishable from
                // `keep(⟦ … ⟧)`, and `rite fmt` rewrote every pipeline in the corpus
                // — `examples/02-pipelines` and every snippet in the book included —
                // into the form the book does not use. Same reasoning as
                // `@tcp.listen` above.
                if c.trailing_block && c.args.len() == 1 && matches!(c.args[0], Expr::Block(_)) {
                    self.expr(&c.callee);
                    self.out.push(' ');
                    self.expr(&c.args[0]);
                    return;
                }
                self.expr(&c.callee);
                self.out.push('(');
                for (i, a) in c.args.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.expr(a);
                }
                self.out.push(')');
            }
            Expr::Member(m) => {
                self.expr(&m.object);
                self.out.push('.');
                self.out.push_str(&m.field.name);
            }
            Expr::Index(i) => {
                self.expr(&i.object);
                self.out.push('[');
                self.expr(&i.index);
                self.out.push(']');
            }
            Expr::Pipeline(p) => {
                // A pipeline broken across lines is the language's most readable shape —
                // one stage per line, arrows aligned. Collapsing it onto one line was the
                // single biggest loss of rhythm in the formatter.
                let multi = self.spans_lines(p.span);
                self.expr(&p.input);
                if multi {
                    self.indent += 1;
                }
                for s in &p.stages {
                    if multi {
                        self.out.push('\n');
                        self.pad();
                    } else {
                        self.out.push(' ');
                    }
                    self.out.push_str(&self.sigil("→", "->"));
                    self.out.push(' ');
                    self.expr(s);
                }
                if multi {
                    self.indent -= 1;
                }
            }
            Expr::If(i) => {
                self.out.push_str(&self.sigil("?", "if"));
                self.out.push(' ');
                self.expr(&i.condition);
                self.out.push(' ');
                self.block(&i.then_branch);
                if let Some(e) = &i.else_branch {
                    // Was a hardcoded " : ", so an ASCII-dialect file came back with the
                    // glyph spelling of `else` while its `if` stayed ASCII.
                    self.out.push(' ');
                    self.out.push_str(&self.sigil(":", "else"));
                    self.out.push(' ');
                    self.block(e);
                }
            }
            Expr::Match(m) => {
                self.out.push_str(&self.sigil("~", "match"));
                self.out.push(' ');
                self.expr(&m.scrutinee);
                self.out.push(' ');
                self.out.push_str(&self.sigil("⟦", "[["));
                self.out.push('\n');
                self.indent += 1;
                self.last_line = Some(self.line_at(m.span.start.as_usize()));
                let arms_end = m.span.end.as_usize().saturating_sub(1);
                for arm in &m.arms {
                    let start = arm.span.start.as_usize();
                    let end = arm.span.end.as_usize();
                    self.flush_leading(start);
                    self.newline();
                    self.gap_before(self.line_at(start));
                    self.pad();
                    self.pattern(&arm.pattern);
                    self.out.push(' ');
                    self.out.push_str(&self.sigil("→", "->"));
                    self.out.push(' ');
                    self.expr(&arm.body);
                    self.last_line = Some(self.line_at(end.saturating_sub(1).max(start)));
                    self.flush_trailing(end);
                    self.out.push('\n');
                }
                self.flush_leading(arms_end);
                self.indent -= 1;
                self.pad();
                self.out.push_str(&self.sigil("⟧", "]]"));
            }
            Expr::Block(b) => self.block(b),
            Expr::Capability(c) => {
                self.out.push_str(&self.sigil("@", "host."));
                self.out.push_str(&c.path.join("."));
            }
            Expr::Placeholder(_) => self.out.push('$'),
            Expr::Try(t) => {
                self.expr(&t.expr);
                self.out.push('?');
            }
            Expr::Coalesce(c) => {
                self.expr(&c.left);
                self.out.push_str(" ?? ");
                self.expr(&c.right);
            }
            Expr::HttpListen(h) => {
                self.out.push_str(&self.sigil("@", "host."));
                self.out.push_str("http.listen ");
                self.expr(&h.addr);
                self.out.push(' ');
                self.block(&h.body);
            }
            Expr::Route(r) => {
                let method = match r.method {
                    rite_syntax::HttpMethod::Get => "GET",
                    rite_syntax::HttpMethod::Post => "POST",
                    rite_syntax::HttpMethod::Put => "PUT",
                    rite_syntax::HttpMethod::Patch => "PATCH",
                    rite_syntax::HttpMethod::Delete => "DELETE",
                    rite_syntax::HttpMethod::Head => "HEAD",
                    rite_syntax::HttpMethod::Options => "OPTIONS",
                };
                self.out.push_str(method);
                self.out.push(' ');
                self.out.push('"');
                self.out.push_str(&r.path);
                self.out.push('"');
                // The handler's parameter list binds `req`: dropping it turns a
                // working route into `undefined name` at resolve time.
                if !r.params.is_empty() {
                    self.out.push_str(" |");
                    for (i, p) in r.params.iter().enumerate() {
                        if i > 0 {
                            self.out.push_str(", ");
                        }
                        self.out.push_str(&p.name.name);
                    }
                    self.out.push('|');
                }
                self.out.push(' ');
                self.block(&r.body);
            }
            Expr::Group(g) => {
                self.out.push('(');
                self.expr(&g.expr);
                self.out.push(')');
            }
        }
    }

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Ident(i) => self.out.push_str(&i.name),
            Pattern::Atom(a) => {
                self.out.push_str(&self.sigil("#", ":"));
                self.out.push_str(&a.parts.join("."));
            }
            Pattern::Literal(l) => self.expr(&Expr::Literal(l.clone())),
            Pattern::Wildcard(_) => self.out.push('_'),
            Pattern::List(l) => {
                self.out.push('[');
                for (i, e) in l.elements.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.pattern(e);
                }
                if let Some(r) = &l.rest {
                    if !l.elements.is_empty() {
                        self.out.push_str(", ");
                    }
                    self.out.push_str("..");
                    self.pattern(r);
                }
                self.out.push(']');
            }
            Pattern::Record(r) => {
                self.out.push_str(&self.sigil("⟨", "<<"));
                for (i, f) in r.fields.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.out.push_str(&f.name.name);
                    if let Some(p) = &f.pattern {
                        self.out.push_str(": ");
                        self.pattern(p);
                    }
                }
                self.out.push_str(&self.sigil("⟩", ">>"));
            }
            Pattern::Result(r) => {
                self.out.push_str(match r.kind {
                    rite_syntax::ResultPatKind::Ok => "ok",
                    rite_syntax::ResultPatKind::Err => "err",
                    rite_syntax::ResultPatKind::Some => "some",
                    rite_syntax::ResultPatKind::None => "none",
                });
                if let Some(b) = &r.binding {
                    self.out.push(' ');
                    self.pattern(b);
                }
            }
        }
    }

    fn record_key(&mut self, key: &rite_syntax::RecordKey) {
        match key {
            rite_syntax::RecordKey::Ident(i) => self.out.push_str(&i.name),
            rite_syntax::RecordKey::String(s) => {
                self.out.push('"');
                self.out.push_str(s);
                self.out.push('"');
            }
            rite_syntax::RecordKey::Atom(a) => {
                self.out.push_str(&self.sigil("#", ":"));
                self.out.push_str(&a.parts.join("."));
            }
            rite_syntax::RecordKey::Spread => self.out.push_str(".."),
        }
    }

    fn type_expr(&mut self, ty: &rite_syntax::TypeExpr) {
        match ty {
            rite_syntax::TypeExpr::Named(i) => self.out.push_str(&i.name),
            rite_syntax::TypeExpr::List(inner) => {
                self.out.push('[');
                self.type_expr(inner);
                self.out.push(']');
            }
            rite_syntax::TypeExpr::Result(_) => self.out.push_str("result"),
            rite_syntax::TypeExpr::Record(_) => self.out.push_str("record"),
            rite_syntax::TypeExpr::Any(_) => self.out.push_str("any"),
        }
    }
}

fn item_span(item: &Item) -> Span {
    match item {
        Item::Function(f) => f.span,
        Item::Data(d) => d.span,
        Item::Event(e) => e.span,
        Item::Import(i) => i.span,
        Item::Test(t) => t.span,
        Item::Statement(s) => stmt_span(s),
    }
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Binding(b) => b.span,
        Stmt::Assign(a) => a.span,
        Stmt::Return(r) => r.span,
        Stmt::Expr(e) => e.span(),
    }
}

/// Load canonical alias table (embedded fallback).
pub fn aliases_json() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../grammar/aliases.json"))
        .unwrap_or(serde_json::json!({}))
}
