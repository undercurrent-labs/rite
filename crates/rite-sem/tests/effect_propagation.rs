//! Effect discipline across the call graph.
//!
//! `!` used to mark only the syntactic site of a host call, so a function that
//! wrapped one was callable with no marker at all and `rite check` accepted it —
//! the "explicit effects" claim stopped holding at the first function boundary.
//! A declaration now states the effect, the compiler checks the body against it,
//! and callers mark the call.

fn diagnostics(src: &str) -> String {
    let (_program, diags, _sources) = rite_sem::compile_source("t.rite", src);
    format!("{:?}", diags.into_vec())
}

fn accepts(src: &str) -> bool {
    let (_program, diags, _sources) = rite_sem::compile_source("t.rite", src);
    !diags.has_errors()
}

#[test]
fn a_declared_effect_needs_a_marker_at_the_call() {
    assert!(accepts("◆! f() ⟦ ! @console.println(\"x\") ⟧\n! f()"));
    let text = diagnostics("◆! f() ⟦ ! @console.println(\"x\") ⟧\nf()");
    assert!(text.contains("requires `!`"), "{text}");
}

#[test]
fn an_effectful_body_must_declare_itself() {
    let text = diagnostics("◆ f() ⟦ ! @console.println(\"x\") ⟧");
    assert!(
        text.contains("not declared") && text.contains('f'),
        "reported at the declaration: {text}"
    );
}

/// The point of the whole change: one layer of wrapping used to hide the effect.
#[test]
fn effects_travel_through_the_call_graph() {
    let text = diagnostics("◆! inner() ⟦ ! @console.println(\"x\") ⟧\n◆ outer() ⟦ ! inner() ⟧");
    assert!(
        text.contains("outer") && text.contains("not declared"),
        "`outer` calls an effectful function, so it is effectful: {text}"
    );
    assert!(accepts(
        "◆! inner() ⟦ ! @console.println(\"x\") ⟧\n◆! outer() ⟦ ! inner() ⟧\n! outer()"
    ));
}

/// Recursion makes this a fixed point rather than a walk; it must terminate.
#[test]
fn mutual_recursion_settles() {
    assert!(accepts(
        "◆! a(n) ⟦ ? n <= 0 ⟦ ^ 0 ⟧ : ⟦ ^ ! b(n - 1) ⟧ ⟧\n\
         ◆! b(n) ⟦ ^ ! a(n - 1) ⟧\n! a(3)"
    ));
}

#[test]
fn pure_functions_are_untouched() {
    assert!(accepts(
        "◆ add(a, b) ⟦ ^ a + b ⟧\n! @console.println(str(add(1, 2)))"
    ));
}

/// A promise about the API, not a description of today's body — so a function may
/// reserve the right to perform effects later without breaking callers.
#[test]
fn declaring_an_effect_without_performing_one_is_allowed() {
    assert!(accepts("◆! reserved() ⟦ ^ 1 ⟧\n! reserved()"));
}

#[test]
fn the_console_builtins_need_a_marker_too() {
    // `println(...)` reached the terminal with no marker at all, which made the
    // whole discipline optional.
    assert!(diagnostics("println(\"x\")").contains("requires `!`"));
    assert!(accepts("! println(\"x\")"));
}

#[test]
fn passing_an_effectful_function_marks_the_call() {
    let text = diagnostics("◆! shout(n) ⟦ ! @console.println(str(n)) ⟧\n[1] → each(shout)");
    assert!(text.contains("shout"), "{text}");
    assert!(accepts(
        "◆! shout(n) ⟦ ! @console.println(str(n)) ⟧\n! ([1] → each(shout))"
    ));
}

/// An inline lambda carries its own `!` in plain sight, so no second marker.
#[test]
fn an_inline_effectful_lambda_needs_no_extra_marker() {
    assert!(accepts("[1] → each { |n| ! @console.println(str(n)) }"));
}
