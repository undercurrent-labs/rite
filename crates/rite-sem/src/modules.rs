//! Module loading, cycle detection, and pub export merging.

use crate::resolve::{resolve, FunctionMeta, ResolvedProgram};
use rite_core::{
    simple_error, Diagnostics, FileId, SourceFile, SourceMap, Span, E022_DUPLICATE_BINDING,
    E024_IMPORT_CYCLE, E025_PRIVATE_IMPORT, E026_MODULE_NOT_FOUND,
};
use rite_syntax::{parse_file, ImportDecl, Item, Program};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Rewrites a copied module function's bare references to that module's own
/// top-level functions into their mangled `qualifier__name` spellings.
///
/// The merge used to leave copied bodies untouched, so a copy of
/// `helper.outer` still called `inner` by its bare name and depended on the
/// entry's flat scope holding it. Two failures came from that: a private
/// sibling was never injected at all, so `helper.outer` calling a private
/// `inner` was E020 in every importer; and an entry-file binding named
/// `inner` replaced the injected function, failing later with
/// `cannot call value of type int` at a call site in another module. After
/// the rewrite, a module's own names resolve within that module no matter
/// what its importers declare.
///
/// A locally bound name is never rewritten, so a parameter or binding named
/// like a sibling function keeps shadowing it inside the module, the same as
/// before the copy. The scope rules mirror `Resolver`: block params bind,
/// nested `def`s pre-declare in their block, a binding's pattern binds after
/// its value is walked, and match-arm patterns bind over the guard and body.
struct InternalRefRewriter<'a> {
    qualifier: &'a str,
    module_fns: &'a HashSet<String>,
    scopes: Vec<HashSet<String>>,
}

impl<'a> InternalRefRewriter<'a> {
    fn new(qualifier: &'a str, module_fns: &'a HashSet<String>) -> Self {
        Self {
            qualifier,
            module_fns,
            scopes: vec![HashSet::new()],
        }
    }

    fn bound(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn define(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn rewrite_fn(&mut self, f: &mut rite_syntax::FunctionDecl) {
        self.scopes.push(HashSet::new());
        for p in &f.params {
            self.define(&p.name.name);
        }
        self.rewrite_block_body(&mut f.body);
        self.scopes.pop();
    }

    fn rewrite_block(&mut self, block: &mut rite_syntax::Block) {
        self.scopes.push(HashSet::new());
        for p in &block.params {
            self.define(&p.name.name);
        }
        self.rewrite_block_body(block);
        self.scopes.pop();
    }

    /// The body walk minus the scope push, for callers that bind their own
    /// parameters first (function decls, routes, mcp declarations).
    fn rewrite_block_body(&mut self, block: &mut rite_syntax::Block) {
        // Nested `def`s bind in the enclosing block, visible to earlier
        // statements — same pre-declaration the resolver does.
        for item in &block.body {
            if let Item::Function(f) = item {
                self.define(&f.name.name);
            }
        }
        for item in &mut block.body {
            self.rewrite_item(item);
        }
    }

    fn rewrite_item(&mut self, item: &mut Item) {
        match item {
            Item::Function(f) => self.rewrite_fn(f),
            Item::Statement(s) => self.rewrite_stmt(s),
            Item::Test(t) => self.rewrite_block(&mut t.body),
            Item::Event(e) => self.rewrite_block(&mut e.body),
            Item::Data(_) | Item::Import(_) => {}
        }
    }

    fn rewrite_stmt(&mut self, stmt: &mut rite_syntax::Stmt) {
        use rite_syntax::{Stmt, SugarForm};
        match stmt {
            Stmt::Binding(b) => {
                self.rewrite_expr(&mut b.value);
                let mut names = Vec::new();
                pattern_names(&b.pattern, &mut names);
                for n in names {
                    self.define(&n);
                }
            }
            Stmt::Assign(a) => self.rewrite_expr(&mut a.value),
            Stmt::Expr(e) => self.rewrite_expr(e),
            Stmt::Return(r) => {
                if let Some(v) = &mut r.value {
                    self.rewrite_expr(v);
                }
            }
            Stmt::Sugared(s) => {
                // `lowered` is the semantic truth; the source `form` is only
                // printed by the formatter, which never sees injected copies.
                // Both are rewritten so they cannot disagree.
                match &mut s.form {
                    SugarForm::Say { value } => self.rewrite_expr(value),
                    SugarForm::Unless {
                        condition,
                        then_branch,
                        else_branch,
                    } => {
                        self.rewrite_expr(condition);
                        self.rewrite_block(then_branch);
                        if let Some(b) = else_branch {
                            self.rewrite_block(b);
                        }
                    }
                    SugarForm::ForIn { var, iter, body } => {
                        self.rewrite_expr(iter);
                        self.scopes.push(HashSet::new());
                        let var = var.name.clone();
                        self.define(&var);
                        self.rewrite_block_body(body);
                        self.scopes.pop();
                    }
                    SugarForm::While { condition, body } => {
                        self.rewrite_expr(condition);
                        self.rewrite_block(body);
                    }
                    SugarForm::Loop { count, body } => {
                        self.rewrite_expr(count);
                        self.rewrite_block(body);
                    }
                    SugarForm::Break | SugarForm::Continue => {}
                }
                self.rewrite_stmt(&mut s.lowered);
            }
        }
    }

    fn rewrite_expr(&mut self, expr: &mut rite_syntax::Expr) {
        use rite_syntax::Expr;
        match expr {
            Expr::Ident(i) => {
                if !i.name.starts_with("__")
                    && !self.bound(&i.name)
                    && self.module_fns.contains(&i.name)
                {
                    i.name = format!("{}__{}", self.qualifier, i.name);
                }
            }
            Expr::List(l) => {
                for e in &mut l.elements {
                    self.rewrite_expr(e);
                }
            }
            Expr::Record(r) => {
                for entry in &mut r.entries {
                    self.rewrite_expr(&mut entry.value);
                }
            }
            Expr::Binary(b) => {
                self.rewrite_expr(&mut b.left);
                self.rewrite_expr(&mut b.right);
            }
            Expr::Unary(u) => self.rewrite_expr(&mut u.expr),
            Expr::Call(c) => {
                self.rewrite_expr(&mut c.callee);
                for a in &mut c.args {
                    self.rewrite_expr(a);
                }
            }
            Expr::Member(m) => self.rewrite_expr(&mut m.object),
            Expr::Index(i) => {
                self.rewrite_expr(&mut i.object);
                self.rewrite_expr(&mut i.index);
            }
            Expr::Pipeline(p) => {
                self.rewrite_expr(&mut p.input);
                for s in &mut p.stages {
                    self.rewrite_expr(s);
                }
            }
            Expr::If(i) => {
                self.rewrite_expr(&mut i.condition);
                self.rewrite_block(&mut i.then_branch);
                if let Some(b) = &mut i.else_branch {
                    self.rewrite_block(b);
                }
            }
            Expr::Match(m) => {
                self.rewrite_expr(&mut m.scrutinee);
                for arm in &mut m.arms {
                    self.scopes.push(HashSet::new());
                    let mut names = Vec::new();
                    pattern_names(&arm.pattern, &mut names);
                    for n in names {
                        self.define(&n);
                    }
                    if let Some(g) = &mut arm.guard {
                        self.rewrite_expr(g);
                    }
                    self.rewrite_expr(&mut arm.body);
                    self.scopes.pop();
                }
            }
            Expr::Block(b) => self.rewrite_block(b),
            Expr::Try(t) => self.rewrite_expr(&mut t.expr),
            Expr::Group(g) => self.rewrite_expr(&mut g.expr),
            Expr::Coalesce(c) => {
                self.rewrite_expr(&mut c.left);
                self.rewrite_expr(&mut c.right);
            }
            Expr::HttpListen(h) => {
                self.rewrite_expr(&mut h.addr);
                self.rewrite_block(&mut h.body);
            }
            Expr::Route(r) => {
                self.scopes.push(HashSet::new());
                for p in &r.params {
                    self.define(&p.name.name);
                }
                self.rewrite_block_body(&mut r.body);
                self.scopes.pop();
            }
            Expr::McpServe(m) => {
                self.rewrite_expr(&mut m.config);
                self.rewrite_block(&mut m.body);
            }
            Expr::McpDecl(d) => {
                self.scopes.push(HashSet::new());
                for p in &d.params {
                    self.define(&p.name.name);
                }
                self.rewrite_block_body(&mut d.body);
                self.scopes.pop();
            }
            Expr::Literal(_) | Expr::Atom(_) | Expr::Capability(_) | Expr::Placeholder(_) => {}
        }
    }
}

/// Every name a pattern binds, in source order.
fn pattern_names(pattern: &rite_syntax::Pattern, out: &mut Vec<String>) {
    use rite_syntax::Pattern;
    match pattern {
        Pattern::Ident(i) => out.push(i.name.clone()),
        Pattern::List(l) => {
            for p in &l.elements {
                pattern_names(p, out);
            }
            if let Some(rest) = &l.rest {
                pattern_names(rest, out);
            }
        }
        Pattern::Record(r) => {
            for f in &r.fields {
                match &f.pattern {
                    Some(p) => pattern_names(p, out),
                    None => out.push(f.name.name.clone()),
                }
            }
        }
        Pattern::Result(r) => {
            if let Some(p) = &r.binding {
                pattern_names(p, out);
            }
        }
        Pattern::Or(o) => {
            // Every alternative must bind the same names (checked in resolve),
            // so the first is as good as any.
            if let Some(first) = o.alternatives.first() {
                pattern_names(first, out);
            }
        }
        Pattern::Atom(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => {}
    }
}

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
    /// In-memory modules, keyed by dotted module name (`coolio`, `lib.helpers`).
    /// Consulted before the filesystem, which is what lets a browser run — no
    /// filesystem at all — resolve `use`. Relative imports drop their `./`
    /// prefix for the lookup, since an overlay has no directories.
    virtual_files: HashMap<String, String>,
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
            virtual_files: HashMap::new(),
        }
    }

    pub fn with_virtual_files(mut self, files: HashMap<String, String>) -> Self {
        self.virtual_files = files;
        self
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
            // In-memory modules win over the filesystem: a browser run has no
            // filesystem, and an embedder that supplies both meant the overlay.
            // Relative prefixes drop out of the key — an overlay has no
            // directories, so `use ./helpers` finds `files["helpers"]`.
            let overlay_key = segs
                .iter()
                .filter(|s| s.as_str() != "." && s.as_str() != "..")
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(".");
            let (path, text) = match self.virtual_files.get(&overlay_key).cloned() {
                Some(text) => (PathBuf::from(format!("{overlay_key}.rite")), text),
                None => {
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
                            continue;
                        }
                    };
                    (path, text)
                }
            };
            self.visiting.push(key.clone());
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
                                    effectful: false,
                                    declares_effect: f.is_effectful,
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
///
/// Returns the merged qualifiers and the names of every function this merge
/// injected, for `resolve_with_qualifiers`: a copied body's `i.double` refers
/// to an import the entry does not have, so the resolver and desugar must be
/// told about it or the copy fails as an undefined name. The injected-function
/// set is what scopes that knowledge — a module's qualifiers work inside the
/// bodies copied out of modules and nowhere else, which is what keeps
/// `i.double` in the *entry* an undefined name.
pub fn merge_exports_into_entry(
    entry: &mut Program,
    graph: &ModuleGraph,
    diagnostics: &mut Diagnostics,
) -> MergedImports {
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

    let qualifiers: HashSet<String> = bindings
        .iter()
        .map(|(alias, key, _, _)| {
            alias
                .clone()
                .unwrap_or_else(|| key.rsplit('.').next().unwrap_or(key.as_str()).to_string())
        })
        .collect();
    let mut injected_functions: HashSet<String> = HashSet::new();
    let mut private_injected: HashSet<String> = HashSet::new();

    // Which module put each unqualified name in scope, so a clash can name both.
    let mut flat_origin: HashMap<String, String> = HashMap::new();

    let mut inject = |entry: &mut Program, mut f: rite_syntax::FunctionDecl, keep_pub: bool| {
        if !keep_pub {
            f.is_pub = false;
        }
        injected_functions.insert(f.name.name.clone());
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

        // Every copied body gets its intra-module references rewritten to the
        // mangled spelling, so a copy resolves its own siblings no matter what
        // the entry declares. See [`InternalRefRewriter`].
        let module_fns: HashSet<String> = mod_ast
            .program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Function(f) => Some(f.name.name.clone()),
                _ => None,
            })
            .collect();

        for item in &mod_ast.program.items {
            let Item::Function(f) = item else { continue };

            let mangled_name = format!("{}__{}", qualifier, f.name.name);
            let mut qualified = f.clone();
            qualified.name.name = mangled_name.clone();
            InternalRefRewriter::new(&qualifier, &module_fns).rewrite_fn(&mut qualified);
            // A private function is injected under the mangled name alone: its
            // public siblings call it, nothing else may. Before this it was not
            // injected at all, and a module whose export called a private helper
            // was E020 `undefined name` in every importer, reported at an
            // unrelated span in the entry.
            inject(entry, qualified, false);

            if !f.is_pub {
                private_injected.insert(mangled_name);
                continue;
            }

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
                    None if crate::resolve::BUILTIN_NAMES.contains(&f.name.name.as_str()) => {
                        // An export named after a builtin would replace that
                        // builtin at every bare call site in the entry,
                        // including ones written before the `use`. The
                        // qualified copy is already injected, so the module
                        // stays usable — only the bare name is refused.
                        diagnostics.push(
                            simple_error(
                                E022_DUPLICATE_BINDING,
                                format!(
                                    "`{}` exported by `{}` shadows the builtin of the same name",
                                    f.name.name, key
                                ),
                                entry.file,
                                f.name.span,
                                "importing this unqualified would replace the builtin",
                            )
                            .with_help(format!(
                                "import it as `use {} as …` and call it qualified, or rename \
                                 the export — the reserved names are listed in the builtin \
                                 reference (docs/generated/builtins.md)",
                                key
                            )),
                        );
                    }
                    None => {
                        flat_origin.insert(f.name.name.clone(), key.clone());
                        let mut bare = f.clone();
                        InternalRefRewriter::new(&qualifier, &module_fns).rewrite_fn(&mut bare);
                        inject(entry, bare, *is_pub_reexport);
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

    MergedImports {
        qualifiers,
        injected_functions,
        private_injected,
        injected_origin: flat_origin,
    }
}

/// What [`merge_exports_into_entry`] added to the entry: the import qualifiers
/// the copied bodies rely on, and the names of the copies themselves.
pub struct MergedImports {
    pub qualifiers: HashSet<String>,
    pub injected_functions: HashSet<String>,
    /// Mangled names of injected private-function copies — reachable from the
    /// module's own rewritten bodies, refused as qualified access.
    pub private_injected: HashSet<String>,
    /// Which module supplied each injected *bare* name, for diagnostics.
    pub injected_origin: HashMap<String, String>,
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
