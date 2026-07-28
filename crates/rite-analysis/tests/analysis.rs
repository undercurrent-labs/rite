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
