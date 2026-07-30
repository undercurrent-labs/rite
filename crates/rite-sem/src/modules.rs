//! Module loading, cycle detection, and pub export merging.

use crate::resolve::{resolve, FunctionMeta, ResolvedProgram};
use rite_core::{
    simple_error, Diagnostics, FileId, SourceFile, SourceMap, Span, E022_DUPLICATE_BINDING,
    E024_IMPORT_CYCLE, E025_PRIVATE_IMPORT, E026_MODULE_NOT_FOUND,
};
use rite_syntax::{parse_file, ImportDecl, Item, Program};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One resolved `use` in the entry module: (alias, module name, its exports, is_pub).
type ImportBinding = (Option<String>, String, HashMap<String, FunctionMeta>, bool);

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

/// Resolve a module path like `tools.math` or `./helpers` relative to `from_dir` and optional `roots`.
pub fn resolve_module_path(
    module_path: &[String],
    from_dir: &Path,
    roots: &[PathBuf],
) -> Option<PathBuf> {
    // Relative: `.` / `..` prefix from `use ./foo` / `use ../lib/bar`
    if module_path.first().map(|s| s.as_str()) == Some(".")
        || module_path.first().map(|s| s.as_str()) == Some("..")
    {
        let mut base = from_dir.to_path_buf();
        let mut i = 0;
        while i < module_path.len() {
            match module_path[i].as_str() {
                "." => {
                    i += 1;
                }
                ".." => {
                    if let Some(parent) = base.parent() {
                        base = parent.to_path_buf();
                    }
                    i += 1;
                }
                _ => break,
            }
        }
        let rest = module_path[i..].join("/");
        if rest.is_empty() {
            return None;
        }
        let candidates = [
            base.join(format!("{}.rite", rest)),
            base.join(&rest).join("mod.rite"),
        ];
        for c in candidates {
            if c.is_file() {
                return Some(c);
            }
        }
        return None;
    }

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
                        format!(
                            "looked for {}.rite and {}/mod.rite under {}",
                            segs.join("/"),
                            segs.join("/"),
                            from_dir.display()
                        ),
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
                // A module is resolved here only to learn what it exports, but that
                // resolution still reports undefined names — and its own imports were
                // not in scope, so any call into another module was reported as
                // undefined and the graph could never be more than one level deep.
                // Loading above is depth-first, so this module's dependencies are in
                // the graph already and can be brought into its scope first.
                inject_dependencies(child, &self.graph);
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
                // `pub use other` re-exports: pull already-loaded module exports
                for item in &child.items {
                    if let Item::Import(imp) = item {
                        if !imp.is_pub {
                            continue;
                        }
                        let dep_key = imp
                            .path
                            .segments
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join(".");
                        if let Some(dep) = self.graph.modules.get(&dep_key) {
                            for (n, meta) in &dep.exports {
                                exports.insert(n.clone(), meta.clone());
                            }
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

/// Bring a module's own imports into its scope, as private names.
///
/// Mirrors what `merge_exports_into_entry` does for the entry, so a module that
/// `use`s another resolves against the same names it will be evaluated with.
/// Everything injected is private: a module's imports are its own business and
/// must not leak into what it exports.
fn inject_dependencies(program: &mut Program, graph: &ModuleGraph) {
    let imports: Vec<(Option<String>, String)> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Import(imp) => Some((
                imp.alias.as_ref().map(|a| a.name.clone()),
                imp.path
                    .segments
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join("."),
            )),
            _ => None,
        })
        .collect();

    for (alias, key) in imports {
        let Some(dep) = graph.modules.get(&key) else {
            continue;
        };
        let qualifier = alias
            .clone()
            .unwrap_or_else(|| key.rsplit('.').next().unwrap_or(key.as_str()).to_string());
        for item in &dep.program.items {
            let Item::Function(f) = item else { continue };
            if !f.is_pub {
                continue;
            }
            let mut qualified = f.clone();
            qualified.name.name = format!("{}__{}", qualifier, f.name.name);
            qualified.is_pub = false;
            program.items.insert(0, Item::Function(qualified));

            if alias.is_none() {
                let clash = program
                    .items
                    .iter()
                    .any(|i| matches!(i, Item::Function(g) if g.name.name == f.name.name));
                if !clash {
                    let mut flat = f.clone();
                    flat.is_pub = false;
                    program.items.insert(0, Item::Function(flat));
                }
            }
        }
    }
}

/// Merge loaded modules' public functions into the entry program AST (as non-pub
/// copies for execution), and bind a qualifier for each import.
pub fn merge_exports_into_entry(
    entry: &mut Program,
    graph: &ModuleGraph,
    diagnostics: &mut Diagnostics,
) {
    // Build import alias map from the entry.
    let mut bindings: Vec<ImportBinding> = Vec::new();
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
                bindings.push((alias, key, m.exports.clone(), imp.is_pub));
            }
        }
    }

    // …and from every module's own imports, so a module can use another module.
    //
    // Merging copies each module's public functions into the entry's single flat
    // scope. Only the entry's imports were collected before, so a function copied
    // out of `mid.rite` referred to names from `mid`'s own `use` that had never
    // been brought along — leaving the whole graph one level deep: an entry plus
    // leaves that could not share anything. Collecting the modules' imports too
    // brings those names into the same scope, which is where the copied bodies
    // look for them.
    for module in graph.modules.values() {
        for item in &module.program.items {
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
                    // A module's import is private to it: never re-exported.
                    bindings.push((alias, key, m.exports.clone(), false));
                }
            }
        }
    }

    // Two modules may bring in the same module, so inject each one once.
    let mut seen_bindings: HashSet<(Option<String>, String)> = HashSet::new();
    bindings.retain(|(alias, key, _, _)| seen_bindings.insert((alias.clone(), key.clone())));

    // Which module put each unqualified name in scope, so a clash can name both.
    let mut flat_origin: HashMap<String, String> = HashMap::new();

    let inject = |entry: &mut Program, mut f: rite_syntax::FunctionDecl, keep_pub: bool| {
        if !keep_pub {
            f.is_pub = false;
        }
        entry.items.insert(0, Item::Function(f));
    };

    for (alias, key, exports, is_pub_reexport) in &bindings {
        let Some(mod_ast) = graph.modules.get(key) else {
            continue;
        };

        // Every import binds a qualifier — `use math` gives `math.square`, and
        // `use math as m` gives `m.square`. Qualifying is how two modules that
        // export the same name stay usable, so it cannot require an alias.
        let qualifier = alias
            .clone()
            .unwrap_or_else(|| key.rsplit('.').next().unwrap_or(key.as_str()).to_string());

        for item in &mod_ast.program.items {
            let Item::Function(f) = item else { continue };
            if !f.is_pub {
                continue;
            }

            let mut qualified = f.clone();
            qualified.name.name = format!("{}__{}", qualifier, f.name.name);
            inject(entry, qualified, false);

            // The unqualified name is only injected for a plain `use`; an alias
            // deliberately keeps the module's names behind its qualifier.
            if alias.is_none() {
                match flat_origin.get(&f.name.name) {
                    Some(first) if first != key => {
                        diagnostics.push(
                            simple_error(
                                E022_DUPLICATE_BINDING,
                                format!(
                                    "`{}` is exported by both `{}` and `{}`",
                                    f.name.name, first, key
                                ),
                                entry.file,
                                f.name.span,
                                "an unqualified call here would be ambiguous",
                            )
                            .with_help(format!(
                                "call it as `{}.{}` or `{}.{}`, or import one with `use … as …`",
                                first, f.name.name, key, f.name.name
                            )),
                        );
                    }
                    Some(_) => {}
                    None => {
                        flat_origin.insert(f.name.name.clone(), key.clone());
                        inject(entry, f.clone(), *is_pub_reexport);
                    }
                }
            }
        }

        // Names that exist only in the exports map, from a nested `pub use`.
        for name in exports.keys() {
            let qualified_name = format!("{}__{}", qualifier, name);
            let have_qualified = entry
                .items
                .iter()
                .any(|i| matches!(i, Item::Function(f) if f.name.name == qualified_name));
            if !have_qualified {
                if let Some(f) = find_pub_function(graph, name) {
                    let mut q = f;
                    q.name.name = qualified_name;
                    inject(entry, q, false);
                }
            }
            if alias.is_none() && !flat_origin.contains_key(name) {
                let already = entry
                    .items
                    .iter()
                    .any(|i| matches!(i, Item::Function(f) if f.name.name == *name));
                if !already {
                    if let Some(f) = find_pub_function(graph, name) {
                        flat_origin.insert(name.clone(), key.clone());
                        inject(entry, f, *is_pub_reexport);
                    }
                }
            }
        }
    }

    // Validate no private re-export attempt is needed — warn if import of empty exports
    for (alias, key, exports, _) in &bindings {
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

fn find_pub_function(graph: &ModuleGraph, name: &str) -> Option<rite_syntax::FunctionDecl> {
    for m in graph.modules.values() {
        for item in &m.program.items {
            if let Item::Function(f) = item {
                if f.is_pub && f.name.name == name {
                    return Some(f.clone());
                }
            }
        }
    }
    None
}

/// Expand a multi-module program into one ProgramIr by compiling entry after merge.
pub fn collect_module_function_irs(graph: &ModuleGraph) -> Vec<(String, Program)> {
    graph
        .load_order
        .iter()
        .filter_map(|k| graph.modules.get(k).map(|m| (k.clone(), m.program.clone())))
        .collect()
}
