//! Desugar AST into shared semantic IR.

use crate::ir::*;
use crate::resolve::ResolvedProgram;
use rite_core::Span;
use rite_syntax::{BinOp, Block, Expr, Item, LitKind, Pattern, ResultPatKind, Stmt, UnaryOp};
use std::collections::HashMap;

struct Desugar {
    next_local: u32,
    functions: Vec<FunctionIr>,
    func_map: HashMap<String, FuncId>,
    /// Import aliases: `use math as m` → `"m"` so `m.square` → `m__square`.
    import_aliases: HashMap<String, String>,
}

pub fn desugar_program(resolved: &ResolvedProgram) -> ProgramIr {
    let mut import_aliases = HashMap::new();
    for item in &resolved.ast.items {
        if let Item::Import(i) = item {
            if let Some(alias) = &i.alias {
                import_aliases.insert(alias.name.clone(), alias.name.clone());
            }
        }
    }
    let mut d = Desugar {
        next_local: 0,
        functions: Vec::new(),
        func_map: HashMap::new(),
        import_aliases,
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
                },
                is_pub: f.is_pub,
                span: f.span,
            });
        }
    }

    let mut statements = Vec::new();
    let mut has_main = false;

    for item in &resolved.ast.items {
        match item {
            Item::Function(f) => {
                let id = d.func_map[&f.name.name];
                d.push_scope_params();
                let mut params = Vec::new();
                let mut param_names = Vec::new();
                for p in &f.params {
                    let lid = d.fresh_local();
                    params.push(lid);
                    param_names.push(p.name.name.clone());
                }
                let body = d.desugar_block(&f.body);
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
                // Lower tests as named functions __test_N
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
                    .map(|e| {
                        let key = match &e.key {
                            rite_syntax::RecordKey::Ident(i) => KeyIr::Ident(i.name.clone()),
                            rite_syntax::RecordKey::String(s) => KeyIr::String(s.clone()),
                            rite_syntax::RecordKey::Atom(a) => KeyIr::Atom(a.parts.join(".")),
                            rite_syntax::RecordKey::Spread => KeyIr::Ident("_spread".into()),
                        };
                        (key, d.desugar_expr(&e.value))
                    })
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

impl Desugar {
    fn fresh_local(&mut self) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        id
    }

    fn push_scope_params(&mut self) {}

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
        }
    }

    fn desugar_stmt(&mut self, stmt: &Stmt) -> ExprIr {
        match stmt {
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
                            rite_syntax::RecordKey::Spread => self.desugar_expr(&e.value),
                            other => {
                                let key = match other {
                                    rite_syntax::RecordKey::Ident(i) => {
                                        KeyIr::Ident(i.name.clone())
                                    }
                                    rite_syntax::RecordKey::String(s) => KeyIr::String(s.clone()),
                                    rite_syntax::RecordKey::Atom(a) => {
                                        KeyIr::Atom(a.parts.join("."))
                                    }
                                    rite_syntax::RecordKey::Spread => unreachable!(),
                                };
                                ExprIr::BuildRecord(
                                    vec![(key, self.desugar_expr(&e.value))],
                                    e.span,
                                )
                            }
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
                        .map(|e| {
                            let key = match &e.key {
                                rite_syntax::RecordKey::Ident(i) => KeyIr::Ident(i.name.clone()),
                                rite_syntax::RecordKey::String(s) => KeyIr::String(s.clone()),
                                rite_syntax::RecordKey::Atom(a) => KeyIr::Atom(a.parts.join(".")),
                                rite_syntax::RecordKey::Spread => KeyIr::Ident("_spread".into()),
                            };
                            (key, self.desugar_expr(&e.value))
                        })
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
                // `use math as m` → `m.square` rewrites to global `m__square`
                if let Expr::Ident(obj) = m.object.as_ref() {
                    if self.import_aliases.contains_key(&obj.name) {
                        let mangled = format!("{}__{}", obj.name, m.field.name);
                        return ExprIr::Global(mangled);
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
                        body: self.desugar_expr(&a.body),
                        span: a.span,
                    })
                    .collect(),
                span: m.span,
            },
            Expr::Block(b) => {
                // Block as expression = closure if has params, else seq
                if !b.params.is_empty() {
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
            Expr::Capability(c) => ExprIr::CapabilityCall {
                path: c.path.clone(),
                args: vec![],
                effect: EffectKind::Pure,
                span: c.span,
            },
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
                                    path: r.path.clone(),
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
                            ExprIr::Constant(ValueLiteral::String(r.path.clone(), r.span)),
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
            Expr::Ident(i) => PipelineStageIr {
                kind: StageKind::Call,
                expr: ExprIr::NativeCall {
                    name: i.name.clone(),
                    args: vec![],
                    effect: EffectKind::Pure,
                    span: i.span,
                },
            },
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
    fn desugar_interpolation(&mut self, s: &str, span: Span) -> ExprIr {
        let mut parts: Vec<ExprIr> = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let (before, after) = rest.split_at(start);
            if !before.is_empty() {
                parts.push(ExprIr::Constant(ValueLiteral::String(
                    before.to_string(),
                    span,
                )));
            }
            let after = &after[1..]; // skip '{'
            if let Some(end) = after.find('}') {
                let expr_src = &after[..end];
                parts.push(self.parse_interp_expr(expr_src, span));
                rest = &after[end + 1..];
            } else {
                // unmatched brace — treat rest literally
                parts.push(ExprIr::Constant(ValueLiteral::String(
                    format!("{{{}", after),
                    span,
                )));
                rest = "";
                break;
            }
        }
        if !rest.is_empty() {
            parts.push(ExprIr::Constant(ValueLiteral::String(
                rest.to_string(),
                span,
            )));
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
                    LitKind::String(s) => ValueLiteral::String(s.clone(), l.span),
                };
                PatternIr::Literal(lit)
            }
            Pattern::Wildcard(_) => PatternIr::Wildcard,
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
            Pattern::Typed(t) => self.desugar_pattern(&t.pattern),
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
