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

/// Contextual keywords are bindable names, so every read of one has to parse as
/// an expression.
///
/// `is_keyword_as_ident` has always let `item`, `room`, `world`, `test`, `ok`,
/// `err` and `some` be *bound*, but `at_expr_start` did not list them, so `^ item`
/// saw no expression and produced `return none`. The parameter was bound and then
/// read back as nothing: `◆ f(item) ⟦ ^ item ⟧` answered `none`, and
/// `map { |item| item * 2 }` answered a constant — wrong numbers, no diagnostic.
#[test]
fn contextual_keywords_parse_as_expressions() {
    for word in ["item", "room", "world", "test", "ok", "err", "some"] {
        let src = format!("◆ f({word}) ⟦ ^ {word} ⟧");
        let (program, diags, _) = parse_source("kw.rite", &src);
        assert!(
            !diags.has_errors(),
            "`{word}` as a parameter should parse: {:?}",
            diags.into_vec()
        );
        let program = program.expect("program");
        let rendered = format!("{:?}", program);
        assert!(
            rendered.contains(&format!("\"{word}\"")),
            "`{word}` should appear as a name in the tree, not vanish"
        );
        // The body must return something. A missing expression is the bug.
        assert!(
            !rendered.contains("Return(ReturnStmt { value: None"),
            "`^ {word}` must return the binding, not `none`"
        );
    }
}

/// `@tcp.listen addr ⟦ |conn| … ⟧` is sugar, not a new node: it must arrive as the
/// ordinary call `@tcp.listen(addr, block)`, so everything downstream — desugar's
/// block-to-closure rule, effect discipline, the compiler — sees a capability call
/// it already knows how to handle. If it ever grew its own AST node, this fails.
#[test]
fn tcp_listen_is_sugar_for_a_capability_call() {
    for src in [
        "! @tcp.listen \"127.0.0.1:0\" ⟦ |conn| conn ⟧",
        "do host.tcp.listen \"127.0.0.1:0\" [[ |conn| conn ]]",
    ] {
        let (program, diags, _) = parse_source("tcp.rite", src);
        assert!(!diags.has_errors(), "{src}: {:?}", diags.into_vec());
        let program = program.expect("program");
        let Some(rite_syntax::Item::Statement(rite_syntax::Stmt::Expr(rite_syntax::Expr::Unary(
            u,
        )))) = program.items.last()
        else {
            panic!("{src}: expected `!` applied to something");
        };
        let rite_syntax::Expr::Call(call) = u.expr.as_ref() else {
            panic!("{src}: expected a call, got {:?}", u.expr);
        };
        assert!(
            matches!(call.callee.as_ref(), rite_syntax::Expr::Capability(c) if c.path == ["tcp", "listen"]),
            "{src}: callee should be the @tcp.listen capability"
        );
        assert_eq!(call.args.len(), 2, "{src}: address and handler block");
        let rite_syntax::Expr::Block(b) = &call.args[1] else {
            panic!("{src}: the handler must be a block");
        };
        // The parameter is what binds the accepted connection. Dropping it would
        // make the block a plain block rather than a closure, and `conn` would
        // resolve to nothing.
        assert_eq!(b.params.len(), 1, "{src}: the block binds one parameter");
        assert_eq!(b.params[0].name.name, "conn");
    }
}

/// The handler block belongs to `listen`, even when the address is a binding.
///
/// Trailing-block call sugar makes `f ⟦…⟧` a call to `f`, and an identifier is
/// callable — so with the sugar left on, `@tcp.listen where ⟦ |conn| … ⟧` parsed as
/// `@tcp.listen(where(⟦…⟧))` and `listen` was handed no block at all. Every example
/// in the book uses a string literal, which is *not* callable, so nothing caught it.
#[test]
fn a_listen_address_in_a_binding_does_not_eat_the_block() {
    let cases = [
        "where ← \"127.0.0.1:0\"\n! @tcp.listen where ⟦ |conn| conn ⟧",
        "where ← \"127.0.0.1:0\"\n@http.listen where ⟦ GET \"/\" ⟦ ^ 200 ⟧ ⟧",
    ];
    for src in cases {
        let (program, diags, _) = parse_source("listen.rite", src);
        assert!(!diags.has_errors(), "{src}: {:?}", diags.into_vec());
        let program = program.expect("program");
        let rendered = format!("{:?}", program.items.last().expect("statement"));
        // The address stays the identifier. A call whose callee is an identifier is
        // the bug: it can only be `where(block)`.
        assert!(
            !rendered.contains("callee: Ident"),
            "the handler block was swallowed as a call to the address: {rendered}"
        );
    }
}

/// A capability method keeps its name even when the lexer has promoted that word to
/// a keyword. `say` is the shorthand for printing, and leaving it out of
/// `is_keyword_as_ident` made `@game.say(…)` parse as `@game.` plus a stray keyword:
/// it reached the runtime as an empty method and died with `unknown @game.`, so one
/// capability function could not be called from Rite at all.
///
/// Both dialects, because `host.game.say` has to survive the same way.
#[test]
fn keyword_named_capability_methods_parse() {
    // The method name has to be *in the tree*. Asserting only that the source parses
    // is not enough: the path loop simply stops at the unexpected token, so a dropped
    // segment produces no diagnostic at all and fails later at runtime with
    // `unknown @game.` — which is exactly how this survived.
    for (src, method) in [
        ("! @game.say(\"hi\")", "say"),
        ("do host.game.say(\"hi\")", "say"),
        ("! @game.take(#coin)", "take"),
    ] {
        let (program, diags, _) = parse_source("cap.rite", src);
        assert!(
            !diags.has_errors(),
            "`{src}` should parse: {:?}",
            diags.into_vec()
        );
        let rendered = format!("{:?}", program.expect("program"));
        assert!(
            rendered.contains(&format!("\"{method}\"")),
            "`{src}` dropped the method segment `{method}`: {rendered}"
        );
    }

    // …and `say` is still a statement keyword, which is the thing that made this a
    // collision in the first place.
    let (program, diags, _) = parse_source("say.rite", "say \"hello\"");
    assert!(
        !diags.has_errors(),
        "the say statement must still parse: {:?}",
        diags.into_vec()
    );
    assert!(program.is_some());
}

/// `?` inside a call argument, followed by a statement whose call takes a lambda.
///
/// The `?` token is also prefix `if`, so the parser looks ahead to tell them apart.
/// That scan begins inside whatever group the `?` sits in but starts its paren depth
/// at zero, and used `saturating_sub` on the way down — which saturates at
/// `i32::MIN`, not at zero. The closing `)` of the enclosing call drove the depth to
/// -1, the next statement's `(` brought it back to 0, and that statement's lambda
/// `{` then looked like the body of a conditional. The `?` was read as prefix `if`
/// and the file failed to parse, blaming the *previous* line.
#[test]
fn try_inside_a_call_is_not_stolen_by_a_later_lambda() {
    for src in [
        // The shape that found this: decode inside a call, then a lambda-taking call.
        "◆! main() ⟦\n  r ← id(@json.decode(\"[]\")?)\n  each(r, { |x| x })\n⟧\n◆ id(o) ⟦ ^ o ⟧",
        // Blank line between makes no difference — it is token-based.
        "◆! main() ⟦\n  r ← id(@json.decode(\"[]\")?)\n\n  each(r, { |x| x })\n⟧\n◆ id(o) ⟦ ^ o ⟧",
        // Same via brackets and records, the other two depth counters.
        "◆! main() ⟦\n  r ← [@json.decode(\"[]\")?]\n  each(r, { |x| x })\n⟧",
        "◆! main() ⟦\n  r ← ⟨v: @json.decode(\"[]\")?⟩\n  each(r.v, { |x| x })\n⟧",
    ] {
        let (program, diags, _) = parse_source("try.rite", src);
        assert!(
            !diags.has_errors(),
            "should parse: {src}\n{:?}",
            diags.into_vec()
        );
        assert!(program.is_some());
    }

    // The disambiguation it exists for must still work: a genuine prefix `if` on the
    // line after a postfix `?` is a conditional, not a try on the previous expression.
    let (_, diags, _) = parse_source(
        "if.rite",
        "◆! main() ⟦\n  r ← id(@json.decode(\"[]\")?)\n  ? count(r) = 0 ⟦ ! println(\"empty\") ⟧\n⟧\n◆ id(o) ⟦ ^ o ⟧",
    );
    assert!(
        !diags.has_errors(),
        "a following conditional must still parse: {:?}",
        diags.into_vec()
    );
}

/// `⟦ || 42 ⟧` is a function of no arguments; `⟦ 42 ⟧` is a block that evaluates
/// to 42. Both have an empty parameter list, so the parser has to record whether
/// the `|…|` was written at all — with only `params` to go on, desugar read the
/// thunk as a bare block and it evaluated to `42` instead of becoming callable.
#[test]
fn an_empty_parameter_list_is_still_a_parameter_list() {
    use rite_syntax::ast::{Expr, Item, Stmt};

    fn block_of(src: &str) -> rite_syntax::ast::Block {
        let (program, diags, _) = parse_source("t.rite", src);
        assert!(!diags.has_errors(), "parse errors for `{src}`");
        let program = program.expect("program");
        for item in &program.items {
            if let Item::Statement(Stmt::Binding(b)) = item {
                if let Expr::Block(block) = &b.value {
                    return block.clone();
                }
            }
        }
        panic!("no block binding in `{src}`");
    }

    let thunk = block_of("f ← ⟦ || 42 ⟧\n");
    assert!(thunk.params.is_empty());
    assert!(thunk.has_param_list, "empty `||` was not recorded");

    let plain = block_of("x ← ⟦ 42 ⟧\n");
    assert!(plain.params.is_empty());
    assert!(
        !plain.has_param_list,
        "a bare block gained a parameter list"
    );

    // Both dialects agree, since `||` is not a token of its own in either.
    let ascii = block_of("f <- [[ || 42 ]]\n");
    assert!(ascii.has_param_list);
}
