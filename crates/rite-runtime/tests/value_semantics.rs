//! `Value` semantics — the rules the book states as guarantees.
//!
//! `value.rs` had sixteen public methods and no tests, including `is_truthy`, which the
//! docs commit to explicitly: *only* `false` and `none` are falsey, so an empty list or
//! `0` passes an `if`. That is the rule most likely to be "helpfully" changed to match
//! JavaScript by someone who does not know it was deliberate.

use rite_runtime::{AtomInterner, Key, Value};

/// docs/book/values.md: only `false` and `none` are falsey.
#[test]
fn only_false_and_none_are_falsey() {
    let atoms = AtomInterner::new();
    let falsey = [Value::None, Value::Bool(false)];
    let truthy = [
        Value::Bool(true),
        Value::Int(0),
        Value::Int(-1),
        Value::Float(0.0),
        Value::string(""),
        Value::string("x"),
        Value::list(Vec::<Value>::new()),
        Value::list(vec![Value::Int(1)]),
        Value::record(Vec::<(Key, Value)>::new()),
        Value::Atom(atoms.intern("ok")),
        Value::ok(Value::None),
        Value::err(Value::None),
        Value::Bytes(Vec::<u8>::new().into()),
    ];
    for v in falsey {
        assert!(!v.is_truthy(), "{} should be falsey", v.type_name());
    }
    for v in truthy {
        assert!(
            v.is_truthy(),
            "{} ({:?}) should be truthy — empty collections and zero are not falsey",
            v.type_name(),
            v
        );
    }
}

#[test]
fn type_name_covers_every_variant() {
    let atoms = AtomInterner::new();
    let cases = [
        (Value::None, "none"),
        (Value::Bool(true), "bool"),
        (Value::Int(1), "int"),
        (Value::Float(1.0), "float"),
        (Value::string("s"), "string"),
        (Value::Atom(atoms.intern("a")), "atom"),
        (Value::list(vec![]), "list"),
        (Value::record(vec![]), "record"),
        (Value::ok(Value::None), "result"),
        (Value::Bytes(vec![1].into()), "bytes"),
    ];
    for (v, want) in cases {
        assert_eq!(v.type_name(), want);
    }
}

#[test]
fn structural_equality_compares_contents_not_identity() {
    assert!(Value::list(vec![Value::Int(1), Value::string("a")])
        .structural_eq(&Value::list(vec![Value::Int(1), Value::string("a")])));
    assert!(!Value::list(vec![Value::Int(1)]).structural_eq(&Value::list(vec![Value::Int(2)])));
    // Nested.
    let nest = |n| {
        Value::record(vec![(
            Key::String("k".into()),
            Value::list(vec![Value::Int(n)]),
        )])
    };
    assert!(nest(1).structural_eq(&nest(1)));
    assert!(!nest(1).structural_eq(&nest(2)));
    // Different types are never equal.
    assert!(!Value::Int(1).structural_eq(&Value::string("1")));
    assert!(!Value::None.structural_eq(&Value::Bool(false)));
    // Results compare by branch and payload.
    assert!(Value::ok(Value::Int(1)).structural_eq(&Value::ok(Value::Int(1))));
    assert!(!Value::ok(Value::Int(1)).structural_eq(&Value::err(Value::Int(1))));
}

/// docs/book/values.md: "Dot access on a missing key yields `none`, not an error."
#[test]
fn field_access_is_forgiving_and_accepts_either_key_kind() {
    let rec = Value::record(vec![
        (Key::String("name".into()), Value::string("aura")),
        (Key::Atom("kind".into()), Value::Int(7)),
    ]);
    assert_eq!(rec.get_field("name"), Value::string("aura"));
    // An atom key is reachable by the same dotted name.
    assert_eq!(rec.get_field("kind"), Value::Int(7));
    assert_eq!(rec.get_field("missing"), Value::None);
    // Non-records simply have no fields, rather than erroring.
    assert_eq!(Value::Int(1).get_field("x"), Value::None);
    assert_eq!(Value::None.get_field("x"), Value::None);
}

#[test]
fn numeric_and_string_accessors_do_not_coerce() {
    assert_eq!(Value::Int(3).as_int(), Some(3));
    assert_eq!(Value::Float(2.5).as_float(), Some(2.5));
    assert_eq!(Value::string("hi").as_str(), Some("hi"));
    // A string that looks like a number is still a string.
    assert_eq!(Value::string("3").as_int(), None);
    assert_eq!(Value::Bool(true).as_int(), None);
    assert_eq!(Value::Int(1).as_str(), None);
}

#[test]
fn json_round_trips_the_shapes_json_can_hold() {
    let atoms = AtomInterner::new();
    let original = Value::record(vec![
        (Key::String("n".into()), Value::Int(1)),
        (Key::String("f".into()), Value::Float(1.5)),
        (Key::String("s".into()), Value::string("x")),
        (Key::String("b".into()), Value::Bool(true)),
        (Key::String("nil".into()), Value::None),
        (
            Key::String("xs".into()),
            Value::list(vec![Value::Int(1), Value::Int(2)]),
        ),
    ]);
    let json = original.to_json(&atoms);
    let back = Value::from_json(&json);
    assert!(
        back.structural_eq(&original),
        "round trip changed the value:\n{original:?}\n{back:?}"
    );
}

#[test]
fn atoms_become_strings_in_json() {
    let atoms = AtomInterner::new();
    let v = Value::record(vec![(
        Key::String("status".into()),
        Value::Atom(atoms.intern("ok")),
    )]);
    assert_eq!(v.to_json(&atoms)["status"], serde_json::json!("ok"));
}

#[test]
fn display_renders_structures_readably() {
    let atoms = AtomInterner::new();
    assert_eq!(Value::Int(3).to_display(&atoms), "3");
    assert_eq!(Value::string("hi").to_display(&atoms), "hi");
    assert_eq!(Value::None.to_display(&atoms), "none");
    assert_eq!(Value::Bool(false).to_display(&atoms), "false");
    let atom = Value::Atom(atoms.intern("ok"));
    assert!(atom.to_display(&atoms).contains("ok"));
    let list = Value::list(vec![Value::Int(1), Value::Int(2)]).to_display(&atoms);
    assert!(
        list.starts_with('[') && list.contains('1') && list.contains('2'),
        "{list}"
    );
}

#[test]
fn interning_the_same_name_yields_the_same_atom() {
    let atoms = AtomInterner::new();
    let a = atoms.intern("ok");
    let b = atoms.intern("ok");
    let c = atoms.intern("err");
    assert_eq!(a, b, "the same name must intern to the same id");
    assert_ne!(a, c);
    assert_eq!(atoms.name(a), "ok");
    assert_eq!(atoms.name(c), "err");
    // Atom equality is by id, and structural equality follows it.
    assert!(Value::Atom(a).structural_eq(&Value::Atom(b)));
    assert!(!Value::Atom(a).structural_eq(&Value::Atom(c)));
}
