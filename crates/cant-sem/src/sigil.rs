//! Adapting a Cant graph into a normalized Sigil graph.
//!
//! This lives here, on the Cant side, because it is the only place allowed to
//! know both shapes: ADR 0001 fixes the dependency edge as `cant-* -> rite-*`,
//! and ADR 0006 forbids `rite-sigil` from knowing what Cant is. A renderer that
//! took `&CantProgram` would drag this crate's parser into every build that
//! wanted to draw a picture from a JSON file.
//!
//! # What the adapter has to decide
//!
//! Seven Cant node kinds map straight across. Three Sigil kinds have no Cant
//! counterpart, and inventing them is most of what this module does:
//!
//! * **`Effect`** — Cant records an effect as a `!` on a leaf plus the
//!   capabilities that leaf names. Sigil needs an *invocation* to place on the
//!   outer boundary, so a stage whose leaf carries `!` becomes one. The
//!   capabilities come from `Node::capabilities`, a field added in `cant.graph`
//!   version 1 precisely so this decision is not a text scan.
//! * **`Output`** — Cant's `exit` points at whatever kind happens to be last.
//!   Sigil needs a closing seal, so the exit node is promoted — *unless* it is a
//!   collect, which is already a seal and keeps its own mark. Promoting a
//!   collect would erase the distinction between "the values came together" and
//!   "the program ended", which are different things that happen to coincide.
//! * **`Unknown`** — unreachable from a live `CantProgram`, whose `NodeKind` is
//!   a closed enum. It exists for a graph deserialized from a *newer* `cant`,
//!   and that path goes through Sigil's own JSON reader rather than here.
//!
//! # What it must not do
//!
//! Nothing here reads leaf text to decide what a node *means*. Leaf text becomes
//! a label, something to draw or withhold, and capability metadata comes from
//! the field. That separation is what the version 1 graph added.

use rite_sigil::{
    Capability, CapabilityFamily, EdgeId, EdgeKind, EffectMetadata, GraphMetadata, NodeId, PortRef,
    RegionId, RegionKind, SigilEdge, SigilGraph, SigilNode, SigilNodeKind, SigilRegion,
    SourceLanguage, SourceRef, SourceSchema,
};

use crate::graph::{CantProgram, EdgeRole, LeafExpr, Node, NodeKind, Subgraph};

/// How much a Cant node's own text to carry into the normalized graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdaptOptions {
    /// Carry leaf text as node labels and source snippets.
    ///
    /// Off by default, and the default is the point: leaf text is the user's
    /// source. A normalized graph that carried it unconditionally would leak it
    /// into every debug dump and every scene JSON export, and Veiled mode would
    /// be a filter applied late rather than a thing that was never there
    /// (ADR 0007). `cant sigil --mode revealed` turns it on.
    pub include_labels: bool,
}

impl AdaptOptions {
    pub fn with_labels() -> Self {
        AdaptOptions {
            include_labels: true,
        }
    }
}

/// Adapt a validated Cant graph into the renderer's input model.
///
/// Infallible. Everything that could fail — dangling edges, duplicate
/// identifiers, cycles that are not orbit feedback — is `cant_sem::validate`'s
/// job, and Sigil validates again on its own terms afterwards. An adapter that
/// also validated would be a third place for the rules to drift.
pub fn to_sigil_graph(program: &CantProgram, options: AdaptOptions) -> SigilGraph {
    let mut graph = SigilGraph::new(SourceLanguage::Cant, node_id(program.entry));

    graph.source_schema = Some(SourceSchema {
        name: crate::GRAPH_SCHEMA_NAME.to_string(),
        version: program.version.clone(),
    });
    graph.metadata = GraphMetadata {
        source_name: Some(program.source.name.clone()),
        source_length: Some(program.source.length),
        producer: Some(program.producer.name.clone()),
        producer_version: Some(program.producer.version.clone()),
        extra: Default::default(),
    };

    // The exit is a seal unless it is already one. A collect keeps its knot;
    // anything else is promoted to `Output`, because "the program ended" needs a
    // mark whatever the last stage happened to be.
    let exit_is_its_own_seal = program
        .node(program.exit)
        .is_some_and(|n| matches!(n.kind, NodeKind::Collect));

    for node in &program.nodes {
        graph
            .nodes
            .push(adapt_node(node, program, exit_is_its_own_seal, options));
    }
    graph.exits.push(node_id(program.exit));

    for (index, edge) in program.edges.iter().enumerate() {
        graph.edges.push(SigilEdge {
            id: edge_id(index, edge),
            from: PortRef::new(node_id(edge.from.node), edge.from.index),
            to: PortRef::new(node_id(edge.to.node), edge.to.index),
            ordinal: edge.ordinal,
            kind: match edge.role {
                EdgeRole::Flow => EdgeKind::Flow,
                EdgeRole::Enter => EdgeKind::Enter,
                EdgeRole::Join => EdgeKind::Join,
                EdgeRole::OrbitFeedback => EdgeKind::Feedback,
            },
            region: region_of(program, edge.to.node).map(region_id),
        });
    }

    for subgraph in &program.subgraphs {
        graph.regions.push(adapt_region(subgraph, program));
    }

    graph
}

fn adapt_node(
    node: &Node,
    program: &CantProgram,
    exit_is_its_own_seal: bool,
    options: AdaptOptions,
) -> SigilNode {
    let leaf = node.kind.leaf();
    let performs = leaf.is_some_and(|l| l.effectful);

    let kind = if performs {
        // An invocation, whatever stage it was written as. Placement on the
        // outer boundary is what an effect *is* in the visual grammar, and a
        // node that reaches the host world is that before it is a stage.
        SigilNodeKind::Effect
    } else if node.id == program.exit && !exit_is_its_own_seal {
        SigilNodeKind::Output
    } else {
        match &node.kind {
            NodeKind::Source { .. } => SigilNodeKind::Source,
            NodeKind::Stage { .. } => SigilNodeKind::Stage,
            NodeKind::Scatter => SigilNodeKind::Scatter,
            NodeKind::Collect => SigilNodeKind::Collect,
            NodeKind::Ward { .. } => SigilNodeKind::Ward,
            NodeKind::Fork { .. } => SigilNodeKind::Fork,
            NodeKind::Orbit { .. } => SigilNodeKind::Orbit,
        }
    };

    let mut out = SigilNode::new(node_id(node.id), kind);
    out.region = node.subgraph.map(region_id);

    // Effect metadata is read from the field, never from the text. This is the
    // line ADR 0006 draws, and why `cant.graph` went to version 1.
    if performs || !node.capabilities.is_empty() {
        out.effect = Some(EffectMetadata {
            performs,
            capabilities: node
                .capabilities
                .iter()
                .map(|c| Capability {
                    // The family always, the name only when labels were asked
                    // for. `@fs.read` is text the user wrote; `fs` is a
                    // classification this renderer invented. See `Capability`.
                    name: options.include_labels.then(|| c.name.clone()),
                    family: CapabilityFamily::from_namespace(&c.family),
                })
                .collect(),
        });
    }

    // A span always; a snippet only when asked for. The span is a position and
    // costs nothing to carry; the snippet is the user's source.
    if !node.span.is_dummy() {
        out.source = Some(SourceRef {
            span: node.span,
            snippet: options
                .include_labels
                .then(|| leaf.map(|l| l.text.clone()))
                .flatten(),
        });
    }

    if options.include_labels {
        // An explicit `label` outranks leaf text: it is there because something
        // put it there deliberately, which leaf text never is.
        out.label = node
            .label
            .clone()
            .or_else(|| leaf.map(|l| l.text.clone()))
            .or_else(|| structural_label(&node.kind));
        out.short_label = out.label.as_deref().map(abbreviate);
    }

    // Orbit policy is what the ring's tick group and inner lock are drawn from,
    // so it travels as attributes instead of being recovered from a label.
    if let NodeKind::Orbit {
        identity,
        max_items,
        ..
    } = &node.kind
    {
        out.attributes
            .insert("max_items".into(), serde_json::json!(max_items));
        out.attributes
            .insert("deduplicates".into(), serde_json::json!(identity.is_some()));
        if options.include_labels {
            if let Some(identity) = identity {
                out.attributes
                    .insert("identity".into(), serde_json::json!(identity.text));
            }
        }
    }
    if let Some(LeafExpr { placeholder, .. }) = leaf {
        if *placeholder {
            out.attributes
                .insert("placeholder".into(), serde_json::json!(true));
        }
    }

    out
}

/// A name for a node that has no leaf, so Revealed mode does not show a blank.
///
/// Structural rather than source text: a scatter has nothing written in it, and
/// "scatter" is the true thing to say about it.
fn structural_label(kind: &NodeKind) -> Option<String> {
    match kind {
        NodeKind::Scatter => Some("scatter".into()),
        NodeKind::Collect => Some("collect".into()),
        NodeKind::Fork { branches } => Some(format!("fork ({} branches)", branches.len())),
        NodeKind::Orbit { max_items, .. } => Some(format!("orbit (max {max_items})")),
        _ => None,
    }
}

/// A short form for Inscribed mode, where there is room for a few characters.
///
/// Cuts on a character boundary and marks the cut, for the same reason the
/// validator's truncation does: a silently shortened label reads as the whole
/// thing.
fn abbreviate(label: &str) -> String {
    const MAX: usize = 18;
    let mut chars = label.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_none() {
        head
    } else {
        format!("{head}…")
    }
}

fn adapt_region(subgraph: &Subgraph, program: &CantProgram) -> SigilRegion {
    let owner = program.node(subgraph.owner);
    let kind = match owner.map(|n| &n.kind) {
        Some(NodeKind::Orbit { .. }) => RegionKind::Orbit,
        Some(NodeKind::Fork { .. }) => RegionKind::Branch,
        _ => RegionKind::Group,
    };

    let mut region = SigilRegion::new(region_id(subgraph.id), kind);
    region.owner = Some(node_id(subgraph.owner));
    region.members = subgraph.nodes.iter().map(|id| node_id(*id)).collect();
    region.entry = subgraph.entry.map(node_id);
    region.exits = subgraph.exit.into_iter().map(node_id).collect();

    // Branch order, from the fork's own list rather than from the order
    // subgraphs happen to appear in — the `ordinal`-not-array-position rule the
    // Cant schema insists on, applied at the one place it decides a sector.
    region.ordinal = match owner.map(|n| &n.kind) {
        Some(NodeKind::Fork { branches }) => {
            branches.iter().position(|b| *b == subgraph.id).unwrap_or(0) as u32
        }
        _ => 0,
    };

    // Cant's subgraphs are flat — a branch inside a branch is still a top-level
    // entry — so parenthood is recovered from where the owning node lives.
    region.parent = owner.and_then(|n| n.subgraph).map(region_id);

    region
}

fn region_of(program: &CantProgram, node: crate::NodeId) -> Option<crate::SubgraphId> {
    program.node(node).and_then(|n| n.subgraph)
}

fn node_id(id: crate::NodeId) -> NodeId {
    NodeId::new(id.to_string())
}

fn region_id(id: crate::SubgraphId) -> RegionId {
    RegionId::new(id.to_string())
}

/// A stable identity for an edge Cant does not number.
///
/// Cant's edges have no `id`, but they have a deterministic identity: the pair
/// of ports, the ordinal and the role. The array index is included so two
/// genuinely parallel edges stay distinct, and it is stable because Cant's
/// lowering is a depth-first walk in source order.
fn edge_id(index: usize, edge: &crate::graph::Edge) -> EdgeId {
    EdgeId::new(format!(
        "e{index}:{}.{}->{}.{}",
        edge.from.node, edge.from.index, edge.to.node, edge.to.index
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cant_syntax::parse_source;

    fn graph_of(source: &str) -> CantProgram {
        let (parsed, sources) = parse_source("t.cant", source);
        assert!(
            !parsed.has_errors(),
            "{}",
            parsed.diagnostics.render_all(&sources)
        );
        crate::lower(&parsed.program.expect("program"), "t.cant", source.len())
    }

    fn adapt(source: &str) -> SigilGraph {
        to_sigil_graph(&graph_of(source), AdaptOptions::default())
    }

    fn kinds(g: &SigilGraph) -> Vec<&'static str> {
        g.nodes.iter().map(|n| n.kind.name()).collect()
    }

    /// The end-to-end claim: a Cant program becomes a graph the renderer accepts.
    #[test]
    fn an_adapted_graph_normalizes_without_complaint() {
        let g = adapt("[1, 2] -> * -> ?{ $ > 1 } -> $ * 10 -> []");
        let normalized = rite_sigil::normalize(g, &rite_sigil::NormalizeOptions::default())
            .expect("the adapter produces a graph Sigil accepts");
        assert!(
            !normalized.diagnostics.has_errors(),
            "{}",
            normalized.diagnostics
        );
        assert_eq!(normalized.fingerprint.as_str().len(), 32);
    }

    #[test]
    fn every_cant_node_kind_maps_across() {
        let g = adapt("[1, 2] -> * -> |{ ?{ $ > 1 } ; ~{ $ + 1 } :max 4 } -> []");
        let names = kinds(&g);
        for expected in ["source", "scatter", "fork", "ward", "orbit", "stage"] {
            assert!(names.contains(&expected), "no {expected} in {names:?}");
        }
    }

    /// An effectful node is an invocation whatever it was written as, because
    /// placement on the outer boundary is what an effect *is* in the grammar.
    #[test]
    fn an_effectful_stage_becomes_an_invocation_with_its_family() {
        let g = adapt(r#"["a.txt"] -> * -> ! @fs.read($) -> []"#);
        let effect = g
            .nodes
            .iter()
            .find(|n| n.kind == SigilNodeKind::Effect)
            .expect("an effect node");
        assert!(effect.is_invocation());
        let meta = effect.effect.as_ref().expect("effect metadata");
        assert!(meta.performs);
        assert_eq!(
            meta.capabilities[0].name, None,
            "a capability name is source text and is withheld by default"
        );
        assert_eq!(meta.capabilities[0].family, CapabilityFamily::Fs);

        // The family is what layout needs, and it is present either way.
        let revealed = to_sigil_graph(
            &graph_of(r#"["a.txt"] -> * -> ! @fs.read($) -> []"#),
            AdaptOptions::with_labels(),
        );
        let named = revealed
            .nodes
            .iter()
            .find_map(|n| n.effect.as_ref())
            .expect("effect metadata");
        assert_eq!(named.capabilities[0].name.as_deref(), Some("@fs.read"));
        assert_eq!(g.capability_families(), vec![CapabilityFamily::Fs]);
    }

    /// `@http` is Rite's spelling of the network capability, and a renderer that
    /// only knew `net` would draw every HTTP call with the generic mark.
    #[test]
    fn http_lands_in_the_network_family() {
        let g = adapt(r#"["https://example.com"] -> * -> ! @http.get($) -> []"#);
        assert_eq!(g.capability_families(), vec![CapabilityFamily::Net]);
    }

    /// A collect is already a seal; promoting it would erase the difference
    /// between "the values came together" and "the program ended".
    #[test]
    fn a_trailing_collect_keeps_its_own_mark_but_a_stage_is_promoted() {
        let collected = adapt("[1, 2] -> * -> []");
        assert_eq!(
            collected.nodes.last().expect("a last node").kind.name(),
            "collect"
        );
        assert!(!kinds(&collected).contains(&"output"));

        let bare = adapt("[1, 2] -> * -> $ + 1");
        assert_eq!(
            bare.nodes.last().expect("a last node").kind.name(),
            "output"
        );
    }

    /// Branch order decides a sector, so it comes from the fork's own list.
    #[test]
    fn fork_branches_carry_their_ordinal() {
        let g = adapt("[1] -> * -> |{ $ + 1 ; $ + 2 ; $ + 3 } -> []");
        let mut branches: Vec<u32> = g
            .regions
            .iter()
            .filter(|r| r.kind == RegionKind::Branch)
            .map(|r| r.ordinal)
            .collect();
        branches.sort_unstable();
        assert_eq!(branches, vec![0, 1, 2]);
    }

    #[test]
    fn an_orbit_becomes_a_ring_region_carrying_its_policy() {
        let g = adapt("[1] -> * -> ~{ $ + 1 } :by str :max 12 -> []");
        assert!(g.regions.iter().any(|r| r.kind == RegionKind::Orbit));
        let orbit = g
            .nodes
            .iter()
            .find(|n| n.kind == SigilNodeKind::Orbit)
            .expect("an orbit node");
        assert_eq!(orbit.attributes["max_items"], serde_json::json!(12));
        assert_eq!(orbit.attributes["deduplicates"], serde_json::json!(true));
    }

    /// The privacy default. Leaf text is the user's source; it arrives only when
    /// asked for (ADR 0007).
    #[test]
    fn no_source_text_travels_unless_it_is_asked_for() {
        let program = graph_of(r#"["secret.txt"] -> * -> $ + "suffix" -> []"#);

        let veiled = to_sigil_graph(&program, AdaptOptions::default());
        let json = serde_json::to_string(&veiled).expect("serializes");
        assert!(!json.contains("secret.txt"), "leaf text leaked: {json}");
        assert!(!json.contains("suffix"), "leaf text leaked: {json}");
        assert!(veiled.nodes.iter().all(|n| n.label.is_none()));
        assert!(veiled
            .nodes
            .iter()
            .all(|n| n.source.as_ref().is_none_or(|s| s.snippet.is_none())));

        let revealed = to_sigil_graph(&program, AdaptOptions::with_labels());
        let json = serde_json::to_string(&revealed).expect("serializes");
        assert!(json.contains("secret.txt"), "labels were asked for");
    }

    /// Spans travel either way: a position is not source text, and the Codex
    /// needs one to point at a line.
    #[test]
    fn spans_travel_even_when_labels_do_not() {
        let g = adapt("[1, 2] -> * -> []");
        assert!(
            g.nodes.iter().all(|n| n.source.is_some()),
            "every node should carry its span"
        );
        assert_eq!(
            g.metadata.source_length,
            Some("[1, 2] -> * -> []".len() as u32)
        );
    }

    /// Distinct edges get distinct identifiers, including genuinely parallel
    /// ones — the property Sigil's duplicate-ID check would otherwise trip on.
    #[test]
    fn edge_identifiers_are_unique_and_stable() {
        let source = "[1] -> * -> |{ $ + 1 ; $ + 2 } -> ~{ $ + 1 } :max 4 -> []";
        let first = adapt(source);
        let second = adapt(source);
        assert_eq!(first, second, "adaptation is deterministic");

        let mut ids: Vec<&str> = first.edges.iter().map(|e| e.id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate edge identifiers");
    }

    /// The orbit feedback edge is the only cycle, and it must survive adaptation
    /// as a distinguishable kind — the ring's re-entry arc is drawn from it.
    #[test]
    fn orbit_feedback_survives_as_its_own_edge_kind() {
        let g = adapt("[1] -> * -> ~{ $ + 1 } :max 4 -> []");
        assert_eq!(
            g.edges
                .iter()
                .filter(|e| e.kind == EdgeKind::Feedback)
                .count(),
            1
        );
    }

    /// Every adapted graph is a graph the renderer accepts. A fixture set rather
    /// than one program, because the failure this catches — a node kind that
    /// adapts into something validation rejects — appears one construct at a time.
    #[test]
    fn every_construct_adapts_into_something_renderable() {
        for source in [
            "[1] -> []",
            "[1, 2] -> * -> []",
            "[1, 2] -> * -> ?{ $ > 1 } -> []",
            "[1] -> * -> |{ $ + 1 ; $ + 2 } -> []",
            "[1] -> * -> ~{ $ + 1 } :max 4 -> []",
            "[1] -> * -> ~{ $ + 1 } :by str :max 4 -> []",
            r#"["a"] -> * -> ! @fs.read($) -> []"#,
            "[1] -> * -> |{ ?{ $ > 1 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []",
        ] {
            for options in [AdaptOptions::default(), AdaptOptions::with_labels()] {
                let g = to_sigil_graph(&graph_of(source), options);
                let result = rite_sigil::normalize(g, &rite_sigil::NormalizeOptions::default());
                match result {
                    Ok(normalized) => assert!(
                        !normalized.diagnostics.has_errors(),
                        "{source:?}: {}",
                        normalized.diagnostics
                    ),
                    Err(d) => panic!("{source:?} did not adapt into a renderable graph:\n{d}"),
                }
            }
        }
    }
}
