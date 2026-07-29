//! Dual-dialect locks: glyph and ASCII sugar must evaluate to the same value.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    run_source("dual.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("eval failed for `{src}`: {e}"))
}

async fn assert_same(glyph: &str, ascii: &str) {
    let g = eval(glyph).await;
    let a = eval(ascii).await;
    assert!(
        g.structural_eq(&a),
        "dialect mismatch\n glyph: {glyph}\n  => {g:?}\n ascii: {ascii}\n  => {a:?}"
    );
}

#[tokio::test]
async fn dual_ranges() {
    assert_same("(1..4) → sum", "(1..4) -> sum").await;
    assert_same("(1..=3) → sum", "(1..=3) -> sum").await;
}

#[tokio::test]
async fn dual_pipeline_stages() {
    assert_same("[1,2,3,4] → rest → sum", "[1,2,3,4] -> rest -> sum").await;
    assert_same("[1,2,3,4] → take(2) → sum", "[1,2,3,4] -> take(2) -> sum").await;
    assert_same(r#""a b c" → words → count"#, r#""a b c" -> words -> count"#).await;
}

#[tokio::test]
async fn dual_logic() {
    assert_same("true ∧ false", "true and false").await;
    assert_same("false ∨ true", "false or true").await;
    assert_same("¬ false", "not false").await;
    assert_same("true ⊻ false", "true xor false").await;
}

#[tokio::test]
async fn dual_power_idiv() {
    assert_same("2 ** 8", "pow(2, 8)").await;
    assert_same("7 ÷ 2", "idiv(7, 2)").await;
}

#[tokio::test]
async fn dual_if_else() {
    assert_same(
        r#"? false ⟦ 1 ⟧ : ⟦ 2 ⟧"#,
        r#"if false [[ 1 ]] else [[ 2 ]]"#,
    )
    .await;
}

#[tokio::test]
async fn dual_unless() {
    assert_same(r#"unless false ⟦ 9 ⟧"#, r#"¿ false ⟦ 9 ⟧"#).await;
}

#[tokio::test]
async fn dual_for_in() {
    let g = eval(
        r#"
s ↢ 0
∀ n ∈ [1, 2, 3] ⟦
  s := s + n
⟧
s
"#,
    )
    .await;
    let a = eval(
        r#"
s <~ 0
for n in [1, 2, 3] [[
  s := s + n
]]
s
"#,
    )
    .await;
    assert_eq!(g, Value::Int(6));
    assert_eq!(a, Value::Int(6));
}

#[tokio::test]
async fn dual_ok_err_marks() {
    assert_same(
        r#"~ ✓ 7 ⟦ ok v → v  err _ → 0 ⟧"#,
        r#"~ ok(7) ⟦ ok v → v  err _ → 0 ⟧"#,
    )
    .await;
}

#[tokio::test]
async fn dual_bind_ops() {
    assert_same("x ← 1 + 2\nx", "x <- 1 + 2\nx").await;
    assert_same(
        r#"
c ↢ 1
c += 2
c
"#,
        r#"
c <~ 1
c += 2
c
"#,
    )
    .await;
}

#[tokio::test]
async fn dual_function_abs() {
    assert_same(
        r#"
◆ abs(n) ⟦
  ? n < 0 ⟦ ^ -n ⟧
  ^ n
⟧
abs(-4)
"#,
        r#"
def abs(n) [[
  if n < 0 [[
    return -n
  ]]
  return n
]]
abs(-4)
"#,
    )
    .await;
}
