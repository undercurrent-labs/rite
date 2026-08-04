//! Comprehensive tests for the Rite sugar pack (v0.1.3).

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    run_source("sugar.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("eval failed for `{src}`: {e}"))
}

#[tokio::test]
async fn ranges_exclusive_and_inclusive() {
    assert_eq!(eval("(1..5) → sum").await, Value::Int(10)); // 1+2+3+4
    assert_eq!(eval("(1..=5) → sum").await, Value::Int(15));
    assert_eq!(eval("(1‥3) → sum").await, Value::Int(6)); // glyph incl
}

#[tokio::test]
async fn pipeline_rest_take_drop_reverse() {
    assert_eq!(eval("[1,2,3,4] → rest → sum").await, Value::Int(9));
    assert_eq!(eval("[1,2,3,4] → tail → first").await, Value::Int(2));
    assert_eq!(eval("[1,2,3,4] → take(2) → sum").await, Value::Int(3));
    assert_eq!(eval("[1,2,3,4] → drop(2) → sum").await, Value::Int(7));
    assert_eq!(eval("[1,2,3] → reverse → first").await, Value::Int(3));
    assert_eq!(eval("[1,2,3] → init → sum").await, Value::Int(3));
}

#[tokio::test]
async fn pipeline_field_projection() {
    assert_eq!(
        eval(r#"[⟨n: 1⟩, ⟨n: 2⟩, ⟨n: 3⟩] → .n → sum"#).await,
        Value::Int(6)
    );
}

#[tokio::test]
async fn words_lines_join_enumerate() {
    assert_eq!(eval(r#""a b c" → words → count"#).await, Value::Int(3));
    assert_eq!(eval("\"a\\nb\\nc\" → lines → count").await, Value::Int(3));
    let v = eval(r#"["a", "b"] → join("-")"#).await;
    assert_eq!(v.as_str(), Some("a-b"));
    assert_eq!(eval("[10, 20] → enumerate → count").await, Value::Int(2));
}

#[tokio::test]
async fn ascii_else_keyword() {
    assert_eq!(
        eval(r#"if false [[ 1 ]] else [[ 2 ]]"#).await,
        Value::Int(2)
    );
    assert_eq!(eval(r#"if true [[ 1 ]] else [[ 2 ]]"#).await, Value::Int(1));
}

#[tokio::test]
async fn unless_and_glyph_unless() {
    assert_eq!(eval(r#"unless false ⟦ 7 ⟧"#).await, Value::Int(7));
    assert_eq!(eval(r#"¿ true ⟦ 1 ⟧ : ⟦ 2 ⟧"#).await, Value::Int(2));
}

#[tokio::test]
async fn for_in_and_forall() {
    assert_eq!(
        eval(
            r#"
s ↢ 0
for n in 1..4 ⟦
  s := s + n
⟧
s
"#
        )
        .await,
        Value::Int(6)
    );
    assert_eq!(
        eval(
            r#"
s ↢ 0
∀ n ∈ [1, 2, 3] ⟦
  s := s + n
⟧
s
"#
        )
        .await,
        Value::Int(6)
    );
}

#[tokio::test]
async fn loop_n_times() {
    assert_eq!(
        eval(
            r#"
s ↢ 0
loop 5 ⟦
  s := s + 1
⟧
s
"#
        )
        .await,
        Value::Int(5)
    );
}

#[tokio::test]
async fn while_loop_sugar() {
    assert_eq!(
        eval(
            r#"
c ↢ 0
while c < 3 ⟦
  c := c + 1
⟧
c
"#
        )
        .await,
        Value::Int(3)
    );
}

#[tokio::test]
async fn op_assign() {
    assert_eq!(
        eval(
            r#"
c ↢ 10
c += 5
c -= 3
c *= 2
c
"#
        )
        .await,
        Value::Int(24)
    );
}

#[tokio::test]
async fn power_and_idiv() {
    assert_eq!(eval("2 ** 10").await, Value::Int(1024));
    assert_eq!(eval("pow(2, 8)").await, Value::Int(256));
    assert_eq!(eval("7 ÷ 2").await, Value::Int(3));
    assert_eq!(eval("idiv(7, 2)").await, Value::Int(3));
}

#[tokio::test]
async fn xor_and_logical_glyphs() {
    assert_eq!(eval("true xor false").await, Value::Bool(true));
    assert_eq!(eval("true ⊻ true").await, Value::Bool(false));
    assert_eq!(eval("true ∧ false").await, Value::Bool(false));
}

#[tokio::test]
async fn ok_err_marks() {
    assert_eq!(
        eval(
            r#"
~ ✓ 42 ⟦
  ok v → v
  err _ → 0
⟧
"#
        )
        .await,
        Value::Int(42)
    );
    assert_eq!(
        eval(
            r#"
~ ✗ "nope" ⟦
  ok _ → 1
  err e → 2
⟧
"#
        )
        .await,
        Value::Int(2)
    );
}

#[tokio::test]
async fn result_helpers() {
    assert_eq!(eval(r#"is_ok(ok(1))"#).await, Value::Bool(true));
    assert_eq!(eval(r#"is_err(err(1))"#).await, Value::Bool(true));
    assert_eq!(eval(r#"unwrap_or(err(1), 99)"#).await, Value::Int(99));
    assert_eq!(eval(r#"or_else(ok(5), 9)"#).await, Value::Int(5));
}

#[tokio::test]
async fn abs_clamp_repeat_concat() {
    assert_eq!(eval("abs(-7)").await, Value::Int(7));
    assert_eq!(eval("clamp(15, 0, 10)").await, Value::Int(10));
    let v = eval(r#"repeat("ab", 3)"#).await;
    assert_eq!(v.as_str(), Some("ababab"));
    assert_eq!(eval("concat([1], [2, 3], [4]) → sum").await, Value::Int(10));
}

#[tokio::test]
async fn keys_values_contains() {
    assert_eq!(eval(r#"keys(⟨a: 1, b: 2⟩) → count"#).await, Value::Int(2));
    assert_eq!(eval(r#"values(⟨a: 1, b: 2⟩) → sum"#).await, Value::Int(3));
    assert_eq!(eval("contains([1, 2, 3], 2)").await, Value::Bool(true));
}

#[tokio::test]
async fn say_and_paragraph() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let _ = run_source("t.rite", r#"say "hello-sugar""#, &mut ctx)
        .await
        .unwrap();
    assert!(ctx.stdout.join("").contains("hello-sugar"));

    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let _ = run_source("t.rite", "¶ 99", &mut ctx).await.unwrap();
    assert!(ctx.stdout.join("").contains('9'));
}

#[tokio::test]
async fn record_update_via_merge() {
    // Structural update without spread literal: base + ⟨…⟩
    assert_eq!(
        eval(
            r#"
base ← ⟨a: 1, b: 2⟩
(base + ⟨b: 9, c: 3⟩).b
"#
        )
        .await,
        Value::Int(9)
    );
}

#[tokio::test]
async fn compose_functions() {
    assert_eq!(
        eval(
            r#"
◆ double(n) ⟦ ^ n * 2 ⟧
◆ inc(n) ⟦ ^ n + 1 ⟧
f ← double ∘ inc
f(3)
"#
        )
        .await,
        Value::Int(8) // double(inc(3)) = 8
    );
}

/// Record spread is sugar for the record-merge operator: `⟨..a, k: v⟩` ≡ `a + ⟨k: v⟩`.
/// Entries flow left to right and later ones win, so a spread reads as "start from
/// this, then override". The parser used to reject the form outright.
#[tokio::test]
async fn record_spread_is_sugar_for_merge() {
    let src = r#"
base ← ⟨host: "h", port: 80⟩
over ← ⟨port: 443⟩
⟨..base, ..over⟩ = base + over
"#;
    assert_eq!(eval(src).await, Value::Bool(true));
}

#[tokio::test]
async fn record_spread_lets_later_entries_win() {
    let src = r#"
base ← ⟨host: "h", port: 80, tls: false⟩
⟨..base, port: 443, tls: true⟩.port
"#;
    assert_eq!(eval(src).await, Value::Int(443));

    // ...and a spread placed *after* a literal key wins over it, same rule.
    let src = r#"
base ← ⟨port: 80⟩
⟨port: 1, ..base⟩.port
"#;
    assert_eq!(eval(src).await, Value::Int(80));
}

#[tokio::test]
async fn record_spread_composes_and_is_position_free() {
    // Several spreads, spreads mixed with keys, and a lone spread (identity).
    let src = r#"
a ← ⟨x: 1⟩
b ← ⟨y: 2⟩
⟨..a, ..b, z: 3⟩ = ⟨x: 1, y: 2, z: 3⟩
"#;
    assert_eq!(eval(src).await, Value::Bool(true));
    assert_eq!(eval("a ← ⟨x: 1⟩\n⟨..a⟩ = a").await, Value::Bool(true));
    assert_eq!(eval("⟨..⟨x: 5⟩⟩.x").await, Value::Int(5));
}

#[tokio::test]
async fn record_spread_accepts_both_glyphs_in_both_dialects() {
    // `..` is canonical; `...` is a synonym the formatter normalises.
    assert_eq!(eval("a ← ⟨x: 1⟩\n⟨...a, y: 2⟩.x").await, Value::Int(1));
    assert_eq!(eval("a <- <<x: 1>>\n<<..a, y: 2>>.y").await, Value::Int(2));
}
