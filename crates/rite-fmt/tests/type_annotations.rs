//! Formatting must not lose a type annotation.
//!
//! A dropped annotation is not a cosmetic difference. It is the contract the runtime
//! enforces, and for an `@mcp` declaration it is also the JSON Schema the server
//! publishes to its clients — so `rite fmt` silently narrowing `|a: int|` to `|a|`
//! would change what a tool advertises without changing anything a reader would notice.
//!
//! `parse_both_equivalent` compares the structural dump from `rite-syntax`, which
//! includes declaration parameter types for exactly this reason.

use rite_fmt::{convert_source, format_with_dialect, Dialect};
use rite_syntax::{parse_both_equivalent, parse_source};

/// Formatting is a no-op on meaning: the result must parse to the same program.
fn round_trips(src: &str) {
    for d in [Dialect::Glyph, Dialect::Ascii] {
        let out = format_with_dialect(src, d)
            .unwrap_or_else(|e| panic!("format failed for {d:?}: {e:?}"))
            .text;
        let (_, diags, _) = parse_source("out.rite", &out);
        assert!(
            !diags.has_errors(),
            "formatted output does not parse ({d:?}):\n{out}\n{diags:?}"
        );
        parse_both_equivalent(src, &out)
            .unwrap_or_else(|e| panic!("formatting changed the program ({d:?}):\n{out}\n{e:?}"));

        let twice = format_with_dialect(&out, d).unwrap().text;
        assert_eq!(out, twice, "formatting is not idempotent ({d:?})");
    }
}

#[test]
fn a_result_payload_type_survives_formatting() {
    // `result<int>` used to print as bare `result`, which is a different type.
    round_trips("◆ f(x: result<int>) ⟦ ^ 1 ⟧\n");
}

#[test]
fn a_record_type_survives_formatting() {
    // `⟨a: int, b: string⟩` used to print as bare `record`, discarding every field.
    round_trips("◆ f(who: ⟨name: string, age: int⟩) ⟦ ^ 1 ⟧\n");
}

#[test]
fn a_nested_type_survives_formatting() {
    round_trips("◆ f(xs: [⟨id: int, tags: [string]⟩]) → result<[int]> ⟦ ^ ok([1]) ⟧\n");
}

#[test]
fn a_route_parameter_annotation_survives_formatting() {
    // The `Expr::Route` arm printed parameter names only.
    round_trips("@http.listen \"127.0.0.1:0\" ⟦\n  GET \"/x\" |req: any| ⟦ ^ 200 ⟧\n⟧\n");
}

#[test]
fn mcp_declaration_annotations_survive_formatting() {
    round_trips(
        "! @mcp.serve \"calculator\" ⟦\n  \
         use @mcp.log\n  \
         tool \"add\" \"Add two numbers\" |a: int, b: int| ⟦ ^ a + b ⟧\n  \
         resource \"config://app\" \"App config\" ⟦ ^ \"{}\" ⟧\n  \
         prompt \"review\" |code: string| ⟦ ^ code ⟧\n\
         ⟧\n",
    );
}

#[test]
fn mcp_declaration_survives_dialect_conversion() {
    let glyph = "! @mcp.serve \"s\" ⟦\n  tool \"add\" \"Adds\" |a: int| ⟦ ^ a ⟧\n⟧\n";
    let ascii = convert_source(glyph, Dialect::Ascii).unwrap().text;
    assert!(
        ascii.contains("host.mcp.serve"),
        "capability glyph not converted:\n{ascii}"
    );
    assert!(
        ascii.contains("a: int"),
        "annotation lost in conversion:\n{ascii}"
    );
    parse_both_equivalent(glyph, &ascii).unwrap();

    let back = convert_source(&ascii, Dialect::Glyph).unwrap().text;
    parse_both_equivalent(glyph, &back).unwrap();
}

/// The converse of the bug: formatting must not *invent* an annotation either.
#[test]
fn an_unannotated_parameter_stays_unannotated() {
    let src = "! @mcp.serve \"s\" ⟦\n  tool \"ping\" |x| ⟦ ^ x ⟧\n⟧\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(
        !out.contains("x:"),
        "formatter added an annotation that was not written:\n{out}"
    );
    parse_both_equivalent(src, &out).unwrap();
}
