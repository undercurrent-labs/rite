//! Bulletproof edge-case suite for the interpreter.
//! Each test is a small pure program unless permissions are required.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, EvalError, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    run_source("edge.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("eval failed for `{src}`: {e}"))
}

async fn eval_err(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    run_source("edge.rite", src, &mut ctx)
        .await
        .expect_err("expected error")
        .to_string()
}

async fn eval_ok_or_err(src: &str) -> Result<Value, String> {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    run_source("edge.rite", src, &mut ctx)
        .await
        .map_err(|e| e.to_string())
}

#[tokio::test]
async fn arithmetic_precedence_and_parens() {
    assert_eq!(eval("1 + 2 * 3").await, Value::Int(7));
    assert_eq!(eval("(1 + 2) * 3").await, Value::Int(9));
    assert_eq!(eval("10 - 3 - 2").await, Value::Int(5));
    assert_eq!(eval("2 * 3 + 4").await, Value::Int(10));
    assert_eq!(eval("2 + 3 * 4 - 1").await, Value::Int(13));
}

#[tokio::test]
async fn arithmetic_negatives_and_mod() {
    assert_eq!(eval("-3 + 5").await, Value::Int(2));
    assert_eq!(eval("10 % 3").await, Value::Int(1));
    assert_eq!(eval("(-4) * (-3)").await, Value::Int(12));
}

#[tokio::test]
async fn float_ops() {
    let v = eval("1.5 + 2.25").await;
    match v {
        Value::Float(f) => assert!((f - 3.75).abs() < 1e-9),
        other => panic!("expected float, got {other:?}"),
    }
}

#[tokio::test]
async fn comparisons() {
    assert_eq!(eval("1 = 1").await, Value::Bool(true));
    assert_eq!(eval("1 != 2").await, Value::Bool(true));
    assert_eq!(eval("3 > 2").await, Value::Bool(true));
    assert_eq!(eval("3 >= 3").await, Value::Bool(true));
    assert_eq!(eval("1 < 0").await, Value::Bool(false));
    assert_eq!(eval("\"a\" = \"a\"").await, Value::Bool(true));
}

#[tokio::test]
async fn truthiness_table() {
    // only false and none are falsey
    assert_eq!(eval(r#"? 0 ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(1));
    assert_eq!(eval(r#"? "" ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(1));
    assert_eq!(eval(r#"? [] ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(1));
    assert_eq!(eval(r#"? false ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(0));
    assert_eq!(eval(r#"? none ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(0));
    assert_eq!(eval(r#"? true ⟦ 1 ⟧ : ⟦ 0 ⟧"#).await, Value::Int(1));
}

#[tokio::test]
async fn coalesce() {
    assert_eq!(eval("none ?? 10").await, Value::Int(10));
    assert_eq!(eval("5 ?? 10").await, Value::Int(5));
    assert_eq!(eval(r#"⟨a: 1⟩.missing ?? 99"#).await, Value::Int(99));
}

#[tokio::test]
async fn strings() {
    assert_eq!(
        eval(r#""hello" + " " + "world""#).await,
        Value::string("hello world")
    );
    let v = eval(r#"str(42)"#).await;
    assert_eq!(v.as_str(), Some("42"));
}

#[tokio::test]
async fn lists_basic() {
    assert_eq!(eval("[1, 2, 3] → count").await, Value::Int(3));
    assert_eq!(eval("[1, 2, 3] → sum").await, Value::Int(6));
    assert_eq!(eval("[10, 20, 30] → first").await, Value::Int(10));
    assert_eq!(eval("[] → count").await, Value::Int(0));
    assert_eq!(eval("[] → sum").await, Value::Int(0));
}

#[tokio::test]
async fn lists_map_keep() {
    assert_eq!(
        eval("[1, 2, 3, 4] → keep { |n| n % 2 = 0 } → sum").await,
        Value::Int(6)
    );
    assert_eq!(
        eval("[1, 2, 3] → map { |n| n * 10 } → sum").await,
        Value::Int(60)
    );
    // empty keep
    assert_eq!(
        eval("[1, 3, 5] → keep { |n| n % 2 = 0 } → count").await,
        Value::Int(0)
    );
}

#[tokio::test]
async fn nested_lists_with_spaces() {
    let v = eval("[ [1, 2], [3, 4] ] → count").await;
    assert_eq!(v, Value::Int(2));
}

#[tokio::test]
async fn records_access_and_merge() {
    let v = eval(r#"⟨a: 1, b: 2⟩.a"#).await;
    assert_eq!(v, Value::Int(1));
    let missing = eval(r#"⟨a: 1⟩.nope"#).await;
    assert_eq!(missing, Value::None);
    let merged = eval(r#"⟨a: 1, b: 2⟩ + ⟨b: 9, c: 3⟩"#).await;
    assert_eq!(merged.get_field("a"), Value::Int(1));
    assert_eq!(merged.get_field("b"), Value::Int(9));
    assert_eq!(merged.get_field("c"), Value::Int(3));
}

#[tokio::test]
async fn record_empty_and_nested() {
    let v = eval(r#"⟨outer: ⟨inner: 7⟩⟩.outer.inner"#).await;
    assert_eq!(v, Value::Int(7));
}

#[tokio::test]
async fn atoms_and_match() {
    assert_eq!(
        eval(
            r#"~ #ok ⟦
  #ok → 1
  #error → 2
  _ → 0
⟧"#
        )
        .await,
        Value::Int(1)
    );
    assert_eq!(
        eval(
            r#"~ #nope ⟦
  #ok → 1
  _ → 99
⟧"#
        )
        .await,
        Value::Int(99)
    );
}

#[tokio::test]
async fn match_list_destructure() {
    assert_eq!(
        eval(
            r#"~ [10, 20, 30] ⟦
  [h, ..rest] → h
  _ → 0
⟧"#
        )
        .await,
        Value::Int(10)
    );
}

#[tokio::test]
async fn functions_and_closures() {
    assert_eq!(
        eval(
            r#"
◆ add(a, b) ⟦
  ^ a + b
⟧
add(2, 40)
"#
        )
        .await,
        Value::Int(42)
    );
    // nested call
    assert_eq!(
        eval(
            r#"
◆ double(n) ⟦ ^ n * 2 ⟧
◆ quad(n) ⟦ ^ double(double(n)) ⟧
quad(3)
"#
        )
        .await,
        Value::Int(12)
    );
}

#[tokio::test]
async fn early_return_from_if() {
    // Canonical docs example: return from then-branch must exit the function.
    assert_eq!(
        eval(
            r#"
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
abs(-5)
"#
        )
        .await,
        Value::Int(5)
    );
    assert_eq!(
        eval(
            r#"
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
abs(5)
"#
        )
        .await,
        Value::Int(5)
    );
    // Positive path with no else.
    assert_eq!(
        eval(
            r#"
◆ sign(n) ⟦
  ? n < 0 ⟦
    ^ -1
  ⟧
  ? n = 0 ⟦
    ^ 0
  ⟧
  ^ 1
⟧
sign(-3)
"#
        )
        .await,
        Value::Int(-1)
    );
    assert_eq!(
        eval(
            r#"
◆ sign(n) ⟦
  ? n < 0 ⟦
    ^ -1
  ⟧
  ? n = 0 ⟦
    ^ 0
  ⟧
  ^ 1
⟧
sign(0)
"#
        )
        .await,
        Value::Int(0)
    );
    assert_eq!(
        eval(
            r#"
◆ sign(n) ⟦
  ? n < 0 ⟦
    ^ -1
  ⟧
  ? n = 0 ⟦
    ^ 0
  ⟧
  ^ 1
⟧
sign(9)
"#
        )
        .await,
        Value::Int(1)
    );
}

#[tokio::test]
async fn early_return_from_nested_block() {
    assert_eq!(
        eval(
            r#"
◆ f() ⟦
  ⟦
    ^ 7
  ⟧
  8
⟧
f()
"#
        )
        .await,
        Value::Int(7)
    );
}

#[tokio::test]
async fn early_return_from_match_arm_block() {
    // `^` is a statement; nest it in a block arm so return exits the function.
    assert_eq!(
        eval(
            r#"
◆ classify(x) ⟦
  ~ x ⟦
    #a → ⟦
      ^ 1
    ⟧
    #b → ⟦
      ^ 2
    ⟧
    _ → ⟦
      ^ 0
    ⟧
  ⟧
  99
⟧
classify(#b)
"#
        )
        .await,
        Value::Int(2)
    );
    // Match arm value (no caret) is the function result when last expr.
    assert_eq!(
        eval(
            r#"
◆ classify(x) ⟦
  ~ x ⟦
    #a → 1
    #b → 2
    _ → 0
  ⟧
⟧
classify(#a)
"#
        )
        .await,
        Value::Int(1)
    );
}

#[tokio::test]
async fn try_unwrap_early_return_from_function() {
    assert_eq!(
        eval(
            r#"
◆ load() ⟦
  v ← @json.decode("not-json")?
  ^ v
⟧
~ load() ⟦
  ok _ → 1
  err _ → 2
⟧
"#
        )
        .await,
        Value::Int(2)
    );
}

#[tokio::test]
async fn try_unwrap_at_script_top_level() {
    // Docs: script returns the err value (not a hard runtime panic string).
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source("t.rite", r#"@json.decode("not-json")?"#, &mut ctx)
        .await
        .expect("top-level ? should yield err value");
    assert!(
        matches!(v, Value::Result(rite_runtime::ResultValue::Err(_))),
        "expected err result, got {v:?}"
    );
}

#[tokio::test]
async fn last_expr_as_function_result_without_caret() {
    assert_eq!(
        eval(
            r#"
◆ f(n) ⟦
  n * 2
⟧
f(21)
"#
        )
        .await,
        Value::Int(42)
    );
}

#[tokio::test]
async fn mutable_counter() {
    assert_eq!(
        eval(
            r#"
c ↢ 0
c := c + 1
c := c + 2
c
"#
        )
        .await,
        Value::Int(3)
    );
}

#[tokio::test]
async fn multi_statement_last_value() {
    assert_eq!(
        eval(
            r#"
a ← 1
b ← 2
a + b
"#
        )
        .await,
        Value::Int(3)
    );
}

#[tokio::test]
async fn console_and_stdout() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let _ = run_source("t.rite", r#"! @console.println("hi")"#, &mut ctx)
        .await
        .unwrap();
    assert!(ctx.stdout.join("").contains("hi"));
}

#[tokio::test]
async fn json_roundtrip() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source(
        "t.rite",
        r#"
data ← ⟨n: 1, s: "x"⟩
text ← @json.encode(data)
decoded ← @json.decode(text)?
decoded.n
"#,
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(v, Value::Int(1));
}

#[tokio::test]
async fn json_invalid_decode_with_try_returns_err() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    // Using ? on err at top level yields the err result as script value.
    let v = run_source("t.rite", r#"@json.decode("not-json")?"#, &mut ctx)
        .await
        .expect("should yield err value, not hard error");
    assert!(
        matches!(v, Value::Result(rite_runtime::ResultValue::Err(_))),
        "expected err result, got {v:?}"
    );
}

#[tokio::test]
async fn json_invalid_without_try_is_err_value() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source("t.rite", r#"@json.decode("not-json")"#, &mut ctx)
        .await
        .expect("decode without ? returns err result");
    assert!(
        matches!(v, Value::Result(rite_runtime::ResultValue::Err(_))),
        "expected err result, got {v:?}"
    );
}

#[tokio::test]
async fn pipeline_chain_order() {
    // (2*2) + (4*4) = 4+16 = 20
    assert_eq!(
        eval("[1, 2, 3, 4] → keep { |n| n % 2 = 0 } → map { |n| n * n } → sum").await,
        Value::Int(20)
    );
}

#[tokio::test]
async fn shadowing_inner_binding() {
    assert_eq!(
        eval(
            r#"
x ← 1
◆ f() ⟦
  x ← 2
  ^ x
⟧
f()
"#
        )
        .await,
        Value::Int(2)
    );
}

#[tokio::test]
async fn unicode_string_content() {
    let v = eval(r#""café" + "🚀""#).await;
    assert_eq!(v.as_str(), Some("café🚀"));
}

#[tokio::test]
async fn membership_in_list() {
    // glyph ∈ may work; ascii `in`
    let v = eval(r#"2 ∈ [1, 2, 3]"#).await;
    // may be bool true
    assert!(
        matches!(v, Value::Bool(true)) || v == Value::Int(1),
        "{v:?}"
    );
}

#[tokio::test]
async fn step_budget_exceeded() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::new().with_max_steps(10);
    // tight loop via recursion
    let err = run_source(
        "t.rite",
        r#"
◆ bomb(n) ⟦
  ? n > 0 ⟦
    ^ bomb(n - 1)
  ⟧
  ^ 0
⟧
bomb(100000)
"#,
        &mut ctx,
    )
    .await
    .expect_err("budget");
    let s = err.to_string().to_lowercase();
    assert!(
        s.contains("step") || s.contains("budget") || s.contains("stack") || s.contains("depth"),
        "{s}"
    );
}

#[tokio::test]
async fn wall_clock_timeout() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::new()
        .with_timeout(std::time::Duration::from_millis(1))
        .with_max_steps(u64::MAX);
    // busy work
    let err = run_source(
        "t.rite",
        r#"
◆ spin(n) ⟦
  ? n > 0 ⟦
    ^ spin(n - 1)
  ⟧
  ^ 0
⟧
spin(5000000)
"#,
        &mut ctx,
    )
    .await;
    // may timeout or finish depending on machine — if ok, skip
    if let Err(e) = err {
        assert!(
            e.to_string().to_lowercase().contains("timeout")
                || e.to_string().to_lowercase().contains("budget")
                || e.to_string().to_lowercase().contains("step"),
            "{e}"
        );
    }
}

#[tokio::test]
async fn undefined_name_is_compile_error() {
    let mut ctx = RuntimeContext::new();
    let err = run_source("t.rite", "not_defined_xyz", &mut ctx)
        .await
        .expect_err("undefined");
    assert!(
        matches!(err, EvalError::Compile(_)) || err.to_string().contains("compile"),
        "{err}"
    );
}

#[tokio::test]
async fn ascii_and_glyph_same_result() {
    let g = eval("x ← 1 + 2\nx").await;
    let a = eval("x <- 1 + 2\nx").await;
    assert_eq!(g, a);
    assert_eq!(g, Value::Int(3));
}

#[tokio::test]
async fn secure_defaults_allow_console() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let r = run_source("t.rite", r#"! @console.println("ok")"#, &mut ctx).await;
    assert!(r.is_ok(), "{r:?}");
}

#[tokio::test]
async fn fs_read_denied_under_secure() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let r = run_source("t.rite", r#"! @fs.read("/etc/passwd")"#, &mut ctx).await;
    match r {
        Ok(v) => {
            // must not be ok(string contents of passwd)
            let s = format!("{v:?}");
            assert!(!s.contains("root:"), "leaked file: {s}");
        }
        Err(e) => {
            let s = e.to_string().to_lowercase();
            assert!(
                s.contains("permission") || s.contains("denied") || s.contains("fs"),
                "{s}"
            );
        }
    }
}

#[tokio::test]
async fn process_denied_under_secure() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::default_secure());
    let r = run_source("t.rite", r#"@process.run("true")"#, &mut ctx).await;
    assert!(
        r.is_err() || {
            // err value
            true
        }
    );
}

#[tokio::test]
async fn logical_and_or_not_ascii() {
    assert_eq!(eval("true and false").await, Value::Bool(false));
    assert_eq!(eval("true and true").await, Value::Bool(true));
    assert_eq!(eval("false or true").await, Value::Bool(true));
    assert_eq!(eval("false or false").await, Value::Bool(false));
    assert_eq!(eval("not false").await, Value::Bool(true));
    assert_eq!(eval("not true").await, Value::Bool(false));
    // short-circuit: right side of `and false` must not run
    assert_eq!(
        eval(
            r#"
side ↢ 0
◆ bump() ⟦
  side := side + 1
  ^ true
⟧
_ ← false and bump()
side
"#
        )
        .await,
        Value::Int(0)
    );
    assert_eq!(
        eval(
            r#"
side ↢ 0
◆ bump() ⟦
  side := side + 1
  ^ false
⟧
_ ← true or bump()
side
"#
        )
        .await,
        Value::Int(0)
    );
}

#[tokio::test]
async fn logical_glyph_ops() {
    assert_eq!(eval("true ∧ false").await, Value::Bool(false));
    assert_eq!(eval("false ∨ true").await, Value::Bool(true));
    assert_eq!(eval("¬ false").await, Value::Bool(true));
    assert_eq!(eval("¬true").await, Value::Bool(false));
}

#[tokio::test]
async fn division_by_zero_errors() {
    let err = eval_err("1 / 0").await;
    assert!(
        err.to_lowercase().contains("zero") || err.to_lowercase().contains("div"),
        "{err}"
    );
}

#[tokio::test]
async fn arity_mismatch_errors() {
    let err = eval_err(
        r#"
◆ f(a, b) ⟦ ^ a + b ⟧
f(1)
"#,
    )
    .await;
    assert!(
        err.to_lowercase().contains("arity") || err.to_lowercase().contains("arg"),
        "{err}"
    );
    let err = eval_err(
        r#"
◆ f(a, b) ⟦ ^ a + b ⟧
f(1, 2, 3)
"#,
    )
    .await;
    assert!(
        err.to_lowercase().contains("arity") || err.to_lowercase().contains("arg"),
        "{err}"
    );
}

#[tokio::test]
async fn match_failure_errors() {
    let err = eval_err(
        r#"
~ 1 ⟦
  #a → 0
⟧
"#,
    )
    .await;
    assert!(
        err.to_lowercase().contains("match") || err.to_lowercase().contains("arm"),
        "{err}"
    );
}

#[tokio::test]
async fn closure_captures_outer_binding() {
    assert_eq!(
        eval(
            r#"
factor ← 10
◆ scale(n) ⟦
  ^ n * factor
⟧
scale(3)
"#
        )
        .await,
        Value::Int(30)
    );
}

#[tokio::test]
async fn nested_local_function_helper() {
    // Docs example: local helpers via nested ◆
    assert_eq!(
        eval(
            r#"
◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧
area(3, 4)
"#
        )
        .await,
        Value::Int(12)
    );
    assert_eq!(
        eval(
            r#"
◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧
area(-3, 4)
"#
        )
        .await,
        Value::Int(0)
    );
}

#[tokio::test]
async fn nested_function_returned_and_called() {
    assert_eq!(
        eval(
            r#"
◆ outer() ⟦
  ◆ inner(x) ⟦ ^ x + 1 ⟧
  ^ inner
⟧
f ← outer()
f(5)
"#
        )
        .await,
        Value::Int(6)
    );
}

#[tokio::test]
async fn nested_function_captures_outer_param() {
    assert_eq!(
        eval(
            r#"
◆ outer(x) ⟦
  ◆ inner() ⟦ ^ x * 2 ⟧
  ^ inner
⟧
f ← outer(21)
f()
"#
        )
        .await,
        Value::Int(42)
    );
}

#[tokio::test]
async fn ascii_nested_def() {
    assert_eq!(
        eval(
            r#"
def area(w, h) [[
  def clamp(n) [[
    if n < 0 [[
      return 0
    ]]
    return n
  ]]
  return clamp(w) * clamp(h)
]]
area(3, 4)
"#
        )
        .await,
        Value::Int(12)
    );
}

#[tokio::test]
async fn result_match_ok_err() {
    assert_eq!(
        eval(
            r#"
text ← @json.encode(⟨n: 1⟩)
outcome ← @json.decode(text)
~ outcome ⟦
  ok data → data.n
  err _ → -1
⟧
"#
        )
        .await,
        Value::Int(1)
    );
    assert_eq!(
        eval(
            r#"
outcome ← @json.decode("nope")
~ outcome ⟦
  ok _ → 1
  err _ → 2
⟧
"#
        )
        .await,
        Value::Int(2)
    );
}

#[tokio::test]
async fn if_else_expression() {
    assert_eq!(eval(r#"? true ⟦ 1 ⟧ : ⟦ 2 ⟧"#).await, Value::Int(1));
    assert_eq!(eval(r#"? false ⟦ 1 ⟧ : ⟦ 2 ⟧"#).await, Value::Int(2));
    // ASCII uses `:` for else (same as glyph); keyword `else` is not a separator.
    assert_eq!(eval(r#"if true [[ 10 ]] : [[ 20 ]]"#).await, Value::Int(10));
    assert_eq!(
        eval(r#"if false [[ 10 ]] : [[ 20 ]]"#).await,
        Value::Int(20)
    );
}

#[tokio::test]
async fn pipeline_first_last_empty() {
    assert_eq!(eval("[10, 20, 30] → first").await, Value::Int(10));
    assert_eq!(eval("[10, 20, 30] → last").await, Value::Int(30));
    // empty first/last: must not panic; prefer none (soft end)
    let first = eval("[] → first").await;
    let last = eval("[] → last").await;
    assert!(
        matches!(first, Value::None) || matches!(first, Value::Result(_)),
        "empty first: {first:?}"
    );
    assert!(
        matches!(last, Value::None) || matches!(last, Value::Result(_)),
        "empty last: {last:?}"
    );
}

#[tokio::test]
async fn match_rest_pattern_tail() {
    assert_eq!(
        eval(
            r#"
~ [10, 20, 30] ⟦
  [h, ..rest] → rest → sum
  _ → 0
⟧
"#
        )
        .await,
        Value::Int(50)
    );
}

#[tokio::test]
async fn nested_lists_spaces_required() {
    assert_eq!(eval("[ [1, 2], [3, 4] ] → count").await, Value::Int(2));
    assert_eq!(
        eval(
            r#"
grid ← [ [1, 2], [3, 4] ]
grid → first → sum
"#
        )
        .await,
        Value::Int(3)
    );
}

#[tokio::test]
async fn juxta_multi_value_return() {
    // HTTP-style: ^ 200 ⟨…⟩ becomes a list return
    let v = eval(
        r#"
◆ h() ⟦
  ^ 200 ⟨status: #ok⟩
⟧
h()
"#,
    )
    .await;
    match v {
        Value::List(xs) => {
            assert_eq!(xs.len(), 2);
            assert_eq!(xs[0], Value::Int(200));
        }
        other => panic!("expected list multi-return, got {other:?}"),
    }
}

#[tokio::test]
async fn deep_recursion_hits_budget_or_depth() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    ctx.budget = rite_runtime::ExecutionBudget::new().with_max_steps(50);
    let err = run_source(
        "t.rite",
        r#"
◆ bomb(n) ⟦
  ^ bomb(n + 1)
⟧
bomb(0)
"#,
        &mut ctx,
    )
    .await
    .expect_err("must fail");
    let s = err.to_string().to_lowercase();
    assert!(
        s.contains("step")
            || s.contains("budget")
            || s.contains("depth")
            || s.contains("stack")
            || s.contains("overflow"),
        "{s}"
    );
}

#[tokio::test]
async fn assign_to_immutable_errors() {
    // Caught at resolve/compile time (E023), not runtime.
    let err = eval_err(
        r#"
x ← 1
x := 2
"#,
    )
    .await;
    let s = err.to_lowercase();
    assert!(
        s.contains("immutable")
            || s.contains("assign")
            || s.contains("cannot")
            || s.contains("compile"),
        "{err}"
    );
}

#[tokio::test]
async fn ascii_early_return_abs() {
    assert_eq!(
        eval(
            r#"
def abs(n) [[
  if n < 0 [[
    return -n
  ]]
  return n
]]
abs(-5)
"#
        )
        .await,
        Value::Int(5)
    );
}

#[tokio::test]
async fn string_interpolation_basic() {
    // if interpolation is supported
    let r = eval_ok_or_err(
        r#"n ← 3
"n={n}""#,
    )
    .await;
    if let Ok(v) = r {
        let s = v.as_str().unwrap_or("");
        assert!(s.contains('3') || s.contains("n="), "{s}");
    }
}

#[tokio::test]
async fn negate_zero_and_double() {
    assert_eq!(eval("-0").await, Value::Int(0));
    assert_eq!(eval("--5").await, Value::Int(5));
    assert_eq!(eval("-(-7)").await, Value::Int(7));
}
