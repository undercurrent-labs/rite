//! `parallel` — concurrency with deterministic results.
//!
//! It used to be a lie: dispatched straight to `map`, running every branch in
//! sequence while the name promised otherwise. These pin the properties it now
//! has, and the ones it must keep having.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext, Value};
use std::time::{Duration, Instant};

async fn eval(src: &str) -> (Value, Vec<String>) {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source("p.rite", src, &mut ctx)
        .await
        .expect("program should run");
    (v, ctx.stdout.clone())
}

/// The point of the whole thing: branches overlap at their await points.
///
/// Eight 100 ms sleeps are ~800 ms in sequence and ~100 ms together. The ceiling
/// sits at 400 ms, which leaves 300 ms of slack below a serial run and 300 ms
/// above a concurrent one — the gap itself is the signal, so a loaded runner
/// cannot land in the middle of it.
#[tokio::test]
async fn branches_overlap() {
    let started = Instant::now();
    let (_v, _out) = eval(
        r#"
◆! nap(ms) ⟦
  ! @clock.sleep(ms)
  ^ ms
⟧
! parallel([100, 100, 100, 100, 100, 100, 100, 100], nap)
"#,
    )
    .await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(400),
        "eight 100ms sleeps took {elapsed:?}; sequential would be ~800ms"
    );
}

/// Completion order is the reverse of input order here, and neither the results
/// nor the output may follow it.
#[tokio::test]
async fn results_and_output_follow_input_order() {
    let (v, out) = eval(
        r#"
◆! work(ms) ⟦
  ! @clock.sleep(ms)
  ! @console.println("done " + str(ms))
  ^ ms
⟧
! parallel([200, 120, 40], work)
"#,
    )
    .await;
    let printed: Vec<&str> = out.iter().map(|s| s.trim()).collect();
    assert_eq!(
        printed,
        vec!["done 200", "done 120", "done 40"],
        "output must splice in input order, not completion order"
    );
    match v {
        Value::List(xs) => {
            let got: Vec<i64> = xs.iter().filter_map(|v| v.as_int()).collect();
            assert_eq!(got, vec![200, 120, 40], "results follow input order");
        }
        other => panic!("expected a list, got {other:?}"),
    }
}

/// Two branches fail. The one reported is the first in *input* order, not the
/// first to fail in time — same reason the results are ordered.
#[tokio::test]
async fn the_first_failure_in_input_order_is_reported() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let err = run_source(
        "p.rite",
        r#"
◆! maybe(n) ⟦
  ! @clock.sleep(n * 20)
  ? n < 3 ⟦ fail("branch " + str(n)) ⟧
  ^ n
⟧
! parallel([3, 2, 1], maybe)
"#,
        &mut ctx,
    )
    .await
    .expect_err("a branch fails");
    let text = format!("{err}");
    assert!(
        text.contains("branch 2"),
        "expected the earlier branch in input order, got: {text}"
    );
}

/// Branches share the host, so state one writes is state the others and the
/// parent can read. Forking copies output buffers, not the world.
#[tokio::test]
async fn branches_share_host_state() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source(
        "p.rite",
        r#"
◆! stash(n) ⟦
  ! @store.set("acc", str(n), n)
  ^ n
⟧
! parallel([1, 2, 3], stash)
@store.get("acc", "2")
"#,
        &mut ctx,
    )
    .await
    .expect("program should run");
    // Rendering a Result needs the atom table, which is why this reads the value
    // through the context rather than `Display`.
    assert_eq!(
        v.to_display(&ctx.atoms),
        "ok(2)",
        "a branch's write must be visible to the parent"
    );
}

/// Output written before a branch fails is not lost.
#[tokio::test]
async fn output_survives_a_failing_branch() {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let _ = run_source(
        "p.rite",
        r#"
◆! noisy(n) ⟦
  ! @console.println("start " + str(n))
  ? n = 2 ⟦ fail("boom") ⟧
  ^ n
⟧
! parallel([1, 2], noisy)
"#,
        &mut ctx,
    )
    .await;
    let printed = ctx.stdout.join("");
    assert!(
        printed.contains("start 1") && printed.contains("start 2"),
        "output before the failure should still arrive: {printed:?}"
    );
}

#[tokio::test]
async fn an_empty_list_is_an_empty_list() {
    let (v, _) = eval("◆! f(n) ⟦ ^ n ⟧\n! parallel([], f)").await;
    assert_eq!(format!("{v}"), "[]");
}
