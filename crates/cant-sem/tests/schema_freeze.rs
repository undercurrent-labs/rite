//! The graph JSON schema is frozen at version 0.
//!
//! Frozen meaning: `docs/cant/graph-schema.md` is the contract, and a change to
//! the shape must bump `version` and be a deliberate act. These tests are what
//! make that true rather than aspirational — they fail on any field added,
//! removed or renamed, and the fix is either to revert or to bump.
//!
//! Two separate obligations, because they fail differently:
//!
//! * **The shape** — a golden document, so a diff shows exactly what changed.
//! * **The documentation** — every field name the schema emits must appear in
//!   the document that claims to describe it. A schema whose doc has drifted is
//!   worse than an undocumented one, because it is trusted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cant_sem::validate_deserialized;
use cant_syntax::parse_source;
use rite_core::FileId;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-sem has two ancestors")
        .to_path_buf()
}

/// A program using every construct, so the golden covers every node kind, every
/// edge role, and both port conventions.
const EVERYTHING: &str =
    "[1, 2] -> * -> |{ ?{ $ > 1 } ; ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 64 } -> []";

fn graph_json() -> serde_json::Value {
    let (parsed, sources) = parse_source("schema.cant", EVERYTHING);
    assert!(
        !parsed.has_errors(),
        "{}",
        parsed.diagnostics.render_all(&sources)
    );
    cant_sem::lower(
        &parsed.program.expect("program"),
        "schema.cant",
        EVERYTHING.len(),
    )
    .to_json()
}

#[test]
fn the_schema_version_is_zero() {
    assert_eq!(cant_sem::GRAPH_SCHEMA_VERSION, "0");
    assert_eq!(graph_json()["version"], serde_json::json!("0"));
}

/// Every key the schema emits, anywhere in the document, flattened.
fn keys(value: &serde_json::Value, into: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                into.insert(key.clone());
                keys(child, into);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| keys(i, into)),
        _ => {}
    }
}

/// The frozen key set. Adding to this is a schema change.
const FROZEN_KEYS: &[&str] = &[
    // top level
    "version",
    "language_version",
    "entry",
    "exit",
    "nodes",
    "edges",
    "subgraphs",
    "source",
    // source info
    "name",
    "length",
    // nodes
    "id",
    "kind",
    "span",
    "subgraph",
    // node payloads
    "expr",
    "predicate",
    "branches",
    "body",
    "identity",
    "max_items",
    // leaves
    "text",
    "effectful",
    "placeholder",
    // spans
    "start",
    "end",
    // edges
    "from",
    "to",
    "ordinal",
    "role",
    "node",
    "index",
    // subgraphs
    "owner",
];

#[test]
fn the_key_set_is_frozen() {
    let mut found = BTreeSet::new();
    keys(&graph_json(), &mut found);
    let frozen: BTreeSet<String> = FROZEN_KEYS.iter().map(|k| k.to_string()).collect();

    let added: Vec<_> = found.difference(&frozen).collect();
    let removed: Vec<_> = frozen.difference(&found).collect();
    assert!(
        added.is_empty() && removed.is_empty(),
        "the graph schema changed — bump `GRAPH_SCHEMA_VERSION` and update \
         docs/cant/graph-schema.md, or revert.\n  added: {added:?}\n  removed: {removed:?}"
    );
}

/// `label` and `layout` are reserved. They are absent from a lowered graph, and
/// must be accepted on the way back in.
#[test]
fn the_reserved_keys_are_absent_but_accepted() {
    let mut json = graph_json();
    let mut found = BTreeSet::new();
    keys(&json, &mut found);
    assert!(!found.contains("label"), "`label` should not be emitted");
    assert!(!found.contains("layout"), "`layout` should not be emitted");

    // And a graph carrying them round-trips and validates identically.
    let node = &mut json["nodes"][0];
    node["label"] = serde_json::json!("hand-written");
    node["layout"] = serde_json::json!({"x": 1.0, "y": 2.0, "width": 3.0});
    let text = serde_json::to_string(&json).expect("json");
    let analysis = validate_deserialized(&text, FileId(0)).expect("still a valid graph");
    assert!(!analysis.diagnostics.has_errors());
    assert_eq!(
        analysis.graph.nodes[0].label.as_deref(),
        Some("hand-written")
    );
}

#[test]
fn every_node_kind_and_edge_role_is_named_in_the_document() {
    let doc = std::fs::read_to_string(repo_root().join("docs/cant/graph-schema.md"))
        .expect("docs/cant/graph-schema.md");
    for kind in [
        "source", "stage", "scatter", "collect", "ward", "fork", "orbit",
    ] {
        assert!(doc.contains(&format!("`{kind}`")), "`{kind}` undocumented");
    }
    for role in ["flow", "enter", "join", "orbit_feedback"] {
        assert!(doc.contains(&format!("`{role}`")), "`{role}` undocumented");
    }
}

/// Every emitted key appears in the document. A schema whose documentation has
/// drifted is worse than an undocumented one, because it is trusted.
#[test]
fn every_emitted_key_is_documented() {
    let doc = std::fs::read_to_string(repo_root().join("docs/cant/graph-schema.md"))
        .expect("docs/cant/graph-schema.md");
    let mut found = BTreeSet::new();
    keys(&graph_json(), &mut found);
    let missing: Vec<_> = found.iter().filter(|k| !doc.contains(*k)).collect();
    assert!(
        missing.is_empty(),
        "keys the schema emits but the document never mentions: {missing:?}"
    );
}

/// The document must state the stability, because "version 0" alone does not
/// tell a reader whether it is safe to store one.
#[test]
fn the_document_says_it_is_experimental() {
    let doc = std::fs::read_to_string(repo_root().join("docs/cant/graph-schema.md"))
        .expect("docs/cant/graph-schema.md");
    assert!(doc.contains("experimental"), "stability unstated");
    assert!(
        doc.contains("never has to parse Cant source"),
        "the consumer contract is the point of the seam and must be stated"
    );
}

/// A stored graph from a different schema version is refused, not guessed at.
#[test]
fn a_foreign_schema_version_is_refused() {
    let mut json = graph_json();
    json["version"] = serde_json::json!("1");
    let text = serde_json::to_string(&json).expect("json");
    let err = validate_deserialized(&text, FileId(0)).expect_err("version mismatch");
    assert!(err.contains('1') && err.contains('0'), "{err}");
}

/// The seam's actual claim: everything a renderer needs is in the JSON.
#[test]
fn a_consumer_never_needs_the_source_text() {
    let json = graph_json();
    let text = serde_json::to_string(&json).expect("json");
    let analysis = validate_deserialized(&text, FileId(0)).expect("valid");
    let graph = &analysis.graph;

    // Structure, without touching the `.cant`.
    assert!(!graph.nodes.is_empty());
    assert!(!graph.edges.is_empty());
    // Two fork branches plus the orbit body inside the second of them.
    assert_eq!(graph.subgraphs.len(), 3);
    // Order — and the reason the document insists `ordinal` is the authority.
    // The raw enter edges come out `[0, 0, 1]`: the fork's edge into branch 0,
    // then the *orbit's* edge into its body (emitted while lowering branch 1),
    // then the fork's edge into branch 1. Array position says nothing; the
    // ordinal, read per owning node, says everything.
    let fork = graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, cant_sem::NodeKind::Fork { .. }))
        .expect("a fork");
    let branch_order: Vec<_> = graph
        .edges
        .iter()
        .filter(|e| e.role == cant_sem::EdgeRole::Enter && e.from.node == fork.id)
        .map(|e| e.ordinal)
        .collect();
    assert_eq!(
        branch_order,
        vec![0, 1],
        "the fork's two branches, in order"
    );
    // Sorting the edge list must not change that.
    let mut sorted = graph.edges.clone();
    sorted.sort_by_key(|e| (e.to.node.0, e.from.node.0));
    let after_sorting: Vec<_> = sorted
        .iter()
        .filter(|e| e.role == cant_sem::EdgeRole::Enter && e.from.node == fork.id)
        .map(|e| e.ordinal)
        .collect();
    assert_eq!(after_sorting, branch_order);
    // Policy, effects, spans.
    assert_eq!(graph.max_orbit_items(), Some(64));
    assert!(graph.nodes.iter().all(|n| !n.span.is_dummy()));
    // And the leaf text, so a renderer can label a node without the file.
    assert!(graph
        .nodes
        .iter()
        .filter_map(|n| n.kind.leaf())
        .any(|l| l.text == "$ > 1"));
}
