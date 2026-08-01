//! What generated code can reach with only a `&mut RuntimeContext`.
//!
//! `rite build` emitting real Rust has to do everything the tree-walker does without being
//! inside it: apply operators, match patterns, call builtins and closures, and fall back to
//! interpretation for anything the backend cannot lower yet. Each of those was a private
//! method on `Evaluator` until this branch.
//!
//! These tests stand in for the generated code. If one stops compiling, a backend stops
//! being expressible — worth failing a build over rather than discovering halfway through
//! writing codegen.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{
    ops, patterns, run_source, AtomInterner, EvalError, Evaluator, RuntimeContext, Value,
};
use rite_sem::{BinaryOpIr, PatternIr};

fn ctx() -> RuntimeContext {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx
}

/// Compile `source` to IR the way `rite build` does.
fn ir_of(source: &str) -> rite_sem::ProgramIr {
    let file = rite_core::SourceFile::new(rite_core::FileId(0), "gen.rite", source);
    let (ir, diags) = rite_sem::compile_to_ir(&file);
    assert!(!diags.has_errors(), "{source} failed to compile");
    ir.expect("ir")
}

#[test]
fn operators_and_patterns_need_only_the_interner() {
    // The narrow signature is the point: generated code adds two numbers without taking a
    // mutable borrow of the world.
    let atoms = AtomInterner::new();
    let sum = ops::binary(&atoms, BinaryOpIr::Add, Value::Int(2), Value::Int(3)).expect("add");
    assert_eq!(sum.as_int(), Some(5));

    let bound = patterns::match_pattern(
        &atoms,
        &PatternIr::Ident(rite_sem::LocalId(0), "n".into()),
        &Value::Int(7),
    )
    .expect("match")
    .expect("matched");
    assert_eq!(bound, vec![("n".to_string(), Value::Int(7))]);
}

#[test]
fn a_pure_builtin_is_reachable_without_the_evaluator() {
    let out = rite_runtime::builtins::call_builtin(
        "sum",
        vec![Value::list(vec![Value::Int(1), Value::Int(2)])],
        &AtomInterner::new(),
        rite_runtime::Limits::unlimited(),
    )
    .expect("sum");
    assert_eq!(out.as_int(), Some(3));
}

#[tokio::test]
async fn a_callback_builtin_is_reachable_through_the_evaluator() {
    // `map` re-enters the evaluator to invoke its closure argument, so it is not in
    // `call_builtin`. Without a public entry point a compiled `→ map` would have to be
    // reimplemented on the other side of the boundary.
    let mut ctx = ctx();
    let doubler = run_source("gen.rite", "{ |n| n * 2 }", &mut ctx)
        .await
        .expect("closure value");
    assert!(matches!(doubler, Value::Function(_)), "got {doubler:?}");

    let mut ev = Evaluator::new(&mut ctx);
    let mapped = ev
        .call_native_public(
            "map",
            vec![
                Value::list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                doubler,
            ],
        )
        .await
        .expect("map");
    let Value::List(xs) = mapped else {
        panic!("map returned {mapped:?}")
    };
    let got: Vec<i64> = xs.iter().filter_map(|v| v.as_int()).collect();
    assert_eq!(got, vec![2, 4, 6]);
}

#[tokio::test]
async fn console_output_reaches_the_context_buffer() {
    // `print`/`println` are evaluator-dispatched for the same reason: they need the
    // context's buffer, which is what a compiled binary flushes on exit.
    let mut ctx = ctx();
    let mut ev = Evaluator::new(&mut ctx);
    ev.call_native_public("println", vec![Value::string("from generated code")])
        .await
        .expect("println");
    assert_eq!(ctx.stdout.join(""), "from generated code\n");
}

#[tokio::test]
async fn a_recursive_function_needs_its_program_registered_first() {
    // The lesson the budget test below encodes, isolated: registration is a prerequisite
    // for the fallback path, not an optimisation.
    let ir = ir_of("◆ down(n) ⟦ ^ ? n <= 0 ⟦ #done ⟧ : ⟦ down(n - 1) ⟧ ⟧\ndown(3)\n");
    let func = ir.functions.iter().find(|f| f.name == "down").expect("fn");

    let mut bare = ctx();
    let mut ev = Evaluator::new(&mut bare);
    let err = ev
        .call_block_public(&func.body, &func.param_names, vec![Value::Int(2)])
        .await
        .expect_err("an unregistered program cannot resolve its own recursion");
    assert!(err.to_string().contains("undefined name"), "{err}");

    let mut ready = ctx();
    rite_runtime::register_functions(&ir, &mut ready);
    let mut ev = Evaluator::new(&mut ready);
    let out = ev
        .call_block_public(&func.body, &func.param_names, vec![Value::Int(2)])
        .await
        .expect("registered, so it recurses");
    assert_eq!(out.to_display(&ready.atoms), "#done");
}

#[tokio::test]
async fn an_unlowered_function_can_fall_back_to_interpretation() {
    // The per-function fallback the backend relies on: hand a `BlockIr` straight to the
    // evaluator, so a program always builds whatever the backend cannot lower yet.
    let ir = ir_of("◆ f(n) ⟦ ^ n * 3 ⟧\nf(1)\n");
    let func = ir.functions.iter().find(|f| f.name == "f").expect("fn f");

    let mut ctx = ctx();
    let mut ev = Evaluator::new(&mut ctx);
    let out = ev
        .call_block_public(&func.body, &func.param_names, vec![Value::Int(14)])
        .await
        .expect("interpret the block");
    assert_eq!(out.as_int(), Some(42));
}

#[tokio::test]
async fn a_single_expression_can_be_interpreted_in_place() {
    // Finer-grained fallback: one node rather than a whole function.
    let ir = ir_of("1 + 2 * 3\n");
    let stmt = &ir.modules.first().expect("module").statements[0];

    let mut ctx = ctx();
    let mut ev = Evaluator::new(&mut ctx);
    let out = ev.eval_expr(stmt).await.expect("eval");
    assert_eq!(out.as_int(), Some(7));
}

#[tokio::test]
async fn generated_code_can_call_a_rite_closure_value() {
    let mut ctx = ctx();
    let f = run_source("gen.rite", "{ |a, b| a + b }", &mut ctx)
        .await
        .expect("closure");
    let mut ev = Evaluator::new(&mut ctx);
    let out = ev
        .call_value_public(f, vec![Value::Int(20), Value::Int(22)])
        .await
        .expect("call");
    assert_eq!(out.as_int(), Some(42));
}

#[tokio::test]
async fn the_budget_still_applies_when_the_tree_walker_is_bypassed() {
    // A compiled binary must not escape the execution budget by going around the
    // interpreter — the budget is what turns a runaway script into a clean error rather
    // than a hung process.
    let mut ctx = ctx();
    ctx.budget.max_steps = 8;
    let ir = ir_of("◆ spin(n) ⟦ ^ ? n <= 0 ⟦ 0 ⟧ : ⟦ spin(n - 1) ⟧ ⟧\nspin(50)\n");
    let func = ir
        .functions
        .iter()
        .find(|f| f.name == "spin")
        .expect("spin");

    // Without this the body cannot find itself: calling a block in isolation resolves no
    // callees, so a recursive function fails with `undefined name` rather than recursing.
    rite_runtime::register_functions(&ir, &mut ctx);
    let mut ev = Evaluator::new(&mut ctx);
    let err = ev
        .call_block_public(&func.body, &func.param_names, vec![Value::Int(50)])
        .await
        .expect_err("the budget must stop it");
    assert!(
        matches!(err, EvalError::Budget(_)),
        "expected a budget error, got {err:?}"
    );
}
