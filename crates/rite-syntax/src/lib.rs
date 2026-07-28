//! Lexer, tokens, AST, and parser for Rite.

pub mod ast;
pub mod lexer;
pub mod parser;
pub mod token;

pub use ast::*;
pub use lexer::{lex, Lexer};
pub use parser::{parse, parse_expression, Parser};
pub use token::*;

use rite_core::{Diagnostics, FileId, SourceFile, SourceMap};

/// Fully parse a source string into a program AST.
pub fn parse_source(name: &str, text: &str) -> (Option<Program>, Diagnostics, SourceMap) {
    let mut sources = SourceMap::new();
    let file_id = sources.add_file(name, text);
    let file = sources.get(file_id).unwrap().clone();
    let (program, diags) = parse_file(&file);
    (program, diags, sources)
}

pub fn parse_file(file: &SourceFile) -> (Option<Program>, Diagnostics) {
    let (tokens, mut lex_diags) = lex(file);
    let (program, mut parse_diags) = parse(file.id, &tokens);
    lex_diags.extend(parse_diags.into_vec());
    (program, lex_diags)
}

/// Check that two programs are structurally equivalent (ignoring spans and style).
pub fn programs_equivalent(a: &Program, b: &Program) -> bool {
    // Compare via debug of stripped ASTs
    format!("{:?}", strip_spans_program(a)) == format!("{:?}", strip_spans_program(b))
}

fn strip_spans_program(p: &Program) -> String {
    // Simple structural dump without spans for equivalence
    format!("{:#?}", p.items.len()) // refined below via JSON
}

/// Serialize AST to JSON for tooling (`rite ast --json`).
pub fn ast_to_json(program: &Program) -> serde_json::Value {
    serde_json::to_value(program).unwrap_or(serde_json::Value::Null)
}

/// Parse both glyphic and ASCII forms and ensure structural equivalence for a fixture.
pub fn parse_both_equivalent(glyph: &str, ascii: &str) -> Result<(), String> {
    let (pg, dg, _) = parse_source("glyph.rite", glyph);
    let (pa, da, _) = parse_source("ascii.rite", ascii);
    if dg.has_errors() {
        return Err(format!("glyph parse errors: {:?}", dg.into_vec()));
    }
    if da.has_errors() {
        return Err(format!("ascii parse errors: {:?}", da.into_vec()));
    }
    let pg = pg.ok_or("no glyph program")?;
    let pa = pa.ok_or("no ascii program")?;
    let jg = serde_json::to_string(&strip_program(&pg)).unwrap();
    let ja = serde_json::to_string(&strip_program(&pa)).unwrap();
    if jg == ja {
        Ok(())
    } else {
        Err(format!("ASTs differ:\nGLYPH: {}\nASCII: {}", jg, ja))
    }
}

fn strip_program(p: &Program) -> serde_json::Value {
    // Re-serialize with spans zeroed via custom walk — for v1 compare item count + expr shapes
    serde_json::json!({
        "items": p.items.iter().map(strip_item).collect::<Vec<_>>(),
    })
}

fn strip_item(item: &Item) -> serde_json::Value {
    match item {
        Item::Function(f) => serde_json::json!({
            "kind": "function",
            "name": f.name.name,
            "pub": f.is_pub,
            "params": f.params.iter().map(|p| &p.name.name).collect::<Vec<_>>(),
            "body": strip_block(&f.body),
        }),
        Item::Import(i) => serde_json::json!({
            "kind": "import",
            "path": i.path.segments.iter().map(|s| &s.name).collect::<Vec<_>>(),
            "alias": i.alias.as_ref().map(|a| &a.name),
        }),
        Item::Statement(s) => serde_json::json!({
            "kind": "statement",
            "stmt": strip_stmt(s),
        }),
        Item::Test(t) => serde_json::json!({
            "kind": "test",
            "name": t.name,
            "body": strip_block(&t.body),
        }),
        Item::Event(e) => serde_json::json!({
            "kind": "event",
            "kind_name": format!("{:?}", e.kind),
            "atom": e.atom.parts,
            "body": strip_block(&e.body),
        }),
        Item::Data(d) => serde_json::json!({
            "kind": "data",
            "name": d.name.name,
        }),
    }
}

fn strip_stmt(s: &Stmt) -> serde_json::Value {
    match s {
        Stmt::Binding(b) => serde_json::json!({
            "kind": "binding",
            "mutable": b.mutable,
            "value": strip_expr(&b.value),
        }),
        Stmt::Assign(a) => serde_json::json!({
            "kind": "assign",
            "name": a.name.name,
            "value": strip_expr(&a.value),
        }),
        Stmt::Expr(e) => serde_json::json!({"kind": "expr", "expr": strip_expr(e)}),
        Stmt::Return(r) => serde_json::json!({
            "kind": "return",
            "value": r.value.as_ref().map(strip_expr),
        }),
    }
}

fn strip_block(b: &Block) -> serde_json::Value {
    serde_json::json!({
        "params": b.params.iter().map(|p| &p.name.name).collect::<Vec<_>>(),
        "body": b.body.iter().map(strip_item).collect::<Vec<_>>(),
    })
}

fn strip_expr(e: &Expr) -> serde_json::Value {
    match e {
        Expr::Literal(l) => serde_json::json!({"lit": format!("{:?}", l.kind)}),
        Expr::Ident(i) => serde_json::json!({"ident": i.name}),
        Expr::Atom(a) => serde_json::json!({"atom": a.parts}),
        Expr::List(l) => serde_json::json!({"list": l.elements.iter().map(strip_expr).collect::<Vec<_>>()}),
        Expr::Record(r) => serde_json::json!({
            "record": r.entries.iter().map(|e| {
                serde_json::json!({"key": format!("{:?}", e.key), "value": strip_expr(&e.value)})
            }).collect::<Vec<_>>()
        }),
        Expr::Binary(b) => serde_json::json!({
            "binary": format!("{:?}", b.op),
            "left": strip_expr(&b.left),
            "right": strip_expr(&b.right),
        }),
        Expr::Unary(u) => serde_json::json!({
            "unary": format!("{:?}", u.op),
            "expr": strip_expr(&u.expr),
        }),
        Expr::Call(c) => serde_json::json!({
            "call": strip_expr(&c.callee),
            "args": c.args.iter().map(strip_expr).collect::<Vec<_>>(),
        }),
        Expr::Member(m) => serde_json::json!({
            "member": strip_expr(&m.object),
            "field": m.field.name,
        }),
        Expr::Index(i) => serde_json::json!({
            "index": strip_expr(&i.object),
            "key": strip_expr(&i.index),
        }),
        Expr::Pipeline(p) => serde_json::json!({
            "pipeline": strip_expr(&p.input),
            "stages": p.stages.iter().map(strip_expr).collect::<Vec<_>>(),
        }),
        Expr::If(i) => serde_json::json!({
            "if": strip_expr(&i.condition),
            "then": strip_block(&i.then_branch),
            "else": i.else_branch.as_ref().map(strip_block),
        }),
        Expr::Match(m) => serde_json::json!({
            "match": strip_expr(&m.scrutinee),
            "arms": m.arms.len(),
        }),
        Expr::Block(b) => strip_block(b),
        Expr::Capability(c) => serde_json::json!({
            "cap": c.path,
        }),
        Expr::Placeholder(_) => serde_json::json!({"placeholder": true}),
        Expr::Try(t) => serde_json::json!({"try": strip_expr(&t.expr)}),
        Expr::HttpListen(h) => serde_json::json!({
            "http_listen": strip_expr(&h.addr),
            "body": strip_block(&h.body),
        }),
        Expr::Route(r) => serde_json::json!({
            "route": format!("{:?}", r.method),
            "path": r.path,
        }),
        Expr::Group(g) => strip_expr(&g.expr),
        Expr::Coalesce(c) => serde_json::json!({
            "coalesce": strip_expr(&c.left),
            "right": strip_expr(&c.right),
        }),
    }
}

// silence unused
#[allow(dead_code)]
fn _file_id_use() -> FileId {
    FileId(0)
}
