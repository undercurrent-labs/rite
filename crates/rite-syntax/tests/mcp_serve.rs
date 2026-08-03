//! Parsing `@mcp.serve` declaration tables.
//!
//! The interesting cases are not the happy path — they are the two places this syntax
//! could quietly take something away from a script that does not use MCP at all:
//! `tool` / `resource` / `prompt` are ordinary lowercase words, and the parameter
//! annotations are load-bearing in a way a route's are not.

use rite_syntax::{parse_both_equivalent, parse_source, Expr, Item, McpDeclKind, Stmt, TypeExpr};

const SERVER: &str = r#"
! @mcp.serve "calculator" ⟦
  use @mcp.log

  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧

  resource "config://app" "App config" ⟦
    ^ "{}"
  ⟧

  prompt "review" |code: string| ⟦
    ^ code
  ⟧
⟧
"#;

/// Walk to the `@mcp.serve` body and return its declarations in source order.
fn decls(src: &str) -> Vec<rite_syntax::McpDeclExpr> {
    let (program, diags, _) = parse_source("t.rite", src);
    assert!(!diags.has_errors(), "parse errors: {:?}", diags);
    let program = program.expect("program");

    fn find(items: &[Item]) -> Option<&rite_syntax::McpServeExpr> {
        for item in items {
            if let Item::Statement(Stmt::Expr(e)) = item {
                if let Some(s) = walk(e) {
                    return Some(s);
                }
            }
        }
        None
    }
    fn walk(e: &Expr) -> Option<&rite_syntax::McpServeExpr> {
        match e {
            Expr::McpServe(s) => Some(s),
            // `! @mcp.serve …` wraps the node in the effect marker.
            Expr::Unary(u) => walk(&u.expr),
            Expr::Group(g) => walk(&g.expr),
            _ => None,
        }
    }

    let serve = find(&program.items).expect("no @mcp.serve found");
    serve
        .body
        .body
        .iter()
        .filter_map(|item| match item {
            Item::Statement(Stmt::Expr(Expr::McpDecl(d))) => Some(d.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn declarations_parse_in_source_order() {
    let ds = decls(SERVER);
    let kinds: Vec<_> = ds.iter().map(|d| d.kind).collect();
    assert_eq!(
        kinds,
        vec![
            McpDeclKind::Tool,
            McpDeclKind::Resource,
            McpDeclKind::Prompt
        ]
    );
    let names: Vec<_> = ds.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["add", "config://app", "review"]);
}

#[test]
fn the_description_is_optional_and_positional() {
    let ds = decls(SERVER);
    assert_eq!(ds[0].description.as_deref(), Some("Add two numbers"));
    assert_eq!(ds[1].description.as_deref(), Some("App config"));
    // `prompt "review" |code: string|` — no second string, so no description, and the
    // parameter list must not have been mistaken for one.
    assert_eq!(ds[2].description, None);
    assert_eq!(ds[2].params.len(), 1);
}

/// The declared types are the published schema, so losing one is not cosmetic.
#[test]
fn parameter_annotations_survive_parsing() {
    let ds = decls(SERVER);
    let params = &ds[0].params;
    assert_eq!(params.len(), 2);
    for (p, expected) in params.iter().zip(["int", "int"]) {
        match p.ty.as_ref().expect("annotation dropped") {
            TypeExpr::Named(i) => assert_eq!(i.name, expected),
            other => panic!("expected a named type, got {other:?}"),
        }
    }
}

#[test]
fn nested_annotations_survive_parsing() {
    let ds = decls(
        r#"
! @mcp.serve "s" ⟦
  tool "t" |xs: [string], who: ⟨name: string, age: int⟩| ⟦ ^ 1 ⟧
⟧
"#,
    );
    let params = &ds[0].params;
    assert!(matches!(params[0].ty, Some(TypeExpr::List(_))));
    match params[1].ty.as_ref().expect("annotation dropped") {
        TypeExpr::Record(fields) => {
            let keys: Vec<_> = fields.iter().map(|(k, _)| k.name.as_str()).collect();
            assert_eq!(keys, vec!["name", "age"]);
        }
        other => panic!("expected a record type, got {other:?}"),
    }
}

/// `tool`, `resource` and `prompt` are not tokens — a script may still use them as
/// names, including inside a serve block. Only a string literal directly after the
/// word makes it a declaration.
#[test]
fn the_declaration_words_are_still_usable_as_names() {
    let ds = decls(
        r#"
! @mcp.serve "s" ⟦
  tool ← "hammer"
  resource ← 3
  prompt ← [1, 2]
  tool "real" ⟦ ^ tool ⟧
⟧
"#,
    );
    assert_eq!(ds.len(), 1, "bindings were read as declarations");
    assert_eq!(ds[0].name, "real");
}

#[test]
fn a_declaration_needs_no_parameters() {
    let ds = decls(r#"! @mcp.serve "s" ⟦ tool "ping" "Pong" ⟦ ^ "pong" ⟧ ⟧"#);
    assert_eq!(ds.len(), 1);
    assert!(ds[0].params.is_empty());
    assert_eq!(ds[0].description.as_deref(), Some("Pong"));
}

#[test]
fn a_record_config_selects_a_transport() {
    let ds = decls(
        r#"
! @mcp.serve ⟨name: "calculator", transport: #http, addr: "127.0.0.1:8080"⟩ ⟦
  tool "add" |a: int| ⟦ ^ a ⟧
⟧
"#,
    );
    assert_eq!(ds.len(), 1);
}

#[test]
fn glyph_and_ascii_agree() {
    parse_both_equivalent(
        r#"! @mcp.serve "s" ⟦ tool "add" "Adds" |a: int| ⟦ ^ a ⟧ ⟧"#,
        r#"! host.mcp.serve "s" [[ tool "add" "Adds" |a: int| [[ return a ]] ]]"#,
    )
    .unwrap();
}
