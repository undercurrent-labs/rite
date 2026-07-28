//! Module loading, cycle detection, and pub export merging.

use crate::resolve::{resolve, FunctionMeta, ResolvedProgram};
use rite_core::{
    simple_error, Diagnostics, FileId, SourceFile, SourceMap, Span, E024_IMPORT_CYCLE,
    E025_PRIVATE_IMPORT, E026_MODULE_NOT_FOUND,
};
use rite_syntax::{parse_file, ImportDecl, Item, Program};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub name: String,
    pub path: PathBuf,
    pub program: Program,
    pub resolved: ResolvedProgram,
    pub exports: HashMap<String, FunctionMeta>,
}

#[derive(Debug, Default)]
pub struct ModuleGraph {
    pub modules: HashMap<String, LoadedModule>,
    pub load_order: Vec<String>,
}

/// Resolve a module path like `tools.math` relative to `from_dir` and optional `roots`.
pub fn resolve_module_path(
    module_path: &[String],
    from_dir: &Path,
    roots: &[PathBuf],
) -> Option<PathBuf> {
    let rel = module_path.join("/");
    let candidates = |base: &Path| -> Vec<PathBuf> {
        vec![
            base.join(format!("{}.rite", rel)),
            base.join(&rel).join("mod.rite"),
        ]
    };
    for c in candidates(from_dir) {
        if c.is_file() {
            return Some(c);
        }
    }
    for root in roots {
        for c in candidates(root) {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    // also try CWD
    if let Ok(cwd) = std::env::current_dir() {
        for c in candidates(&cwd) {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

pub struct ModuleLoader<'a> {
    pub roots: Vec<PathBuf>,
    pub sources: &'a mut SourceMap,
    pub diagnostics: Diagnostics,
    visiting: Vec<String>,
    visited: HashSet<String>,
    graph: ModuleGraph,
}

impl<'a> ModuleLoader<'a> {
    pub fn new(sources: &'a mut SourceMap, roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            sources,
            diagnostics: Diagnostics::new(),
            visiting: Vec::new(),
            visited: HashSet::new(),
            graph: ModuleGraph::default(),
        }
    }

    pub fn into_graph(self) -> (ModuleGraph, Diagnostics) {
        (self.graph, self.diagnostics)
    }

    /// Load the entry file and all transitive imports.
    pub fn load_entry(&mut self, entry: &SourceFile, entry_path: Option<&Path>) -> Option<Program> {
        let from_dir = entry_path
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let mut ast = {
            let (ast, diags) = parse_file(entry);
            self.diagnostics.extend(diags.into_vec());
            ast?
        };
        self.load_imports_of(&mut ast, &from_dir, entry.id);
        Some(ast)
    }

    fn load_imports_of(&mut self, program: &mut Program, from_dir: &Path, file: FileId) {
        let imports: Vec<ImportDecl> = program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Import(imp) => Some(imp.clone()),
                _ => None,
            })
            .collect();

        for imp in imports {
            let segs: Vec<String> = imp.path.segments.iter().map(|s| s.name.clone()).collect();
            let key = segs.join(".");
            if self.visiting.contains(&key) {
                let chain = self
                    .visiting
                    .iter()
                    .chain(std::iter::once(&key))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" → ");
                self.diagnostics.push(
                    simple_error(
                        E024_IMPORT_CYCLE,
                        format!("circular import involving `{}`", key),
                        file,
                        imp.span,
                        format!("import chain: {}", chain),
                    )
                    .with_help("break the cycle by moving shared code to a third module"),
                );
                continue;
            }
            if self.visited.contains(&key) {
                continue;
            }
            let path = match resolve_module_path(&segs, from_dir, &self.roots) {
                Some(p) => p,
                None => {
                    self.diagnostics.push(simple_error(
                        E026_MODULE_NOT_FOUND,
                        format!("module `{}` not found", key),
                        file,
                        imp.span,
                        "looked for path.rite and path/mod.rite",
                    ));
                    continue;
                }
            };
            self.visiting.push(key.clone());
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    self.diagnostics.push(simple_error(
                        E026_MODULE_NOT_FOUND,
                        format!("failed to read module `{}`: {}", key, e),
                        file,
                        imp.span,
                        path.display().to_string(),
                    ));
                    self.visiting.pop();
                    continue;
                }
            };
            let id = self.sources.add_file(path.display().to_string(), text);
            let sf = self.sources.get(id).unwrap().clone();
            let (mut child_ast, pdiags) = parse_file(&sf);
            self.diagnostics.extend(pdiags.into_vec());
            if let Some(ref mut child) = child_ast {
                let child_dir = path.parent().unwrap_or(from_dir);
                self.load_imports_of(child, child_dir, id);
                let (resolved, rdiags) = resolve(child, &sf);
                self.diagnostics.extend(rdiags.into_vec());
                let mut exports = HashMap::new();
                for (name, meta) in &resolved.functions {
                    if meta.is_pub {
                        exports.insert(name.clone(), meta.clone());
                    }
                }
                // also scan AST for pub functions not in resolved map edge cases
                for item in &child.items {
                    if let Item::Function(f) = item {
                        if f.is_pub {
                            exports.insert(
                                f.name.name.clone(),
                                FunctionMeta {
                                    name: f.name.name.clone(),
                                    arity: f.params.len(),
                                    is_pub: true,
                                    span: f.span,
                                },
                            );
                        }
                    }
                }
                self.graph.modules.insert(
                    key.clone(),
                    LoadedModule {
                        name: key.clone(),
                        path: path.clone(),
                        program: child.clone(),
                        resolved,
                        exports,
                    },
                );
                self.graph.load_order.push(key.clone());
            }
            self.visiting.pop();
            self.visited.insert(key);
        }
    }
}

/// Merge loaded modules' public functions into the entry program AST (as non-pub copies for execution).
pub fn merge_exports_into_entry(
    entry: &mut Program,
    graph: &ModuleGraph,
    diagnostics: &mut Diagnostics,
) {
    // Build import alias map from entry
    let mut bindings: Vec<(Option<String>, String, HashMap<String, FunctionMeta>)> = Vec::new();
    for item in &entry.items {
        if let Item::Import(imp) = item {
            let key = imp
                .path
                .segments
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(".");
            let alias = imp.alias.as_ref().map(|a| a.name.clone());
            if let Some(m) = graph.modules.get(&key) {
                bindings.push((alias, key, m.exports.clone()));
            }
        }
    }

    // Inject pub functions from modules that were imported without alias into entry scope
    // by appending function decls from loaded modules (only pub ones).
    for (alias, key, exports) in &bindings {
        if let Some(mod_ast) = graph.modules.get(key) {
            for item in &mod_ast.program.items {
                if let Item::Function(f) = item {
                    if !f.is_pub {
                        // If someone tries to reference private later, resolver will catch
                        continue;
                    }
                    if alias.is_none() {
                        // bring into scope under original name
                        let f2 = f.clone();
                        // keep as accessible (treat as local after import)
                        entry.items.insert(0, Item::Function(f2));
                    }
                    let _ = exports;
                }
            }
        }
        // alias form: we still inject as `alias_name` wrappers later at desugar
        if alias.is_some() {
            // For aliased imports, inject with prefixed names `__mod_alias_fn`
            if let Some(mod_ast) = graph.modules.get(key) {
                let prefix = alias.as_ref().unwrap();
                for item in &mod_ast.program.items {
                    if let Item::Function(f) = item {
                        if f.is_pub {
                            let mut f2 = f.clone();
                            f2.name.name = format!("{}__{}", prefix, f.name.name);
                            f2.is_pub = false;
                            entry.items.insert(0, Item::Function(f2));
                        }
                    }
                }
            }
        }
    }

    // Validate no private re-export attempt is needed — warn if import of empty exports
    for (alias, key, exports) in &bindings {
        if exports.is_empty() {
            diagnostics.push(simple_error(
                E025_PRIVATE_IMPORT,
                format!("module `{}` exports no public declarations", key),
                entry.file,
                Span::DUMMY,
                "mark exports with `pub ◆`",
            ));
        }
        let _ = alias;
    }
}

/// Expand a multi-module program into one ProgramIr by compiling entry after merge.
pub fn collect_module_function_irs(graph: &ModuleGraph) -> Vec<(String, Program)> {
    graph
        .load_order
        .iter()
        .filter_map(|k| graph.modules.get(k).map(|m| (k.clone(), m.program.clone())))
        .collect()
}
