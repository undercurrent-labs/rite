//! Expression and block evaluation, including the allocation-free sync path.

use super::*;
use crate::value::{Closure, Key, Value};
use indexmap::IndexMap;
use rite_sem::{
    BinaryOpIr, BlockIr, EffectKind, EntryPoint, ExprIr, ProgramIr, StageKind, UnaryOpIr,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl<'a> Evaluator<'a> {
    pub async fn eval_program(&mut self, ir: &ProgramIr) -> Result<Value, EvalError> {
        crate::register_functions(ir, self.ctx);

        let mut last = Value::None;
        if let Some(module) = ir.modules.first() {
            for stmt in &module.statements {
                match self.eval_operand(stmt).await {
                    // Top-level `^` / postfix `?` on err: script result is the returned value.
                    Err(EvalError::Return(v)) => {
                        last = v;
                        break;
                    }
                    Err(e) => return Err(e),
                    Ok(v) => last = v,
                }
            }
        }

        match &ir.entry {
            EntryPoint::Main { .. } => {
                if let Some(main) = self.ctx.functions.get("main").cloned() {
                    last = self
                        .call_block(
                            &main.body,
                            &main.params,
                            vec![Value::list(Vec::<Value>::new())],
                        )
                        .await?;
                }
            }
            EntryPoint::Script => {}
        }
        Ok(last)
    }

    pub fn eval_expr<'b>(
        &'b mut self,
        expr: &'b ExprIr,
    ) -> Pin<Box<dyn Future<Output = Result<Value, EvalError>> + Send + 'b>> {
        Box::pin(async move { self.eval_expr_inner(expr).await })
    }

    pub(super) async fn eval_expr_inner(&mut self, expr: &ExprIr) -> Result<Value, EvalError> {
        self.ctx.budget.tick()?;
        match expr {
            ExprIr::Constant(lit) => Ok(self.lit_to_value(lit)),
            ExprIr::Local(id) => self
                .ctx
                .env
                .get_local(*id)
                .ok_or_else(|| EvalError::Message(format!("undefined local {}", id.0))),
            ExprIr::Global(name) => self.lookup_global(name),
            ExprIr::Bind {
                local,
                name,
                mutable,
                value,
                ..
            } => {
                let v = self.eval_operand(value).await?;
                self.ctx.env.define(name, *local, v.clone(), *mutable);
                Ok(v)
            }
            ExprIr::Assign { value, span, .. } => {
                // Special: Seq(Global(name), value) from desugar
                if let ExprIr::Seq(parts, _) = value.as_ref() {
                    if parts.len() == 2 {
                        if let ExprIr::Global(name) = &parts[0] {
                            let v = self.eval_operand(&parts[1]).await?;
                            self.ctx
                                .env
                                .assign(name, v.clone())
                                .map_err(EvalError::Message)?;
                            return Ok(v);
                        }
                    }
                }
                let _ = span;
                let v = self.eval_operand(value).await?;
                Ok(v)
            }
            ExprIr::Call { callee, args, .. } => {
                let c = self.eval_operand(callee).await?;
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_operand(a).await?);
                }
                self.call_value(c, argv).await
            }
            ExprIr::NativeCall { name, args, .. } => {
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_operand(a).await?);
                }
                self.call_native(name, argv).await
            }
            ExprIr::CapabilityCall {
                path, args, effect, ..
            } => {
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval_operand(a).await?);
                }
                let eff = matches!(effect, EffectKind::Effect);
                // Special-case console for when host not fully wired
                if path.first().map(|s| s.as_str()) == Some("console") {
                    return self.eval_console(path, argv).await;
                }
                self.ctx.capabilities.call(path, argv, eff, self.ctx).await
            }
            ExprIr::Closure(c) => Ok(Value::Function(Closure {
                id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
                name: None,
                params: c.param_names.clone(),
                // Capture the defining chain. Frames are shared, so this is a handful of
                // `Arc` clones, the closure keeps them alive after the defining call
                // returns, and mutables assigned through it are visible to that scope.
                env: Arc::new(parking_lot::RwLock::new(self.ctx.env.clone())),
                body: c.body.clone(),
                // A lambda has no annotation syntax, so there is nothing to enforce.
                contract: None,
            })),
            ExprIr::Pipeline { input, stages, .. } => {
                let mut val = self.eval_operand(input).await?;
                for stage in stages {
                    val = self.eval_pipeline_stage(val, stage).await?;
                }
                Ok(val)
            }
            ExprIr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let cond = self.eval_operand(condition).await?;
                if cond.is_truthy() {
                    self.eval_block(then_branch).await
                } else if let Some(e) = else_branch {
                    self.eval_block(e).await
                } else {
                    Ok(Value::None)
                }
            }
            ExprIr::Match { value, arms, .. } => {
                let scrut = self.eval_operand(value).await?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &scrut)? {
                        self.ctx.env.push_frame();
                        for (name, val) in bindings {
                            self.ctx.env.define_name(&name, val, false);
                        }
                        let result = self.eval_operand(&arm.body).await;
                        self.ctx.env.pop_frame();
                        return result;
                    }
                }
                // The value is right there and used to be left out, so the message
                // said only that something had not matched — in a `~` over a record
                // or an atom, that is the one thing you already knew.
                Err(EvalError::Message(format!(
                    "match failure: no arm matched {} value `{}`",
                    scrut.type_name(),
                    scrut.to_display(&self.ctx.atoms)
                )))
            }
            ExprIr::Return(val, _) => {
                let v = match val {
                    Some(e) => self.eval_operand(e).await?,
                    None => Value::None,
                };
                Err(EvalError::Return(v))
            }
            ExprIr::BuildList(xs, _) => {
                let mut out = im::Vector::new();
                for x in xs {
                    out.push_back(self.eval_operand(x).await?);
                }
                Ok(Value::List(out))
            }
            ExprIr::BuildRecord(entries, _) => {
                let mut rec = IndexMap::new();
                for (k, v) in entries {
                    rec.insert(key_of(k), self.eval_operand(v).await?);
                }
                Ok(Value::Record(rec))
            }
            ExprIr::Member { object, field, .. } => {
                let obj = self.eval_operand(object).await?;
                Ok(obj.get_field(field))
            }
            ExprIr::Index { object, index, .. } => {
                let obj = self.eval_operand(object).await?;
                let idx = self.eval_operand(index).await?;
                Ok(self.index_value(&obj, &idx))
            }
            ExprIr::Unary { op, expr, .. } => {
                let v = self.eval_operand(expr).await?;
                self.apply_unary(*op, v)
            }
            ExprIr::Binary {
                op, left, right, ..
            } => self.eval_binary(*op, left, right).await,
            ExprIr::Try { expr, .. } => {
                let v = self.eval_operand(expr).await?;
                self.unwrap_try(v)
            }
            ExprIr::Coalesce { left, right, .. } => {
                let l = self.eval_operand(left).await?;
                if matches!(l, Value::None) {
                    self.eval_operand(right).await
                } else {
                    Ok(l)
                }
            }
            ExprIr::Block(b) => self.eval_block(b).await,
            ExprIr::Atom(name, _) => Ok(Value::Atom(self.ctx.atoms.intern(name))),
            ExprIr::Placeholder(_) => Err(EvalError::Message(
                "pipeline placeholder `$` outside pipeline stage".into(),
            )),
            ExprIr::HttpListen {
                addr,
                routes,
                middleware,
                ..
            } => {
                let a = self.eval_operand(addr).await?;
                // `to_display`, not `Display`: the latter cannot resolve an atom and
                // renders it as its interner index, so a non-string address reported a
                // bind failure for `#0` instead of naming what was written.
                let addr_str = a
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| a.to_display(&self.ctx.atoms));
                // Middleware: `use @http.log` → Named, `use { |req, next| … }` → Function
                let mut mw_specs: Vec<crate::value::HttpMiddleware> = Vec::new();
                for m in middleware {
                    match m {
                        ExprIr::CapabilityCall { path, .. } => {
                            let name = path.join(".");
                            let short = name
                                .strip_prefix("http.")
                                .unwrap_or(name.as_str())
                                .trim_start_matches('@')
                                .to_string();
                            mw_specs.push(crate::value::HttpMiddleware::Named(short));
                        }
                        ExprIr::NativeCall { name, .. } => {
                            mw_specs.push(crate::value::HttpMiddleware::Named(name.clone()));
                        }
                        ExprIr::Global(name) => {
                            mw_specs.push(crate::value::HttpMiddleware::Named(name.clone()));
                        }
                        ExprIr::Closure(_) => {
                            let v = self.eval_operand(m).await?;
                            if let Value::Function(c) = v {
                                mw_specs.push(crate::value::HttpMiddleware::Function(c));
                            }
                        }
                        other => {
                            // Evaluate (e.g. already-resolved values) and classify
                            let v = self.eval_operand(other).await?;
                            match v {
                                Value::Function(c) => {
                                    mw_specs.push(crate::value::HttpMiddleware::Function(c));
                                }
                                Value::Atom(id) => {
                                    let n = self.ctx.atoms.name(id);
                                    let short =
                                        n.strip_prefix("http.").unwrap_or(n.as_str()).to_string();
                                    mw_specs.push(crate::value::HttpMiddleware::Named(short));
                                }
                                Value::String(s) => {
                                    mw_specs
                                        .push(crate::value::HttpMiddleware::Named(s.to_string()));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Build route table with real Rite bodies for per-request evaluation
                let rite_routes: Vec<Value> = routes
                    .iter()
                    .map(|r| {
                        Value::record(vec![
                            (
                                Key::String("method".into()),
                                Value::string(r.method.clone()),
                            ),
                            (Key::String("path".into()), Value::string(r.path.clone())),
                        ])
                    })
                    .collect();
                self.ctx.pending_http = Some(PendingHttpServer {
                    addr: addr_str.clone(),
                    routes: routes.to_vec(),
                    middleware: mw_specs.clone(),
                });
                self.ctx
                    .capabilities
                    .call(
                        &["http".into(), "listen".into()],
                        vec![a, Value::list(rite_routes)],
                        true,
                        self.ctx,
                    )
                    .await
            }
            ExprIr::Seq(xs, _) => {
                let mut last = Value::None;
                for x in xs {
                    last = self.eval_operand(x).await?;
                }
                Ok(last)
            }
        }
    } // end eval_expr_inner

    pub(super) async fn eval_pipeline_stage(
        &mut self,
        input: Value,
        stage: &rite_sem::PipelineStageIr,
    ) -> Result<Value, EvalError> {
        match &stage.kind {
            StageKind::MemberProjection(field) => match input {
                Value::List(xs) => {
                    let out: Vec<Value> = xs.iter().map(|x| x.get_field(field)).collect();
                    Ok(Value::list(out))
                }
                other => Ok(other.get_field(field)),
            },
            StageKind::Block | StageKind::Call => {
                match &stage.expr {
                    ExprIr::NativeCall { name, args, .. } => {
                        let mut argv = vec![input];
                        for a in args {
                            argv.push(self.eval_operand(a).await?);
                        }
                        // special: map with member projection stage already handled
                        self.call_native(name, argv).await
                    }
                    ExprIr::Call { callee, args, .. } => {
                        // Check for $ placeholder in args
                        let mut argv = Vec::new();
                        let mut used_placeholder = false;
                        for a in args {
                            if matches!(a, ExprIr::Placeholder(_)) {
                                argv.push(input.clone());
                                used_placeholder = true;
                            } else {
                                argv.push(self.eval_operand(a).await?);
                            }
                        }
                        if !used_placeholder {
                            argv.insert(0, input);
                        }
                        let c = self.eval_operand(callee).await?;
                        self.call_value(c, argv).await
                    }
                    ExprIr::Closure(c) => {
                        let clos = Value::Function(Closure {
                            id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
                            name: None,
                            params: c.param_names.clone(),
                            env: Arc::new(parking_lot::RwLock::new(self.ctx.env.clone())),
                            body: c.body.clone(),
                            contract: None,
                        });
                        // If input is list and closure looks like map body — pipeline stage
                        // is itself the function applied to input
                        self.call_value(clos, vec![input]).await
                    }
                    // No `ExprIr::Global` arm on purpose. Sending a bare name straight
                    // to `call_native` consulted the builtin table and nothing else,
                    // so a user's own function was unreachable as a stage and a
                    // builtin of the same name won over a definition that shadowed it
                    // everywhere else in the language. The `other` arm below evaluates
                    // the name through `lookup_global` — binding, then function, then
                    // builtin — and `call_value` dispatches a `NativeName` back to
                    // `call_native`, so builtin stages still land where they did. The
                    // cost is one environment lookup per stage.
                    ExprIr::Member { object, field, .. }
                        if matches!(object.as_ref(), ExprIr::Placeholder(_)) =>
                    {
                        Ok(input.get_field(field))
                    }
                    other => {
                        // Evaluate stage as function then call with input
                        let f = self.eval_operand(other).await?;
                        self.call_value(f, vec![input]).await
                    }
                }
            }
        }
    }

    /// Resolve a bare global name: a binding, a registered function, or a builtin token.
    pub(super) fn lookup_global(&mut self, name: &str) -> Result<Value, EvalError> {
        crate::lookup_global(self.ctx, name)
    }

    /// Apply a unary operator to an already-evaluated value.
    pub(super) fn apply_unary(&mut self, op: UnaryOpIr, v: Value) -> Result<Value, EvalError> {
        crate::ops::unary(op, v)
    }

    pub(super) fn index_value(&mut self, obj: &Value, idx: &Value) -> Value {
        crate::ops::index(obj, idx)
    }

    pub(super) fn unwrap_try(&mut self, v: Value) -> Result<Value, EvalError> {
        crate::ops::unwrap_try(v)
    }

    /// Evaluate an operand, allocating a future only if it actually needs one.
    ///
    /// `eval_expr` boxes: it is the recursion break for an async tree-walker, and the
    /// allocation happens whether or not the node does anything async. Most nodes in a
    /// hot loop — constants, locals, field reads, arithmetic — cannot suspend at all, so
    /// they run directly and the box is never allocated.
    ///
    /// The decision is made by `is_sync`, which only *inspects* the tree. An earlier
    /// version tried evaluating and bailed out on reaching an async node: the abandoned
    /// work had already charged the step budget, and the async path then charged for it
    /// again. Deciding first also rules out double-applying an environment mutation —
    /// not reachable from source today, since `←` and `:=` are statement forms and so
    /// cannot sit beside a call inside one expression, but not something to leave
    /// resting on that.
    pub(super) async fn eval_operand(&mut self, expr: &ExprIr) -> Result<Value, EvalError> {
        if is_sync(expr) {
            return self.eval_sync(expr);
        }
        self.eval_expr(expr).await
    }

    /// Evaluate a subtree that [`is_sync`] has already accepted.
    ///
    /// Every arm delegates to the same helper the async path uses (`apply_binary`,
    /// `apply_unary`, `index_value`, `unwrap_try`, `Value::get_field`), so there is one
    /// implementation of each operation rather than a fast copy that can drift.
    ///
    /// Panics only if called on a node `is_sync` rejects — the two must stay in step, and
    /// `sync_and_async_paths_agree` pins that.
    pub(super) fn eval_sync(&mut self, expr: &ExprIr) -> Result<Value, EvalError> {
        // Charged once per node, exactly as `eval_expr_inner` does.
        self.ctx.budget.tick()?;
        match expr {
            ExprIr::Constant(lit) => Ok(self.lit_to_value(lit)),
            ExprIr::Atom(name, _) => Ok(Value::Atom(self.ctx.atoms.intern(name))),
            ExprIr::Local(id) => self
                .ctx
                .env
                .get_local(*id)
                .ok_or_else(|| EvalError::Message(format!("undefined local {}", id.0))),
            ExprIr::Global(name) => self.lookup_global(name),
            ExprIr::Unary { op, expr, .. } => {
                let v = self.eval_sync(expr)?;
                self.apply_unary(*op, v)
            }
            ExprIr::Binary {
                op, left, right, ..
            } => {
                if matches!(op, BinaryOpIr::And | BinaryOpIr::Or) {
                    // Short-circuit: the right side must not run when the left decides.
                    let l = self.eval_sync(left)?.is_truthy();
                    let decided = if *op == BinaryOpIr::And { !l } else { l };
                    if decided {
                        return Ok(Value::Bool(l));
                    }
                    return Ok(Value::Bool(self.eval_sync(right)?.is_truthy()));
                }
                let l = self.eval_sync(left)?;
                let r = self.eval_sync(right)?;
                self.apply_binary(*op, l, r)
            }
            ExprIr::Member { object, field, .. } => {
                let obj = self.eval_sync(object)?;
                Ok(obj.get_field(field))
            }
            ExprIr::Index { object, index, .. } => {
                let obj = self.eval_sync(object)?;
                let idx = self.eval_sync(index)?;
                Ok(self.index_value(&obj, &idx))
            }
            ExprIr::Coalesce { left, right, .. } => {
                let l = self.eval_sync(left)?;
                if matches!(l, Value::None) {
                    self.eval_sync(right)
                } else {
                    Ok(l)
                }
            }
            ExprIr::Bind {
                local,
                name,
                mutable,
                value,
                ..
            } => {
                let v = self.eval_sync(value)?;
                self.ctx.env.define(name, *local, v.clone(), *mutable);
                Ok(v)
            }
            // `x := v`, which desugar lowers to `Assign { value: Seq[Global, value] }`.
            ExprIr::Assign { value, .. } => {
                if let ExprIr::Seq(parts, _) = value.as_ref() {
                    if parts.len() == 2 {
                        if let ExprIr::Global(name) = &parts[0] {
                            let v = self.eval_sync(&parts[1])?;
                            return self
                                .ctx
                                .env
                                .assign(name, v.clone())
                                .map(|()| v)
                                .map_err(EvalError::Message);
                        }
                    }
                }
                self.eval_sync(value)
            }
            ExprIr::Seq(parts, _) => {
                let mut last = Value::None;
                for part in parts {
                    last = self.eval_sync(part)?;
                }
                Ok(last)
            }
            ExprIr::BuildList(items, _) => {
                let mut out = im::Vector::new();
                for item in items {
                    out.push_back(self.eval_sync(item)?);
                }
                Ok(Value::List(out))
            }
            ExprIr::BuildRecord(entries, _) => {
                let mut rec = IndexMap::new();
                for (k, v) in entries {
                    let value = self.eval_sync(v)?;
                    rec.insert(key_of(k), value);
                }
                Ok(Value::Record(rec))
            }
            ExprIr::Try { expr, .. } => {
                let v = self.eval_sync(expr)?;
                self.unwrap_try(v)
            }
            other => Err(EvalError::Message(format!(
                "internal: eval_sync reached a node it cannot handle ({:?}); \
                 is_sync and eval_sync disagree",
                std::mem::discriminant(other)
            ))),
        }
    }

    pub(super) async fn eval_binary(
        &mut self,
        op: BinaryOpIr,
        left: &ExprIr,
        right: &ExprIr,
    ) -> Result<Value, EvalError> {
        // Short-circuit and/or
        if op == BinaryOpIr::And {
            let l = self.eval_operand(left).await?;
            if !l.is_truthy() {
                return Ok(Value::Bool(false));
            }
            let r = self.eval_operand(right).await?;
            return Ok(Value::Bool(r.is_truthy()));
        }
        if op == BinaryOpIr::Or {
            let l = self.eval_operand(left).await?;
            if l.is_truthy() {
                return Ok(Value::Bool(true));
            }
            let r = self.eval_operand(right).await?;
            return Ok(Value::Bool(r.is_truthy()));
        }

        let l = self.eval_operand(left).await?;
        let r = self.eval_operand(right).await?;
        self.apply_binary(op, l, r)
    }

    /// Apply a binary operator to values that are already evaluated.
    ///
    /// The single implementation of operator semantics: both the async path above and
    /// the allocation-free path in `try_sync` funnel through here, so the two cannot
    /// drift into disagreeing about what `+` means.
    pub(super) fn apply_binary(
        &mut self,
        op: BinaryOpIr,
        l: Value,
        r: Value,
    ) -> Result<Value, EvalError> {
        crate::ops::binary(&self.ctx.atoms, op, l, r)
    }

    pub(super) async fn eval_block(&mut self, block: &BlockIr) -> Result<Value, EvalError> {
        // Nested blocks (if/match bodies, bare blocks) must *propagate* `^` / `return`
        // as `EvalError::Return` so it can exit the enclosing function. Only
        // `call_block` (function/closure boundary) converts Return → Ok.
        self.ctx.env.push_frame();
        let mut last = Value::None;
        let result = async {
            for expr in &block.body {
                match self.eval_operand(expr).await {
                    Err(EvalError::Return(v)) => return Err(EvalError::Return(v)),
                    Err(e) => return Err(e),
                    Ok(v) => last = v,
                }
            }
            Ok(last)
        }
        .await;
        self.ctx.env.pop_frame();
        result
    }
}
