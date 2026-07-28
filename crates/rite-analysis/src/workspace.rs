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
            for item in &program.items {
                if let Item::Function(f) = item {
                    let (line, character) = file
                        .map(|sf| {
                            let lc = sf.line_col(f.name.span.start);
                            (lc.line, lc.column.saturating_sub(1))
                        })
                        .unwrap_or((1, 0));
                    self.symbols.push(WorkspaceSymbol {
                        name: f.name.name.clone(),
                        kind: "function".into(),
                        uri: uri.clone(),
                        line,
                        character,
                        is_pub: f.is_pub,
                        module: module_name.clone(),
                    });
                }
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

    /// Find textual references to `name` across open/indexed documents.
    pub fn find_references(&self, name: &str) -> Vec<ReferenceLoc> {
        let mut out = Vec::new();
        for (uri, text) in &self.documents {
            for (line_idx, line) in text.lines().enumerate() {
                let mut start = 0usize;
                while let Some(rel) = line[start..].find(name) {
                    let abs = start + rel;
                    // word boundary check (byte-safe via char slices)
                    let before_ok = abs == 0
                        || !line[..abs]
                            .chars()
                            .last()
                            .map(|c| c.is_alphanumeric() || c == '_')
                            .unwrap_or(false);
                    let after = abs + name.len();
                    let after_ok = after >= line.len()
                        || !line[after..]
                            .chars()
                            .next()
                            .map(|c| c.is_alphanumeric() || c == '_')
                            .unwrap_or(false);
                    if before_ok && after_ok {
                        // utf16 columns
                        let col = line[..abs].chars().map(|c| c.len_utf16() as u32).sum();
                        let end_col = col + name.chars().map(|c| c.len_utf16() as u32).sum::<u32>();
                        out.push(ReferenceLoc {
                            uri: uri.clone(),
                            line: (line_idx as u32) + 1,
                            character: col,
                            end_character: end_col,
                        });
                    }
                    start = abs + name.len();
                }
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
    for item in &program.items {
        if let Item::Function(f) = item {
            let (line, character) = file
                .map(|sf| {
                    let lc = sf.line_col(f.name.span.start);
                    (lc.line, lc.column.saturating_sub(1))
                })
                .unwrap_or((1, 0));
            symbols.push(WorkspaceSymbol {
                name: f.name.name.clone(),
                kind: "function".into(),
                uri: uri.to_string(),
                line,
                character,
                is_pub: f.is_pub,
                module: module.to_string(),
            });
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
