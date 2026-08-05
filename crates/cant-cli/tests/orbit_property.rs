//! Property: an orbit accepts at most its `:max` candidates.
//!
//! The last unproven row of the property table in docs/cant/checklist.md.
//! Hitting the limit is a failure (CANT-O002), not a truncated answer, so the
//! property has two halves: a run that succeeds collected no more than `:max`
//! items, and a run whose reachable set exceeds `:max` fails naming CANT-O002.

use proptest::prelude::*;

fn run_cant(text: &str) -> cant::run::ExecutionResult {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(cant::run::run(
        "orbit_property.cant",
        text,
        cant::run::RunOptions::default(),
    ))
}

fn collected_len(r: &cant::run::ExecutionResult) -> Option<usize> {
    match r.value.as_ref() {
        Some(rite_runtime::Value::List(xs)) => Some(xs.len()),
        _ => None,
    }
}

/// The reachable set is {1..=k} (the ward stops growth at k), so a `:max` of
/// exactly k succeeds and one below it fails.
#[test]
fn orbit_limit_boundary_is_exact() {
    let ok = run_cant("[1] -> * -> ~{ ?{ $ < 6 } -> $ + 1 } :max 6 -> []");
    assert!(ok.succeeded(), "at the boundary: {}", ok.render());
    assert_eq!(collected_len(&ok), Some(6));

    let over = run_cant("[1] -> * -> ~{ ?{ $ < 6 } -> $ + 1 } :max 5 -> []");
    assert!(!over.succeeded(), "one under the reachable set must fail");
    assert!(
        over.render().contains("CANT-O002"),
        "failure must name CANT-O002: {}",
        over.render()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Unbounded growth hits the limit for every `:max`, and says so.
    #[test]
    fn unbounded_growth_always_fails_with_o002(max in 1usize..=48) {
        let src = format!("[0] -> * -> ~{{ $ + 1 }} :max {max} -> []");
        let r = run_cant(&src);
        prop_assert!(!r.succeeded(), ":max {} accepted unbounded growth", max);
        prop_assert!(
            r.render().contains("CANT-O002"),
            "failure did not name CANT-O002: {}",
            r.render()
        );
    }

    /// A run that succeeds never collected more than `:max` items.
    #[test]
    fn accepted_items_never_exceed_max(k in 1usize..=32) {
        let src = format!("[1] -> * -> ~{{ ?{{ $ < {k} }} -> $ + 1 }} :max 64 -> []");
        let r = run_cant(&src);
        prop_assert!(r.succeeded(), "bounded orbit failed: {}", r.render());
        let n = collected_len(&r).expect("collect answers a list");
        prop_assert!(n <= 64, "collected {} > :max 64", n);
        prop_assert_eq!(n, k, "reachable set is 1..={}", k);
    }
}
