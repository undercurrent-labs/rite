//! Idempotent Rite formatter and dialect converter (V1).

use rite_syntax::{
    parse_source, BinOp, Block, Expr, Item, LitKind, Pattern, Program, Stmt, UnaryOp,
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
    let (program, diags, _) = parse_source("fmt.rite", source);
    if diags.has_errors() {
        // Best-effort: still try to return source unchanged for invalid input
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
    let text = format_program(
        &program,
        &FormatOptions {
            dialect,
            ..Default::default()
        },
    );
    let source_map = Some(build_line_source_map(source, &text));
    Ok(FormatResult {
        text,
        dialect,
        source_map,
    })
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

pub fn format_program(program: &Program, opts: &FormatOptions) -> String {
    let mut f = Formatter {
        opts: *opts,
        out: String::new(),
        indent: 0,
    };
    for (i, item) in program.items.iter().enumerate() {
        if i > 0 {
            f.out.push('\n');
        }
        f.item(item);
        f.out.push('\n');
    }
    f.out
}

struct Formatter {
    opts: FormatOptions,
    out: String,
    indent: usize,
}

impl Formatter {
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
                self.out.push_str("use ");
                for (i, s) in imp.path.segments.iter().enumerate() {
                    if i > 0 {
                        self.out.push('.');
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
                self.out.push_str(" := ");
                self.expr(&a.value);
            }
            Stmt::Return(r) => {
                self.out.push_str(&self.sigil("^", "return"));
                if let Some(v) = &r.value {
                    self.out.push(' ');
                    self.expr(v);
                }
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn block(&mut self, block: &Block) {
        self.out.push_str(&self.sigil("⟦", "[["));
        if !block.params.is_empty() {
            self.out.push_str(" |");
            for (i, p) in block.params.iter().enumerate() {
                if i > 0 {
                    self.out.push_str(", ");
                }
                self.out.push_str(&p.name.name);
            }
            self.out.push_str("|");
        }
        if block.body.is_empty() {
            self.out.push(' ');
            self.out.push_str(&self.sigil("⟧", "]]"));
            return;
        }
        self.out.push('\n');
        self.indent += 1;
        for item in &block.body {
            self.pad();
            self.item(item);
            self.out.push('\n');
        }
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
                self.out.push('[');
                for (i, e) in l.elements.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.expr(e);
                }
                self.out.push(']');
            }
            Expr::Record(r) => {
                self.out.push_str(&self.sigil("⟨", "<<"));
                for (i, e) in r.entries.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    self.record_key(&e.key);
                    self.out.push_str(": ");
                    self.expr(&e.value);
                }
                self.out.push_str(&self.sigil("⟩", ">>"));
            }
            Expr::Binary(b) => {
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
                }
                self.expr(&u.expr);
            }
            Expr::Call(c) => {
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
                self.expr(&p.input);
                for s in &p.stages {
                    self.out.push(' ');
                    self.out.push_str(&self.sigil("→", "->"));
                    self.out.push(' ');
                    self.expr(s);
                }
            }
            Expr::If(i) => {
                self.out.push_str(&self.sigil("?", "if"));
                self.out.push(' ');
                self.expr(&i.condition);
                self.out.push(' ');
                self.block(&i.then_branch);
                if let Some(e) = &i.else_branch {
                    self.out.push_str(" : ");
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
                for arm in &m.arms {
                    self.pad();
                    self.pattern(&arm.pattern);
                    self.out.push(' ');
                    self.out.push_str(&self.sigil("→", "->"));
                    self.out.push(' ');
                    self.expr(&arm.body);
                    self.out.push('\n');
                }
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
            Pattern::Typed(t) => self.pattern(&t.pattern),
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

/// Load canonical alias table (embedded fallback).
pub fn aliases_json() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../grammar/aliases.json"))
        .unwrap_or(serde_json::json!({}))
}
