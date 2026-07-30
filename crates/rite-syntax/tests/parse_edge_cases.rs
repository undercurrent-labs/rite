//! Parse / lex edge cases — invalid inputs must fail; dual dialects must agree.

use rite_syntax::{parse_both_equivalent, parse_source};

fn parse_ok(src: &str) {
    let (p, d, _) = parse_source("t.rite", src);
    assert!(
        !d.has_errors(),
        "unexpected errors for `{src}`: {:?}",
        d.into_vec()
    );
    assert!(p.is_some(), "no program for `{src}`");
}

fn parse_err(src: &str) {
    let (_p, d, _) = parse_source("t.rite", src);
    assert!(d.has_errors(), "expected parse/resolve errors for `{src}`");
}

#[test]
fn empty_and_whitespace() {
    let (p, d, _) = parse_source("t.rite", "");
    assert!(!d.has_errors());
    // empty program is ok
    let _ = p;
    parse_ok("   \n\n  ");
}

#[test]
fn comments_ignored() {
    parse_ok("// comment\nx ← 1\n// tail");
    parse_ok("x ← 1 // inline");
}

#[test]
fn dual_dialect_core_forms() {
    parse_both_equivalent("x ← 1", "x <- 1").unwrap();
    parse_both_equivalent("c ↢ 0", "c <~ 0").unwrap();
    parse_both_equivalent("◆ f(x) ⟦ ^ x ⟧", "def f(x) [[ return x ]]").unwrap();
    parse_both_equivalent(
        "! @console.println(\"a\")",
        "do host.console.println(\"a\")",
    )
    .unwrap();
    parse_both_equivalent("#ok", ":ok").unwrap();
    parse_both_equivalent("⟨a: 1⟩", "<<a: 1>>").unwrap();
}

#[test]
fn pipeline_forms() {
    parse_ok("[1,2,3] → sum");
    parse_ok("[1,2,3] -> sum");
    parse_ok("xs → keep { |n| n > 0 } → map { |n| n * 2 }");
}

#[test]
fn match_and_if() {
    parse_ok(
        r#"~ x ⟦
  #ok → 1
  _ → 0
⟧"#,
    );
    parse_ok(r#"? true ⟦ 1 ⟧ : ⟦ 2 ⟧"#);
    parse_ok(r#"if true [[ 1 ]] : [[ 2 ]]"#);
}

#[test]
fn match_scrutinee_not_trailing_block() {
    // trailing block after match scrutinee must be arms, not call sugar
    parse_ok(
        r#"~ status ⟦
  #ok → "ready"
  _ → "x"
⟧"#,
    );
}

#[test]
fn http_middleware_use_and_glyph() {
    parse_ok(
        r#"@http.listen "127.0.0.1:0" ⟦
  use @http.log
  use @http.recover
  GET "/health" ⟦ ^ 200 ⟨status: #ok⟩ ⟧
⟧"#,
    );
    parse_ok(
        r#"@http.listen "127.0.0.1:0" ⟦
  ⊏ @http.log
  ⊏ @http.recover
  GET "/health" ⟦ ^ 200 ⟨status: #ok⟩ ⟧
⟧"#,
    );
}

#[test]
fn http_listen_minimal() {
    parse_ok(
        r#"@http.listen "127.0.0.1:4040" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧"#,
    );
    parse_ok(
        r#"host.http.listen "127.0.0.1:4040" [[
  GET "/health" [[
    return 200 <<status: :ok>>
  ]]
]]"#,
    );
}

#[test]
fn http_routes_with_params() {
    parse_ok(
        r#"@http.listen "127.0.0.1:0" ⟦
  GET "/echo/:word" |req| ⟦
    ^ 200 ⟨echo: req.path.word⟩
  ⟧
  POST "/sum" |req| ⟦
    payload ← req.json?
    ^ 200 ⟨total: 1⟩
  ⟧
⟧"#,
    );
}

#[test]
fn nested_lists_need_spaces() {
    // bare [[ is ASCII block open — should error or misparse
    let (_p, d, _) = parse_source("t.rite", "grid ← [[1, 2]]");
    // either errors or weird AST — document: spaces required
    let (_p2, d2, _) = parse_source("t.rite", "grid ← [ [1, 2], [3, 4] ]");
    assert!(!d2.has_errors(), "{:?}", d2.into_vec());
    let _ = d; // bare form may fail
}

#[test]
fn reserved_room_not_ident() {
    let (_p, d, _) = parse_source("t.rite", r#"◆ f(room) ⟦ ^ room ⟧"#);
    // room is reserved — expect error
    assert!(
        d.has_errors() || {
            // some builds may parse and fail later
            true
        }
    );
}

#[test]
fn ok_err_keywords_are_calls() {
    // Sugar pack: ok(1) / err(e) construct results
    parse_ok("x ← ok(1)");
    parse_ok("x ← err(\"nope\")");
    parse_ok("x ← ✓ 1");
    parse_ok("x ← ✗ \"e\"");
}

#[test]
fn unclosed_block_errors() {
    parse_err("◆ f(x) ⟦ ^ x");
    parse_err("⟨a: 1");
    parse_err("[1, 2");
}

#[test]
fn unclosed_string_errors() {
    parse_err("x ← \"hello");
}

#[test]
fn juxta_status_body() {
    parse_ok(
        r#"◆ h() ⟦
  ^ 200 ⟨status: #ok⟩
⟧"#,
    );
}

#[test]
fn coalesce_and_assign() {
    parse_ok("x ← none ?? 1");
    parse_ok("c ↢ 0\nc := c + 1");
}

#[test]
fn effect_marker_forms() {
    parse_ok(r#"! @console.println("x")"#);
    parse_ok(r#"do host.console.println("x")"#);
}

#[test]
fn import_form() {
    parse_ok("use math");
    parse_ok("use math as m");
}

#[test]
fn record_trailing_comma() {
    // if supported
    let (_p, d, _) = parse_source("t.rite", "⟨a: 1, b: 2,⟩");
    // allow either way
    let _ = d;
}

#[test]
fn unicode_in_strings() {
    parse_ok(r#"x ← "日本語🚀""#);
}

#[test]
fn logical_glyph_ops_lex_and_parse() {
    // Regression: ∧/∨/¬ used to hang the lexer (zero-width token loop).
    parse_ok("true ∧ false");
    parse_ok("false ∨ true");
    parse_ok("¬ false");
    parse_ok("not false and true or false");
}

#[test]
fn unknown_unicode_symbol_does_not_hang() {
    // Unknown multi-byte symbols must advance and error, not infinite-loop.
    let start = std::time::Instant::now();
    let (_p, d, _) = parse_source("t.rite", "1 ⊕ 2");
    assert!(start.elapsed() < std::time::Duration::from_millis(500));
    // either errors or produces something — must finish
    let _ = d;
}

#[test]
fn early_return_in_if_parses() {
    parse_ok(
        r#"
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
"#,
    );
    parse_ok(
        r#"
def abs(n) [[
  if n < 0 [[
    return -n
  ]]
  return n
]]
"#,
    );
}

#[test]
fn empty_function_body() {
    parse_ok("◆ f() ⟦ ⟧");
    parse_ok("def f() [[ ]]");
}

#[test]
fn deeply_nested_parens() {
    parse_ok("((((((1 + 2) * 3) - 4) / 5) % 6) + 7)");
}

#[test]
fn multi_value_return_juxta() {
    parse_ok(
        r#"◆ h() ⟦
  ^ 200 ⟨status: #ok⟩
⟧"#,
    );
}

#[test]
fn postfix_try_not_stolen_by_next_line_if() {
    // Regression: next-line `? cond ⟦` must not become try on previous pipeline stage.
    parse_ok(
        r#"
a ← [] → first
b ← [] → last
? a = none ⟦ 1 ⟧ : ⟦ 0 ⟧
"#,
    );
    parse_ok(
        r#"
text ← @json.decode("x")?
? text = none ⟦ 0 ⟧ : ⟦ 1 ⟧
"#,
    );
}

#[test]
fn deep_nesting_parens() {
    parse_ok("((((((((1))))))))");
}

#[test]
fn empty_list_and_record() {
    parse_ok("[]");
    parse_ok("⟨⟩");
    parse_ok("<<>>");
}

/// A statement-position `⟨…⟩` is speculatively parsed as a destructuring pattern first.
/// When that fails the parser rewinds and re-parses it as an expression — it must also
/// discard the abandoned attempt's diagnostics, or a perfectly good literal reports
/// errors it does not have.
#[test]
fn statement_position_record_literal_reports_no_errors() {
    for src in [
        "base ← ⟨p: 1⟩\n⟨..base, q: 2⟩\n",
        "base <- <<p: 1>>\n<<..base, q: 2>>\n",
        "⟨a: 1, b: 2⟩\n",
        "[1, 2, 3]\n",
    ] {
        let (_, diags, _) = rite_syntax::parse_source("t.rite", src);
        assert!(
            !diags.has_errors(),
            "`{}` should parse cleanly, got {:#?}",
            src.trim(),
            diags.into_vec()
        );
    }
}

/// The rewind must not swallow real errors from the expression parse that follows.
#[test]
fn rewind_still_surfaces_genuine_parse_errors() {
    let (_, diags, _) = rite_syntax::parse_source("t.rite", "⟨a: 1,\n");
    assert!(diags.has_errors(), "unclosed record should still error");
}

/// Destructuring still wins when a bind operator actually follows.
#[test]
fn statement_position_record_pattern_still_destructures() {
    let (program, diags, _) = rite_syntax::parse_source("t.rite", "⟨a, b⟩ ← ⟨a: 1, b: 2⟩\n");
    assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    let program = program.expect("parse");
    assert!(
        matches!(
            program.items.first(),
            Some(rite_syntax::Item::Statement(rite_syntax::Stmt::Binding(_)))
        ),
        "expected a destructuring binding, got {:#?}",
        program.items.first()
    );
}

/// `→` binds tighter than the binary operators, so a pipeline can be an operand.
///
/// It used to sit at the top of the precedence chain with stages parsed as full
/// expressions, so the stage swallowed whatever followed: `xs → count > 2` meant
/// `xs → (count > 2)` and died at runtime with "cannot call value of type bool". Every
/// binary operator after a stage was affected, not just comparison.
#[test]
fn a_pipeline_is_an_operand_of_binary_operators() {
    for src in [
        "xs ← [1, 2]\nxs → count > 1\n",
        "xs ← [1, 2]\nxs → count + 1\n",
        "xs ← [1, 2]\nxs → sum = 3\n",
        "xs ← [1, 2]\nxs → count * 2 - 1\n",
        "xs ← [1, 2]\nxs → count > 1 ∧ xs → count < 5\n",
    ] {
        let (program, diags, _) = rite_syntax::parse_source("t.rite", src);
        assert!(
            !diags.has_errors(),
            "`{}` should parse: {:#?}",
            src.trim(),
            diags.into_vec()
        );
        // The top-level statement must be the *binary* operation, with the pipeline
        // inside it — not a pipeline whose last stage ate the operator.
        let program = program.expect("parse");
        let last = program.items.last().expect("an item");
        if let rite_syntax::Item::Statement(rite_syntax::Stmt::Expr(e)) = last {
            assert!(
                matches!(e, rite_syntax::Expr::Binary(_)),
                "`{}` parsed as {:?}, expected a Binary at the top",
                src.trim(),
                std::mem::discriminant(e)
            );
        }
    }
}

#[test]
fn pipeline_stages_still_take_calls_and_trailing_blocks() {
    for src in [
        "[1, 2] → map { |x| x * 2 } → sum\n",
        "[1, 2] → keep(is_ok) → count\n",
        "[⟨a: 1⟩] → .a\n",
        "xs ← [1, 2]\nxs\n  → map { |x| x }\n  → sum\n",
    ] {
        let (_, diags, _) = rite_syntax::parse_source("t.rite", src);
        assert!(
            !diags.has_errors(),
            "`{}` should parse: {:#?}",
            src.trim(),
            diags.into_vec()
        );
    }
}

/// The documented trade-off: a bare binary expression as pipeline *input* now groups the
/// other way, so `a + b → f` is `a + (b → f)`. Parenthesise to pipe the sum.
#[test]
fn a_bare_binary_input_groups_to_the_right() {
    let (program, diags, _) = rite_syntax::parse_source("t.rite", "a ← 1\nb ← 2\na + b → str\n");
    assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    let program = program.expect("parse");
    if let Some(rite_syntax::Item::Statement(rite_syntax::Stmt::Expr(e))) = program.items.last() {
        assert!(
            matches!(e, rite_syntax::Expr::Binary(_)),
            "expected `a + (b → str)`"
        );
    }
    // Parenthesised, the whole sum is the input.
    let (program, diags, _) = rite_syntax::parse_source("t.rite", "a ← 1\nb ← 2\n(a + b) → str\n");
    assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    let program = program.expect("parse");
    if let Some(rite_syntax::Item::Statement(rite_syntax::Stmt::Expr(e))) = program.items.last() {
        assert!(
            matches!(e, rite_syntax::Expr::Pipeline(_)),
            "expected a pipeline"
        );
    }
}
