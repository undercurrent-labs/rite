//! Immutable analysis snapshots for LSP / Studio / WASM.

pub mod workspace;

pub use workspace::{ReferenceLoc, WorkspaceIndex, WorkspaceSymbol};

use rite_core::{Diagnostics, SourceFile, SourceMap};
use rite_sem::{compile_to_ir, ProgramIr};
use rite_syntax::{parse_source, Program};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisSnapshot {
    pub version: u64,
    pub uri: String,
    pub source: String,
    pub diagnostics: Vec<serde_json::Value>,
    pub symbols: Vec<SymbolInfo>,
    pub has_errors: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub detail: String,
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone)]
pub struct AnalysisEngine {
    revision: u64,
}

impl Default for AnalysisEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisEngine {
    pub fn new() -> Self {
        Self { revision: 0 }
    }

    pub fn analyze(&mut self, uri: &str, source: &str) -> AnalysisSnapshot {
        self.revision += 1;
        let (program, diags, sources) = parse_source(uri, source);
        let mut symbols = Vec::new();
        if let Some(ref p) = program {
            collect_symbols(p, &sources, &mut symbols);
        }
        // also compile for semantic diagnostics
        let file = SourceFile::new(rite_core::FileId(0), uri, source);
        let (_ir, sdiags) = compile_to_ir(&file);
        let mut all = diags;
        // merge semantic errors without duplicating if parse failed hard
        for d in sdiags.into_vec() {
            all.push(d);
        }
        let diagnostics: Vec<_> = all
            .iter()
            .map(|d| enrich_diagnostic_json(d, &file))
            .collect();
        let has_errors = all.has_errors();
        AnalysisSnapshot {
            version: self.revision,
            uri: uri.to_string(),
            source: source.to_string(),
            diagnostics,
            symbols,
            has_errors,
        }
    }

    pub fn completions(&self, source: &str, _line: u32, _character: u32) -> Vec<CompletionItem> {
        let mut items = builtin_completions();
        // Add identifiers from source heuristically
        let (program, _, _) = parse_source("c.rite", source);
        if let Some(p) = program {
            for item in &p.items {
                if let rite_syntax::Item::Function(f) = item {
                    // Prefer the first line of the `///` block as the detail — a
                    // summary is more use in a completion list than the arity alone.
                    let signature = format!("fn {}/{}", f.name.name, f.params.len());
                    let detail = match f
                        .doc
                        .as_deref()
                        .and_then(|d| d.lines().next())
                        .map(str::trim)
                    {
                        Some(first) if !first.is_empty() => format!("{signature} — {first}"),
                        _ => signature,
                    };
                    items.push(CompletionItem {
                        label: f.name.name.clone(),
                        kind: "function".into(),
                        detail,
                        insert_text: f.name.name.clone(),
                    });
                }
            }
        }
        items
    }

    pub fn hover(&self, source: &str, line: u32, character: u32) -> Option<HoverInfo> {
        let word = word_at(source, line, character)?;
        if let Some(cap) = capability_hover(&word) {
            return Some(cap);
        }
        let (program, _, _) = parse_source("h.rite", source);
        if let Some(p) = program {
            for item in &p.items {
                if let rite_syntax::Item::Function(f) = item {
                    if f.name.name == word {
                        // Show the `///` block when the source has one; the parser
                        // attaches it to the declaration. Falls back to the signature
                        // line so an undocumented function still hovers usefully.
                        let body = match f.doc.as_deref().map(str::trim) {
                            Some(doc) if !doc.is_empty() => doc.to_string(),
                            _ => "User-defined function.".to_string(),
                        };
                        return Some(HoverInfo {
                            title: format!("function {}", f.name.name),
                            markdown: format!(
                                "**{}** (`{}/{}`)\n\n{}",
                                f.name.name,
                                f.name.name,
                                f.params.len(),
                                body
                            ),
                        });
                    }
                }
            }
        }
        Some(HoverInfo {
            title: word.clone(),
            markdown: format!("`{}`", word),
        })
    }

    pub fn compile_ir(&self, source: &str) -> Result<ProgramIr, Diagnostics> {
        let file = SourceFile::new(rite_core::FileId(0), "a.rite", source);
        let (ir, diags) = compile_to_ir(&file);
        if diags.has_errors() {
            return Err(diags);
        }
        ir.ok_or(diags)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    pub detail: String,
    pub insert_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HoverInfo {
    pub title: String,
    pub markdown: String,
}

/// One declaration a document exposes.
///
/// The single source for "what symbols does this file declare". There were three
/// separate walks (here and twice in `workspace.rs`), all of which handled only
/// `Item::Function` — which is why every symbol in an editor outline was labelled
/// `FUNCTION`, and why nothing but functions appeared at all.
#[derive(Debug, Clone)]
pub struct DeclaredSymbol {
    pub name: String,
    /// `function`, `constant`, `variable`, `event`, or `test`. Callers map this onto
    /// their own vocabulary (`SymbolKind` for LSP).
    pub kind: &'static str,
    pub detail: String,
    /// Span of the *name*, so a jump lands on the identifier rather than the keyword.
    pub span: rite_core::Span,
    pub is_pub: bool,
}

/// Every top-level declaration in `program`, in source order.
pub fn declared_symbols(program: &Program) -> Vec<DeclaredSymbol> {
    let mut out = Vec::new();
    for item in &program.items {
        match item {
            rite_syntax::Item::Function(f) => out.push(DeclaredSymbol {
                name: f.name.name.clone(),
                kind: "function",
                detail: format!("{}/{}", f.name.name, f.params.len()),
                span: f.name.span,
                is_pub: f.is_pub,
            }),
            // `◆ Cfg ⟨…⟩` — an immutable record binding, so a constant.
            rite_syntax::Item::Data(d) => out.push(DeclaredSymbol {
                name: d.name.name.clone(),
                kind: "constant",
                detail: format!("record, {} field(s)", d.fields.len()),
                span: d.name.span,
                is_pub: false,
            }),
            // Top-level bindings. `↢`/`<~` is a variable; `←`/`<-` cannot be reassigned,
            // so it reads as a constant.
            rite_syntax::Item::Statement(rite_syntax::Stmt::Binding(b)) => {
                if let rite_syntax::Pattern::Ident(id) = &b.pattern {
                    out.push(DeclaredSymbol {
                        name: id.name.clone(),
                        kind: if b.mutable { "variable" } else { "constant" },
                        detail: if b.mutable {
                            "mutable binding".into()
                        } else {
                            "binding".into()
                        },
                        span: id.span,
                        is_pub: false,
                    });
                }
            }
            rite_syntax::Item::Event(e) => out.push(DeclaredSymbol {
                name: e.atom.parts.join("."),
                kind: "event",
                detail: format!("{:?} event", e.kind).to_lowercase(),
                span: e.atom.span,
                is_pub: false,
            }),
            rite_syntax::Item::Test(t) => out.push(DeclaredSymbol {
                name: t.name.clone(),
                kind: "test",
                detail: "test".into(),
                span: t.span,
                is_pub: false,
            }),
            rite_syntax::Item::Import(_) | rite_syntax::Item::Statement(_) => {}
        }
    }
    out
}

fn collect_symbols(program: &Program, sources: &SourceMap, out: &mut Vec<SymbolInfo>) {
    let file = sources.files().first();
    for d in declared_symbols(program) {
        // UTF-16 columns, matching `references`. These two used to disagree: symbols
        // reported character columns and references reported UTF-16 units, so an astral
        // character earlier on the line put "go to definition" and "find references" on
        // different columns of the same identifier.
        let (line, character) = file
            .map(|sf| {
                let (l, c) = sf.line_utf16_col(d.span.start);
                (l + 1, c)
            })
            .unwrap_or((1, 0));
        out.push(SymbolInfo {
            name: d.name,
            kind: d.kind.to_string(),
            detail: d.detail,
            line,
            character,
        });
    }
}

fn word_at(source: &str, line: u32, character: u32) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let line = lines
        .get((line.saturating_sub(1)) as usize)
        .copied()
        .unwrap_or("");
    let col = character as usize;
    if line.is_empty() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut start = col.min(bytes.len());
    let mut end = start;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'_'
            || bytes[start - 1] == b'@')
    {
        start -= 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if start >= end {
        return None;
    }
    Some(line[start..end].to_string())
}

fn capability_hover(word: &str) -> Option<HoverInfo> {
    let table: HashMap<&str, &str> = [
        (
            "@console",
            "Console I/O capability. Effectful: print, println, warn, error.",
        ),
        (
            "@fs",
            "Filesystem capability. Requires fs:read / fs:write permissions.",
        ),
        (
            "@json",
            "JSON encode/decode. Pure decode; write requires fs.",
        ),
        (
            "@http",
            "HTTP server and client. Browser uses virtual listener.",
        ),
        ("@clock", "Clock and sleep. Nondeterministic unless faked."),
        ("@random", "Seedable RNG."),
        ("@game", "Text RPG entity and event runtime."),
        ("console", "See @console"),
        ("fs", "See @fs"),
        ("json", "See @json"),
        ("http", "See @http"),
    ]
    .into_iter()
    .collect();
    table.get(word).map(|md| HoverInfo {
        title: word.to_string(),
        markdown: md.to_string(),
    })
}

/// Attach start_line/start_character/end_* for LSP consumers.
///
/// Positions are UTF-16 columns with 1-based lines, from the one implementation in
/// `rite-core`. This crate previously carried two more of its own — an editor jumping
/// to a diagnostic and an editor jumping to that same symbol had no reason to land in
/// the same place.
fn enrich_diagnostic_json(d: &rite_core::Diagnostic, file: &SourceFile) -> serde_json::Value {
    let mut v = d.to_json();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("code_str".into(), serde_json::json!(format!("{}", d.code)));
        if let Some(span) = d.primary_span() {
            let (sl, sc) = file.line_utf16_col(span.span.start);
            let (el, ec) = file.line_utf16_col(span.span.end);
            obj.insert("start_line".into(), serde_json::json!(sl + 1));
            obj.insert("start_character".into(), serde_json::json!(sc));
            obj.insert("end_line".into(), serde_json::json!(el + 1));
            obj.insert("end_character".into(), serde_json::json!(ec));
        }
    }
    v
}

/// Width of `text` in UTF-16 code units — the unit an LSP `Position` counts in.
///
/// Rite identifiers may hold any non-ASCII byte, so `café` is five bytes but four
/// UTF-16 units. Deriving a range end from `str::len` overshot it by one per non-ASCII
/// character, which put the closing edge of a rename or highlight inside the next token.
pub fn utf16_width(text: &str) -> u32 {
    text.chars().map(|c| c.len_utf16() as u32).sum()
}

fn builtin_completions() -> Vec<CompletionItem> {
    let mut items = vec![
        ("def", "keyword", "def name() [[ ]]"),
        ("◆", "keyword", "◆ name() ⟦ ⟧"),
        ("if", "keyword", "if"),
        ("match", "keyword", "match"),
        ("return", "keyword", "return"),
        ("map", "function", "map"),
        ("keep", "function", "keep"),
        ("sum", "function", "sum"),
        ("count", "function", "count"),
        ("@console", "capability", "@console"),
        ("@fs", "capability", "@fs"),
        ("@json", "capability", "@json"),
        ("@http", "capability", "@http"),
        ("@clock", "capability", "@clock"),
        ("@random", "capability", "@random"),
        ("@game", "capability", "@game"),
        ("true", "constant", "true"),
        ("false", "constant", "false"),
        ("none", "constant", "none"),
    ];
    items
        .drain(..)
        .map(|(label, kind, insert)| CompletionItem {
            label: label.into(),
            kind: kind.into(),
            detail: kind.into(),
            insert_text: insert.into(),
        })
        .collect()
}
