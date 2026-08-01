//! Atoms must render as their names everywhere a user can see them.
//!
//! `Display for Value` has no interner, so it can only print an atom as its interner
//! index. Every builtin that reached for `format!("{}", v)` therefore rendered `#ok` as
//! `#0` — `str`, `join` and `panic` all did, and because string interpolation desugars
//! to `str(...)`, so did `"{status}"`. `@console.println` was correct the whole time,
//! which is what made it look like a display quirk rather than a bug: the same atom
//! printed two different ways depending on how it got to the screen.

use rite_runtime::builtins::call_builtin;
use rite_runtime::{AtomInterner, Limits, Value};

fn atoms_with(names: &[&str]) -> (AtomInterner, Vec<Value>) {
    let interner = AtomInterner::new();
    let values = names
        .iter()
        .map(|n| Value::Atom(interner.intern(n)))
        .collect();
    (interner, values)
}

fn str_of(value: Value, atoms: &AtomInterner) -> String {
    let out = call_builtin("str", vec![value], atoms, Limits::unlimited()).expect("str");
    out.as_str().expect("str returns a string").to_string()
}

#[test]
fn str_of_an_atom_is_its_name() {
    let (atoms, vals) = atoms_with(&["ok"]);
    assert_eq!(str_of(vals[0].clone(), &atoms), "#ok");
}

#[test]
fn the_second_atom_is_not_the_first() {
    // The failure this pins rendered by index, so *every* atom read as `#0` for the
    // first one interned. Two atoms make that unmistakable.
    let (atoms, vals) = atoms_with(&["first", "second"]);
    assert_eq!(str_of(vals[0].clone(), &atoms), "#first");
    assert_eq!(str_of(vals[1].clone(), &atoms), "#second");
}

#[test]
fn atoms_nested_in_collections_render_by_name() {
    let (atoms, vals) = atoms_with(&["a", "b"]);
    assert_eq!(str_of(Value::List(vals.clone().into()), &atoms), "[#a, #b]");

    let mut rec = indexmap::IndexMap::new();
    rec.insert(rite_runtime::Key::String("status".into()), vals[0].clone());
    assert_eq!(str_of(Value::Record(rec), &atoms), "⟨status: #a⟩");
}

#[test]
fn join_renders_atom_elements_by_name() {
    let (atoms, vals) = atoms_with(&["a", "b"]);
    let joined = call_builtin(
        "join",
        vec![Value::List(vals.into()), Value::string(", ")],
        &atoms,
        Limits::unlimited(),
    )
    .expect("join");
    assert_eq!(joined.as_str(), Some("#a, #b"));
}

#[test]
fn panic_reports_the_atom_it_was_given() {
    let (atoms, vals) = atoms_with(&["boom"]);
    let err = call_builtin("panic", vec![vals[0].clone()], &atoms, Limits::unlimited())
        .expect_err("panics");
    assert!(
        err.to_string().contains("#boom"),
        "a panic reason must name the atom, got: {err}"
    );
}

#[test]
fn str_agrees_with_to_display() {
    // The two must not drift apart again: `println` used `to_display` and was right,
    // `str` used `Display` and was wrong, and nothing compared them.
    let (atoms, vals) = atoms_with(&["ok", "err"]);
    for v in [
        vals[0].clone(),
        Value::List(vals.clone().into()),
        Value::Int(7),
        Value::string("plain"),
        Value::Bool(true),
        Value::None,
    ] {
        assert_eq!(
            str_of(v.clone(), &atoms),
            v.to_display(&atoms),
            "str and to_display disagree on {v:?}"
        );
    }
}

#[test]
fn non_atom_values_are_unchanged() {
    let atoms = AtomInterner::new();
    assert_eq!(str_of(Value::Int(99), &atoms), "99");
    assert_eq!(str_of(Value::Float(1.5), &atoms), "1.5");
    assert_eq!(str_of(Value::string("hi"), &atoms), "hi");
    assert_eq!(str_of(Value::Bool(false), &atoms), "false");
    assert_eq!(str_of(Value::None, &atoms), "none");
}
