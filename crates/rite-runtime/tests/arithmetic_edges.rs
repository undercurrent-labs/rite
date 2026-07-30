//! Arithmetic that must produce a Rite error instead of panicking the host process.

use rite_runtime::{run_source, RuntimeContext, Value};

/// `0 - 9223372036854775807 - 1` is the only way to spell `i64::MIN`: the lexer has no
/// negative literals, and `9223372036854775808` does not fit an `i64`.
const MIN: &str = "min ← 0 - 9223372036854775807 - 1\n";

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

async fn eval_err(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    match run_source("t.rite", src, &mut ctx).await {
        Ok(v) => panic!("expected an error, got {v:?}\n--- source ---\n{src}"),
        Err(e) => e.to_string().to_lowercase(),
    }
}

#[tokio::test]
async fn int_min_div_negative_one_errors() {
    let err = eval_err(&format!("{MIN}min / (0 - 1)")).await;
    assert!(err.contains("overflow"), "{err}");
}

#[tokio::test]
async fn int_min_rem_negative_one_errors() {
    let err = eval_err(&format!("{MIN}min % (0 - 1)")).await;
    assert!(err.contains("overflow"), "{err}");
}

#[tokio::test]
async fn int_min_idiv_negative_one_errors() {
    let err = eval_err(&format!("{MIN}idiv(min, 0 - 1)")).await;
    assert!(err.contains("overflow"), "{err}");
}

/// The existing division-by-zero errors must survive the checked-arithmetic change.
#[tokio::test]
async fn division_by_zero_still_errors() {
    for src in ["1 / 0", "1 % 0", "idiv(1, 0)"] {
        let err = eval_err(src).await;
        assert!(err.contains("division by zero"), "{src}: {err}");
    }
}

#[tokio::test]
async fn ordinary_division_and_remainder_unchanged() {
    assert_eq!(eval("7 / 2").await, Value::Int(3));
    assert_eq!(eval("7 % 3").await, Value::Int(1));
    assert_eq!(eval("(0 - 7) / 2").await, Value::Int(-3));
    assert_eq!(eval("(0 - 7) % 2").await, Value::Int(-1));
    assert_eq!(eval("idiv(7, 2)").await, Value::Int(3));
    assert_eq!(eval("7.0 / 2.0").await, Value::Float(3.5));
    // Float division by zero stays IEEE, as before.
    assert_eq!(eval("1.0 / 0.0").await, Value::Float(f64::INFINITY));
}

#[tokio::test]
async fn add_sub_mul_overflow_error_not_panic() {
    for src in [
        "9223372036854775807 + 1",
        "0 - 9223372036854775807 - 2",
        "9223372036854775807 * 2",
    ] {
        let err = eval_err(src).await;
        assert!(err.contains("overflow"), "{src}: {err}");
    }
}

#[tokio::test]
async fn negate_and_abs_of_int_min_error_not_panic() {
    let err = eval_err(&format!("{MIN}0 - min")).await;
    assert!(err.contains("overflow"), "{err}");
    let err = eval_err(&format!("{MIN}abs(min)")).await;
    assert!(err.contains("overflow"), "{err}");
}

/// `pow` used `i64::pow`, which panics on overflow. Exponents that do not fit fall
/// back to the float result, matching what exponents above 32 already did.
#[tokio::test]
async fn pow_overflow_falls_back_to_float() {
    assert_eq!(eval("pow(2, 10)").await, Value::Int(1024));
    let v = eval("pow(10, 30)").await;
    match v {
        Value::Float(f) => assert!(f > 0.0 && f.is_finite(), "{f}"),
        other => panic!("expected float, got {other:?}"),
    }
    let v = eval("pow(9223372036854775807, 32)").await;
    assert!(matches!(v, Value::Float(_)), "{v:?}");
}

/// `Ord::clamp` asserts `min <= max`; reversed bounds must be an error, not an abort.
#[tokio::test]
async fn clamp_with_reversed_bounds_errors() {
    assert_eq!(eval("clamp(15, 0, 10)").await, Value::Int(10));
    let err = eval_err("clamp(5, 10, 1)").await;
    assert!(err.contains("clamp"), "{err}");
    let err = eval_err("clamp(5.0, 10.0, 1.0)").await;
    assert!(err.contains("clamp"), "{err}");
}

/// Stepping past `i64::MAX` ends the range instead of overflowing the step counter.
#[tokio::test]
async fn range_step_past_int_max_terminates() {
    assert_eq!(
        eval("range(9223372036854775805, 9223372036854775807, 5)").await,
        Value::list(vec![Value::Int(9223372036854775805)])
    );
    assert_eq!(
        eval("range_incl(9223372036854775805, 9223372036854775807, 5)").await,
        Value::list(vec![Value::Int(9223372036854775805)])
    );
    assert_eq!(
        eval(&format!("{MIN}range(min + 2, min, 0 - 5)")).await,
        Value::list(vec![Value::Int(-9223372036854775806)])
    );
    // Ordinary ranges are unaffected.
    assert_eq!(
        eval("range(0, 4)").await,
        Value::list(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3)
        ])
    );
    assert_eq!(
        eval("range_incl(1, 3)").await,
        Value::list(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
}

/// `str::repeat` aborts the process on capacity overflow.
#[tokio::test]
async fn repeat_with_absurd_count_errors() {
    assert_eq!(eval(r#"repeat("ab", 3)"#).await, Value::string("ababab"));
    let err = eval_err(r#"repeat("ab", 9223372036854775807)"#).await;
    assert!(err.contains("repeat"), "{err}");
    let err = eval_err("xs ← [1, 2]\nrepeat(xs, 9223372036854775807)").await;
    assert!(err.contains("repeat"), "{err}");
}

/// Negative and out-of-range indexes stay `none` rather than panicking.
#[tokio::test]
async fn index_out_of_range_is_none() {
    assert_eq!(eval("xs ← [1, 2]\nxs[0 - 1]").await, Value::None);
    assert_eq!(eval("xs ← [1, 2]\nxs[99]").await, Value::None);
    assert_eq!(
        eval(&format!("{MIN}xs ← [1, 2]\nxs[min]")).await,
        Value::None
    );
}

/// Unbounded recursion must be stopped by the budget, not by the native stack.
///
/// A stack overflow is a process *abort*: no error value, no buffered output, nothing a
/// host can catch. The depth limit exists to prevent exactly that, so it has to be low
/// enough that the budget wins. It was 256 on every profile, which is unreachable in a
/// debug build — one Rite call costs several nested async frames, measured at roughly
/// 64 KB in debug — so this aborted instead. Linux CI passed only because the tests that
/// recursed also had a step budget that tripped first; macOS, with a smaller test-thread
/// stack, did not.
///
/// Runs on an 8 MiB thread, matching the process main thread that `rite run` evaluates on
/// — the depth limit is only meaningful relative to a stack size, and that is the one the
/// CLI actually provides. See `DEFAULT_MAX_CALL_DEPTH` for the measured relationship.
#[test]
fn deep_recursion_reports_a_budget_error_not_an_abort() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("runtime");
            rt.block_on(async {
                let mut ctx = rite_runtime::RuntimeContext::new();
                // Only the depth limit may stop this: no step ceiling, no timeout.
                ctx.budget = rite_runtime::ExecutionBudget::new()
                    .with_max_steps(u64::MAX)
                    .with_timeout(std::time::Duration::from_secs(60));
                let err = rite_runtime::run_source(
                    "deep.rite",
                    "◆ bomb(n) ⟦ ^ bomb(n + 1) ⟧\nbomb(0)\n",
                    &mut ctx,
                )
                .await
                .expect_err("unbounded recursion must fail");
                err.to_string()
            })
        })
        .expect("spawn");

    // A stack overflow aborts the process, so reaching this line at all is part of the
    // assertion; `join` would report a panic, and an abort would take the test binary out.
    let message = handle
        .join()
        .expect("the evaluator must not abort the process");
    assert!(
        message.to_lowercase().contains("depth"),
        "expected a call-depth budget error, got: {message}"
    );
}

/// The default limit must be low enough for the profile actually being run.
#[test]
fn the_default_depth_limit_matches_the_build_profile() {
    let d = rite_runtime::ExecutionBudget::new().max_call_depth;
    if cfg!(debug_assertions) {
        assert!(
            d <= 32,
            "debug frames are ~64 KB per Rite call; {d} levels will not fit the stack"
        );
    } else {
        assert!(
            d >= 128,
            "release frames are small; {d} is needlessly strict"
        );
    }
}
