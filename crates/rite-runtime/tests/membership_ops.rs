//! `∈` / `∉` (`in` / `not in`): one evaluation per operand, same membership rules.

use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval_with_output(src: &str) -> (Value, Vec<String>) {
    let mut ctx = RuntimeContext::new();
    match run_source("t.rite", src, &mut ctx).await {
        Ok(v) => (v, ctx.stdout.clone()),
        Err(rite_runtime::EvalError::Compile(d)) => {
            let msgs: Vec<String> = d.iter().map(|x| x.code.to_string()).collect();
            panic!("compile failed: {}\n--- source ---\n{src}", msgs.join(", "))
        }
        Err(e) => panic!("eval failed: {e}\n--- source ---\n{src}"),
    }
}

async fn eval(src: &str) -> Value {
    eval_with_output(src).await.0
}

/// `∉` used to re-evaluate both operands after the shared evaluation above it, so a
/// side-effecting operand ran twice.
#[tokio::test]
async fn not_in_evaluates_each_operand_once() {
    let (v, out) = eval_with_output(
        r#"
◆! noisy(x) ⟦
  do println("tick")
  ^ x
⟧
xs ← [1, 2]
res ← ! noisy(1) ∉ xs
res
"#,
    )
    .await;
    assert_eq!(v, Value::Bool(false));
    assert_eq!(out, vec!["tick\n".to_string()], "left operand ran twice");
}

#[tokio::test]
async fn in_evaluates_each_operand_once() {
    let (v, out) = eval_with_output(
        r#"
◆! noisy(x) ⟦
  do println("tick")
  ^ x
⟧
xs ← [1, 2]
res ← ! noisy(1) ∈ xs
res
"#,
    )
    .await;
    assert_eq!(v, Value::Bool(true));
    assert_eq!(out, vec!["tick\n".to_string()], "left operand ran twice");
}

/// Both sides, both operators: four effects for four operand evaluations.
#[tokio::test]
async fn both_operands_of_both_operators_run_once() {
    let (v, out) = eval_with_output(
        r#"
◆! noisy(x) ⟦
  do println("tick")
  ^ x
⟧
◆! noisy_list() ⟦
  do println("list")
  ^ [1, 2]
⟧
a ← ! noisy(1) ∈ ! noisy_list()
b ← ! noisy(9) ∉ ! noisy_list()
res ← [a, b]
res
"#,
    )
    .await;
    assert_eq!(v, Value::list(vec![Value::Bool(true), Value::Bool(true)]));
    assert_eq!(
        out,
        vec![
            "tick\n".to_string(),
            "list\n".to_string(),
            "tick\n".to_string(),
            "list\n".to_string(),
        ]
    );
}

/// `∉` must stay the exact negation of `∈` for every container kind.
#[tokio::test]
async fn not_in_is_the_negation_of_in() {
    assert_eq!(eval("xs ← [1, 2]\n1 ∈ xs").await, Value::Bool(true));
    assert_eq!(eval("xs ← [1, 2]\n1 ∉ xs").await, Value::Bool(false));
    assert_eq!(eval("xs ← [1, 2]\n3 ∈ xs").await, Value::Bool(false));
    assert_eq!(eval("xs ← [1, 2]\n3 ∉ xs").await, Value::Bool(true));
    // Atoms match list entries by identity and by name.
    assert_eq!(eval("xs ← [#a, #b]\n#a ∈ xs").await, Value::Bool(true));
    assert_eq!(eval("xs ← [#a, #b]\n#c ∉ xs").await, Value::Bool(true));
    assert_eq!(eval("xs ← [\"a\"]\n#a ∈ xs").await, Value::Bool(true));
    assert_eq!(eval("xs ← [\"a\"]\n#a ∉ xs").await, Value::Bool(false));
    // Atoms match record keys.
    assert_eq!(eval("r ← ⟨a: 1⟩\n#a ∈ r").await, Value::Bool(true));
    assert_eq!(eval("r ← ⟨a: 1⟩\n#b ∉ r").await, Value::Bool(true));
    // Strings match substrings; records match keys and values.
    assert_eq!(eval(r#""ell" ∈ "hello""#).await, Value::Bool(true));
    assert_eq!(eval(r#""xyz" ∉ "hello""#).await, Value::Bool(true));
    assert_eq!(eval("r ← ⟨a: 1⟩\n\"a\" ∈ r").await, Value::Bool(true));
    assert_eq!(eval("r ← ⟨a: 1⟩\n\"b\" ∉ r").await, Value::Bool(true));
}

/// ASCII dialect spelling of the same operators.
#[tokio::test]
async fn ascii_in_and_not_in() {
    assert_eq!(eval("xs <- [1, 2]\n1 in xs").await, Value::Bool(true));
    assert_eq!(eval("xs <- [1, 2]\n3 not in xs").await, Value::Bool(true));
}
