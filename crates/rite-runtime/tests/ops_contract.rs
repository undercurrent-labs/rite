//! The operator table as a public contract.
//!
//! `rite_runtime::ops` exists so `rite build` can emit real Rust that reaches the same
//! definition of `+` the interpreter uses, instead of carrying a second copy. That makes
//! these signatures an API rather than an internal detail, and the semantics worth
//! pinning here directly rather than only through the evaluator.
//!
//! Existing suites cover overflow and edge cases through the language surface
//! (`arithmetic_edges.rs`, `membership_ops.rs`); this covers the boundary itself.

use rite_runtime::ops;
use rite_runtime::{AtomInterner, EvalError, Key, Value};
use rite_sem::{BinaryOpIr, UnaryOpIr};

fn atoms() -> AtomInterner {
    AtomInterner::new()
}

fn bin(op: BinaryOpIr, l: Value, r: Value) -> Result<Value, EvalError> {
    ops::binary(&atoms(), op, l, r)
}

fn int(v: Result<Value, EvalError>) -> i64 {
    v.expect("ok").as_int().expect("int")
}

// --------------------------------------------------------------------------- arithmetic

#[test]
fn integer_arithmetic_promotes_and_checks() {
    assert_eq!(int(bin(BinaryOpIr::Add, Value::Int(2), Value::Int(3))), 5);
    assert_eq!(int(bin(BinaryOpIr::Sub, Value::Int(2), Value::Int(3))), -1);
    assert_eq!(int(bin(BinaryOpIr::Mul, Value::Int(6), Value::Int(7))), 42);
    assert_eq!(int(bin(BinaryOpIr::Div, Value::Int(7), Value::Int(2))), 3);
    assert_eq!(int(bin(BinaryOpIr::Rem, Value::Int(7), Value::Int(3))), 1);
}

#[test]
fn mixing_an_int_and_a_float_yields_a_float() {
    for (l, r) in [
        (Value::Int(1), Value::Float(0.5)),
        (Value::Float(0.5), Value::Int(1)),
    ] {
        let got = bin(BinaryOpIr::Add, l, r).expect("ok");
        assert!(
            matches!(got, Value::Float(f) if (f - 1.5).abs() < f64::EPSILON),
            "expected 1.5 as a float, got {got:?}"
        );
    }
}

#[test]
fn overflow_is_an_error_not_a_wrap_or_a_panic() {
    // Generated Rust would abort the process on a debug overflow, so these must stay
    // checked rather than relying on the profile.
    for op in [BinaryOpIr::Add, BinaryOpIr::Mul] {
        let err = bin(op, Value::Int(i64::MAX), Value::Int(2)).expect_err("must not wrap");
        assert!(err.to_string().contains("overflow"), "{err}");
    }
    let err = bin(BinaryOpIr::Sub, Value::Int(i64::MIN), Value::Int(1)).expect_err("must not wrap");
    assert!(err.to_string().contains("overflow"), "{err}");
    // `i64::MIN / -1` and `i64::MIN % -1` overflow where Rust would panic.
    for op in [BinaryOpIr::Div, BinaryOpIr::Rem] {
        let err = bin(op, Value::Int(i64::MIN), Value::Int(-1)).expect_err("must not panic");
        assert!(err.to_string().contains("overflow"), "{err}");
    }
}

#[test]
fn integer_division_by_zero_is_an_error() {
    for op in [BinaryOpIr::Div, BinaryOpIr::Rem] {
        let err = bin(op, Value::Int(1), Value::Int(0)).expect_err("must not panic");
        assert!(err.to_string().contains("division by zero"), "{err}");
    }
}

#[test]
fn adding_mismatched_types_names_both_of_them() {
    let err = bin(BinaryOpIr::Add, Value::string("a"), Value::Int(1)).expect_err("type error");
    let msg = err.to_string();
    assert!(msg.contains("string") && msg.contains("int"), "{msg}");
}

// -------------------------------------------------------------------------- collections

#[test]
fn add_concatenates_strings_and_lists_and_merges_records() {
    let cat = bin(BinaryOpIr::Add, Value::string("ab"), Value::string("c")).expect("ok");
    assert_eq!(cat.as_str(), Some("abc"));

    let joined = bin(
        BinaryOpIr::Add,
        Value::list(vec![Value::Int(1)]),
        Value::list(vec![Value::Int(2)]),
    )
    .expect("ok");
    assert!(joined.structural_eq(&Value::list(vec![Value::Int(1), Value::Int(2)])));

    // A non-list right operand appends rather than failing.
    let pushed = bin(
        BinaryOpIr::Add,
        Value::list(vec![Value::Int(1)]),
        Value::Int(9),
    )
    .expect("ok");
    assert!(pushed.structural_eq(&Value::list(vec![Value::Int(1), Value::Int(9)])));
}

#[test]
fn record_merge_is_right_biased() {
    let mut a = indexmap::IndexMap::new();
    a.insert(Key::String("k".into()), Value::Int(1));
    a.insert(Key::String("keep".into()), Value::Int(7));
    let mut b = indexmap::IndexMap::new();
    b.insert(Key::String("k".into()), Value::Int(2));

    let merged = bin(BinaryOpIr::Add, Value::Record(a), Value::Record(b)).expect("ok");
    assert_eq!(merged.get_field("k").as_int(), Some(2));
    assert_eq!(
        merged.get_field("keep").as_int(),
        Some(7),
        "an unshadowed key survives"
    );
}

#[test]
fn subtracting_an_atom_from_a_record_removes_that_key() {
    // The one place `binary` needs the interner: the key is stored by name.
    let interner = atoms();
    let mut rec = indexmap::IndexMap::new();
    rec.insert(Key::String("gone".into()), Value::Int(1));
    rec.insert(Key::String("stays".into()), Value::Int(2));

    let out = ops::binary(
        &interner,
        BinaryOpIr::Sub,
        Value::Record(rec),
        Value::Atom(interner.intern("gone")),
    )
    .expect("ok");
    assert!(
        matches!(out.get_field("gone"), Value::None),
        "key was not removed"
    );
    assert_eq!(out.get_field("stays").as_int(), Some(2));
}

#[test]
fn subtracting_from_a_list_removes_the_first_match_only() {
    let list = Value::list(vec![Value::Int(1), Value::Int(2), Value::Int(1)]);
    let out = bin(BinaryOpIr::Sub, list, Value::Int(1)).expect("ok");
    assert!(out.structural_eq(&Value::list(vec![Value::Int(2), Value::Int(1)])));
}

// ------------------------------------------------------------------------- comparisons

#[test]
fn equality_is_structural() {
    let a = Value::list(vec![Value::Int(1), Value::string("x")]);
    let b = Value::list(vec![Value::Int(1), Value::string("x")]);
    assert_eq!(
        bin(BinaryOpIr::Eq, a.clone(), b).expect("ok"),
        Value::Bool(true)
    );
    assert_eq!(
        bin(BinaryOpIr::NotEq, a, Value::Int(1)).expect("ok"),
        Value::Bool(true)
    );
}

#[test]
fn ordering_operators_agree_with_each_other() {
    let cases = [(1, 2), (2, 2), (3, 2)];
    for (a, b) in cases {
        let lt = bin(BinaryOpIr::Lt, Value::Int(a), Value::Int(b)).expect("ok");
        let gt = bin(BinaryOpIr::Gt, Value::Int(a), Value::Int(b)).expect("ok");
        let lte = bin(BinaryOpIr::LtEq, Value::Int(a), Value::Int(b)).expect("ok");
        let gte = bin(BinaryOpIr::GtEq, Value::Int(a), Value::Int(b)).expect("ok");
        assert_eq!(lt, Value::Bool(a < b), "{a} < {b}");
        assert_eq!(gt, Value::Bool(a > b), "{a} > {b}");
        assert_eq!(lte, Value::Bool(a <= b));
        assert_eq!(gte, Value::Bool(a >= b));
        // `<` and `>=` must be exact complements, or a compiled comparison could
        // disagree with the interpreted one at a boundary.
        assert_ne!(lt, gte, "`<` and `>=` must be complements at {a},{b}");
    }
}

// ------------------------------------------------------------------------- short-circuit

#[test]
fn and_or_refuse_pre_evaluated_operands_instead_of_panicking() {
    // These short-circuit, so they cannot be applied to two already-evaluated values.
    // As a private method this was `unreachable!()`; now that generated code can call it,
    // a panic would abort the process rather than report a fault.
    for op in [BinaryOpIr::And, BinaryOpIr::Or] {
        let err = bin(op, Value::Bool(true), Value::Bool(false))
            .expect_err("must be an error, not a panic");
        assert!(err.to_string().contains("short-circuit"), "{err}");
    }
}

// -------------------------------------------------------------------------------- unary

#[test]
fn negation_is_checked_and_typed() {
    assert_eq!(int(ops::unary(UnaryOpIr::Neg, Value::Int(5))), -5);
    assert!(
        ops::unary(UnaryOpIr::Neg, Value::Int(i64::MIN)).is_err(),
        "must not wrap"
    );
    assert!(ops::unary(UnaryOpIr::Neg, Value::string("x")).is_err());
    match ops::unary(UnaryOpIr::Neg, Value::Float(1.5)).expect("ok") {
        Value::Float(f) => assert!((f + 1.5).abs() < f64::EPSILON),
        other => panic!("expected a float, got {other:?}"),
    }
}

#[test]
fn not_follows_rite_truthiness() {
    // Only `false` and `none` are falsey — zero and the empty collections are not.
    for (v, expected) in [
        (Value::Bool(false), true),
        (Value::None, true),
        (Value::Bool(true), false),
        (Value::Int(0), false),
        (Value::string(""), false),
        (Value::list(Vec::<Value>::new()), false),
    ] {
        assert_eq!(
            ops::unary(UnaryOpIr::Not, v.clone()).expect("ok"),
            Value::Bool(expected),
            "not {v:?}"
        );
    }
}

#[test]
fn the_effect_marker_does_not_transform_its_operand() {
    let v = Value::string("unchanged");
    assert_eq!(
        ops::unary(UnaryOpIr::Effect, v.clone())
            .expect("ok")
            .as_str(),
        v.as_str()
    );
}

// ------------------------------------------------------------------------------ indexing

#[test]
fn indexing_out_of_range_or_by_a_bad_type_is_none() {
    let list = Value::list(vec![Value::Int(10), Value::Int(20)]);
    assert_eq!(ops::index(&list, &Value::Int(1)).as_int(), Some(20));
    for idx in [Value::Int(2), Value::Int(-1), Value::string("x")] {
        assert!(
            matches!(ops::index(&list, &idx), Value::None),
            "index {idx:?} should be none"
        );
    }
    // Not indexable at all.
    assert!(matches!(
        ops::index(&Value::Int(1), &Value::Int(0)),
        Value::None
    ));
}

#[test]
fn a_record_indexes_by_string_key() {
    let mut rec = indexmap::IndexMap::new();
    rec.insert(Key::String("k".into()), Value::Int(3));
    let rec = Value::Record(rec);
    assert_eq!(ops::index(&rec, &Value::string("k")).as_int(), Some(3));
    assert!(matches!(
        ops::index(&rec, &Value::string("absent")),
        Value::None
    ));
}

// ----------------------------------------------------------------------------------- try

#[test]
fn try_unwraps_ok_and_early_returns_err() {
    assert_eq!(
        ops::unwrap_try(Value::ok(Value::Int(7)))
            .expect("ok")
            .as_int(),
        Some(7)
    );
    // `err` becomes a Return, not a failure: a caller that is not a function boundary
    // has to propagate it, and one that is converts it to the function's value.
    match ops::unwrap_try(Value::err(Value::string("boom"))) {
        Err(EvalError::Return(v)) => assert!(
            matches!(v, Value::Result(_)),
            "the early return carries the err, got {v:?}"
        ),
        other => panic!("expected EvalError::Return, got {other:?}"),
    }
}

/// `?` requires a result.
///
/// It used to pass a non-result through, so `42?` was `42` and the operator that
/// says "this can fail" could be written over something that cannot — most
/// usefully over a call that had stopped answering a result, where the `?` then
/// quietly did nothing. This test asserted that pass-through; it asserts the
/// contract that replaced it.
///
/// `ok(none)` still unwraps to `none`, which is how `@fs.read_line` reports the
/// end of a file — the result is what matters, not what is inside it.
#[test]
fn try_requires_a_result() {
    assert!(ops::unwrap_try(Value::Int(1)).is_err());
    assert!(ops::unwrap_try(Value::None).is_err());
    assert!(matches!(
        ops::unwrap_try(Value::ok(Value::None)).expect("ok"),
        Value::None
    ));
}

// ----------------------------------------------------------------------------- membership

#[test]
fn membership_matches_atoms_by_name() {
    let interner = atoms();
    let a = Value::Atom(interner.intern("a"));

    assert!(ops::contains(&interner, &a, &Value::list(vec![Value::string("a")])).unwrap());
    assert!(ops::contains(&interner, &a, &Value::list(vec![a.clone()])).unwrap());

    let mut rec = indexmap::IndexMap::new();
    rec.insert(Key::String("a".into()), Value::Int(1));
    assert!(ops::contains(&interner, &a, &Value::Record(rec)).unwrap());

    assert!(!ops::contains(&interner, &a, &Value::list(vec![Value::string("b")])).unwrap());

    // A non-container has no answer, rather than `false`.
    assert!(ops::contains(&interner, &a, &Value::Int(42)).is_err());
}

#[test]
fn in_and_not_in_are_exact_complements() {
    let interner = atoms();
    let list = Value::list(vec![Value::Int(1), Value::Int(2)]);
    for probe in [Value::Int(1), Value::Int(9), Value::string("1")] {
        let is_in =
            ops::binary(&interner, BinaryOpIr::In, probe.clone(), list.clone()).expect("ok");
        let not_in =
            ops::binary(&interner, BinaryOpIr::NotIn, probe.clone(), list.clone()).expect("ok");
        assert_ne!(is_in, not_in, "∈ and ∉ disagree for {probe:?}");
    }
}
