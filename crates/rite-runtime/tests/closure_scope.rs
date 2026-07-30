//! Lexical scoping of closures: a closure evaluates in the environment it captured,
//! never in its caller's, while assignments through the capture still reach the
//! defining scope (loop counters mutated inside `for` / `while` bodies).

use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    match run_source("t.rite", src, &mut ctx).await {
        Ok(v) => v,
        Err(rite_runtime::EvalError::Compile(d)) => {
            let msgs: Vec<String> = d.iter().map(|x| x.code.to_string()).collect();
            panic!("compile failed: {}\n--- source ---\n{src}", msgs.join(", "))
        }
        Err(e) => panic!("eval failed: {e}\n--- source ---\n{src}"),
    }
}

async fn eval_out(src: &str) -> Vec<String> {
    let mut ctx = RuntimeContext::new();
    match run_source("t.rite", src, &mut ctx).await {
        Ok(_) => ctx.stdout.clone(),
        Err(e) => panic!("eval failed: {e}\n--- source ---\n{src}"),
    }
}

/// The reported bug: a caller that shadowed a captured name hijacked the closure.
#[tokio::test]
async fn capture_wins_over_shadowing_caller() {
    let out = eval_out(
        r#"
def make_adder(n) [[ return { |x| x + n } ]]
def apply_with_shadow(f) [[
  n <- 1000
  return f(5)
]]
add10 <- make_adder(10)
do println(str(add10(5)))
do println(str(apply_with_shadow(add10)))
"#,
    )
    .await;
    assert_eq!(out, vec!["15\n".to_string(), "15\n".to_string()]);
}

/// Same shape in the glyph dialect, invoked two frames deeper: the callers' `n`
/// must stay invisible however many frames sit between capture and call.
#[tokio::test]
async fn capture_wins_through_deeper_call_stack() {
    assert_eq!(
        eval(
            r#"
◆ make_adder(n) ⟦ ^ { |x| x + n } ⟧
◆ inner(f) ⟦
  n ← 7000
  ^ f(1)
⟧
◆ outer(f) ⟦
  n ← 9000
  ^ inner(f)
⟧
add20 ← make_adder(20)
outer(add20)
"#
        )
        .await,
        Value::Int(21)
    );
}

/// A nested `◆` returned from its definer keeps its own capture (docs: `make_adder`).
#[tokio::test]
async fn returned_nested_function_keeps_capture() {
    assert_eq!(
        eval(
            r#"
◆ make_adder(n) ⟦
  ◆ add(x) ⟦ ^ x + n ⟧
  ^ add
⟧
◆ shadowing_caller(f) ⟦
  n ← 1000
  x ← 2000
  ^ f(10)
⟧
plus3 ← make_adder(3)
shadowing_caller(plus3)
"#
        )
        .await,
        Value::Int(13)
    );
}

#[tokio::test]
async fn closures_from_same_factory_have_independent_captures() {
    assert_eq!(
        eval(
            r#"
◆ make_adder(n) ⟦ ^ { |x| x + n } ⟧
add1 ← make_adder(1)
add2 ← make_adder(2)
res ← [add1(10), add2(10), add1(10)]
res
"#
        )
        .await,
        Value::list(vec![Value::Int(11), Value::Int(12), Value::Int(11)])
    );
}

/// Closure captured in one function, parked in a record inside a list, then invoked
/// from the bottom of an unrelated call chain whose frames all shadow `base`.
#[tokio::test]
async fn closure_stored_in_structure_and_called_from_deep_stack() {
    assert_eq!(
        eval(
            r#"
◆ make_ops(base) ⟦
  f ← { |x| x + base }
  entry ← ⟨name: "add", apply: f⟩
  ^ [entry]
⟧
◆ level3(ops, x) ⟦
  base ← 999
  entry ← first(ops)
  g ← entry.apply
  ^ g(x)
⟧
◆ level2(ops, x) ⟦
  base ← 500
  ^ level3(ops, x)
⟧
◆ level1(ops, x) ⟦
  base ← 1
  ^ level2(ops, x)
⟧
ops ← make_ops(100)
level1(ops, 5)
"#
        )
        .await,
        Value::Int(105)
    );
}

/// `for x in xs ⟦ … ⟧` lowers to `xs → each { |x| … }`: a mutable assigned in the
/// body must still be visible to the enclosing scope afterwards.
#[tokio::test]
async fn for_body_mutates_enclosing_counter() {
    assert_eq!(
        eval(
            r#"
total ↢ 0
xs ← [1, 2, 3, 4]
for x in xs ⟦
  total := total + x
⟧
total
"#
        )
        .await,
        Value::Int(10)
    );
}

#[tokio::test]
async fn for_body_mutates_counter_inside_function() {
    assert_eq!(
        eval(
            r#"
◆ sum_up(xs) ⟦
  total ↢ 0
  for x in xs ⟦
    total := total + x
  ⟧
  ^ total
⟧
sum_up([5, 10, 20])
"#
        )
        .await,
        Value::Int(35)
    );
}

/// `while cond ⟦ … ⟧` lowers to `while_loop(pred_closure, body_closure)`; both
/// closures read and write the enclosing mutables.
#[tokio::test]
async fn while_body_mutates_enclosing_bindings() {
    assert_eq!(
        eval(
            r#"
i ↢ 0
acc ↢ 0
while i < 5 ⟦
  acc := acc + i
  i := i + 1
⟧
res ← [i, acc]
res
"#
        )
        .await,
        Value::list(vec![Value::Int(5), Value::Int(10)])
    );
}

/// `loop n ⟦ … ⟧` lowers to `range(0, n) → each { |_| … }`.
#[tokio::test]
async fn loop_body_mutates_enclosing_counter() {
    assert_eq!(
        eval(
            r#"
hits ↢ 0
loop 3 ⟦
  hits := hits + 1
⟧
hits
"#
        )
        .await,
        Value::Int(3)
    );
}

/// Explicit pipeline `each` stage: same mutation requirement, different call path.
#[tokio::test]
async fn pipeline_each_mutates_enclosing_counter() {
    assert_eq!(
        eval(
            r#"
seen ↢ 0
xs ← [1, 2, 3]
xs → each { |x| seen := seen + x }
seen
"#
        )
        .await,
        Value::Int(6)
    );
}

/// Nested `◆` definitions lower to closure bindings; recursion and capture of the
/// enclosing function's parameter must both work. (A `←`-bound `{ |x| … }` cannot
/// name itself — the resolver rejects that as E020 before the runtime sees it — so
/// nested defs are the recursive-closure path.)
#[tokio::test]
async fn recursion_through_nested_function() {
    assert_eq!(
        eval(
            r#"
◆ scaled_countdown(factor) ⟦
  ◆ go(n) ⟦
    ? n < 1 ⟦ ^ 0 ⟧
    ^ n * factor + go(n - 1)
  ⟧
  ^ go(3)
⟧
scaled_countdown(10)
"#
        )
        .await,
        Value::Int(60)
    );
}

/// The recursive call inside an escaped nested function must resolve through the
/// capture even when the caller binds that same name to something uncallable.
#[tokio::test]
async fn recursion_survives_caller_shadowing_the_recursive_name() {
    assert_eq!(
        eval(
            r#"
◆ make_countdown() ⟦
  ◆ go(n) ⟦
    ? n < 1 ⟦ ^ 0 ⟧
    ^ n + go(n - 1)
  ⟧
  ^ go
⟧
◆ shadow_caller(f) ⟦
  go ← 42
  n ← 99
  ^ f(3)
⟧
counter ← make_countdown()
shadow_caller(counter)
"#
        )
        .await,
        Value::Int(6)
    );
}

#[tokio::test]
async fn compose_still_composes() {
    assert_eq!(
        eval(
            r#"
◆ inc(n) ⟦ ^ n + 1 ⟧
◆ dbl(n) ⟦ ^ n * 2 ⟧
f ← compose(inc, dbl)
f(5)
"#
        )
        .await,
        Value::Int(11)
    );
    // Three-argument form applies immediately.
    assert_eq!(
        eval(
            r#"
◆ inc(n) ⟦ ^ n + 1 ⟧
◆ dbl(n) ⟦ ^ n * 2 ⟧
compose(inc, dbl, 5)
"#
        )
        .await,
        Value::Int(11)
    );
}

/// A composed function returned from one scope and applied in another, deeper one.
#[tokio::test]
async fn compose_survives_escaping_its_defining_scope() {
    assert_eq!(
        eval(
            r#"
◆ inc(n) ⟦ ^ n + 1 ⟧
◆ dbl(n) ⟦ ^ n * 2 ⟧
◆ make() ⟦ ^ compose(inc, dbl) ⟧
◆ apply(f) ⟦
  x ← 100
  ^ f(5)
⟧
g ← make()
apply(g)
"#
        )
        .await,
        Value::Int(11)
    );
}

/// `map` / `keep` / `reduce` / `find` all route through `call_value`; none of them may
/// let the caller's bindings leak into the closure body.
#[tokio::test]
async fn higher_order_builtins_use_lexical_capture() {
    assert_eq!(
        eval(
            r#"
◆ scale_all(xs, factor) ⟦
  ^ map(xs, { |x| x * factor })
⟧
◆ caller(xs) ⟦
  factor ← 1000
  ^ scale_all(xs, 2)
⟧
caller([1, 2, 3])
"#
        )
        .await,
        Value::list(vec![Value::Int(2), Value::Int(4), Value::Int(6)])
    );
    assert_eq!(
        eval(
            r#"
◆ above(xs, limit) ⟦ ^ keep(xs, { |x| x > limit }) ⟧
◆ caller(xs) ⟦
  limit ← 0
  ^ above(xs, 2)
⟧
caller([1, 2, 3, 4])
"#
        )
        .await,
        Value::list(vec![Value::Int(3), Value::Int(4)])
    );
    assert_eq!(
        eval(
            r#"
◆ weighted_sum(xs, w) ⟦ ^ reduce(xs, { |acc, x| acc + x * w }, 0) ⟧
◆ caller(xs) ⟦
  w ← 100
  ^ weighted_sum(xs, 2)
⟧
caller([1, 2, 3])
"#
        )
        .await,
        Value::Int(12)
    );
    assert_eq!(
        eval(
            r#"
◆ first_over(xs, limit) ⟦ ^ find(xs, { |x| x > limit }) ⟧
◆ caller(xs) ⟦
  limit ← 0
  ^ first_over(xs, 2)
⟧
caller([1, 2, 3, 4])
"#
        )
        .await,
        Value::Int(3)
    );
}

/// Parameters shadow captured names inside the body only.
#[tokio::test]
async fn parameters_shadow_captured_names() {
    assert_eq!(
        eval(
            r#"
n ← 1
f ← { |n| n * 10 }
res ← [f(5), n]
res
"#
        )
        .await,
        Value::list(vec![Value::Int(50), Value::Int(1)])
    );
}

/// Two closures over the same mutable binding observe each other's writes.
#[tokio::test]
async fn closures_share_one_mutable_cell() {
    assert_eq!(
        eval(
            r#"
◆ make_counter() ⟦
  n ↢ 0
  bump ← { |q| n := n + 1 }
  read ← { |q| n }
  ^ ⟨bump: bump, read: read⟩
⟧
c ← make_counter()
b ← c.bump
r ← c.read
b(none)
b(none)
r(none)
"#
        )
        .await,
        Value::Int(2)
    );
}

/// Separate invocations of the same factory get separate cells.
#[tokio::test]
async fn counters_from_same_factory_are_independent() {
    assert_eq!(
        eval(
            r#"
◆ make_counter() ⟦
  n ↢ 0
  bump ← { |q| n := n + 1 }
  read ← { |q| n }
  ^ ⟨bump: bump, read: read⟩
⟧
a ← make_counter()
b ← make_counter()
ab ← a.bump
bb ← b.bump
ab(none)
ab(none)
bb(none)
ar ← a.read
br ← b.read
res ← [ar(none), br(none)]
res
"#
        )
        .await,
        Value::list(vec![Value::Int(2), Value::Int(1)])
    );
}
