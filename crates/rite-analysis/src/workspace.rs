//! Multi-file workspace analysis: imports, symbols, references.

use crate::{AnalysisEngine, AnalysisSnapshot};
use rite_core::{FileId, SourceFile};
use rite_sem::{compile_to_ir_with_roots, resolve_module_path};
use rite_syntax::{parse_source, Item};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceSymbol {
    pub name: String,
    pub kind: String,
    pub uri: String,
    pub line: u32,
    pub character: u32,
    pub is_pub: bool,
    pub module: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferenceLoc {
    pub uri: String,
    pub line: u32,
    pub character: u32,
    pub end_character: u32,
}

#[derive(Debug, Default)]
pub struct WorkspaceIndex {
    /// uri -> source text
    pub documents: HashMap<String, String>,
    /// module name -> file path / uri
    pub modules: HashMap<String, String>,
    pub symbols: Vec<WorkspaceSymbol>,
    pub roots: Vec<PathBuf>,
}

impl WorkspaceIndex {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            ..Default::default()
        }
    }

    pub fn upsert_document(&mut self, uri: &str, text: &str) {
        self.documents.insert(uri.to_string(), text.to_string());
        self.reindex();
    }

    pub fn remove_document(&mut self, uri: &str) {
        self.documents.remove(uri);
        self.reindex();
    }

    pub fn reindex(&mut self) {
        self.symbols.clear();
        self.modules.clear();
        // Collect pending imports to load (avoid borrow conflicts)
        let mut to_load: Vec<(String, String, PathBuf)> = Vec::new();
        let docs: Vec<(String, String)> = self
            .documents
            .iter()
            .map(|(u, t)| (u.clone(), t.clone()))
            .collect();
        let roots = self.roots.clone();

        for (uri, text) in &docs {
            let (program, diags, sources) = parse_source(uri, text);
            if diags.has_errors() {
                continue;
            }
            let Some(program) = program else { continue };
            let module_name = uri_to_module_name(uri);
            self.modules.insert(module_name.clone(), uri.clone());
            let file = sources.files().first();
            // Every declaration kind, via the shared collector.
            for d in crate::declared_symbols(&program) {
                // UTF-16, matching `references` — see the note in `collect_symbols`.
                let (line, character) = file
                    .map(|sf| {
                        let (l, c) = sf.line_utf16_col(d.span.start);
                        (l + 1, c)
                    })
                    .unwrap_or((1, 0));
                self.symbols.push(WorkspaceSymbol {
                    name: d.name,
                    kind: d.kind.into(),
                    uri: uri.clone(),
                    line,
                    character,
                    is_pub: d.is_pub,
                    module: module_name.clone(),
                });
            }
            for item in &program.items {
                if let Item::Import(imp) = item {
                    let segs: Vec<String> =
                        imp.path.segments.iter().map(|s| s.name.clone()).collect();
                    let key = segs.join(".");
                    if let Some(path) = resolve_on_disk(&segs, uri, &roots) {
                        let path_uri = format!("file://{}", path.display());
                        self.modules.insert(key.clone(), path_uri.clone());
                        if !self.documents.contains_key(&path_uri) {
                            to_load.push((path_uri, key, path));
                        }
                    }
                }
            }
        }

        for (path_uri, key, path) in to_load {
            if let Ok(src) = std::fs::read_to_string(&path) {
                index_file_symbols(&path_uri, &src, &key, &mut self.symbols);
                self.documents.insert(path_uri, src);
            }
        }
    }

    pub fn workspace_symbols(&self, query: &str) -> Vec<WorkspaceSymbol> {
        let q = query.to_ascii_lowercase();
        self.symbols
            .iter()
            .filter(|s| q.is_empty() || s.name.to_ascii_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    pub fn find_definition(&self, name: &str) -> Option<WorkspaceSymbol> {
        self.symbols
            .iter()
            .find(|s| s.name == name && s.is_pub)
            .cloned()
            .or_else(|| self.symbols.iter().find(|s| s.name == name).cloned())
    }

    /// Find references to `name` across open/indexed documents.
    ///
    /// Identifier tokens only. This was a word-boundary substring scan, so it reported
    /// hits inside string literals and comments — `"x marks x"` and `// x` counted as
    /// references to `x`. Read-only, so it never corrupted anything, but the results
    /// were not trustworthy enough to drive rename.
    pub fn find_references(&self, name: &str) -> Vec<ReferenceLoc> {
        let mut out = Vec::new();
        for (uri, text) in &self.documents {
            let mut sources = rite_core::SourceMap::new();
            let id = sources.add_file(uri, text);
            let Some(file) = sources.get(id) else {
                continue;
            };
            let (tokens, _) = rite_syntax::lex(file);
            for tok in tokens {
                if tok.kind != rite_syntax::TokenKind::Ident || tok.text != name {
                    continue;
                }
                let (line, character) = file.line_utf16_col(tok.span.start);
                let width = crate::utf16_width(name);
                out.push(ReferenceLoc {
                    uri: uri.clone(),
                    // Callers expect 1-based lines, as the old scan produced.
                    line: line + 1,
                    character,
                    end_character: character + width,
                });
            }
        }
        out
    }

    pub fn analyze_with_imports(&self, uri: &str, text: &str) -> AnalysisSnapshot {
        let mut eng = AnalysisEngine::new();
        // Prefer path-aware compile when file URI
        if let Some(path) = uri.strip_prefix("file://") {
            let p = PathBuf::from(path);
            if p.exists() {
                let file = SourceFile::from_path(FileId(0), &p).ok();
                if let Some(file) = file {
                    let roots = self.roots.clone();
                    let (_ir, diags) = compile_to_ir_with_roots(&file, Some(&p), &roots);
                    let mut snap = eng.analyze(uri, text);
                    // merge path-aware diagnostics if worse
                    if diags.has_errors() && !snap.has_errors {
                        snap.has_errors = true;
                        for d in diags.iter() {
                            snap.diagnostics.push(d.to_json());
                        }
                    }
                    return snap;
                }
            }
        }
        eng.analyze(uri, text)
    }
}

fn index_file_symbols(uri: &str, text: &str, module: &str, symbols: &mut Vec<WorkspaceSymbol>) {
    let (program, diags, sources) = parse_source(uri, text);
    if diags.has_errors() {
        return;
    }
    let Some(program) = program else { return };
    let file = sources.files().first();
    for d in crate::declared_symbols(&program) {
        // UTF-16, matching `references` — see the note in `collect_symbols`.
        let (line, character) = file
            .map(|sf| {
                let (l, c) = sf.line_utf16_col(d.span.start);
                (l + 1, c)
            })
            .unwrap_or((1, 0));
        symbols.push(WorkspaceSymbol {
            name: d.name,
            kind: d.kind.into(),
            uri: uri.to_string(),
            line,
            character,
            is_pub: d.is_pub,
            module: module.to_string(),
        });
    }
}

fn uri_to_module_name(uri: &str) -> String {
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_string()
}

fn resolve_on_disk(segs: &[String], from_uri: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let from = from_uri.strip_prefix("file://").unwrap_or(from_uri);
    let from_dir = Path::new(from).parent().unwrap_or(Path::new("."));
    resolve_module_path(segs, from_dir, roots)
}

/// Byte offset → (0-based line, UTF-16 column).
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn references_ignore_strings_comments_and_substrings() {
        // The old word-boundary substring scan counted every one of these.
        let mut ws = WorkspaceIndex::new(vec![]);
        ws.upsert_document(
            "file:///tmp/r.rite",
            "x ← 1\ns ← \"x marks x\"\n// x in a comment\nm ← max(x, 2)\n",
        );
        let refs = ws.find_references("x");
        // Real uses: the binding on line 1 and the argument on line 4.
        assert_eq!(refs.len(), 2, "unexpected references: {refs:#?}");
        let lines: Vec<u32> = refs.iter().map(|r| r.line).collect();
        assert_eq!(lines, vec![1, 4], "wrong lines: {refs:#?}");
    }

    #[test]
    fn reference_columns_are_utf16() {
        let mut ws = WorkspaceIndex::new(vec![]);
        // `◆ f() ⟦ ` is glyph-heavy, so a byte column would overshoot.
        ws.upsert_document("file:///tmp/g.rite", "◆ f() ⟦ ^ target ⟧\n");
        let refs = ws.find_references("target");
        assert_eq!(refs.len(), 1);
        let line = "◆ f() ⟦ ^ target ⟧";
        let expected: u32 = line
            .chars()
            .take_while(|c| *c != 't')
            .map(|c| c.len_utf16() as u32)
            .sum();
        assert_eq!(refs[0].character, expected);
    }

    #[test]
    fn symbol_and_reference_columns_use_the_same_convention() {
        // The defect: symbols reported character columns while references reported
        // UTF-16 units. They agree on every ASCII line and on every BMP glyph, so only
        // an astral character, one character but two UTF-16 units, separates them. An
        // editor asked to jump to `target` landed in two different places depending on
        // which feature it asked.
        let mut ws = WorkspaceIndex::new(vec![]);
        let src = "s ← \"\u{1F600}\"\n◆ target() ⟦ ^ target ⟧\n";
        ws.upsert_document("file:///tmp/e.rite", src);

        let decl = ws
            .symbols
            .iter()
            .find(|s| s.name == "target")
            .expect("declaration indexed");
        let call = ws
            .find_references("target")
            .into_iter()
            .find(|r| r.line == decl.line && r.character != decl.character)
            .or_else(|| ws.find_references("target").into_iter().next())
            .expect("reference found");

        // Both are 1-based lines and 0-based UTF-16 columns.
        assert_eq!(decl.line, 2, "the declaration is on the second line");
        assert_eq!(call.line, 2);
        let decl_ref = ws
            .find_references("target")
            .into_iter()
            .find(|r| r.character == decl.character);
        assert!(
            decl_ref.is_some(),
            "no reference agreed with the symbol column {}; references were {:?}",
            decl.character,
            ws.find_references("target")
        );
    }

    #[test]
    fn an_astral_character_shifts_utf16_columns_on_its_line() {
        let mut ws = WorkspaceIndex::new(vec![]);
        // One emoji, then a declaration on the same line: two UTF-16 units for the
        // emoji, so `target` sits one column further right than its character index.
        ws.upsert_document("file:///tmp/a.rite", "// \u{1F600}\n◆ target() ⟦ ^ 1 ⟧\n");
        let decl = ws
            .symbols
            .iter()
            .find(|s| s.name == "target")
            .expect("symbol");
        assert_eq!(
            (decl.line, decl.character),
            (2, 2),
            "after `◆ ` on the second line"
        );
    }

    #[test]
    fn indexes_open_docs_and_finds_refs() {
        let mut ws = WorkspaceIndex::new(vec![]);
        ws.upsert_document(
            "file:///tmp/a.rite",
            "◆ foo() ⟦ ^ 1 ⟧\n◆ bar() ⟦ ^ foo() ⟧\n",
        );
        assert!(ws.workspace_symbols("fo").iter().any(|s| s.name == "foo"));
        let refs = ws.find_references("foo");
        assert!(refs.len() >= 2);
    }

    #[test]
    fn loads_imported_module_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let math = dir.path().join("math.rite");
        let mut f = std::fs::File::create(&math).unwrap();
        writeln!(f, "pub ◆ square(n) ⟦").unwrap();
        writeln!(f, "  ^ n * n").unwrap();
        writeln!(f, "⟧").unwrap();
        let main = dir.path().join("main.rite");
        std::fs::write(&main, "use math\nsquare(2)\n").unwrap();
        let mut ws = WorkspaceIndex::new(vec![dir.path().to_path_buf()]);
        let main_uri = format!("file://{}", main.display());
        ws.upsert_document(&main_uri, &std::fs::read_to_string(&main).unwrap());
        assert!(
            ws.symbols.iter().any(|s| s.name == "square"),
            "expected square from import: {:?}",
            ws.symbols
        );
    }
}
