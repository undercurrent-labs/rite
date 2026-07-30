use rite_analysis::AnalysisEngine;

#[test]
fn analyze_collects_functions_and_diags() {
    let mut eng = AnalysisEngine::new();
    let snap = eng.analyze(
        "t.rite",
        r#"
◆ add(a, b) ⟦
  ^ a + b
⟧
undefined_name
"#,
    );
    assert!(snap.symbols.iter().any(|s| s.name == "add"));
    assert!(snap.has_errors || !snap.diagnostics.is_empty());
}

#[test]
fn completions_include_builtins_and_functions() {
    let eng = AnalysisEngine::new();
    let items = eng.completions("◆ foo() ⟦ ^ 1 ⟧\n", 1, 0);
    assert!(items
        .iter()
        .any(|i| i.label == "map" || i.label == "@console"));
    assert!(items.iter().any(|i| i.label == "foo"));
}

#[test]
fn hover_capability() {
    let eng = AnalysisEngine::new();
    let h = eng.hover("@console.println(\"x\")", 1, 2);
    assert!(h.is_some());
    let h = h.unwrap();
    assert!(h.markdown.to_lowercase().contains("console") || h.title.contains("console"));
}

/// The analysis layer used to collect only functions, which is why an editor outline
/// showed nothing else and labelled everything `FUNCTION`.
#[test]
fn declared_symbols_cover_every_declaration_kind() {
    let src = "\
◆ Cfg ⟨a: 1, b: 2⟩
limit ← 10
counter ↢ 0
◆ helper(x) ⟦ ^ x ⟧
";
    let (program, diags, _) = rite_syntax::parse_source("s.rite", src);
    assert!(!diags.has_errors(), "{:#?}", diags.into_vec());
    let syms = rite_analysis::declared_symbols(&program.expect("parse"));
    let by_name: std::collections::HashMap<_, _> =
        syms.iter().map(|s| (s.name.as_str(), s.kind)).collect();

    assert_eq!(by_name.get("helper"), Some(&"function"));
    assert_eq!(by_name.get("Cfg"), Some(&"constant"), "data declaration");
    assert_eq!(by_name.get("limit"), Some(&"constant"), "← binding");
    assert_eq!(by_name.get("counter"), Some(&"variable"), "↢ binding");
    assert_eq!(syms.len(), 4, "unexpected symbols: {syms:#?}");
}

#[test]
fn declared_symbols_point_at_the_name_not_the_keyword() {
    let src = "◆ square(n) ⟦ ^ n * n ⟧\n";
    let (program, _, _) = rite_syntax::parse_source("s.rite", src);
    let syms = rite_analysis::declared_symbols(&program.expect("parse"));
    let start = syms[0].span.start.as_usize();
    assert_eq!(
        &src[start..start + "square".len()],
        "square",
        "span should cover the identifier"
    );
}

#[test]
fn workspace_symbols_include_non_functions() {
    let mut ws = rite_analysis::WorkspaceIndex::new(vec![]);
    ws.upsert_document("file:///tmp/w.rite", "limit ← 3\n◆ f() ⟦ ^ 1 ⟧\n");
    let names: Vec<_> = ws
        .workspace_symbols("")
        .into_iter()
        .map(|s| (s.name, s.kind))
        .collect();
    assert!(
        names.iter().any(|(n, k)| n == "limit" && k == "constant"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|(n, k)| n == "f" && k == "function"),
        "{names:?}"
    );
}
