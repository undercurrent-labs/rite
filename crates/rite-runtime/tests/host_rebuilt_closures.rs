//! Hosts that invoke Rite functions outside a running program rebuild `Closure`
//! values themselves with a fresh `Environment` (the HTTP capability layer does this
//! per request). Those callables must still resolve module-level names from the
//! context they are invoked in.

use parking_lot::RwLock;
use rite_runtime::{run_source, Closure, Environment, Evaluator, RuntimeContext, Value};
use std::sync::Arc;

/// Rebuild a function value the way a host does: params and body from
/// `ctx.functions`, environment freshly created rather than captured.
fn detached(ctx: &RuntimeContext, name: &str) -> Value {
    let entry = ctx
        .functions
        .get(name)
        .unwrap_or_else(|| panic!("no function {name}"));
    Value::Function(Closure {
        id: 0,
        name: Some(name.to_string()),
        params: entry.params.clone(),
        env: Arc::new(RwLock::new(Environment::new())),
        body: entry.body.clone(),
    })
}

#[tokio::test]
async fn detached_function_value_resolves_sibling_functions() {
    let mut ctx = RuntimeContext::new();
    run_source(
        "t.rite",
        r#"
◆ double(x) ⟦ ^ x * 2 ⟧
◆ outer(x) ⟦ ^ double(x) + 1 ⟧
none
"#,
        &mut ctx,
    )
    .await
    .unwrap();

    let callable = detached(&ctx, "outer");
    let mut ev = Evaluator::new(&mut ctx);
    let v = ev.call_value_public(callable, vec![Value::Int(20)]).await;
    assert_eq!(v.unwrap(), Value::Int(41));
}

#[tokio::test]
async fn detached_function_value_resolves_module_level_bindings() {
    let mut ctx = RuntimeContext::new();
    run_source(
        "t.rite",
        r#"
factor ← 10
◆ scale(x) ⟦ ^ x * factor ⟧
none
"#,
        &mut ctx,
    )
    .await
    .unwrap();

    let callable = detached(&ctx, "scale");
    let mut ev = Evaluator::new(&mut ctx);
    let v = ev.call_value_public(callable, vec![Value::Int(4)]).await;
    assert_eq!(v.unwrap(), Value::Int(40));
}

/// A closure captured normally still wins over the invoking context's globals.
#[tokio::test]
async fn captured_bindings_win_over_host_globals() {
    let mut ctx = RuntimeContext::new();
    let v = run_source(
        "t.rite",
        r#"
factor ← 10
◆ make_scaler(factor) ⟦ ^ { |x| x * factor } ⟧
scaler ← make_scaler(3)
scaler(4)
"#,
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(v, Value::Int(12));

    // Invoking the same closure value through the host entry point behaves the same.
    let scaler = ctx.env.get("scaler").expect("scaler bound");
    let mut ev = Evaluator::new(&mut ctx);
    let v = ev.call_value_public(scaler, vec![Value::Int(4)]).await;
    assert_eq!(v.unwrap(), Value::Int(12));
}
