//! The allocation-free evaluation path must be indistinguishable from the async one.
//!
//! `eval_operand` runs a subtree directly when `is_sync` accepts it, skipping the boxed
//! future that an async tree-walker otherwise allocates per node. That is two code paths
//! for the same semantics, so these tests pin the ways they could diverge:
//!
//!   * a value computed on one path must equal the other;
//!   * side effects must happen exactly once — an earlier design evaluated speculatively
//!     and bailed on reaching an async node, so a `:=` in the abandoned part ran twice;
//!   * the step budget must be charged the same, for the same reason.

use rite_runtime::{run_source, RuntimeContext};

async fn eval(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    let v = run_source("sync.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("failed: {src}\n{e}"));
    format!("{}{}", ctx.stdout.join(""), v)
}

/// A pure expression (sync path) and the same expression forced through the async path
/// by a function call must agree.
#[tokio::test]
async fn sync_and_async_paths_agree() {
    // `id` is a user function, so any expression containing it cannot take the sync
    // path — the same arithmetic then runs through the async arms.
    let cases = [
        ("2 + 3 * 4", "id(2) + id(3) * id(4)"),
        ("(10 - 4) / 2", "(id(10) - id(4)) / id(2)"),
        ("true ∧ false", "id(true) ∧ id(false)"),
        ("false ∨ true", "id(false) ∨ id(true)"),
        ("-7 + 2", "-id(7) + id(2)"),
        ("¬ false", "¬ id(false)"),
        ("[1, 2, 3] → count", "[id(1), id(2), id(3)] → count"),
        ("⟨a: 1, b: 2⟩.b", "⟨a: id(1), b: id(2)⟩.b"),
        ("[10, 20, 30][1]", "[id(10), id(20), id(30)][id(1)]"),
        ("none ?? 5", "id(none) ?? id(5)"),
        ("\"ab\" + \"cd\"", "id(\"ab\") + id(\"cd\")"),
        ("1 = 1", "id(1) = id(1)"),
        ("3 < 4", "id(3) < id(4)"),
        ("2 ∈ [1, 2]", "id(2) ∈ [id(1), id(2)]"),
        ("2 ∉ [1, 3]", "id(2) ∉ [id(1), id(3)]"),
    ];
    for (pure, forced) in cases {
        let a = eval(&format!("◆ id(x) ⟦ ^ x ⟧\n{pure}\n")).await;
        let b = eval(&format!("◆ id(x) ⟦ ^ x ⟧\n{forced}\n")).await;
        assert_eq!(a, b, "`{pure}` and `{forced}` disagree");
    }
}

/// Overflow and division errors must surface identically on both paths.
#[tokio::test]
async fn errors_match_on_both_paths() {
    for (pure, forced) in [
        ("1 / 0", "id(1) / id(0)"),
        ("9223372036854775807 + 1", "id(9223372036854775807) + id(1)"),
    ] {
        let mut a = RuntimeContext::new();
        let mut b = RuntimeContext::new();
        let ra = run_source("a.rite", &format!("◆ id(x) ⟦ ^ x ⟧\n{pure}\n"), &mut a).await;
        let rb = run_source("b.rite", &format!("◆ id(x) ⟦ ^ x ⟧\n{forced}\n"), &mut b).await;
        assert_eq!(
            ra.unwrap_err().to_string(),
            rb.unwrap_err().to_string(),
            "`{pure}` and `{forced}` report different errors"
        );
    }
}

/// Mutation through a closure body — the one place assignment mixes with async calls —
/// must apply exactly once.
///
/// Note on reachability: `:=` and `←` are statement forms, so an assignment cannot nest
/// inside a record or list literal beside a call. That means the speculative design could
/// not actually have double-applied a mutation from source; what it *did* do was charge
/// the step budget for work it then discarded. The predicate-first design avoids both by
/// construction, and this pins the behaviour that is reachable.
#[tokio::test]
async fn mutation_in_a_closure_body_applies_once() {
    let out = eval(
        "n ↢ 0\n\
         [1, 2, 3] → each { |i| n := n + i }\n\
         n\n",
    )
    .await;
    assert!(out.ends_with('6'), "counter should be 6, got `{out}`");
}

/// A subtree that cannot take the sync path must not be charged twice for the part that
/// could. The speculative design evaluated the left operand, hit the call, threw the
/// result away, and let the async path charge for it again.
#[tokio::test]
async fn a_bailing_subtree_is_not_charged_twice() {
    // `1 + 2 + id(3)`: the `1 + 2` part is sync-able, the whole is not. Give it a budget
    // that fits one honest evaluation with a little room; double-charging blew it.
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::new().with_max_steps(40);
    run_source(
        "b.rite",
        "◆ id(x) ⟦ ^ x ⟧\nr ← 1 + 2 + id(3)\nr\n",
        &mut ctx,
    )
    .await
    .expect("should fit comfortably in the budget");
}

/// The step budget is charged per node on both paths, so a runaway pure expression is
/// still stopped.
#[tokio::test]
async fn the_budget_still_applies_to_the_sync_path() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::new().with_max_steps(5);
    let err = run_source("b.rite", "1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9\n", &mut ctx)
        .await
        .expect_err("a pure expression must still be charged steps");
    assert!(err.to_string().contains("step budget"), "{err}");
}
