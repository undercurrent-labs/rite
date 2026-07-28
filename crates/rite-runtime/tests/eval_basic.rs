use rite_runtime::{run_source, RuntimeContext, Value};

#[tokio::test]
async fn arithmetic_and_bindings() {
    let mut ctx = RuntimeContext::new();
    let v = run_source("t.rite", "x ← 1 + 2 * 3\nx", &mut ctx)
        .await
        .unwrap();
    assert_eq!(v, Value::Int(7));
}

#[tokio::test]
async fn truthiness() {
    let mut ctx = RuntimeContext::new();
    // empty list is truthy
    let v = run_source("t.rite", r#"? [] ⟦ #yes ⟧ : ⟦ #no ⟧"#, &mut ctx)
        .await
        .unwrap();
    assert!(matches!(v, Value::Atom(_)));
    // false is falsey
    let mut ctx = RuntimeContext::new();
    let v = run_source("t.rite", r#"? false ⟦ 1 ⟧ : ⟦ 2 ⟧"#, &mut ctx)
        .await
        .unwrap();
    assert_eq!(v, Value::Int(2));
}

#[tokio::test]
async fn pipeline_sum() {
    let mut ctx = RuntimeContext::new();
    let v = run_source("t.rite", "[1, 2, 3, 4] → sum", &mut ctx)
        .await
        .unwrap();
    assert_eq!(v, Value::Int(10));
}

#[tokio::test]
async fn functions() {
    let mut ctx = RuntimeContext::new();
    let v = run_source(
        "t.rite",
        r#"
◆ square(n) ⟦
  ^ n * n
⟧
square(6)
"#,
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(v, Value::Int(36));
}

#[tokio::test]
async fn match_atoms() {
    let mut ctx = RuntimeContext::new();
    let v = run_source(
        "t.rite",
        r#"
~ #ok ⟦
  #ok → 1
  _ → 0
⟧
"#,
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(v, Value::Int(1));
}

#[tokio::test]
async fn record_merge() {
    let mut ctx = RuntimeContext::new();
    let v = run_source("t.rite", r#"⟨a: 1⟩ + ⟨a: 2, b: 3⟩"#, &mut ctx)
        .await
        .unwrap();
    assert_eq!(v.get_field("a"), Value::Int(2));
    assert_eq!(v.get_field("b"), Value::Int(3));
}

#[tokio::test]
async fn mutable_assign() {
    let mut ctx = RuntimeContext::new();
    let v = run_source(
        "t.rite",
        r#"
c ↢ 0
c := c + 1
c := c + 1
c
"#,
        &mut ctx,
    )
    .await
    .unwrap();
    assert_eq!(v, Value::Int(2));
}
