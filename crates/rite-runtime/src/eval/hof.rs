//! Builtins that take a callback, so they must re-enter the evaluator.

use super::*;
use crate::value::{Closure, Value};
use indexmap::IndexMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

impl<'a> Evaluator<'a> {
    pub(super) async fn builtin_while_loop(
        &mut self,
        args: Vec<Value>,
    ) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let pred = it.next().unwrap_or(Value::None);
        let body = it.next().unwrap_or(Value::None);
        let mut steps = 0u64;
        loop {
            self.ctx.budget.tick()?;
            steps += 1;
            if steps > 1_000_000 {
                return Err(EvalError::Message(
                    "while loop exceeded iteration guard".into(),
                ));
            }
            let c = self.call_value(pred.clone(), vec![Value::None]).await?;
            if !c.is_truthy() {
                break;
            }
            let _ = self.call_value(body.clone(), vec![Value::None]).await?;
        }
        Ok(Value::None)
    }

    pub(super) async fn builtin_compose(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        // compose(f, g) → function x => f(g(x)); compose(f, g, x) applies immediately.
        let mut it = args.into_iter();
        let f = it.next().unwrap_or(Value::None);
        let g = it.next().unwrap_or(Value::None);
        if let Some(x) = it.next() {
            let y = self.call_value(g, vec![x]).await?;
            return self.call_value(f, vec![y]).await;
        }
        use rite_core::Span;
        use rite_sem::{BlockIr, ExprIr, LocalId};
        // Private frame layered over the ambient scope holds the two composed functions.
        let mut env = self.ctx.env.clone();
        env.push_frame();
        env.define_name("__f", f, false);
        env.define_name("__g", g, false);
        let body = BlockIr {
            params: vec![LocalId(0)],
            body: vec![ExprIr::Call {
                callee: Box::new(ExprIr::Global("__f".into())),
                args: vec![ExprIr::Call {
                    callee: Box::new(ExprIr::Global("__g".into())),
                    args: vec![ExprIr::Global("x".into())],
                    span: Span::DUMMY,
                }],
                span: Span::DUMMY,
            }],
            span: Span::DUMMY,
        };
        Ok(Value::Function(Closure {
            id: CLOSURE_ID.fetch_add(1, Ordering::Relaxed),
            name: Some("compose".into()),
            params: vec!["x".into()],
            env: Arc::new(parking_lot::RwLock::new(env)),
            body,
        }))
    }

    pub(super) async fn builtin_map(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            Some(other) => {
                return Err(EvalError::Message(format!(
                    "map expects list, got {}",
                    other.type_name()
                )))
            }
            None => return Ok(Value::list(Vec::<Value>::new())),
        };
        let f = it.next().unwrap_or(Value::None);
        // Member projection style: second arg missing, used from pipeline with projection stages
        let mut out = im::Vector::new();
        for item in list {
            let mapped = match &f {
                _ if f.is_callable() => self.call_value(f.clone(), vec![item]).await?,
                Value::None => item,
                other => {
                    // If f is not a function, treat as identity error
                    return Err(EvalError::Message(format!(
                        "map expects function, got {}",
                        other.type_name()
                    )));
                }
            };
            out.push_back(mapped);
        }
        Ok(Value::List(out))
    }

    pub(super) async fn builtin_filter(
        &mut self,
        args: Vec<Value>,
        keep: bool,
    ) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::list(Vec::<Value>::new())),
        };
        let f = it.next().unwrap_or(Value::None);
        let mut out = im::Vector::new();
        for item in list {
            let pred = self.call_value(f.clone(), vec![item.clone()]).await?;
            if pred.is_truthy() == keep {
                out.push_back(item);
            }
        }
        Ok(Value::List(out))
    }

    pub(super) async fn builtin_each(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        for item in list {
            let _ = self.call_value(f.clone(), vec![item]).await?;
        }
        Ok(Value::None)
    }

    pub(super) async fn builtin_reduce(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        let mut acc = it.next().unwrap_or(Value::None);
        for item in list {
            acc = self.call_value(f.clone(), vec![acc, item]).await?;
        }
        Ok(acc)
    }

    /// `and_then(result, f)` — call `f` with the value of an `ok`, pass an `err` through.
    ///
    /// This lived in the pure builtin table, which cannot invoke a closure, so it
    /// silently ignored its function and answered its input: `and_then(ok(2), { |n|
    /// ok(n * 10) })` gave `ok(2)`. A chain built on it looked like it worked and did
    /// nothing, which is the worst way for a combinator to fail.
    pub(super) async fn builtin_and_then(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let subject = it.next().unwrap_or(Value::None);
        let f = it.next().unwrap_or(Value::None);
        match subject {
            Value::Result(crate::value::ResultValue::Ok(v)) => self.call_value(f, vec![*v]).await,
            // `err` short-circuits, which is the whole point: the function is not called
            // and the original error travels on unchanged.
            other @ Value::Result(crate::value::ResultValue::Err(_)) => Ok(other),
            // Not a result at all. Treated as the value, so `and_then` composes with
            // functions that answer plainly rather than demanding a wrapper first.
            other => self.call_value(f, vec![other]).await,
        }
    }

    pub(super) async fn builtin_find(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::None),
        };
        let f = it.next().unwrap_or(Value::None);
        for item in list {
            let pred = self.call_value(f.clone(), vec![item.clone()]).await?;
            if pred.is_truthy() {
                return Ok(item);
            }
        }
        Ok(Value::None)
    }

    pub(super) async fn builtin_any_all(
        &mut self,
        args: Vec<Value>,
        is_any: bool,
    ) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::Bool(!is_any)),
        };
        let f = it.next();
        for item in list {
            let pred = if let Some(ref func) = f {
                self.call_value(func.clone(), vec![item]).await?
            } else {
                item
            };
            if is_any && pred.is_truthy() {
                return Ok(Value::Bool(true));
            }
            if !is_any && !pred.is_truthy() {
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(!is_any))
    }

    pub(super) async fn builtin_group(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        // group list by field name if second is string, or function
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            _ => return Ok(Value::list(Vec::<Value>::new())),
        };
        let key_fn = it.next();
        let mut groups: IndexMap<String, im::Vector<Value>> = IndexMap::new();
        for item in list {
            let key = match &key_fn {
                Some(Value::String(s)) => item.get_field(s).to_string(),
                Some(f) if f.is_callable() => {
                    let k = self
                        .call_value(key_fn.clone().unwrap(), vec![item.clone()])
                        .await?;
                    k.to_string()
                }
                _ => item.to_string(),
            };
            groups.entry(key).or_default().push_back(item);
        }
        let out: Vec<Value> = groups
            .into_iter()
            .map(|(k, vs)| Value::list(vec![Value::string(k), Value::List(vs)]))
            .collect();
        Ok(Value::list(out))
    }
}

impl Evaluator<'_> {
    /// `parallel(xs, f)` — like `map`, but the branches make progress together.
    ///
    /// This is concurrency, not parallelism: branches interleave whenever one
    /// awaits, which is where the time goes for the work worth doing this way —
    /// several HTTP requests, several file reads. Pure arithmetic gains nothing
    /// and should use `map`, which does not pay for forked contexts.
    ///
    /// Two properties hold regardless of how the branches happen to schedule:
    /// results come back in input order, and each branch's output is spliced in
    /// input order. Running the same program twice prints the same thing.
    ///
    /// The first failure in *input* order is the one reported, not the first to
    /// occur in time — for the same reason.
    pub(super) async fn builtin_parallel(&mut self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut it = args.into_iter();
        let list = match it.next() {
            Some(Value::List(xs)) => xs,
            Some(other) => {
                return Err(EvalError::Message(format!(
                    "parallel expects list, got {}",
                    other.type_name()
                )))
            }
            None => return Ok(Value::list(Vec::<Value>::new())),
        };
        let f = it.next().unwrap_or(Value::None);
        if !f.is_callable() {
            return Err(EvalError::Message(format!(
                "parallel expects function, got {}",
                f.type_name()
            )));
        }

        // One context per branch: the evaluator borrows the context mutably, so
        // sharing one would serialise them again. `fork` shares the host, the
        // budget and the atom table; only output is kept apart.
        let mut branches: Vec<RuntimeContext> = (0..list.len()).map(|_| self.ctx.fork()).collect();

        let futures: Vec<_> = list
            .into_iter()
            .zip(branches.iter_mut())
            .map(|(item, branch)| {
                let f = f.clone();
                async move {
                    Evaluator::new(branch)
                        .call_value_public(f, vec![item])
                        .await
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        // Splice output back before reporting a failure, so what a branch printed
        // before it failed is not lost.
        for branch in branches {
            self.ctx.absorb(branch);
        }

        let mut out = im::Vector::new();
        for r in results {
            out.push_back(r?);
        }
        Ok(Value::List(out))
    }
}
