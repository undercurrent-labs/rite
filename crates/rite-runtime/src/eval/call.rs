//! Calling closures, blocks and native builtins.

use super::*;
use crate::builtins::call_builtin;
use crate::value::Value;
use rite_sem::BlockIr;

impl<'a> Evaluator<'a> {
    pub(super) async fn call_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.ctx.budget.check_depth(self.ctx.call_depth + 1)?;
        self.ctx.call_depth += 1;
        let result = match callee {
            Value::Function(c) => {
                // Lexical scoping: a closure always runs in the environment it captured,
                // extended with the fresh frame `call_block` pushes for its parameters.
                // Frames are shared (see `env::Environment`), so the capture still sees
                // — and assigns through to — the defining scope's mutable bindings, which
                // is what makes `count := count + 1` inside an `each`/`while_loop` body
                // visible to the enclosing scope.
                // Declared types are checked here rather than inside `call_block`
                // because this is where the function *value* is in hand — a contract
                // has to travel with the value, since `f ← shout` and `each(xs, f)`
                // both call it under a name it was not declared with.
                // No `with_stack` here: the caller's `call_block` adds the trace as
                // the error propagates, and adding one at both ends printed the
                // traceback twice.
                if let Some(contract) = &c.contract {
                    if let Err(e) = check_contract_params(contract, &args) {
                        self.ctx.call_depth -= 1;
                        return Err(e);
                    }
                }
                let mut captured = c.env.read().clone();
                captured.ensure_globals_from(&self.ctx.env);
                let saved = std::mem::replace(&mut self.ctx.env, captured);
                let r = self.call_block(&c.body, &c.params, args).await;
                self.ctx.env = saved;
                match (r, &c.contract) {
                    (Ok(v), Some(contract)) => match &contract.return_type {
                        Some(ty) => {
                            crate::ops::check_return_type(&contract.name, &v, ty).map(|()| v)
                        }
                        None => Ok(v),
                    },
                    (r, _) => r,
                }
            }
            // Same contract as the interpreted arm above: run in the captured environment,
            // extended with a fresh frame for the parameters. `ensure_globals_from` and the
            // shared frames are what let a compiled `{ |n| total := total + n }` assign
            // through to the scope that defined `total`.
            Value::NativeClosure(c) => {
                if c.params.len() != args.len() {
                    self.ctx.call_depth -= 1;
                    return Err(EvalError::Message(format!(
                        "arity mismatch: expected {} args, got {}",
                        c.params.len(),
                        args.len()
                    )));
                }
                let mut captured = c.env.read().clone();
                captured.ensure_globals_from(&self.ctx.env);
                let saved = std::mem::replace(&mut self.ctx.env, captured);
                // The generated function pushes its own frame and binds its parameters,
                // the way `call_block` does for an interpreted body — it knows the local
                // ids, which are not in `params`.
                let r = (c.func)(self.ctx, args).await;
                self.ctx.env = saved;
                // `^` inside a closure body ends the closure, as it does for an
                // interpreted one.
                match r {
                    Err(EvalError::Return(v)) => Ok(v),
                    other => other,
                }
            }
            Value::NativeName(name) => {
                // Indirection avoids infinitely sized async future (call_value ↔ map/each).
                let name = name.clone();
                Box::pin(self.call_native(&name, args)).await
            }
            Value::NativeFunction(_) => Err(EvalError::Message(
                "native function id call not wired".into(),
            )),
            Value::Handle(h) if h.kind == "http.next" => {
                let invoker = self.ctx.http_next.clone();
                match invoker {
                    Some(f) => Box::pin(f(h.id, args)).await,
                    None => Err(EvalError::Message(
                        "http middleware next() is only valid inside a request handler chain"
                            .into(),
                    )),
                }
            }
            other => Err(EvalError::Message(format!(
                "cannot call value of type {}",
                other.type_name()
            ))),
        };
        self.ctx.call_depth -= 1;
        result
    }

    pub async fn call_block_public(
        &mut self,
        body: &BlockIr,
        params: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.call_block(body, params, args).await
    }

    /// Call a Rite function/closure value (used by HTTP middleware).
    pub async fn call_value_public(
        &mut self,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.call_value(callee, args).await
    }

    /// Call a builtin by name, including the ones that take a callback.
    ///
    /// `builtins::call_builtin` handles the pure ones, but `map`, `keep`, `reduce`,
    /// `each`, `print` and friends need to re-enter the evaluator — to invoke a closure
    /// argument, or to reach the context's output buffer — so they are dispatched here
    /// instead. Code generated by `rite build` has a `&mut RuntimeContext` and needs the
    /// same entry point, or `[1,2] → map { |n| n * 2 }` would have to be reimplemented
    /// on the other side of the boundary.
    pub async fn call_native_public(
        &mut self,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        self.call_native(name, args).await
    }

    pub(super) async fn call_block(
        &mut self,
        body: &BlockIr,
        params: &[String],
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        if params.len() != args.len() && !params.is_empty() {
            // Allow under-application only if zero params? Spec: invalid arity error
            if params.len() != args.len() {
                return Err(EvalError::Message(format!(
                    "arity mismatch: expected {} args, got {}",
                    params.len(),
                    args.len()
                )));
            }
        }
        let frame_name = params
            .first()
            .map(|p| format!("fn({})", p))
            .unwrap_or_else(|| "fn".into());
        self.ctx.call_stack.push(StackFrame {
            name: frame_name,
            span: body.span,
        });
        self.ctx.env.push_frame();
        for (i, p) in params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(Value::None);
            self.ctx.env.define_name(p, v, false);
        }
        // also bind block params by local ids if present
        for (i, lid) in body.params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(Value::None);
            let name = params.get(i).cloned().unwrap_or_else(|| format!("${}", i));
            self.ctx.env.define(&name, *lid, v, false);
        }
        let mut last = Value::None;
        let result = async {
            for expr in &body.body {
                match self.eval_operand(expr).await {
                    Err(EvalError::Return(v)) => return Ok(v),
                    Err(e) => return Err(e.with_stack(self.ctx)),
                    Ok(v) => last = v,
                }
            }
            Ok(last)
        }
        .await;
        self.ctx.env.pop_frame();
        self.ctx.call_stack.pop();
        result
    }

    pub(super) async fn call_native(
        &mut self,
        name: &str,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        match name {
            "map" => self.builtin_map(args).await,
            "keep" => self.builtin_filter(args, true).await,
            "reject" => self.builtin_filter(args, false).await,
            "each" => self.builtin_each(args).await,
            "reduce" => self.builtin_reduce(args).await,
            "find" => self.builtin_find(args).await,
            "any" => self.builtin_any_all(args, true).await,
            "all" => self.builtin_any_all(args, false).await,
            "group" => self.builtin_group(args).await,
            "parallel" => self.builtin_parallel(args).await,
            "import" => Ok(Value::None), // module loading handled at higher layer
            "while_loop" => self.builtin_while_loop(args).await,
            "compose" => self.builtin_compose(args).await,
            "and_then" => self.builtin_and_then(args).await,
            "print" | "println" => {
                let s = args
                    .first()
                    .map(|v| v.to_display(&self.ctx.atoms))
                    .unwrap_or_default();
                if name == "println" {
                    self.ctx.print(format!("{}\n", s));
                } else {
                    self.ctx.print(s);
                }
                Ok(Value::None)
            }
            other => call_builtin(other, args, &self.ctx.atoms),
        }
    }
}

/// Check a call's arguments against the function's declared parameter types.
///
/// Arity is checked separately and reported on its own, so a short argument list
/// simply has nothing to check here — reporting "parameter `y` expects int, got
/// none" for a missing argument would name the wrong mistake.
fn check_contract_params(
    contract: &crate::value::FnContract,
    args: &[Value],
) -> Result<(), EvalError> {
    for (i, ty) in contract.param_types.iter().enumerate() {
        let Some(ty) = ty else { continue };
        let Some(v) = args.get(i) else { continue };
        let name = contract
            .param_names
            .get(i)
            .map(String::as_str)
            .unwrap_or("?");
        crate::ops::check_param_type(&contract.name, name, v, ty)?;
    }
    Ok(())
}
