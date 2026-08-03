//! Parser behaviour: every construct, both spellings, and the ambiguities the
//! lexer refused to guess at.

use cant_syntax::{parse_source, structure, CantProgramAst, StageKind};
use serde_json::json;

fn ok(src: &str) -> CantProgramAst {
    let (result, sources) = parse_source("t.cant", src);
    assert!(
        !result.has_errors(),
        "{src:?} should parse:\n{}",
        result.diagnostics.render_all(&sources)
    );
    result.program.expect("a clean parse yields a program")
}

fn shape(src: &str) -> serde_json::Value {
    structure(&ok(src))
}

fn first_error(src: &str) -> String {
    let (result, _) = parse_source("t.cant", src);
    let code = result
        .diagnostics
        .errors()
        .next()
        .map(|d| d.code.to_string());
    code.unwrap_or_else(|| panic!("{src:?} should have been rejected"))
}

// ---- constructs

#[test]
fn a_flow_is_a_chain_of_stages() {
    assert_eq!(
        shape("3 -> square -> double"),
        json!([{"leaf": "3"}, {"leaf": "square"}, {"leaf": "double"}])
    );
}

#[test]
fn scatter_and_collect() {
    assert_eq!(
        shape("[1, 2, 3] -> * -> square -> []"),
        json!([{"leaf": "[1, 2, 3]"}, "scatter", {"leaf": "square"}, "collect"])
    );
}

#[test]
fn a_ward_holds_one_predicate() {
    assert_eq!(
        shape("rows -> ?{ $.level = :error }"),
        json!([{"leaf": "rows"}, {"ward": "$.level = :error"}])
    );
}

#[test]
fn fork_branches_keep_their_order() {
    assert_eq!(
        shape("5 -> |{ $ + 1 ; $ * 2 ; square }"),
        json!([
            {"leaf": "5"},
            {"fork": [[{"leaf": "$ + 1"}], [{"leaf": "$ * 2"}], [{"leaf": "square"}]]}
        ])
    );
}

#[test]
fn an_orbit_body_is_a_whole_flow() {
    assert_eq!(
        shape("roots -> ~{ deps -> * }"),
        json!([{"leaf": "roots"}, {"orbit": [{"leaf": "deps"}, "scatter"]}])
    );
}

#[test]
fn modifiers_attach_to_the_form_on_their_left() {
    assert_eq!(
        shape("roots -> ~{ deps } :by canonical :max 4096 -> []"),
        json!([
            {"leaf": "roots"},
            {
                "kind": {"orbit": [{"leaf": "deps"}]},
                "modifiers": [
                    {"name": "by", "value": "canonical"},
                    {"name": "max", "value": "4096"}
                ]
            },
            "collect"
        ])
    );
}

#[test]
fn blocks_nest() {
    let program = ok("request -> |{ ?{ $.ok } -> handle ; ~{ children -> * } :max 8 }");
    let StageKind::Fork { branches } = &program.flow.stages[1].kind else {
        panic!("expected a fork");
    };
    assert_eq!(branches.len(), 2);
    assert!(matches!(branches[0].stages[0].kind, StageKind::Ward { .. }));
    assert!(matches!(
        branches[1].stages[0].kind,
        StageKind::Orbit { .. }
    ));
    assert_eq!(branches[1].stages[0].modifiers[0].name, "max");
}

// ---- the ambiguities

#[test]
fn a_star_is_scatter_only_when_it_is_the_whole_stage() {
    assert_eq!(shape("xs -> *"), json!([{"leaf": "xs"}, "scatter"]));
    assert_eq!(
        shape("xs -> $ * 2"),
        json!([{"leaf": "xs"}, {"leaf": "$ * 2"}]),
        "`*` between operands is multiplication"
    );
}

#[test]
fn brackets_are_collect_in_a_stage_and_an_empty_list_at_the_start() {
    assert_eq!(shape("xs -> []"), json!([{"leaf": "xs"}, "collect"]));
    assert_eq!(
        shape("[] -> length"),
        json!([{"leaf": "[]"}, {"leaf": "length"}]),
        "nothing has been emitted yet, so `[]` opening a program is a literal"
    );
    assert_eq!(
        shape("xs -> f([]) -> []"),
        json!([{"leaf": "xs"}, {"leaf": "f([])"}, "collect"]),
        "`[]` inside a call is an argument, not a collect"
    );
}

#[test]
fn a_colon_is_a_modifier_after_a_block_and_a_rite_atom_everywhere_else() {
    assert_eq!(
        shape("rows -> ?{ $.level = :error }"),
        json!([{"leaf": "rows"}, {"ward": "$.level = :error"}]),
        "`:error` is Rite's ASCII atom spelling"
    );
    let program = ok("rows -> ~{ f } :max 8");
    assert_eq!(program.flow.stages[1].modifiers[0].name, "max");
}

#[test]
fn a_colon_with_a_space_after_it_is_not_a_modifier() {
    // `: max` cannot be a modifier, so it stays leaf text and Rite gets to
    // complain about it — which is the right place for it.
    assert_eq!(
        shape("rows -> f : g"),
        json!([{"leaf": "rows"}, {"leaf": "f : g"}])
    );
}

#[test]
fn a_rite_closure_brace_does_not_close_a_cant_block() {
    assert_eq!(
        shape("xs -> ?{ any($, { |n| n > 0 }) } -> []"),
        json!([{"leaf": "xs"}, {"ward": "any($, { |n| n > 0 })"}, "collect"])
    );
}

// ---- leaf metadata

#[test]
fn a_leaf_records_whether_it_performs_an_effect() {
    let program = ok("path -> !@fs.read -> @json.decode");
    let StageKind::Leaf(effectful) = &program.flow.stages[1].kind else {
        panic!("expected a leaf");
    };
    assert!(effectful.has_effect_marker);
    let StageKind::Leaf(pure) = &program.flow.stages[2].kind else {
        panic!("expected a leaf");
    };
    assert!(
        !pure.has_effect_marker,
        "`@json.decode` is a value transform and carries no `!`"
    );
}

#[test]
fn not_equal_is_not_an_effect_marker() {
    let program = ok("xs -> ?{ $ != 2 }");
    let StageKind::Ward { predicate } = &program.flow.stages[1].kind else {
        panic!("expected a ward");
    };
    assert!(!predicate.has_effect_marker, "`!=` is one operator");
}

#[test]
fn a_leaf_records_whether_the_current_value_is_placed_explicitly() {
    let program = ok(r#""-" -> join(["a", "b"], $) -> upper"#);
    let StageKind::Leaf(explicit) = &program.flow.stages[1].kind else {
        panic!("expected a leaf");
    };
    assert!(explicit.has_placeholder);
    let StageKind::Leaf(implicit) = &program.flow.stages[2].kind else {
        panic!("expected a leaf");
    };
    assert!(!implicit.has_placeholder);
}

#[test]
fn leaf_text_is_the_source_slice_so_it_survives_into_generated_rite() {
    let program = ok("xs -> replace($, \"->\", \"|{\")");
    let StageKind::Leaf(leaf) = &program.flow.stages[1].kind else {
        panic!("expected a leaf");
    };
    assert_eq!(leaf.text, r#"replace($, "->", "|{")"#);
}

// ---- spellings

/// Every dialect fixture pair, plus the cases above, must agree.
#[test]
fn ascii_and_glyph_parse_to_the_same_program() {
    let pairs = [
        ("3 -> square", "3 → square"),
        ("[1, 2] -> * -> []", "[1, 2] → ⋇ → ⌁"),
        ("rows -> ?{ $ > 0 }", "rows → ⊣⟦ $ > 0 ⟧"),
        ("5 -> |{ $ + 1 ; $ * 2 }", "5 → ⫴⟦ $ + 1 ; $ * 2 ⟧"),
        ("r -> ~{ d -> * } :max 8", "r → ⟲⟦ d → ⋇ ⟧ :max 8"),
        // Mixed input parses; nothing requires one spelling throughout.
        (
            "roots -> * -> ?{ $ > 0 } -> []",
            "roots → * -> ⊣⟦ $ > 0 ⟧ → []",
        ),
    ];
    for (ascii, glyph) in pairs {
        assert_eq!(shape(ascii), shape(glyph), "{ascii:?} vs {glyph:?}");
    }
}

#[test]
fn strings_and_comments_containing_operators_are_left_alone() {
    let src = concat!(
        "// -> ?{ |{ ~{ [] → ⋇ ⌁\n",
        "\"a -> b ?{ c } []\" -> f\n",
        "/* -> ~{ */\n"
    );
    assert_eq!(
        shape(src),
        json!([{"leaf": "\"a -> b ?{ c } []\""}, {"leaf": "f"}])
    );
}

// ---- diagnostics

#[test]
fn malformed_sources_report_the_code_that_names_the_mistake() {
    let cases = [
        ("// nothing\n", "CANT-P001"),
        ("-> f", "CANT-P002"),
        ("a -> -> b", "CANT-P002"),
        ("~{ a", "CANT-P003"),
        ("a }", "CANT-P004"),
        ("a ->", "CANT-P005"),
        ("a ; b", "CANT-P006"),
        ("a -> f ⋇ 2", "CANT-P007"),
        ("a -> ~{ f } -> :max 4", "CANT-P008"),
        ("a -> ~{ f } :max 4 : by g", "CANT-P009"),
        ("a -> ~{ f } :max", "CANT-P010"),
        ("5 -> |{ $ + 1 ; }", "CANT-P011"),
        ("rows -> ?{ $.a -> $.b }", "CANT-P012"),
        ("x -> \"never closed", "CANT-L002"),
        ("x -> f /* never closed", "CANT-L003"),
        ("a \u{7} b", "CANT-L001"),
    ];
    for (src, code) in cases {
        assert_eq!(first_error(src), code, "for {src:?}");
    }
}

#[test]
fn nesting_past_the_limit_is_reported_rather_than_overflowing_the_stack() {
    let deep = format!(
        "x -> {}f{}",
        "~{ ".repeat(cant_syntax::MAX_NESTING + 4),
        " }".repeat(cant_syntax::MAX_NESTING + 4)
    );
    assert_eq!(first_error(&deep), "CANT-P013");
}

#[test]
fn a_rejected_source_still_yields_the_program_that_was_recovered() {
    let (result, _) = parse_source("t.cant", "a -> b }");
    assert!(result.has_errors());
    let program = result.program.expect("recovery keeps what parsed");
    assert_eq!(program.flow.stages.len(), 2);
}

#[test]
fn one_missing_brace_produces_one_error_not_a_cascade() {
    let (result, _) = parse_source("t.cant", "a -> b } -> c -> d -> e");
    assert_eq!(
        result.diagnostics.errors().count(),
        1,
        "{:?}",
        result
            .diagnostics
            .errors()
            .map(|d| d.code.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn diagnostics_point_at_cant_source_with_a_caret() {
    let (result, sources) = parse_source("t.cant", "roots -> square }");
    let rendered = result.diagnostics.render_all(&sources);
    assert!(rendered.contains("t.cant:1:17"), "{rendered}");
    assert!(rendered.contains('^'), "{rendered}");
}

#[test]
fn exit_codes_follow_rites_contract() {
    let (parse_error, _) = parse_source("t.cant", "~{ a");
    assert_eq!(parse_error.diagnostics.rejection_exit_code(), 3);
    let (clean, _) = parse_source("t.cant", "a -> b");
    assert_eq!(clean.diagnostics.rejection_exit_code(), 0);
}
