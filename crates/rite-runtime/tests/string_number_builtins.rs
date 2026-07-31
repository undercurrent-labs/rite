//! Strings and numbers.
//!
//! Rite is pitched at "tools and pipelines", where string handling is most of
//! the work — and until now it had `lines`, `words` and `join` and nothing to
//! split, trim, case or slice with. These pin the behaviour, especially the
//! parts that are choices rather than obligations.

use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    run_source("b.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("eval failed for `{src}`: {e}"))
}

async fn text(src: &str) -> String {
    match eval(src).await {
        Value::String(s) => s.to_string(),
        other => panic!("expected a string from `{src}`, got {other:?}"),
    }
}

async fn int(src: &str) -> i64 {
    eval(src)
        .await
        .as_int()
        .unwrap_or_else(|| panic!("expected an int from `{src}`"))
}

#[tokio::test]
async fn splitting() {
    assert_eq!(int(r#"count(split("a,b,c", ","))"#).await, 3);
    // An empty separator gives characters, not Rust's leading/trailing empties.
    assert_eq!(int(r#"count(split("abc", ""))"#).await, 3);
    assert_eq!(int(r#"count(split("abc"))"#).await, 3);
}

#[tokio::test]
async fn trimming_and_case() {
    assert_eq!(text(r#"trim("  hi  ")"#).await, "hi");
    assert_eq!(text(r#"trim_start("  hi  ")"#).await, "hi  ");
    assert_eq!(text(r#"trim_end("  hi  ")"#).await, "  hi");
    assert_eq!(text(r#"upper("héllo")"#).await, "HÉLLO");
    assert_eq!(text(r#"lower("HÉLLO")"#).await, "héllo");
}

#[tokio::test]
async fn padding_counts_characters() {
    assert_eq!(text(r#"pad_start("7", 3, "0")"#).await, "007");
    assert_eq!(text(r#"pad_end("x", 3, ".")"#).await, "x..");
    // Already long enough: unchanged, not truncated.
    assert_eq!(text(r#"pad_start("abcd", 2, "0")"#).await, "abcd");
}

/// The one that would rot silently: `count` counts characters, so everything
/// here must too, or the API is byte-indexed on non-ASCII input only.
#[tokio::test]
async fn slicing_is_character_indexed() {
    assert_eq!(text(r#"slice("δabcdef", 1, 4)"#).await, "abc");
    assert_eq!(text(r#"slice("abcdef", -2)"#).await, "ef");
    // Out of range clamps rather than failing — usable on untrusted input.
    assert_eq!(text(r#"slice("abc", 0, 99)"#).await, "abc");
    assert_eq!(text(r#"slice("abc", 5, 9)"#).await, "");
    // Lists slice by the same rules.
    assert_eq!(int(r#"count(slice([1,2,3,4], 1, 3))"#).await, 2);
}

/// `none`, not `-1`. A sentinel that is also a valid index is how off-by-one
/// bugs get written, and `??` already covers the absent case.
#[tokio::test]
async fn index_of_answers_none_when_absent() {
    assert_eq!(int(r#"index_of("hello", "ll")"#).await, 2);
    assert!(matches!(
        eval(r#"index_of("hello", "z")"#).await,
        Value::None
    ));
    assert_eq!(int(r#"index_of("hello", "z") ?? (0 - 1)"#).await, -1);
}

#[tokio::test]
async fn rounding_answers_with_ints() {
    assert_eq!(int("round(2.5)").await, 3);
    // Half away from zero, so this is -1 rather than Rust's -0.
    assert_eq!(int("round(0 - 0.5)").await, -1);
    assert_eq!(int("floor(2.9)").await, 2);
    assert_eq!(int("ceil(2.1)").await, 3);
    assert_eq!(int("floor(0 - 2.1)").await, -3);
    assert_eq!(int("sqrt(16)").await, 4);
}

/// Parsing untrusted input answers with a Result, so `?` handles it like every
/// other fallible thing in the language.
#[tokio::test]
async fn parsing_answers_with_a_result() {
    assert_eq!(int(r#"parse_int("41")? + 1"#).await, 42);
    assert_eq!(int(r#"parse_int(" 7 ")?"#).await, 7);
    assert!(matches!(
        eval(r#"is_err(parse_int("x"))"#).await,
        Value::Bool(true)
    ));
    assert!(matches!(
        eval(r#"is_err(parse_float("nope"))"#).await,
        Value::Bool(true)
    ));
    assert!(matches!(
        eval(r#"is_ok(parse_int("12"))"#).await,
        Value::Bool(true)
    ));
}

#[tokio::test]
async fn wrong_types_say_so() {
    for src in ["upper(5)", "trim(5)", "sqrt(\"x\")", "round(\"x\")"] {
        let mut ctx = RuntimeContext::new();
        let err = run_source("b.rite", src, &mut ctx)
            .await
            .err()
            .unwrap_or_else(|| panic!("`{src}` should fail"));
        let text = format!("{err}");
        assert!(
            text.contains("expects"),
            "`{src}` should say what it expected: {text}"
        );
    }
}

/// Replacing every empty position is never what anyone meant.
#[tokio::test]
async fn replace_refuses_an_empty_needle() {
    let mut ctx = RuntimeContext::new();
    let err = run_source("b.rite", r#"replace("abc", "", "x")"#, &mut ctx)
        .await
        .expect_err("should fail");
    assert!(format!("{err}").contains("non-empty"));
}
