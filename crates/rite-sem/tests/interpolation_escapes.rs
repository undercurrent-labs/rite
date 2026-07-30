//! `\{` must escape string interpolation instead of interpolating anyway.
//!
//! The lexer encodes an escaped brace as a doubled brace in the token text and
//! `desugar_interpolation` decodes it — these tests pin the decoded IR, which is
//! what the runtime evaluates.

use rite_core::{FileId, SourceFile};
use rite_sem::{compile_to_ir, ExprIr, ProgramIr, ValueLiteral};

fn ir_of(src: &str) -> ProgramIr {
    let f = SourceFile::new(FileId(0), "t.rite", src);
    let (ir, d) = compile_to_ir(&f);
    assert!(
        !d.has_errors(),
        "unexpected diagnostics for {src:?}: {:?}",
        d.errors().map(|e| e.code.as_str()).collect::<Vec<_>>()
    );
    ir.expect("no IR produced")
}

/// The IR of the single statement `x <- <expr>`, rendered with `Debug`.
fn bound_expr_debug(expr_src: &str) -> String {
    let ir = ir_of(&format!("x <- {expr_src}\n"));
    let stmt = ir.modules[0]
        .statements
        .iter()
        .find_map(|s| match s {
            ExprIr::Bind { value, .. } => Some(value),
            _ => None,
        })
        .expect("no binding in IR");
    format!("{stmt:?}")
}

/// A literal with no interpolation lowers to exactly one string constant.
fn constant_string(expr_src: &str) -> String {
    let ir = ir_of(&format!("x <- {expr_src}\n"));
    let stmt = ir.modules[0]
        .statements
        .iter()
        .find_map(|s| match s {
            ExprIr::Bind { value, .. } => Some(value.as_ref()),
            _ => None,
        })
        .expect("no binding in IR");
    match stmt {
        ExprIr::Constant(ValueLiteral::String(s, _)) => s.clone(),
        other => panic!("expected a single string constant for {expr_src}, got {other:?}"),
    }
}

#[test]
fn escaped_brace_does_not_interpolate() {
    assert_eq!(
        constant_string(r#""literal braces: \{name}""#),
        "literal braces: {name}"
    );
    assert_eq!(constant_string(r#""\{name}""#), "{name}");
}

#[test]
fn escaped_closing_brace_is_literal() {
    assert_eq!(constant_string(r#""\}""#), "}");
    assert_eq!(constant_string(r#""\{\}""#), "{}");
    assert_eq!(constant_string(r#""a \{ b \} c""#), "a { b } c");
}

#[test]
fn doubled_braces_written_directly_are_literal() {
    assert_eq!(constant_string(r#""{{x}}""#), "{x}");
    assert_eq!(constant_string(r#""{{}}""#), "{}");
}

#[test]
fn plain_interpolation_still_works() {
    // Concatenation of the literal prefix with str(name).
    let dbg = bound_expr_debug(r#""hi {name}""#);
    assert!(dbg.contains("Binary"), "expected concatenation, got {dbg}");
    assert!(dbg.contains("\"hi \""), "missing literal prefix in {dbg}");
    assert!(
        dbg.contains("Global(\"name\")"),
        "interpolation hole was not lowered in {dbg}"
    );
}

#[test]
fn interpolation_and_escape_can_be_mixed() {
    let dbg = bound_expr_debug(r#""{name} but \{name} stays""#);
    assert!(
        dbg.contains("Global(\"name\")"),
        "the unescaped hole must interpolate: {dbg}"
    );
    assert!(
        dbg.contains("\" but {name} stays\""),
        "the escaped braces must stay literal: {dbg}"
    );
}

#[test]
fn member_interpolation_still_works() {
    let dbg = bound_expr_debug(r#""{user.name}""#);
    assert!(dbg.contains("Member"), "expected field access in {dbg}");
    assert!(dbg.contains("Global(\"user\")"), "missing base in {dbg}");
}

#[test]
fn unmatched_brace_is_literal() {
    assert_eq!(constant_string(r#""a { b""#), "a { b");
    assert_eq!(constant_string(r#""a } b""#), "a } b");
}

// A string literal that is *named* rather than *evaluated* — a match pattern, a
// record key, a route path — cannot interpolate, so it must be brace-decoded.
// Skipping that decode left the doubled form in place and silently changed the
// value: a pattern that could never match, a field nobody could look up.

/// Debug rendering of the whole lowered program.
fn program_debug(src: &str) -> String {
    format!("{:?}", ir_of(src))
}

#[test]
fn escaped_brace_in_a_pattern_matches_the_literal_brace() {
    let dbg = program_debug("s <- \"x\"\n~ s ⟦ \"\\{x}\" → 1 ⟧\n");
    assert!(
        dbg.contains("Literal(String(\"{x}\""),
        "pattern kept the doubled form (can never match): {dbg}"
    );
    assert!(
        !dbg.contains("{{x}"),
        "pattern still holds an encoded brace: {dbg}"
    );
}

#[test]
fn doubled_brace_in_a_pattern_is_decoded_too() {
    let dbg = program_debug("s <- \"x\"\n~ s ⟦ \"{{x}}\" → 1 ⟧\n");
    assert!(dbg.contains("Literal(String(\"{x}\""), "{dbg}");
}

#[test]
fn escaped_brace_in_a_record_key_names_the_right_field() {
    let dbg = program_debug("r <- ⟨\"\\{a}\": 1⟩\n");
    assert!(
        dbg.contains("String(\"{a}\")"),
        "record key kept the doubled form: {dbg}"
    );
    assert!(!dbg.contains("{{a}"), "key still encoded: {dbg}");
}

#[test]
fn record_key_decode_applies_at_every_depth() {
    // Record keys were lowered by three copies of the same match (record
    // literals, the spread-merge path, and `data` fields); they all go through
    // one helper now. The spread-merge path cannot be reached from source yet —
    // the parser never produces `RecordKey::Spread` — so this covers the nested
    // literal case instead.
    let dbg = program_debug("r <- ⟨outer: ⟨\"\\{a}\": 2⟩⟩\n");
    assert!(dbg.contains("String(\"{a}\")"), "{dbg}");
    assert!(!dbg.contains("{{a}"), "{dbg}");
}

#[test]
fn data_declaration_keys_are_decoded() {
    // `def Name ⟨…⟩` (a def with a record body and no parens) is a data decl,
    // which lowers its field keys on a separate path.
    let dbg = program_debug("def Config ⟨\"\\{a}\": 1⟩\n");
    assert!(dbg.contains("String(\"{a}\")"), "{dbg}");
    assert!(!dbg.contains("{{a}"), "{dbg}");
}

#[test]
fn plain_record_keys_are_untouched() {
    let dbg = program_debug("r <- ⟨\"plain key\": 1, other: 2⟩\n");
    assert!(dbg.contains("String(\"plain key\")"), "{dbg}");
    assert!(dbg.contains("Ident(\"other\")"), "{dbg}");
}

// Raw strings are literal: nothing in `r"…"` may be interpolated.

#[test]
fn raw_string_does_not_interpolate() {
    assert_eq!(constant_string("r\"{x}\""), "{x}");
    assert_eq!(constant_string("r\"hi {name} there\""), "hi {name} there");
}

#[test]
fn raw_string_keeps_doubled_braces_verbatim() {
    // Unlike an escaped or multiline string, a raw string has no escapes, so a
    // doubled brace is two braces.
    assert_eq!(constant_string("r\"{{x}}\""), "{{x}}");
    assert_eq!(constant_string("r\"{\""), "{");
    assert_eq!(constant_string("r\"}\""), "}");
}

#[test]
fn multiline_string_still_interpolates_and_honours_doubling() {
    let dbg = bound_expr_debug("\"\"\"hi {name}\"\"\"");
    assert!(
        dbg.contains("Global(\"name\")"),
        "multiline lost interpolation: {dbg}"
    );
    assert_eq!(
        constant_string("\"\"\"{{ mustache }}\"\"\""),
        "{ mustache }"
    );
}

#[test]
fn multibyte_text_around_interpolation_survives() {
    let dbg = bound_expr_debug(r#""café 🚀 {name} ✓ \{name}""#);
    assert!(dbg.contains("Global(\"name\")"), "{dbg}");
    assert!(dbg.contains("café 🚀 "), "{dbg}");
    assert!(dbg.contains(" ✓ {name}"), "{dbg}");
}
