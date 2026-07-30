//! What the module system guarantees, held to by example.
//!
//! Each of these failed before: a module could not use another module (the graph
//! was one level deep), a plain `use` gave no way to qualify, a typo in a
//! qualified call passed `rite check` and failed at runtime with the mangled name
//! `m__squre` in the message, and two modules exporting one name reported a
//! duplicate at the call site without naming either module.

use std::fs;
use std::path::Path;

fn project(dir: &Path, files: &[(&str, &str)]) {
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
}

fn compile(dir: &Path, entry: &str) -> rite_core::Diagnostics {
    let path = dir.join(entry);
    let (_ir, diags, _sources) = rite_sem::compile_path(&path);
    diags
}

#[test]
fn a_module_can_use_another_module() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("leaf.rite", "pub ◆ val() ⟦ ^ 7 ⟧\n"),
            ("mid.rite", "use leaf\npub ◆ doubled() ⟦ ^ val() * 2 ⟧\n"),
            ("main.rite", "use mid\ndoubled()\n"),
        ],
    );
    let diags = compile(tmp.path(), "main.rite");
    assert!(
        !diags.has_errors(),
        "a module's own imports must be in scope: {:?}",
        diags.into_vec()
    );
}

#[test]
fn a_plain_use_can_be_qualified() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("math.rite", "pub ◆ square(v) ⟦ ^ v * v ⟧\n"),
            ("main.rite", "use math\nmath.square(9)\n"),
        ],
    );
    let diags = compile(tmp.path(), "main.rite");
    assert!(
        !diags.has_errors(),
        "`use math` should bind `math` as a qualifier: {:?}",
        diags.into_vec()
    );
}

#[test]
fn a_typo_in_a_qualified_call_is_caught_before_running() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("math.rite", "pub ◆ square(v) ⟦ ^ v * v ⟧\n"),
            ("main.rite", "use math as m\nm.squre(9)\n"),
        ],
    );
    let diags = compile(tmp.path(), "main.rite");
    let text = format!("{:?}", diags.into_vec());
    assert!(
        text.contains("squre"),
        "the typo should be reported: {text}"
    );
    assert!(
        !text.contains("m__"),
        "the mangled name must not reach the reader: {text}"
    );
}

#[test]
fn reaching_a_private_name_through_a_qualifier_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("lib.rite", "pub ◆ shown() ⟦ ^ 1 ⟧\n◆ hidden() ⟦ ^ 2 ⟧\n"),
            ("main.rite", "use lib\nlib.hidden()\n"),
        ],
    );
    let diags = compile(tmp.path(), "main.rite");
    assert!(diags.has_errors(), "private names stay private");
}

#[test]
fn colliding_exports_name_both_modules() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("alpha.rite", "pub ◆ helper() ⟦ ^ 1 ⟧\n"),
            ("beta.rite", "pub ◆ helper() ⟦ ^ 2 ⟧\n"),
            ("main.rite", "use alpha\nuse beta\nalpha.helper()\n"),
        ],
    );
    let text = format!("{:?}", compile(tmp.path(), "main.rite").into_vec());
    assert!(
        text.contains("alpha") && text.contains("beta"),
        "the clash should name both modules: {text}"
    );
}

/// A parameter may shadow a module name, and then it is an ordinary value.
#[test]
fn a_local_binding_shadows_a_module_qualifier() {
    let tmp = tempfile::tempdir().unwrap();
    project(
        tmp.path(),
        &[
            ("math.rite", "pub ◆ square(v) ⟦ ^ v * v ⟧\n"),
            ("main.rite", "use math\n◆ f(math) ⟦ ^ math.x ⟧\nf(⟨x: 5⟩)\n"),
        ],
    );
    let diags = compile(tmp.path(), "main.rite");
    assert!(
        !diags.has_errors(),
        "`math.x` on a parameter is a field read: {:?}",
        diags.into_vec()
    );
}
