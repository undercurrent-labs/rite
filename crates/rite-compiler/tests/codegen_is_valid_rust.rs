//! The generated Rust must actually parse as Rust.
//!
//! This was the hole. `lowering.rs` asserts on emitted *text* — `code.contains("Box::pin")`
//! passes perfectly happily on source that does not compile — and every test that really
//! builds a generated crate is `#[ignore]`d because it takes minutes. So a code generator
//! landed with nothing in CI ever compiling its output: a stray brace or a bad literal
//! would break `rite build` entirely and leave the suite green.
//!
//! Parsing with `syn` closes the common half of that gap in milliseconds. It catches
//! unbalanced delimiters, malformed literals and bad tokens — the mistakes a
//! string-concatenating backend actually makes. It does *not* type-check, so the
//! end-to-end builds behind `--ignored` still matter; those belong in the release
//! workflow, where minutes are affordable.

use rite_compiler::generate_from_ir;
use std::path::Path;

/// Generate the whole `generated.rs` for a program, as `rite build` would.
fn generated(source: &str) -> String {
    let file = rite_core::SourceFile::new(rite_core::FileId(0), "gen.rite", source);
    let (ir, diags) = rite_sem::compile_to_ir(&file);
    assert!(!diags.has_errors(), "{source:?} failed to compile to IR");
    generate_from_ir(&ir.expect("ir"), Path::new("gen.rite")).expect("codegen")
}

/// Parse as Rust, reporting the offending source when it fails.
fn assert_parses(source: &str, label: &str) {
    let code = generated(source);
    if let Err(e) = syn::parse_file(&code) {
        panic!("generated Rust does not parse for {label}: {e}\n--- source ---\n{code}");
    }
}

#[test]
fn every_lowered_construct_generates_parseable_rust() {
    for (label, src) in [
        ("arithmetic", "1 + 2 * 3 - 4 / 2 % 3"),
        ("comparisons", "1 < 2 = true"),
        ("bindings", "x ← 1\ny ↢ 2\nx + y"),
        // `]]` closes an ASCII block, so a nested list needs the space — the same rule
        // the book gives for `[[`.
        ("list", "[1, [2, 3], [] ]"),
        ("record", "⟨a: 1, b: ⟨c: [#x]⟩⟩"),
        ("member", "⟨a: 1⟩.a"),
        ("index", "[1, 2][0]"),
        ("unary", "-1\nnot true"),
        ("short circuit", "true and false\nfalse or true"),
        ("coalesce", "none ?? 1"),
        ("try", "ok(1)?"),
        ("if", "? true ⟦ 1 ⟧ : ⟦ 2 ⟧"),
        ("if without else", "? true ⟦ 1 ⟧"),
        ("membership", "1 ∈ [1]\n2 ∉ [1]"),
        ("atom", "#ok"),
        ("function", "◆ f(a, b) ⟦ ^ a + b ⟧\nf(1, 2)"),
        (
            "recursion",
            "◆ d(n) ⟦ ^ ? n <= 0 ⟦ 0 ⟧ : ⟦ d(n - 1) ⟧ ⟧\nd(3)",
        ),
        (
            "mutual calls",
            "◆ a(n) ⟦ ^ n ⟧\n◆ b(n) ⟦ ^ a(n) + 1 ⟧\nb(1)",
        ),
        ("zero-arg function", "◆ f() ⟦ ^ 1 ⟧\nf()"),
        ("nested blocks", "? true ⟦ ? false ⟦ 1 ⟧ : ⟦ 2 ⟧ ⟧ : ⟦ 3 ⟧"),
        ("builtin as value", "str(1)"),
        ("capability", "! @clock.now()"),
        ("empty program", ""),
    ] {
        assert_parses(src, label);
    }
}

#[test]
fn fallbacks_generate_parseable_rust_too() {
    // The interpreted path is emitted source as much as the lowered one, and a broken
    // fallback breaks every program that uses the construct.
    for (label, src) in [
        ("pipeline", "[1, 2] → sum"),
        ("closure", "f ← { |x| x * 2 }\nf(1)"),
        ("match", "~ 1 ⟦\n  1 → #one\n  _ → #other\n⟧"),
        ("console", "! @console.println(\"hi\")"),
        (
            "mixed",
            "◆ f(xs) ⟦ ^ xs → sum ⟧\n! @console.println(str(f([1, 2])))",
        ),
    ] {
        assert_parses(src, label);
    }
}

/// String and float literals are where a string-concatenating backend goes wrong.
#[test]
fn hostile_literals_survive_the_round_trip_into_rust() {
    for (label, src) in [
        ("quotes", r#""he said \"hi\"""#),
        ("backslash", r#"r"C:\Users\x""#),
        ("newline and tab", r#""a\nb\tc""#),
        ("nul", r#""a\0b""#),
        ("braces", r#""{{ literal }}""#),
        ("unicode", r#""日本語 😀 café""#),
        ("empty string", r#""""#),
        ("float", "1.0 + 0.5"),
        ("negative float", "-1.5"),
        ("large int", "9223372036854775807"),
        ("min int", "-9223372036854775807"),
        ("hex and binary", "0xff + 0b1010"),
        ("atom with underscore", "#some_tag"),
    ] {
        assert_parses(src, label);
    }
}

#[test]
fn a_unicode_function_name_generates_a_valid_rust_identifier() {
    // Rite identifiers accept any non-ASCII byte; Rust identifiers do not, so the mangled
    // name has to be legal or the whole file fails to parse.
    assert_parses("◆ café(x) ⟦ ^ x ⟧\ncafé(1)", "unicode fn name");
    assert_parses("◆ ünïcödé() ⟦ ^ 1 ⟧\nünïcödé()", "more unicode");
}

#[test]
fn a_record_key_needing_escapes_generates_parseable_rust() {
    assert_parses(r#"⟨"key with spaces": 1⟩"#, "quoted record key");
    assert_parses(r#"⟨"quote\"key": 1⟩"#, "record key with a quote");
}

#[test]
fn the_generated_file_declares_what_it_lowered() {
    // The build note is the honest part of the backend: without it, whether a program was
    // compiled or interpreted is invisible until someone benchmarks it.
    // `g` uses `~`, which still falls back; pipelines no longer do.
    let code = generated("◆ f(n) ⟦ ^ n ⟧\n◆ g(v) ⟦ ^ ~ v ⟦\n  1 → #one\n  _ → #other\n⟧ ⟧\nf(1)");
    assert!(
        code.contains("// backend:"),
        "no backend summary in the generated file"
    );
    assert!(
        code.contains("interpreted function `g`: Match"),
        "a fallback function must be named with its reason:\n{code}"
    );
}

#[test]
fn a_deeply_nested_program_does_not_emit_unbalanced_delimiters() {
    // Every lowering rule wraps its output in braces, so nesting is where an unbalanced
    // emission would first show up.
    let src = "? true ⟦ ? true ⟦ ? true ⟦ ⟨a: [1, [2, ⟨b: 3⟩] ]⟩ ⟧ : ⟦ 1 ⟧ ⟧ : ⟦ 2 ⟧ ⟧ : ⟦ 3 ⟧";
    assert_parses(src, "deep nesting");
}
