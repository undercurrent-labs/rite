//! What the backend lowers, and what it hands back to the interpreter.
//!
//! These are fast — they inspect emitted Rust without compiling it. The end-to-end check
//! that a compiled binary agrees with `rite run` needs a cargo build per program and lives
//! behind `--ignored` in `build_permissions.rs`.

use rite_compiler::lower;
use rite_compiler::lower::Compiled;
use rite_sem::ProgramIr;

fn ir_of(source: &str) -> ProgramIr {
    let file = rite_core::SourceFile::new(rite_core::FileId(0), "gen.rite", source);
    let (ir, diags) = rite_sem::compile_to_ir(&file);
    assert!(!diags.has_errors(), "{source} failed to compile");
    ir.expect("ir")
}

/// Lower every top-level statement, or report the first thing that stopped it.
fn lower_all(source: &str) -> Result<String, &'static str> {
    let ir = ir_of(source);
    let stmts = ir
        .modules
        .first()
        .map(|m| m.statements.clone())
        .unwrap_or_default();
    let mut out = String::new();
    for s in &stmts {
        match lower::expr(s, &Compiled::new()) {
            Ok(code) => out.push_str(&code),
            Err(lower::Unsupported(why)) => return Err(why),
        }
    }
    Ok(out)
}

#[test]
fn the_compilable_subset_produces_no_interpreter_call() {
    // The point of the backend: these must not route back through `eval_expr`.
    for src in [
        "1 + 2 * 3",
        "x ← 21\nx * 2",
        "[1, 2, 3]",
        "⟨a: 1, b: #ok⟩",
        "⟨a: 1⟩.a",
        "[10, 20][1]",
        "-5",
        "not false",
        "none ?? 42",
        "ok(1)?",
        "true and false",
        "false or true",
        "? true ⟦ 1 ⟧ : ⟦ 2 ⟧",
        "2 ∈ [1, 2]",
        "\"a\" + \"b\"",
        "◆ f(n) ⟦ ^ n ⟧\nf(1)",
    ] {
        let code = lower_all(src).unwrap_or_else(|why| panic!("{src:?} fell back on {why}"));
        assert!(
            !code.contains("eval_expr"),
            "{src:?} lowered to an interpreter call:\n{code}"
        );
    }
}

#[test]
fn short_circuit_operators_do_not_evaluate_both_sides() {
    // `and`/`or` cannot go through `ops::binary`, which takes evaluated operands — doing
    // so would run an effect the program says should not run.
    let and = lower_all("true and false").expect("lowered");
    assert!(and.contains("is_truthy"), "no branch emitted: {and}");
    assert!(
        !and.contains("BinaryOpIr::And"),
        "`and` was lowered as an eager binary op: {and}"
    );

    let or = lower_all("false or true").expect("lowered");
    assert!(!or.contains("BinaryOpIr::Or"), "{or}");
}

#[test]
fn a_bare_name_resolves_through_the_shared_three_tier_lookup() {
    // Checking only `ctx.env` reported `undefined name` for every builtin used as a value.
    let code = lower_all("str(1)").expect("lowered");
    assert!(
        code.contains("lookup_global"),
        "a global must use the shared resolver: {code}"
    );
    assert!(
        !code.contains("ctx.env.get(\"str\")"),
        "environment-only lookup is the bug this replaced: {code}"
    );
}

#[test]
fn a_record_is_built_without_naming_indexmap() {
    // The generated crate's manifest lists only what the emitted code names directly, and
    // an unresolved `indexmap` there is a build failure rather than a fallback.
    let code = lower_all("⟨a: 1⟩").expect("lowered");
    assert!(code.contains("Value::record"), "{code}");
    assert!(!code.contains("indexmap"), "{code}");
}

#[test]
fn floats_round_trip_through_the_emitted_literal() {
    // `{}` would print `1` for 1.0 and the generated code would build an int.
    let code = lower_all("1.0").expect("lowered");
    assert!(code.contains("Value::Float(1.0f64)"), "{code}");
}

#[test]
fn a_string_literal_is_escaped_for_rust() {
    let code = lower_all(r#""he said \"hi\"\n""#).expect("lowered");
    assert!(code.contains(r#"\"hi\""#), "quotes not escaped: {code}");
    assert!(
        !code.contains('\n'),
        "a raw newline would break the literal"
    );
}

#[test]
fn unsupported_nodes_report_what_stopped_them() {
    // The build note is only useful if it names the construct.
    for (src, expected) in [
        ("[1] → sum", "Pipeline"),
        ("~ 1 ⟦\n  1 → #one\n  _ → #other\n⟧", "Match"),
        ("{ |x| x }", "Closure"),
        ("! @console.println(\"x\")", "CapabilityCall(@console)"),
    ] {
        match lower_all(src) {
            Err(why) => assert_eq!(why, expected, "for {src:?}"),
            Ok(code) => panic!("{src:?} unexpectedly lowered:\n{code}"),
        }
    }
}

#[test]
fn a_non_console_capability_is_lowered_rather_than_deferred() {
    // Only `@console` needs the evaluator (it writes the context's buffer); everything
    // else goes straight to the capability host.
    let code = lower_all("! @clock.now()").expect("lowered");
    assert!(code.contains("capabilities"), "{code}");
    assert!(!code.contains("eval_expr"), "{code}");
}

#[test]
fn a_block_pops_its_frame_on_every_path() {
    // An early `?` — a Rite `^`, or any error — must not skip the pop, or the environment
    // leaks a frame and later lookups resolve in the wrong scope.
    let code = lower_all("? true ⟦ 1 ⟧ : ⟦ 2 ⟧").expect("lowered");
    let pushes = code.matches("push_frame").count();
    let pops = code.matches("pop_frame").count();
    assert_eq!(pushes, pops, "unbalanced frames:\n{code}");
    assert!(pushes > 0, "a block should scope: {code}");
    // The `?` is applied after the pop, not inside the async block.
    assert!(code.contains("pop_frame(); __r?"), "{code}");
}

#[test]
fn a_survey_separates_lowered_functions_from_fallbacks() {
    let ir = ir_of(
        "◆ pure(n) ⟦ ^ n * 2 ⟧\n\
         ◆ piped(xs) ⟦ ^ xs → sum ⟧\n\
         pure(1)\n",
    );
    let (ok, fell_back) = lower::survey(&ir);
    assert!(ok.contains("pure"), "ok: {ok:?}");
    assert_eq!(
        fell_back,
        vec![("piped".to_string(), "Pipeline")],
        "fallbacks should name the function and the construct"
    );
}

#[test]
fn a_unicode_function_name_mangles_to_a_rust_identifier() {
    // Rite identifiers accept any non-ASCII byte; Rust does not.
    let m = lower::mangle("café");
    assert!(
        m.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "not a Rust identifier: {m}"
    );
    assert_ne!(lower::mangle("café"), lower::mangle("cafe"));
}

// ------------------------------------------------------------------ compiled functions

/// A compiled function must keep every guarantee the interpreter's call path makes.
///
/// The prologue here is hand-written to mirror `call_value`/`call_block`. Dropping any of
/// it would let a compiled binary escape something the interpreter enforces — and the
/// symptom would be a stack overflow or a hung process, not a test failure elsewhere.
#[test]
fn a_compiled_function_keeps_the_interpreter_guarantees() {
    let ir = ir_of("◆ f(n) ⟦ ^ n * 2 ⟧\nf(1)\n");
    let f = ir.functions.iter().find(|f| f.name == "f").expect("fn");
    let code = lower::function(f, &Compiled::new()).expect("lowered");

    assert!(code.contains("budget.tick()"), "no budget tick:\n{code}");
    assert!(
        code.contains("check_depth"),
        "no depth guard — runaway recursion would overflow the stack instead of \
         reporting a budget error:\n{code}"
    );
    assert!(code.contains("call_depth += 1"), "{code}");
    assert!(
        code.contains("call_depth -= 1"),
        "depth must be restored:\n{code}"
    );
    assert!(code.contains("arity mismatch"), "no arity check:\n{code}");
    assert_eq!(
        code.matches("push_frame").count(),
        code.matches("pop_frame").count(),
        "unbalanced frames:\n{code}"
    );
    // `^` becomes the function's value here and nowhere else.
    assert!(code.contains("EvalError::Return(v)) => Ok(v)"), "{code}");
}

#[test]
fn a_compiled_function_boxes_so_it_can_recurse() {
    // A directly-recursive `async fn` has an infinitely-sized future and will not compile.
    let ir = ir_of("◆ down(n) ⟦ ^ ? n <= 0 ⟦ 0 ⟧ : ⟦ down(n - 1) ⟧ ⟧\ndown(3)\n");
    let f = &ir.functions[0];
    let code = lower::function(f, &Compiled::new()).expect("lowered");
    assert!(code.contains("Box::pin"), "{code}");
    assert!(
        !code.contains("pub async fn"),
        "must not be a plain async fn:\n{code}"
    );
}

#[test]
fn a_call_to_a_compiled_function_is_a_direct_rust_call() {
    // The whole point of the second stage: with only top-level statements compiled, a
    // `fib(24)` binary ran in exactly the interpreter's time because the body was not.
    let ir = ir_of("◆ f(n) ⟦ ^ n ⟧\nf(1)\n");
    let mut compiled = Compiled::new();
    compiled.insert("f".to_string());
    let stmt = &ir.modules.first().expect("module").statements[0];
    let code = lower::expr(stmt, &compiled).expect("lowered");

    assert!(
        code.contains(&lower::mangle("f")),
        "not a direct call:\n{code}"
    );
    assert!(
        !code.contains("call_value_public"),
        "a compiled callee should not go through interpreter dispatch:\n{code}"
    );
}

#[test]
fn a_call_to_an_uncompiled_function_still_goes_through_dispatch() {
    // A function that fell back, a closure value, a builtin used as a value — all of them
    // resolve through the interpreter, so the generic path has to stay.
    let ir = ir_of("◆ f(n) ⟦ ^ n ⟧\nf(1)\n");
    let stmt = &ir.modules.first().expect("module").statements[0];
    let code = lower::expr(stmt, &Compiled::new()).expect("lowered");
    assert!(code.contains("call_value_public"), "{code}");
}
