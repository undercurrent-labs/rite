//! Desugar AST into shared semantic IR.

use crate::ir::*;
use crate::resolve::ResolvedProgram;
use rite_core::Span;
use rite_syntax::{
    BinOp, Block, Expr, Item, LitKind, Pattern, RecordKey, ResultPatKind, Stmt, SugarForm, UnaryOp,
};
use std::collections::{HashMap, HashSet};

struct Desugar {
    next_local: u32,
    functions: Vec<FunctionIr>,
    func_map: HashMap<String, FuncId>,
    /// Names the entry's imports bind, so `m.square` / `@m.square` rewrite to
    /// `m__square`. From the resolver rather than a scan of the item list.
    import_aliases: HashSet<String>,
    /// Qualifiers of the merged modules' imports — a body copied out of
    /// `outer.rite` uses imports the entry item list does not have. Consulted
    /// only while `in_injected_fn`, mirroring the resolver's scoping.
    merged_aliases: HashSet<String>,
    /// Names of the function copies `merge_exports_into_entry` injected.
    injected: HashSet<String>,
    /// Lowering the body of an injected copy (or a function nested in one).
    in_injected_fn: bool,
}

pub fn desugar_program(resolved: &ResolvedProgram) -> ProgramIr {
    let mut d = Desugar {
        next_local: 0,
        functions: Vec::new(),
        func_map: HashMap::new(),
        import_aliases: resolved.import_qualifiers.clone(),
        merged_aliases: resolved.merged_qualifiers.clone(),
        injected: resolved.injected_functions.clone(),
        in_injected_fn: false,
    };
    // Pre-register functions
    for item in &resolved.ast.items {
        if let Item::Function(f) = item {
            let id = FuncId(d.functions.len() as u32);
            d.func_map.insert(f.name.name.clone(), id);
            d.functions.push(FunctionIr {
                id,
                name: f.name.name.clone(),
                params: vec![],
                param_names: f.params.iter().map(|p| p.name.name.clone()).collect(),
                body: BlockIr {
                    params: vec![],
                    body: vec![],
                    span: f.body.span,
                    loop_body: false,
                },
                is_pub: f.is_pub,
                span: f.span,
                param_types: f.params.iter().map(|p| p.ty.clone()).collect(),
                return_type: f.return_type.clone(),
            });
        }
    }

    let mut statements = Vec::new();
    let mut has_main = false;

    for item in &resolved.ast.items {
        match item {
            Item::Function(f) => {
                let id = d.func_map[&f.name.name];
                d.in_injected_fn = d.injected.contains(&f.name.name);
                d.push_scope_params();
                let mut params = Vec::new();
                let mut param_names = Vec::new();
                for p in &f.params {
                    let lid = d.fresh_local();
                    params.push(lid);
                    param_names.push(p.name.name.clone());
                }
                let body = d.desugar_block(&f.body);
                d.in_injected_fn = false;
                if let Some(func) = d.functions.iter_mut().find(|x| x.id == id) {
                    func.params = params;
                    func.param_names = param_names;
                    func.body = body;
                }
                if f.name.name == "main" {
                    has_main = true;
                }
            }
            Item::Statement(s) => {
                statements.push(d.desugar_stmt(s));
            }
            Item::Test(t) => {
                // Lower tests as named functions __test_N.
                // `t.name` is deliberately NOT brace-decoded: the test runner
                // pairs it by exact text (`format!("__test_{}", name)` in
                // rite-test/src/lib.rs), so decoding only on this side would
                // stop a test whose name contains braces from being found.
                let id = FuncId(d.functions.len() as u32);
                let body = d.desugar_block(&t.body);
                d.functions.push(FunctionIr {
                    id,
                    name: format!("__test_{}", t.name),
                    params: vec![],
                    param_names: vec![],
                    body,
                    is_pub: true,
                    span: t.span,
                    param_types: vec![],
                    return_type: None,
                });
            }
            Item::Event(e) => {
                // Lower to @game.register_*(atom, block)
                let kind = match e.kind {
                    rite_syntax::EventKind::Item => "register_item",
                    rite_syntax::EventKind::Room => "register_room",
                    rite_syntax::EventKind::World => "register_world",
                };
                let atom = e.atom.parts.join(".");
                let body = d.desugar_block(&e.body);
                statements.push(ExprIr::CapabilityCall {
                    path: vec!["game".into(), kind.into()],
                    args: vec![
                        ExprIr::Atom(atom, e.atom.span),
                        ExprIr::Closure(ClosureIr {
                            params: body.params.clone(),
                            param_names: vec![],
                            body,
                            span: e.span,
                        }),
                    ],
                    effect: EffectKind::Effect,
                    span: e.span,
                });
            }
            Item::Import(i) => {
                statements.push(ExprIr::NativeCall {
                    name: "import".into(),
                    args: vec![ExprIr::Constant(ValueLiteral::String(
                        i.path
                            .segments
                            .iter()
                            .map(|s| s.name.as_str())
                            .collect::<Vec<_>>()
                            .join("."),
                        i.span,
                    ))],
                    effect: EffectKind::Pure,
                    span: i.span,
                });
            }
            Item::Data(data) => {
                let fields: Vec<_> = data
                    .fields
                    .iter()
                    .map(|e| (key_ir(&e.key), d.desugar_expr(&e.value)))
                    .collect();
                let rec = ExprIr::BuildRecord(fields, data.span);
                let lid = d.fresh_local();
                statements.push(ExprIr::Bind {
                    local: lid,
                    name: data.name.name.clone(),
                    mutable: false,
                    value: Box::new(rec),
                    span: data.span,
                });
            }
        }
    }

    let entry = if has_main {
        EntryPoint::Main {
            func: d.func_map["main"],
        }
    } else {
        EntryPoint::Script
    };

    ProgramIr {
        modules: vec![ModuleIr {
            name: "main".into(),
            statements,
            exports: vec![],
        }],
        entry,
        functions: d.functions,
        native_names: HashMap::new(),
    }
}

/// Lower a record key.
///
/// A string key *names* a field — it is never interpolated — so the lexer's
/// doubled-brace encoding is decoded here rather than split into holes. Without
/// this, `⟨"\{a}": 1⟩` builds a field called `{{a}` and every lookup misses.
fn key_ir(key: &RecordKey) -> KeyIr {
    match key {
        RecordKey::Ident(i) => KeyIr::Ident(i.name.clone()),
        RecordKey::String(s) => KeyIr::String(rite_syntax::unescape_braces(s)),
        RecordKey::Atom(a) => KeyIr::Atom(a.parts.join(".")),
        RecordKey::Spread => KeyIr::Ident("_spread".into()),
    }
}

impl Desugar {
    /// `@cool.square` → `cool__square` when `cool` is an import qualifier and
    /// the mangled global exists. Unlike the `Member` rewrite below, no shadow
    /// check: `@cool` always means the module. Resolve has already rejected
    /// qualifiers that collide with capability namespaces, so a `None` here
    /// falls through to an ordinary capability call.
    /// Entry qualifiers hold everywhere; a merged module's only inside the
    /// copies that came with it. Mirrors `Resolver::qualifier_in_scope`.
    fn is_qualifier(&self, name: &str) -> bool {
        self.import_aliases.contains(name)
            || (self.in_injected_fn && self.merged_aliases.contains(name))
    }

    fn module_global(&self, path: &[String]) -> Option<String> {
        if path.len() != 2 || !self.is_qualifier(&path[0]) {
            return None;
        }
        let mangled = format!("{}__{}", path[0], path[1]);
        self.func_map.contains_key(&mangled).then_some(mangled)
    }

    fn fresh_local(&mut self) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        id
    }

    fn push_scope_params(&mut self) {}

    /// Lower one `tool` / `resource` / `prompt` declaration.
    ///
    /// The parameter list is carried across whole. Every other lowering path in this
    /// file reduces a parameter to a fresh local and drops the annotation, which is
    /// right when the annotation is only a contract to check — here it is also the
    /// schema the server publishes, so it has to reach the host.
    fn desugar_mcp_decl(&mut self, d: &rite_syntax::McpDeclExpr) -> McpDeclIr {
        McpDeclIr {
            kind: d.kind.as_str().to_string(),
            name: rite_syntax::unescape_braces(&d.name),
            description: d
                .description
                .as_ref()
                .map(|s| rite_syntax::unescape_braces(s)),
            params: d
                .params
                .iter()
                .map(|p| McpParamIr {
                    name: p.name.name.clone(),
                    ty: p.ty.clone(),
                })
                .collect(),
            body: self.desugar_block(&d.body),
            span: d.span,
        }
    }

    fn desugar_block(&mut self, block: &Block) -> BlockIr {
        let mut params = Vec::new();
        for _ in &block.params {
            params.push(self.fresh_local());
        }
        let mut body = Vec::new();
        for item in &block.body {
            match item {
                Item::Statement(s) => body.push(self.desugar_stmt(s)),
                Item::Function(f) => {
                    // nested function as closure binding
                    let b = self.desugar_block(&f.body);
                    let mut fps = Vec::new();
                    let mut fnames = Vec::new();
                    for p in &f.params {
                        fps.push(self.fresh_local());
                        fnames.push(p.name.name.clone());
                    }
                    let lid = self.fresh_local();
                    body.push(ExprIr::Bind {
                        local: lid,
                        name: f.name.name.clone(),
                        mutable: false,
                        value: Box::new(ExprIr::Closure(ClosureIr {
                            params: fps,
                            param_names: fnames,
                            body: b,
                            span: f.span,
                        })),
                        span: f.span,
                    });
                }
                other => {
                    // re-wrap other items loosely
                    if let Item::Statement(_) = other {
                    } else {
                        // skip nested imports/events inside block or desugar events
                        if let Item::Event(e) = other {
                            let kind = match e.kind {
                                rite_syntax::EventKind::Item => "register_item",
                                rite_syntax::EventKind::Room => "register_room",
                                rite_syntax::EventKind::World => "register_world",
                            };
                            let atom = e.atom.parts.join(".");
                            let b = self.desugar_block(&e.body);
                            body.push(ExprIr::CapabilityCall {
                                path: vec!["game".into(), kind.into()],
                                args: vec![
                                    ExprIr::Atom(atom, e.atom.span),
                                    ExprIr::Closure(ClosureIr {
                                        params: vec![],
                                        param_names: vec![],
                                        body: b,
                                        span: e.span,
                                    }),
                                ],
                                effect: EffectKind::Effect,
                                span: e.span,
                            });
                        }
                    }
                }
            }
        }
        BlockIr {
            params,
            body,
            span: block.span,
            loop_body: false,
        }
    }

    fn desugar_stmt(&mut self, stmt: &Stmt) -> ExprIr {
        match stmt {
            // The sugar's `lowered` form is the semantic truth; the source
            // spelling exists for the formatter alone.
            Stmt::Sugared(s) => {
                let mut ir = self.desugar_stmt(&s.lowered);
                // The loop sugars lower through a closure the source never
                // wrote. `^` inside one must return from the enclosing
                // function, not from that closure, so the synthesized body is
                // flagged and `call_block` passes the return through. The
                // match is on the exact shapes the parser emits; anything
                // else is left alone.
                match &s.form {
                    SugarForm::ForIn { .. } | SugarForm::Loop { .. } => {
                        if let ExprIr::Pipeline { stages, .. } = &mut ir {
                            for stage in stages {
                                if let ExprIr::Call { callee, args, .. } = &mut stage.expr {
                                    if matches!(&**callee, ExprIr::Global(n) if n == "each") {
                                        if let Some(ExprIr::Closure(c)) = args.last_mut() {
                                            c.body.loop_body = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    SugarForm::While { .. } => {
                        if let ExprIr::Call { callee, args, .. } = &mut ir {
                            if matches!(&**callee, ExprIr::Global(n) if n == "while_loop") {
                                // args[0] is the condition closure — an
                                // expression, which cannot contain a `^`.
                                if let Some(ExprIr::Closure(c)) = args.get_mut(1) {
                                    c.body.loop_body = true;
                                }
                            }
                        }
                    }
                    SugarForm::Break | SugarForm::Continue => {
                        // The `__break`/`__continue` call the parser lowered to
                        // becomes a native call, which the loop drivers
                        // intercept — interpreted and compiled dispatch alike.
                        let name = if matches!(s.form, SugarForm::Break) {
                            "__break"
                        } else {
                            "__continue"
                        };
                        ir = ExprIr::NativeCall {
                            name: name.into(),
                            args: vec![],
                            effect: EffectKind::Pure,
                            span: s.span,
                        };
                    }
                    SugarForm::Say { .. } | SugarForm::Unless { .. } => {}
                }
                ir
            }
            Stmt::Binding(b) => {
                let value = self.desugar_expr(&b.value);
                match &b.pattern {
                    Pattern::Ident(i) => {
                        let lid = self.fresh_local();
                        ExprIr::Bind {
                            local: lid,
                            name: i.name.clone(),
                            mutable: b.mutable,
                            value: Box::new(value),
                            span: b.span,
                        }
                    }
                    other => {
                        // Desugar pattern binding as match
                        let lid = self.fresh_local();
                        let bind_tmp = ExprIr::Bind {
                            local: lid,
                            name: "__pat".into(),
                            mutable: false,
                            value: Box::new(value),
                            span: b.span,
                        };
                        let pat = self.desugar_pattern(other);
                        ExprIr::Seq(
                            vec![
                                bind_tmp,
                                ExprIr::Match {
                                    value: Box::new(ExprIr::Local(lid)),
                                    arms: vec![MatchArmIr {
                                        pattern: pat,
                                        guard: None,
                                        body: ExprIr::Local(lid),
                                        span: b.span,
                                    }],
                                    span: b.span,
                                },
                            ],
                            b.span,
                        )
                    }
                }
            }
            Stmt::Assign(a) => {
                let lid = self.fresh_local();
                let rhs = if let Some(op) = a.op {
                    // c += 1  →  c := c + 1
                    ExprIr::Binary {
                        op: bin_op(op),
                        left: Box::new(ExprIr::Global(a.name.name.clone())),
                        right: Box::new(self.desugar_expr(&a.value)),
                        span: a.span,
                    }
                } else {
                    self.desugar_expr(&a.value)
                };
                ExprIr::Assign {
                    local: lid,
                    value: Box::new(ExprIr::Seq(
                        vec![ExprIr::Global(a.name.name.clone()), rhs],
                        a.span,
                    )),
                    span: a.span,
                }
            }
            Stmt::Expr(e) => self.desugar_expr(e),
            Stmt::Return(r) => ExprIr::Return(
                r.value.as_ref().map(|v| Box::new(self.desugar_expr(v))),
                r.span,
            ),
        }
    }

    fn desugar_expr(&mut self, expr: &Expr) -> ExprIr {
        match expr {
            Expr::Literal(l) => match &l.kind {
                LitKind::None => ExprIr::Constant(ValueLiteral::None(l.span)),
                LitKind::Bool(b) => ExprIr::Constant(ValueLiteral::Bool(*b, l.span)),
                LitKind::Int(n) => ExprIr::Constant(ValueLiteral::Int(*n, l.span)),
                LitKind::Float(n) => ExprIr::Constant(ValueLiteral::Float(*n, l.span)),
                LitKind::String(s) => {
                    if s.contains('{') && s.contains('}') {
                        self.desugar_interpolation(s, l.span)
                    } else {
                        ExprIr::Constant(ValueLiteral::String(s.clone(), l.span))
                    }
                }
            },
            Expr::Ident(i) => ExprIr::Global(i.name.clone()),
            Expr::Atom(a) => ExprIr::Atom(a.parts.join("."), a.span),
            Expr::List(l) => {
                // Expand spreads: [a, ..xs, b] → concat([a], xs, [b]) when any spread present
                let has_spread = l
                    .elements
                    .iter()
                    .any(|e| matches!(e, Expr::Unary(u) if u.op == UnaryOp::Spread));
                if has_spread {
                    let mut parts = Vec::new();
                    for e in &l.elements {
                        if let Expr::Unary(u) = e {
                            if u.op == UnaryOp::Spread {
                                parts.push(self.desugar_expr(&u.expr));
                                continue;
                            }
                        }
                        parts.push(ExprIr::BuildList(vec![self.desugar_expr(e)], e.span()));
                    }
                    ExprIr::NativeCall {
                        name: "concat".into(),
                        args: parts,
                        effect: EffectKind::Pure,
                        span: l.span,
                    }
                } else {
                    ExprIr::BuildList(
                        l.elements.iter().map(|e| self.desugar_expr(e)).collect(),
                        l.span,
                    )
                }
            }
            Expr::Record(r) => {
                // Spreads: ⟨..a, k: v, ..b⟩ → fold merge
                let has_spread = r
                    .entries
                    .iter()
                    .any(|e| matches!(e.key, rite_syntax::RecordKey::Spread));
                if has_spread {
                    let mut acc: Option<ExprIr> = None;
                    for e in &r.entries {
                        let piece = match &e.key {
                            RecordKey::Spread => self.desugar_expr(&e.value),
                            other => ExprIr::BuildRecord(
                                vec![(key_ir(other), self.desugar_expr(&e.value))],
                                e.span,
                            ),
                        };
                        acc = Some(match acc {
                            None => piece,
                            Some(left) => ExprIr::Binary {
                                op: BinaryOpIr::Add, // record merge
                                left: Box::new(left),
                                right: Box::new(piece),
                                span: r.span,
                            },
                        });
                    }
                    acc.unwrap_or(ExprIr::BuildRecord(vec![], r.span))
                } else {
                    let entries = r
                        .entries
                        .iter()
                        .map(|e| (key_ir(&e.key), self.desugar_expr(&e.value)))
                        .collect();
                    ExprIr::BuildRecord(entries, r.span)
                }
            }
            Expr::Binary(b) => match b.op {
                BinOp::Xor => ExprIr::NativeCall {
                    name: "xor".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                BinOp::Power => ExprIr::NativeCall {
                    name: "pow".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                BinOp::Idiv => ExprIr::NativeCall {
                    name: "idiv".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                BinOp::Range => ExprIr::NativeCall {
                    name: "range".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                BinOp::RangeIncl => ExprIr::NativeCall {
                    name: "range_incl".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                BinOp::Compose => ExprIr::NativeCall {
                    name: "compose".into(),
                    args: vec![self.desugar_expr(&b.left), self.desugar_expr(&b.right)],
                    effect: EffectKind::Pure,
                    span: b.span,
                },
                other => ExprIr::Binary {
                    op: bin_op(other),
                    left: Box::new(self.desugar_expr(&b.left)),
                    right: Box::new(self.desugar_expr(&b.right)),
                    span: b.span,
                },
            },
            Expr::Unary(u) => {
                if u.op == UnaryOp::Effect {
                    // ! expr — mark effect on call
                    match u.expr.as_ref() {
                        Expr::Call(c) => {
                            if let Expr::Capability(cap) = c.callee.as_ref() {
                                // `! @cool.square(…)` routes to the module before the
                                // host: same shape `! cool.square(…)` lowers to.
                                if let Some(global) = self.module_global(&cap.path) {
                                    return ExprIr::Unary {
                                        op: UnaryOpIr::Effect,
                                        expr: Box::new(ExprIr::Call {
                                            callee: Box::new(ExprIr::Global(global)),
                                            args: c
                                                .args
                                                .iter()
                                                .map(|a| self.desugar_expr(a))
                                                .collect(),
                                            span: c.span,
                                        }),
                                        span: u.span,
                                    };
                                }
                                return ExprIr::CapabilityCall {
                                    path: cap.path.clone(),
                                    args: c.args.iter().map(|a| self.desugar_expr(a)).collect(),
                                    effect: EffectKind::Effect,
                                    span: u.span,
                                };
                            }
                            return ExprIr::Unary {
                                op: UnaryOpIr::Effect,
                                expr: Box::new(self.desugar_expr(&u.expr)),
                                span: u.span,
                            };
                        }
                        Expr::Capability(cap) => {
                            if let Some(global) = self.module_global(&cap.path) {
                                return ExprIr::Unary {
                                    op: UnaryOpIr::Effect,
                                    expr: Box::new(ExprIr::Global(global)),
                                    span: u.span,
                                };
                            }
                            // ! @cap.fn is incomplete without call — leave as capability ref
                            return ExprIr::CapabilityCall {
                                path: cap.path.clone(),
                                args: vec![],
                                effect: EffectKind::Effect,
                                span: u.span,
                            };
                        }
                        _ => {}
                    }
                }
                ExprIr::Unary {
                    op: match u.op {
                        UnaryOp::Neg => UnaryOpIr::Neg,
                        UnaryOp::Not => UnaryOpIr::Not,
                        UnaryOp::Effect => UnaryOpIr::Effect,
                        UnaryOp::Spread => UnaryOpIr::Not, // should be expanded in list/record
                    },
                    expr: Box::new(self.desugar_expr(&u.expr)),
                    span: u.span,
                }
            }
            Expr::Call(c) => {
                // capability call pure
                if let Expr::Capability(cap) = c.callee.as_ref() {
                    // `@cool.square(…)` routes to the module before the host: same
                    // Call-of-Global shape `cool.square(…)` lowers to.
                    if let Some(global) = self.module_global(&cap.path) {
                        return ExprIr::Call {
                            callee: Box::new(ExprIr::Global(global)),
                            args: c.args.iter().map(|a| self.desugar_expr(a)).collect(),
                            span: c.span,
                        };
                    }
                    return ExprIr::CapabilityCall {
                        path: cap.path.clone(),
                        args: c.args.iter().map(|a| self.desugar_expr(a)).collect(),
                        effect: EffectKind::Pure,
                        span: c.span,
                    };
                }
                // Builtins desugar as ordinary calls so local/nested defs can shadow them.
                // Runtime Global lookup returns NativeName for unbound builtin names.
                ExprIr::Call {
                    callee: Box::new(self.desugar_expr(&c.callee)),
                    args: c.args.iter().map(|a| self.desugar_expr(a)).collect(),
                    span: c.span,
                }
            }
            Expr::Member(m) => {
                if matches!(m.object.as_ref(), Expr::Placeholder(_)) {
                    // bare projection stage — handled in pipeline
                    return ExprIr::Member {
                        object: Box::new(ExprIr::Placeholder(m.span)),
                        field: m.field.name.clone(),
                        span: m.span,
                    };
                }
                // `use math` → `math.square` rewrites to the global `math__square`.
                //
                // Only when that global exists. A parameter can shadow the module
                // name — `◆ f(math) ⟦ ^ math.x ⟧` reads a field of the argument —
                // and rewriting on the import alone turned that into a lookup of
                // `math__x`, which failed at runtime. Functions are all registered
                // before any body is lowered, so this map is complete here.
                if let Expr::Ident(obj) = m.object.as_ref() {
                    if self.is_qualifier(&obj.name) {
                        let mangled = format!("{}__{}", obj.name, m.field.name);
                        if self.func_map.contains_key(&mangled) {
                            return ExprIr::Global(mangled);
                        }
                    }
                }
                ExprIr::Member {
                    object: Box::new(self.desugar_expr(&m.object)),
                    field: m.field.name.clone(),
                    span: m.span,
                }
            }
            Expr::Index(i) => ExprIr::Index {
                object: Box::new(self.desugar_expr(&i.object)),
                index: Box::new(self.desugar_expr(&i.index)),
                span: i.span,
            },
            Expr::Pipeline(p) => {
                let input = self.desugar_expr(&p.input);
                let stages = p
                    .stages
                    .iter()
                    .map(|s| self.desugar_pipeline_stage(s))
                    .collect();
                ExprIr::Pipeline {
                    input: Box::new(input),
                    stages,
                    span: p.span,
                }
            }
            Expr::If(i) => ExprIr::If {
                condition: Box::new(self.desugar_expr(&i.condition)),
                then_branch: self.desugar_block(&i.then_branch),
                else_branch: i.else_branch.as_ref().map(|b| self.desugar_block(b)),
                span: i.span,
            },
            Expr::Match(m) => ExprIr::Match {
                value: Box::new(self.desugar_expr(&m.scrutinee)),
                arms: m
                    .arms
                    .iter()
                    .map(|a| MatchArmIr {
                        pattern: self.desugar_pattern(&a.pattern),
                        guard: a.guard.as_ref().map(|g| self.desugar_expr(g)),
                        body: self.desugar_expr(&a.body),
                        span: a.span,
                    })
                    .collect(),
                span: m.span,
            },
            Expr::Block(b) => {
                // Block as expression = closure if a `|…|` was written, else a
                // sequence. The test used to be `!params.is_empty()`, which cannot
                // see the difference between `⟦ 42 ⟧` and `{ || 42 }` — so a
                // zero-argument closure evaluated to its body instead of becoming a
                // function, and calling it failed with `cannot call value of type
                // int`. Named `◆ f()` was unaffected, which is why this survived.
                if b.has_param_list {
                    let body = self.desugar_block(b);
                    let mut params = Vec::new();
                    let mut names = Vec::new();
                    for p in &b.params {
                        params.push(self.fresh_local());
                        names.push(p.name.name.clone());
                    }
                    ExprIr::Closure(ClosureIr {
                        params,
                        param_names: names,
                        body,
                        span: b.span,
                    })
                } else {
                    ExprIr::Block(self.desugar_block(b))
                }
            }
            Expr::Capability(c) => {
                // Bare `@cool.square` is the function value, exactly as bare
                // `cool.square` is — not invoked, unlike a bare capability ref.
                if let Some(global) = self.module_global(&c.path) {
                    return ExprIr::Global(global);
                }
                ExprIr::CapabilityCall {
                    path: c.path.clone(),
                    args: vec![],
                    effect: EffectKind::Pure,
                    span: c.span,
                }
            }
            Expr::Placeholder(p) => ExprIr::Placeholder(p.span),
            Expr::Try(t) => ExprIr::Try {
                expr: Box::new(self.desugar_expr(&t.expr)),
                span: t.span,
            },
            Expr::Coalesce(c) => ExprIr::Coalesce {
                left: Box::new(self.desugar_expr(&c.left)),
                right: Box::new(self.desugar_expr(&c.right)),
                span: c.span,
            },
            Expr::HttpListen(h) => {
                let mut routes = Vec::new();
                let mut middleware = Vec::new();
                for item in &h.body.body {
                    if let Item::Statement(Stmt::Expr(e)) = item {
                        match e {
                            Expr::Route(r) => {
                                let param = r.params.first().map(|_| self.fresh_local());
                                routes.push(RouteIr {
                                    method: format!("{:?}", r.method).to_uppercase(),
                                    // A route path is matched by the router, not
                                    // interpolated: single braces are route
                                    // parameters (`/users/{id}`) and stay as-is,
                                    // while a doubled brace is decoded like any
                                    // other non-evaluated string literal.
                                    path: rite_syntax::unescape_braces(&r.path),
                                    param,
                                    body: self.desugar_block(&r.body),
                                    span: r.span,
                                });
                            }
                            Expr::Call(c)
                                if matches!(
                                    c.callee.as_ref(),
                                    Expr::Ident(i) if i.name == "__middleware_use"
                                ) =>
                            {
                                if let Some(arg) = c.args.first() {
                                    middleware.push(self.desugar_expr(arg));
                                }
                            }
                            other => {
                                // ignore
                                let _ = other;
                            }
                        }
                    }
                }
                // Normalize method names from Debug format (e.g. "Get") to HTTP verbs
                for r in &mut routes {
                    r.method = match r.method.to_ascii_lowercase().as_str() {
                        "get" => "GET".into(),
                        "post" => "POST".into(),
                        "put" => "PUT".into(),
                        "patch" => "PATCH".into(),
                        "delete" => "DELETE".into(),
                        "head" => "HEAD".into(),
                        "options" => "OPTIONS".into(),
                        other => other.to_uppercase(),
                    };
                }
                ExprIr::HttpListen {
                    addr: Box::new(self.desugar_expr(&h.addr)),
                    routes,
                    middleware,
                    span: h.span,
                }
            }
            Expr::McpServe(m) => {
                let mut decls = Vec::new();
                let mut middleware = Vec::new();
                for item in &m.body.body {
                    if let Item::Statement(Stmt::Expr(e)) = item {
                        match e {
                            Expr::McpDecl(d) => decls.push(self.desugar_mcp_decl(d)),
                            Expr::Call(c)
                                if matches!(
                                    c.callee.as_ref(),
                                    Expr::Ident(i) if i.name == "__middleware_use"
                                ) =>
                            {
                                if let Some(arg) = c.args.first() {
                                    middleware.push(self.desugar_expr(arg));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ExprIr::McpServe {
                    config: Box::new(self.desugar_expr(&m.config)),
                    decls,
                    middleware,
                    span: m.span,
                }
            }
            // A declaration outside a serve body has nothing to register with. It
            // describes itself as a record so the shape is still inspectable, matching
            // what a stray `Expr::Route` does.
            Expr::McpDecl(d) => ExprIr::BuildRecord(
                vec![
                    (
                        KeyIr::Ident("kind".into()),
                        ExprIr::Atom(d.kind.as_str().into(), d.span),
                    ),
                    (
                        KeyIr::Ident("name".into()),
                        ExprIr::Constant(ValueLiteral::String(
                            rite_syntax::unescape_braces(&d.name),
                            d.span,
                        )),
                    ),
                ],
                d.span,
            ),
            Expr::Route(r) => {
                // standalone route shouldn't appear; wrap as record
                ExprIr::BuildRecord(
                    vec![
                        (
                            KeyIr::Ident("method".into()),
                            ExprIr::Atom(format!("{:?}", r.method), r.span),
                        ),
                        (
                            KeyIr::Ident("path".into()),
                            ExprIr::Constant(ValueLiteral::String(
                                rite_syntax::unescape_braces(&r.path),
                                r.span,
                            )),
                        ),
                    ],
                    r.span,
                )
            }
            Expr::Group(g) => self.desugar_expr(&g.expr),
        }
    }

    fn desugar_pipeline_stage(&mut self, stage: &Expr) -> PipelineStageIr {
        match stage {
            Expr::Member(m) if matches!(m.object.as_ref(), Expr::Placeholder(_)) => {
                PipelineStageIr {
                    kind: StageKind::MemberProjection(m.field.name.clone()),
                    expr: self.desugar_expr(stage),
                }
            }
            Expr::Block(_) => PipelineStageIr {
                kind: StageKind::Block,
                expr: self.desugar_expr(stage),
            },
            // A bare name stage resolves exactly as a call callee does — `xs → count`
            // and `count(xs)` must name the same function. This arm used to build a
            // `NativeCall` directly, which consulted the builtin table and nothing
            // else: `3 → dbl` for a user-defined `dbl` failed at runtime with
            // "unknown builtin", and a user function shadowing a builtin was ignored,
            // so `count(xs)` answered 99 where `xs → count` answered 3 about the same
            // program. Falling through to `other` desugars it as `Global`, which the
            // evaluator resolves through `lookup_global` (binding, then function, then
            // builtin) — the order the comment on `Expr::Call` above already declares.
            Expr::Call(_c) => {
                // value → f(args) becomes f(value, args...)
                PipelineStageIr {
                    kind: StageKind::Call,
                    expr: self.desugar_expr(stage),
                }
            }
            other => PipelineStageIr {
                kind: StageKind::Call,
                expr: self.desugar_expr(other),
            },
        }
    }

    /// Desugar `"Hello, {name}."` into string concatenations.
    ///
    /// Brace convention in a string token's text (produced by
    /// `rite_syntax::lexer::string_literal`, which documents it in full):
    ///
    /// * a single `{ … }` pair is an interpolation hole;
    /// * a **doubled** brace (`{{`, `}}`) is a literal `{` / `}` — this is what
    ///   the source escapes `\{` and `\}` lex to, so `"\{name}"` prints
    ///   `{name}` instead of interpolating `name`;
    /// * an unmatched `{` is taken literally, as before.
    fn desugar_interpolation(&mut self, s: &str, span: Span) -> ExprIr {
        let mut parts: Vec<ExprIr> = Vec::new();
        // Literal text accumulated since the last interpolation hole.
        let mut lit = String::new();
        let mut rest = s;
        while let Some(start) = rest.find(['{', '}']) {
            let (before, after) = rest.split_at(start);
            lit.push_str(before);
            let brace = after.as_bytes()[0];
            // Skip the brace itself.
            let after = &after[1..];
            // Doubled brace: one literal brace, no interpolation.
            if after.as_bytes().first() == Some(&brace) {
                lit.push(brace as char);
                rest = &after[1..];
                continue;
            }
            if brace == b'}' {
                // Lone closing brace — literal, as there is no hole open.
                lit.push('}');
                rest = after;
                continue;
            }
            if let Some(end) = after.find('}') {
                if !lit.is_empty() {
                    parts.push(ExprIr::Constant(ValueLiteral::String(
                        std::mem::take(&mut lit),
                        span,
                    )));
                }
                let expr_src = &after[..end];
                parts.push(self.parse_interp_expr(expr_src, span));
                rest = &after[end + 1..];
            } else {
                // unmatched brace — literal. No `}` remains, so nothing further
                // can open a hole; keep scanning only to fold doubled braces.
                lit.push('{');
                rest = after;
            }
        }
        lit.push_str(rest);
        if !lit.is_empty() {
            parts.push(ExprIr::Constant(ValueLiteral::String(lit, span)));
        }
        if parts.is_empty() {
            return ExprIr::Constant(ValueLiteral::String(String::new(), span));
        }
        // fold with +
        let mut acc = parts.remove(0);
        for p in parts {
            acc = ExprIr::Binary {
                op: BinaryOpIr::Add,
                left: Box::new(acc),
                right: Box::new(p),
                span,
            };
        }
        acc
    }

    fn parse_interp_expr(&mut self, src: &str, span: Span) -> ExprIr {
        let src = src.trim();
        if src.is_empty() {
            return ExprIr::Constant(ValueLiteral::String(String::new(), span));
        }
        // Support ident and ident.field.field
        let mut parts = src.split('.');
        let first = parts.next().unwrap_or("");
        let mut expr = ExprIr::Global(first.to_string());
        for field in parts {
            if field.is_empty() {
                continue;
            }
            expr = ExprIr::Member {
                object: Box::new(expr),
                field: field.to_string(),
                span,
            };
        }
        // Wrap with str() for non-string values
        ExprIr::NativeCall {
            name: "str".into(),
            args: vec![expr],
            effect: EffectKind::Pure,
            span,
        }
    }

    fn desugar_pattern(&mut self, pat: &Pattern) -> PatternIr {
        match pat {
            Pattern::Ident(i) => {
                let lid = self.fresh_local();
                PatternIr::Ident(lid, i.name.clone())
            }
            Pattern::Atom(a) => PatternIr::Atom(a.parts.join(".")),
            Pattern::Literal(l) => {
                let lit = match &l.kind {
                    LitKind::None => ValueLiteral::None(l.span),
                    LitKind::Bool(b) => ValueLiteral::Bool(*b, l.span),
                    LitKind::Int(n) => ValueLiteral::Int(*n, l.span),
                    LitKind::Float(n) => ValueLiteral::Float(*n, l.span),
                    // A pattern is matched, not evaluated: there is nothing to
                    // interpolate against, so the doubled-brace encoding is
                    // decoded instead of split. `~ s ⟦ "\{x}" → … ⟧` must compare
                    // against the literal `{x}` (it compared against `{{x}` and
                    // could never match).
                    LitKind::String(s) => {
                        ValueLiteral::String(rite_syntax::unescape_braces(s), l.span)
                    }
                };
                PatternIr::Literal(lit)
            }
            Pattern::Wildcard(_) => PatternIr::Wildcard,
            Pattern::Or(o) => PatternIr::Or {
                alternatives: o
                    .alternatives
                    .iter()
                    .map(|p| self.desugar_pattern(p))
                    .collect(),
            },
            Pattern::List(l) => PatternIr::List {
                elements: l.elements.iter().map(|e| self.desugar_pattern(e)).collect(),
                rest: l.rest.as_ref().map(|r| Box::new(self.desugar_pattern(r))),
            },
            Pattern::Record(r) => PatternIr::Record {
                fields: r
                    .fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.name.clone(),
                            f.pattern.as_ref().map(|p| self.desugar_pattern(p)),
                        )
                    })
                    .collect(),
            },
            Pattern::Result(r) => PatternIr::Result {
                kind: match r.kind {
                    ResultPatKind::Ok => ResultPatKindIr::Ok,
                    ResultPatKind::Err => ResultPatKindIr::Err,
                    ResultPatKind::Some => ResultPatKindIr::Some,
                    ResultPatKind::None => ResultPatKindIr::None,
                },
                binding: r
                    .binding
                    .as_ref()
                    .map(|b| Box::new(self.desugar_pattern(b))),
            },
        }
    }
}

fn bin_op(op: BinOp) -> BinaryOpIr {
    match op {
        BinOp::Add => BinaryOpIr::Add,
        BinOp::Sub => BinaryOpIr::Sub,
        BinOp::Mul => BinaryOpIr::Mul,
        BinOp::Div => BinaryOpIr::Div,
        BinOp::Rem => BinaryOpIr::Rem,
        BinOp::Eq => BinaryOpIr::Eq,
        BinOp::NotEq => BinaryOpIr::NotEq,
        BinOp::Lt => BinaryOpIr::Lt,
        BinOp::LtEq => BinaryOpIr::LtEq,
        BinOp::Gt => BinaryOpIr::Gt,
        BinOp::GtEq => BinaryOpIr::GtEq,
        BinOp::And => BinaryOpIr::And,
        BinOp::Or => BinaryOpIr::Or,
        BinOp::In => BinaryOpIr::In,
        BinOp::NotIn => BinaryOpIr::NotIn,
        // Desugared to native calls before bin_op in most paths; fallback:
        BinOp::Xor
        | BinOp::Power
        | BinOp::Idiv
        | BinOp::Range
        | BinOp::RangeIncl
        | BinOp::Compose => BinaryOpIr::Add,
    }
}
