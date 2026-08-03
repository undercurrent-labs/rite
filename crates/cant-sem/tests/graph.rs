//! Graph construction, validation, and the two export formats.
//!
//! The specification's §7.1 list is the spine of this file: each check gets a
//! test that trips it and one that does not, because a validator nobody has seen
//! reject anything is a validator nobody knows is wired up.

use cant_sem::{analyze, to_dot, validate_deserialized, CantProgram, EdgeRole, NodeKind};
use cant_syntax::parse_source;
use rite_core::FileId;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-sem has two ancestors")
        .to_path_buf()
}

fn analysis(source: &str) -> cant_sem::Analysis {
    let (parsed, _) = parse_source("t.cant", source);
    let ast = parsed.program.expect("a program");
    analyze(&ast, FileId(0), "t.cant", source.len())
}

fn graph(source: &str) -> CantProgram {
    let (parsed, sources) = parse_source("t.cant", source);
    assert!(
        !parsed.has_errors(),
        "{source:?} should parse:\n{}",
        parsed.diagnostics.render_all(&sources)
    );
    cant_sem::lower(&parsed.program.expect("program"), "t.cant", source.len())
}

fn codes(source: &str) -> Vec<String> {
    analysis(source)
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}

// ---- clean programs stay clean

#[test]
fn every_valid_fixture_produces_a_graph_with_no_complaints() {
    for dir in ["conformance/cant/syntax", "examples/cant"] {
        let root = repo_root().join(dir);
        for entry in std::fs::read_dir(&root).expect("fixture directory") {
            let case = entry.expect("entry").path();
            if !case.is_dir() {
                continue;
            }
            for name in ["case.cant", "main.cant"] {
                let path = case.join(name);
                if !path.is_file() {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("fixture");
                let (parsed, _) = parse_source(&path.display().to_string(), &source);
                let Some(ast) = parsed.program else { continue };
                let result = analyze(&ast, FileId(0), "case.cant", source.len());
                assert!(
                    !result.diagnostics.has_errors(),
                    "{} should validate, got {:?}",
                    case.display(),
                    result
                        .diagnostics
                        .iter()
                        .map(|d| format!("{}: {}", d.code, d.title))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

/// The bug this test exists for: a fork's enter/join pair looked like an illegal
/// cycle, so **every** program containing a fork was rejected.
#[test]
fn a_fork_is_not_a_cycle() {
    assert!(codes("x -> |{ a ; b }").is_empty());
    assert!(codes("x -> |{ ?{ $.ok } -> handle ; ~{ c } :max 4 } -> []").is_empty());
}

#[test]
fn an_orbit_is_not_a_cycle_either() {
    assert!(codes("roots -> ~{ deps -> * } :max 8 -> []").is_empty());
}

// ---- §7.1, one at a time

#[test]
fn scatter_or_collect_as_the_first_stage_is_rejected() {
    assert_eq!(codes("* -> f"), vec!["CANT-G005"]);
    // ASCII `[]` opening a program is the empty list, so only the glyph reaches
    // this check — which is the asymmetry the parser documents.
    assert_eq!(codes("⌁ -> f"), vec!["CANT-G006"]);
    assert!(codes("[] -> length").is_empty(), "`[]` here is a literal");
}

#[test]
fn an_orbit_limit_that_is_not_a_positive_integer_is_rejected() {
    for source in [
        "r -> ~{ d } :max 0",
        "r -> ~{ d } :max eight",
        "r -> ~{ d } :max -3",
        "r -> ~{ d } :max 1.5",
    ] {
        assert!(
            codes(source).contains(&"CANT-G007".to_string()),
            "{source:?} gave {:?}",
            codes(source)
        );
    }
    assert!(codes("r -> ~{ d } :max 1").is_empty());
}

#[test]
fn an_effectful_orbit_identity_is_rejected() {
    let found = codes("r -> ~{ d } :by !@fs.read");
    assert!(found.contains(&"CANT-G008".to_string()), "{found:?}");
    assert!(codes("r -> ~{ d } :by canonical").is_empty());
}

#[test]
fn an_effectful_ward_predicate_is_rejected() {
    let found = codes("rows -> ?{ !@fs.exists($) }");
    assert!(found.contains(&"CANT-G014".to_string()), "{found:?}");
    assert!(codes("rows -> ?{ $ > 0 }").is_empty());
}

#[test]
fn a_modifier_the_form_does_not_take_is_rejected() {
    assert!(codes("r -> ~{ d } :nonsense 1").contains(&"CANT-G010".to_string()));
    // A ward takes none at all, and the message should say so rather than
    // suggest an orbit's.
    let found = analysis("r -> ?{ $ } :max 4");
    let diagnostic = found
        .diagnostics
        .iter()
        .find(|d| d.code.to_string() == "CANT-G010")
        .expect("unknown modifier");
    assert!(
        diagnostic
            .help
            .as_deref()
            .unwrap_or("")
            .contains("no modifier applies"),
        "{:?}",
        diagnostic.help
    );
}

#[test]
fn a_repeated_modifier_is_reported() {
    assert!(codes("r -> ~{ d } :max 4 :max 8").contains(&"CANT-G011".to_string()));
}

#[test]
fn an_empty_orbit_body_is_rejected() {
    // `~{ }` parses (the parser reports nothing for an empty orbit body) and the
    // graph has to be the thing that refuses it.
    let (parsed, _) = parse_source("t.cant", "r -> ~{ }");
    let ast = parsed.program.expect("program");
    let result = analyze(&ast, FileId(0), "t.cant", 9);
    let found: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.code.to_string())
        .collect();
    assert!(found.contains(&"CANT-G015".to_string()), "{found:?}");
}

// ---- deserialized graphs are untrusted

fn json_of(source: &str) -> serde_json::Value {
    graph(source).to_json()
}

fn diagnose_json(value: &serde_json::Value) -> Vec<String> {
    let text = serde_json::to_string(value).expect("json");
    match validate_deserialized(&text, FileId(0)) {
        Ok(analysis) => analysis
            .diagnostics
            .iter()
            .map(|d| d.code.to_string())
            .collect(),
        Err(e) => vec![format!("parse: {e}")],
    }
}

#[test]
fn a_graph_round_trips_through_json_unchanged() {
    let before = graph("roots -> * -> |{ a ; b } -> ~{ c } :max 8 -> []");
    let text = serde_json::to_string(&before).expect("serialize");
    let after: CantProgram = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(before, after);
    assert!(diagnose_json(&before.to_json()).is_empty());
}

#[test]
fn a_graph_from_a_future_schema_is_refused_rather_than_misread() {
    let mut value = json_of("a -> b");
    value["version"] = serde_json::json!("99");
    let text = serde_json::to_string(&value).expect("json");
    let err = validate_deserialized(&text, FileId(0)).expect_err("version mismatch");
    assert!(err.contains("99"), "{err}");
}

/// The specification's requirement: a fuzzed graph must not be able to smuggle
/// in a cycle nothing bounds.
#[test]
fn a_hand_written_cycle_is_rejected() {
    let mut value = json_of("a -> b -> c");
    // Route the last node back to the first along an ordinary flow edge.
    value["edges"]
        .as_array_mut()
        .expect("edges")
        .push(serde_json::json!({
            "from": {"node": 2, "kind": "out", "index": 0},
            "to": {"node": 0, "kind": "in", "index": 0},
            "ordinal": 0,
            "role": "flow"
        }));
    assert!(
        diagnose_json(&value).contains(&"CANT-G009".to_string()),
        "{:?}",
        diagnose_json(&value)
    );
}

/// Relabelling a cycle as orbit feedback must not launder it: the edge has to
/// actually belong to an orbit.
#[test]
fn a_cycle_disguised_as_orbit_feedback_is_still_caught() {
    let mut value = json_of("a -> b -> c");
    value["edges"]
        .as_array_mut()
        .expect("edges")
        .push(serde_json::json!({
            "from": {"node": 2, "kind": "out", "index": 0},
            "to": {"node": 0, "kind": "in", "index": 1},
            "ordinal": 0,
            "role": "orbit_feedback"
        }));
    let found = diagnose_json(&value);
    // `n0` is a Source: it has one input port, so port 1 does not exist. The
    // label buys nothing because the *shape* is still wrong.
    assert!(found.contains(&"CANT-G003".to_string()), "{found:?}");
}

#[test]
fn a_dangling_edge_is_rejected() {
    let mut value = json_of("a -> b");
    value["edges"]
        .as_array_mut()
        .expect("edges")
        .push(serde_json::json!({
            "from": {"node": 1, "kind": "out", "index": 0},
            "to": {"node": 99, "kind": "in", "index": 0},
            "ordinal": 0,
            "role": "flow"
        }));
    assert!(diagnose_json(&value).contains(&"CANT-G002".to_string()));
}

#[test]
fn a_port_a_node_does_not_have_is_rejected() {
    let mut value = json_of("a -> b");
    value["edges"][0]["to"]["index"] = serde_json::json!(7);
    assert!(diagnose_json(&value).contains(&"CANT-G003".to_string()));
}

#[test]
fn an_edge_running_backwards_between_port_kinds_is_rejected() {
    let mut value = json_of("a -> b");
    value["edges"][0]["from"]["kind"] = serde_json::json!("in");
    assert!(diagnose_json(&value).contains(&"CANT-G003".to_string()));
}

#[test]
fn duplicate_node_identifiers_are_rejected() {
    let mut value = json_of("a -> b");
    let nodes = value["nodes"].as_array_mut().expect("nodes");
    let mut clone = nodes[1].clone();
    clone["id"] = serde_json::json!(0);
    nodes.push(clone);
    assert!(diagnose_json(&value).contains(&"CANT-G012".to_string()));
}

#[test]
fn an_unreachable_node_is_a_warning_not_an_error() {
    let mut value = json_of("a -> b");
    // Drop the only edge; `n1` is now unreachable.
    value["edges"] = serde_json::json!([]);
    let text = serde_json::to_string(&value).expect("json");
    let analysis = validate_deserialized(&text, FileId(0)).expect("valid schema");
    assert!(!analysis.diagnostics.has_errors(), "should be a warning");
    assert!(analysis
        .diagnostics
        .iter()
        .any(|d| d.code.to_string() == "CANT-G013"));
}

#[test]
fn a_missing_entry_is_rejected() {
    let mut value = json_of("a -> b");
    value["entry"] = serde_json::json!(42);
    assert!(diagnose_json(&value).contains(&"CANT-G001".to_string()));
}

#[test]
fn a_branch_that_does_not_rejoin_its_fork_is_rejected() {
    let mut value = json_of("x -> |{ a ; b }");
    // Remove the join edges, leaving branches that flow nowhere.
    let edges = value["edges"].as_array_mut().expect("edges");
    edges.retain(|e| e["role"] != serde_json::json!("join"));
    assert!(diagnose_json(&value).contains(&"CANT-G004".to_string()));
}

/// Deeply nested JSON must not overflow the stack — the validator walks with an
/// explicit stack for exactly this.
#[test]
fn a_very_long_chain_from_json_is_validated_without_recursing() {
    let source = (0..2000)
        .map(|i| format!("f{i}"))
        .collect::<Vec<_>>()
        .join(" -> ");
    let value = json_of(&source);
    assert!(diagnose_json(&value).is_empty());
}

// ---- exports

#[test]
fn json_and_dot_are_deterministic() {
    let source = "roots -> * -> |{ a ; b -> c } -> ~{ d -> * } :by k :max 8 -> []";
    assert_eq!(json_of(source), json_of(source));
    assert_eq!(to_dot(&graph(source)), to_dot(&graph(source)));
}

#[test]
fn the_graph_carries_what_a_renderer_and_an_explainer_need() {
    let g = graph("\"p\" -> !@fs.read -> ~{ deps -> * } :by canonical :max 4096 -> []");
    assert_eq!(g.capabilities(), vec!["@fs.read"]);
    assert_eq!(g.effectful_nodes().len(), 1);
    assert_eq!(g.max_orbit_items(), Some(4096));
    assert!(
        g.nodes.iter().all(|n| n.layout.is_none()),
        "layout is opt-in"
    );
    // Every node keeps its span, which is what a diagnostic and a click-through
    // both need.
    assert!(g.nodes.iter().all(|n| !n.span.is_dummy()));
}

/// Layout is reserved and non-semantic: stripping it must change nothing that
/// validation or lowering can observe.
#[test]
fn layout_hints_do_not_affect_anything() {
    let mut with_layout = graph("a -> b -> c");
    for (i, node) in with_layout.nodes.iter_mut().enumerate() {
        node.layout = Some(cant_sem::LayoutHint {
            x: i as f32 * 10.0,
            y: 0.0,
            width: Some(80.0),
            height: None,
        });
        node.label = Some(format!("hand-written {i}"));
    }
    let stripped = graph("a -> b -> c");
    assert_eq!(
        diagnose_json(&with_layout.to_json()),
        diagnose_json(&stripped.to_json())
    );
    // And it survives a round trip, so a renderer's work is not silently lost.
    let text = serde_json::to_string(&with_layout).expect("serialize");
    let back: CantProgram = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, with_layout);
}

#[test]
fn ascii_and_glyph_sources_produce_identical_graphs() {
    for case in std::fs::read_dir(repo_root().join("conformance/cant/dialect")).expect("dialect") {
        let dir = case.expect("entry").path();
        let ascii = std::fs::read_to_string(dir.join("ascii.cant")).expect("ascii");
        let glyph = std::fs::read_to_string(dir.join("glyph.cant")).expect("glyph");
        let a = graph(&ascii);
        let g = graph(&glyph);
        // Spans differ between the two spellings; everything else must not.
        assert_eq!(a.nodes.len(), g.nodes.len(), "{}", dir.display());
        assert_eq!(a.edges.len(), g.edges.len(), "{}", dir.display());
        for (an, gn) in a.nodes.iter().zip(&g.nodes) {
            assert_eq!(an.id, gn.id, "{}", dir.display());
            assert_eq!(
                std::mem::discriminant(&an.kind),
                std::mem::discriminant(&gn.kind),
                "{}",
                dir.display()
            );
            assert_eq!(
                an.kind.leaf().map(|l| &l.text),
                gn.kind.leaf().map(|l| &l.text),
                "{}",
                dir.display()
            );
        }
        for (ae, ge) in a.edges.iter().zip(&g.edges) {
            assert_eq!(ae, ge, "{}", dir.display());
        }
    }
}

#[test]
fn node_kinds_report_the_ports_their_edges_use() {
    let g = graph("x -> |{ a ; b ; c }");
    for edge in &g.edges {
        let from = g.node(edge.from.node).expect("from node");
        let to = g.node(edge.to.node).expect("to node");
        assert!(
            edge.from.index < from.kind.out_ports(),
            "{:?} out port {} of {}",
            from.kind.name(),
            edge.from.index,
            from.kind.out_ports()
        );
        assert!(edge.to.index < to.kind.in_ports());
    }
}

#[test]
fn a_forks_branch_order_survives_sorting_the_edges() {
    let g = graph("x -> |{ first ; second ; third }");
    let mut enters: Vec<_> = g
        .edges
        .iter()
        .filter(|e| e.role == EdgeRole::Enter)
        .collect();
    enters.sort_by_key(|e| e.to.node.0);
    let names: Vec<_> = enters
        .iter()
        .map(|e| {
            let node = g.node(e.to.node).expect("branch entry");
            match &node.kind {
                NodeKind::Stage { expr } => expr.text.clone(),
                other => other.name().to_string(),
            }
        })
        .collect();
    assert_eq!(names, vec!["first", "second", "third"]);
    // The ordinal is the authority, not the position in the list.
    let ordinals: Vec<_> = enters.iter().map(|e| e.ordinal).collect();
    assert_eq!(ordinals, vec![0, 1, 2]);
}
