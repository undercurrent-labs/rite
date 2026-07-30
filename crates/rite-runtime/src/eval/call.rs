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
                let mut captured = c.env.read().clone();
                captured.ensure_globals_from(&self.ctx.env);
                let saved = std::mem::replace(&mut self.ctx.env, captured);
                let r = self.call_block(&c.body, &c.params, args).await;
                self.ctx.env = saved;
                r
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
            "parallel" => self.builtin_map(args).await, // sequential fallback with same semantics for pure
            "import" => Ok(Value::None),                // module loading handled at higher layer
            "while_loop" => self.builtin_while_loop(args).await,
            "compose" => self.builtin_compose(args).await,
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
            other => call_builtin(other, args),
        }
    }
}
