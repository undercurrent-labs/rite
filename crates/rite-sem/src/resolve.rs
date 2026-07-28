//! Lexical resolver: scopes, bindings, effect checks, imports.

use crate::ir::*;
use rite_core::{
    simple_error, Diagnostics, SourceFile, Span, E020_UNDEFINED_NAME, E021_EFFECT_REQUIRED,
    E022_DUPLICATE_BINDING, E023_IMMUTABLE_ASSIGN, E029_NON_EXHAUSTIVE_MATCH,
};
use rite_syntax::{
    BinOp, Binding, Block, EventDecl, Expr, FunctionDecl, Item, LitKind, Pattern, Program, Stmt,
    UnaryOp,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub ast: Program,
    pub functions: HashMap<String, FunctionMeta>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FunctionMeta {
    pub name: String,
    pub arity: usize,
    pub is_pub: bool,
    pub span: Span,
}

pub struct Resolver {
    scopes: Vec<Scope>,
    next_local: u32,
    diagnostics: Diagnostics,
    functions: HashMap<String, FunctionMeta>,
    /// Names known as effectful when called without !
    effectful_caps: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct Scope {
    bindings: HashMap<String, BindingInfo>,
}

#[derive(Debug, Clone)]
struct BindingInfo {
    #[allow(dead_code)]
    local: LocalId,
    mutable: bool,
    #[allow(dead_code)]
    span: Span,
}

pub fn resolve(program: &Program, _file: &SourceFile) -> (ResolvedProgram, Diagnostics) {
    let mut r = Resolver::new();
    r.resolve_program(program);
    let resolved = ResolvedProgram {
        ast: program.clone(),
        functions: r.functions.clone(),
        warnings: vec![],
    };
    (resolved, r.diagnostics)
}

impl Resolver {
    pub fn new() -> Self {
        let mut r = Self {
            scopes: vec![Scope::default()],
            next_local: 0,
            diagnostics: Diagnostics::new(),
            functions: HashMap::new(),
            effectful_caps: vec![
                "console.print".into(),
                "console.println".into(),
                "console.warn".into(),
                "console.error".into(),
                "console.inspect".into(),
                "console.read_line".into(),
                "fs.write".into(),
                "fs.append".into(),
                "fs.remove".into(),
                "fs.mkdir".into(),
                "fs.copy".into(),
                "fs.move".into(),
                "json.write".into(),
                // clock.now / random.* may appear in pure contexts in demos; still effectful when called.
                // Listed so missing `!` is diagnosed (E021).
                "clock.now".into(),
                "clock.sleep".into(),
                "process.run".into(),
                "http.listen".into(),
                "random.int".into(),
                "random.float".into(),
                "random.choose".into(),
                "random.shuffle".into(),
                "random.uuid".into(),
                "random.seed".into(),
            ],
        };
        // Predefine pure builtins
        for name in [
            "map",
            "keep",
            "reject",
            "reduce",
            "each",
            "flatten",
            "count",
            "first",
            "last",
            "find",
            "any",
            "all",
            "sum",
            "min",
            "max",
            "sort",
            "unique",
            "zip",
            "chunk",
            "parallel",
            "ok",
            "err",
            "panic",
            "expect",
            "fail",
            "str",
            "len",
            "type_of",
            "require",
            "collect_results",
            "group",
            "lines",
            "number?",
            "range",
        ] {
            r.functions.insert(
                name.into(),
                FunctionMeta {
                    name: name.into(),
                    arity: 0,
                    is_pub: true,
                    span: Span::DUMMY,
                },
            );
        }
        r
    }

    fn resolve_program(&mut self, program: &Program) {
        // First pass: collect function declarations
        for item in &program.items {
            if let Item::Function(f) = item {
                if self.functions.contains_key(&f.name.name) {
                    self.diagnostics.push(simple_error(
                        E022_DUPLICATE_BINDING,
                        format!("duplicate function `{}`", f.name.name),
                        program.file,
                        f.name.span,
                        "already defined",
                    ));
                }
                self.functions.insert(
                    f.name.name.clone(),
                    FunctionMeta {
                        name: f.name.name.clone(),
                        arity: f.params.len(),
                        is_pub: f.is_pub,
                        span: f.span,
                    },
                );
            }
        }
        // Second pass: walk for effect checks and undefined names
        for item in &program.items {
            self.resolve_item(item, program.file);
        }
    }

    fn resolve_item(&mut self, item: &Item, file: rite_core::FileId) {
        match item {
            Item::Function(f) => self.resolve_function(f, file),
            Item::Statement(s) => self.resolve_stmt(s, file),
            Item::Test(t) => {
                self.push_scope();
                self.resolve_block(&t.body, file);
                self.pop_scope();
            }
            Item::Event(e) => self.resolve_event(e, file),
            Item::Import(_) => {}
            Item::Data(_) => {}
        }
    }

    fn resolve_function(&mut self, f: &FunctionDecl, file: rite_core::FileId) {
        self.push_scope();
        for p in &f.params {
            self.define(&p.name.name, false, p.span, file);
        }
        self.resolve_block(&f.body, file);
        self.pop_scope();
    }

    fn resolve_event(&mut self, e: &EventDecl, file: rite_core::FileId) {
        self.push_scope();
        self.resolve_block(&e.body, file);
        self.pop_scope();
    }

    fn resolve_block(&mut self, block: &Block, file: rite_core::FileId) {
        self.push_scope();
        for p in &block.params {
            self.define(&p.name.name, false, p.span, file);
        }
        for item in &block.body {
            self.resolve_item(item, file);
        }
        self.pop_scope();
    }

    fn resolve_stmt(&mut self, stmt: &Stmt, file: rite_core::FileId) {
        match stmt {
            Stmt::Binding(b) => {
                self.resolve_expr(&b.value, file, false);
                self.define_pattern(&b.pattern, b.mutable, file);
            }
            Stmt::Assign(a) => {
                if let Some(info) = self.lookup(&a.name.name) {
                    if !info.mutable {
                        self.diagnostics.push(simple_error(
                            E023_IMMUTABLE_ASSIGN,
                            format!("cannot assign to immutable binding `{}`", a.name.name),
                            file,
                            a.span,
                            "use ↢ / <~ for mutable bindings",
                        ));
                    }
                } else {
                    self.diagnostics.push(simple_error(
                        E020_UNDEFINED_NAME,
                        format!("undefined name `{}`", a.name.name),
                        file,
                        a.name.span,
                        "not found in scope",
                    ));
                }
                self.resolve_expr(&a.value, file, false);
            }
            Stmt::Expr(e) => {
                self.resolve_expr(e, file, false);
            }
            Stmt::Return(r) => {
                if let Some(v) = &r.value {
                    self.resolve_expr(v, file, false);
                }
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr, file: rite_core::FileId, in_effect: bool) {
        match expr {
            Expr::Ident(i) => {
                if i.name != "_"
                    && !i.name.starts_with("__") // internal desugar symbols
                    && self.lookup(&i.name).is_none()
                    && !self.functions.contains_key(&i.name)
                    && !is_builtin_name(&i.name)
                {
                    // Strict undefined-name check when statically knowable
                    self.diagnostics.push(simple_error(
                        E020_UNDEFINED_NAME,
                        format!("undefined name `{}`", i.name),
                        file,
                        i.span,
                        "not found in scope",
                    ));
                }
            }
            Expr::Capability(c) => {
                let path = c.path.join(".");
                if self
                    .effectful_caps
                    .iter()
                    .any(|e| e == &path || path.starts_with(e))
                    && !in_effect
                {
                    // Will be checked on call site
                }
            }
            Expr::Call(c) => {
                let mut effect = in_effect;
                if let Expr::Unary(u) = c.callee.as_ref() {
                    if u.op == UnaryOp::Effect {
                        effect = true;
                    }
                }
                // effectful capability call without !
                if let Expr::Capability(cap) = strip_effect(c.callee.as_ref()) {
                    let path = cap.path.join(".");
                    if is_effectful(&path) && !effect && !has_effect_marker(c.callee.as_ref()) {
                        self.diagnostics.push(
                            simple_error(
                                E021_EFFECT_REQUIRED,
                                "effectful capability call requires `!`",
                                file,
                                c.span,
                                "this operation performs an external effect",
                            )
                            .with_help(format!(
                                "mark the operation as an explicit effect: ! @{}",
                                path
                            )),
                        );
                    }
                }
                self.resolve_expr(&c.callee, file, effect);
                for a in &c.args {
                    self.resolve_expr(a, file, false);
                }
            }
            Expr::Unary(u) => {
                let eff = u.op == UnaryOp::Effect || in_effect;
                if u.op == UnaryOp::Effect {
                    if let Expr::Call(c) = u.expr.as_ref() {
                        self.resolve_expr(&Expr::Call(c.clone()), file, true);
                        return;
                    }
                    if let Expr::Capability(_) = u.expr.as_ref() {
                        // bare capability ref with ! is ok
                    }
                }
                self.resolve_expr(&u.expr, file, eff);
            }
            Expr::Binary(b) => {
                self.resolve_expr(&b.left, file, false);
                self.resolve_expr(&b.right, file, false);
            }
            Expr::Pipeline(p) => {
                self.resolve_expr(&p.input, file, false);
                for s in &p.stages {
                    self.resolve_expr(s, file, false);
                }
            }
            Expr::If(i) => {
                self.resolve_expr(&i.condition, file, false);
                self.resolve_block(&i.then_branch, file);
                if let Some(e) = &i.else_branch {
                    self.resolve_block(e, file);
                }
            }
            Expr::Match(m) => {
                self.resolve_expr(&m.scrutinee, file, false);
                let mut has_wild = false;
                for arm in &m.arms {
                    if matches!(arm.pattern, Pattern::Wildcard(_)) {
                        has_wild = true;
                    }
                    self.push_scope();
                    self.define_pattern(&arm.pattern, false, file);
                    self.resolve_expr(&arm.body, file, false);
                    self.pop_scope();
                }
                if !has_wild && !m.arms.is_empty() {
                    // soft warning for atom-only matches
                    let all_atoms = m.arms.iter().all(|a| matches!(a.pattern, Pattern::Atom(_)));
                    if all_atoms {
                        self.diagnostics.push(
                            rite_core::Diagnostic::warning(
                                E029_NON_EXHAUSTIVE_MATCH,
                                "match has no wildcard arm",
                            )
                            .with_primary(
                                rite_core::SourceSpan::new(file, m.span),
                                "consider adding `_ → ...`",
                            ),
                        );
                    }
                }
            }
            Expr::Block(b) => self.resolve_block(b, file),
            Expr::List(l) => {
                for e in &l.elements {
                    self.resolve_expr(e, file, false);
                }
            }
            Expr::Record(r) => {
                for e in &r.entries {
                    self.resolve_expr(&e.value, file, false);
                }
            }
            Expr::Member(m) => self.resolve_expr(&m.object, file, false),
            Expr::Index(i) => {
                self.resolve_expr(&i.object, file, false);
                self.resolve_expr(&i.index, file, false);
            }
            Expr::Try(t) => self.resolve_expr(&t.expr, file, false),
            Expr::Coalesce(c) => {
                self.resolve_expr(&c.left, file, false);
                self.resolve_expr(&c.right, file, false);
            }
            Expr::HttpListen(h) => {
                self.resolve_expr(&h.addr, file, false);
                self.resolve_block(&h.body, file);
            }
            Expr::Route(r) => {
                self.push_scope();
                for p in &r.params {
                    self.define(&p.name.name, false, p.span, file);
                }
                self.resolve_block(&r.body, file);
                self.pop_scope();
            }
            Expr::Group(g) => self.resolve_expr(&g.expr, file, false),
            Expr::Literal(_) | Expr::Atom(_) | Expr::Placeholder(_) => {}
        }
    }

    fn define_pattern(&mut self, pat: &Pattern, mutable: bool, file: rite_core::FileId) {
        match pat {
            Pattern::Ident(i) => {
                self.define(&i.name, mutable, i.span, file);
            }
            Pattern::List(l) => {
                for e in &l.elements {
                    self.define_pattern(e, mutable, file);
                }
                if let Some(r) = &l.rest {
                    self.define_pattern(r, mutable, file);
                }
            }
            Pattern::Record(r) => {
                for f in &r.fields {
                    if let Some(p) = &f.pattern {
                        self.define_pattern(p, mutable, file);
                    } else {
                        self.define(&f.name.name, mutable, f.span, file);
                    }
                }
            }
            Pattern::Result(r) => {
                if let Some(b) = &r.binding {
                    self.define_pattern(b, mutable, file);
                }
            }
            Pattern::Typed(t) => self.define_pattern(&t.pattern, mutable, file),
            Pattern::Atom(_) | Pattern::Literal(_) | Pattern::Wildcard(_) => {}
        }
    }

    fn define(&mut self, name: &str, mutable: bool, span: Span, file: rite_core::FileId) {
        if name == "_" {
            return;
        }
        let local = LocalId(self.next_local);
        self.next_local += 1;
        let scope = self.scopes.last_mut().unwrap();
        if scope.bindings.contains_key(name) {
            self.diagnostics.push(simple_error(
                E022_DUPLICATE_BINDING,
                format!("duplicate binding `{}`", name),
                file,
                span,
                "already bound in this scope",
            ));
        }
        scope.bindings.insert(
            name.to_string(),
            BindingInfo {
                local,
                mutable,
                span,
            },
        );
    }

    fn lookup(&self, name: &str) -> Option<&BindingInfo> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.bindings.get(name) {
                return Some(b);
            }
        }
        None
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

fn is_effectful(path: &str) -> bool {
    let effectful = [
        "console.print",
        "console.println",
        "console.warn",
        "console.error",
        "console.inspect",
        "console.read_line",
        "fs.write",
        "fs.append",
        "fs.remove",
        "fs.mkdir",
        "fs.copy",
        "fs.move",
        "json.write",
        "clock.now",
        "clock.sleep",
        "process.run",
        "http.listen",
        "env.get",
        "env.require",
        "env.all",
        "random.int",
        "random.float",
        "random.choose",
        "random.shuffle",
        "random.uuid",
        "random.seed",
        "game.say",
        "store.set",
    ];
    effectful
        .iter()
        .any(|e| path == *e || path.starts_with(&format!("{}.", e)))
        || path.starts_with("console.")
        || path == "http.listen"
}

fn strip_effect(expr: &Expr) -> &Expr {
    match expr {
        Expr::Unary(u) if u.op == UnaryOp::Effect => strip_effect(&u.expr),
        other => other,
    }
}

fn has_effect_marker(expr: &Expr) -> bool {
    matches!(expr, Expr::Unary(u) if u.op == UnaryOp::Effect)
}

fn is_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "map"
            | "keep"
            | "reject"
            | "reduce"
            | "each"
            | "flatten"
            | "count"
            | "first"
            | "last"
            | "find"
            | "any"
            | "all"
            | "sum"
            | "min"
            | "max"
            | "sort"
            | "unique"
            | "zip"
            | "chunk"
            | "parallel"
            | "ok"
            | "err"
            | "panic"
            | "expect"
            | "fail"
            | "str"
            | "len"
            | "type_of"
            | "require"
            | "collect_results"
            | "group"
            | "lines"
            | "range"
            | "print"
            | "println"
            | "true"
            | "false"
            | "none"
    )
}

// silence unused imports
#[allow(dead_code)]
fn _unused(_b: &Binding, _o: BinOp, _l: &LitKind) {}
