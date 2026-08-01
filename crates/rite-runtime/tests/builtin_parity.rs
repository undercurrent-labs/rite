//! The resolver's builtin list and the interpreter's dispatch must agree.
//!
//! `rite_sem::resolve::BUILTIN_NAMES` is what makes a bare name resolve instead of
//! reporting E020. If the interpreter cannot dispatch one of those names, the program
//! type-checks and then fails at runtime; if the interpreter dispatches a name the
//! resolver does not list, the feature is unreachable. Neither is caught by any other
//! test, and this is the same drift that let `@db.*` skip its effect marker for a while.

use rite_runtime::builtins::call_builtin;
use rite_runtime::AtomInterner;
use rite_runtime::EvalError;
use rite_runtime::Limits;
use rite_sem::resolve::BUILTIN_NAMES;

/// Names the evaluator handles itself because they take a callback (`call_native` in
/// eval.rs). `call_builtin` reports them distinctly rather than as unknown, so this
/// test does not need to know which is which — it only rejects "unknown builtin".
fn is_unknown(err: &EvalError) -> bool {
    err.to_string().starts_with("unknown builtin")
}

#[test]
fn every_declared_builtin_is_dispatchable() {
    let mut unknown = Vec::new();
    for name in BUILTIN_NAMES {
        // Call with no arguments: a missing arg is an arity/type error, which still
        // proves the name reached an implementation. Only "unknown builtin" means the
        // resolver promised a name the runtime cannot honour.
        if let Err(e) = call_builtin(name, vec![], &AtomInterner::new(), Limits::unlimited()) {
            if is_unknown(&e) {
                unknown.push(*name);
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "declared in rite-sem BUILTIN_NAMES but not dispatched by rite-runtime: {unknown:?}\n\
         add a `call_builtin` arm (or an evaluator arm in `call_native`) in the same change"
    );
}

#[test]
fn an_undeclared_name_is_reported_as_unknown() {
    // Guards the test above: if `call_builtin` ever stopped saying "unknown builtin",
    // `every_declared_builtin_is_dispatchable` would silently pass for everything.
    let err = call_builtin(
        "definitely_not_a_builtin",
        vec![],
        &AtomInterner::new(),
        Limits::unlimited(),
    )
    .expect_err("should not resolve");
    assert!(is_unknown(&err), "unexpected error text: {err}");
}

#[test]
fn declared_builtins_all_lex_as_a_single_identifier() {
    // A multi-token name can never be looked up: `number?` was listed for a while and
    // was unreachable, because the lexer splits the `?` off.
    for name in BUILTIN_NAMES {
        assert!(
            name.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "`{name}` cannot lex as one identifier"
        );
    }
}
