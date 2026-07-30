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
            .map(|d| enrich_diagnostic_json(d, source))
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

fn collect_symbols(program: &Program, sources: &SourceMap, out: &mut Vec<SymbolInfo>) {
    let file = sources.files().first();
    for item in &program.items {
        if let rite_syntax::Item::Function(f) = item {
            let (line, character) = file
                .map(|sf| {
                    let lc = sf.line_col(f.name.span.start);
                    (lc.line, lc.column.saturating_sub(1))
                })
                .unwrap_or((1, 0));
            out.push(SymbolInfo {
                name: f.name.name.clone(),
                kind: "function".into(),
                detail: format!("{}/{}", f.name.name, f.params.len()),
                line,
                character,
            });
        }
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
fn enrich_diagnostic_json(d: &rite_core::Diagnostic, source: &str) -> serde_json::Value {
    let mut v = d.to_json();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("code_str".into(), serde_json::json!(format!("{}", d.code)));
        if let Some(span) = d.primary_span() {
            let start = byte_to_line_col(source, span.span.start.as_usize());
            let end = byte_to_line_col(source, span.span.end.as_usize());
            obj.insert("start_line".into(), serde_json::json!(start.0));
            obj.insert("start_character".into(), serde_json::json!(start.1));
            obj.insert("end_line".into(), serde_json::json!(end.0));
            obj.insert("end_character".into(), serde_json::json!(end.1));
        }
    }
    v
}

fn byte_to_line_col(text: &str, byte: usize) -> (u32, u32) {
    let byte = byte.min(text.len());
    let mut line = 1u32;
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
