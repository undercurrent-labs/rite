//! Validating a normalized graph.
//!
//! Every graph is untrusted, including one Sigil produced itself. That is not
//! paranoia about our own adapter — it is that the adapter and a hostile JSON
//! file arrive at the same function, so a check that only runs on one path is a
//! check that does not run. `cant_sem::validate_deserialized` takes the same
//! position for the same reason.
//!
//! # What "valid" means here
//!
//! Not "this is a good program" — Cant and Rite already answered that. It means
//! **the layout engine can walk this without panicking or looping**: every
//! reference resolves, no identifier is claimed twice, region nesting has an
//! outermost ring, and nothing is unbounded.
//!
//! # Errors and warnings are different things
//!
//! An error means the graph cannot be drawn: no entry, a dangling edge, a
//! parenthood cycle. A warning means it can be drawn but something is worth
//! saying: an unreachable node, an unknown kind, a graph past the size where the
//! result stays legible. Warnings never stop a render, because the alternative
//! is a renderer that refuses to draw a picture the user can see is fine.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::diagnostic::*;
use crate::graph::*;
use crate::limits::NormalizeOptions;

/// A validated graph, plus everything that was worth saying about it.
#[derive(Debug, Clone)]
pub struct Validated {
    pub graph: SigilGraph,
    pub diagnostics: Diagnostics,
}

impl Validated {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Check a graph, and normalize what can be normalized in place.
///
/// Returns the graph even when there are errors: a caller reporting diagnostics
/// wants the graph to name things in, and the Codex can show a broken graph's
/// structure. Callers that intend to *render* must check [`Validated::has_errors`]
/// first — [`crate::normalize`] does that for them.
pub fn validate(mut graph: SigilGraph, options: &NormalizeOptions) -> Validated {
    let mut d = Diagnostics::new();

    check_schema(&graph, &mut d);
    check_limits(&graph, options, &mut d);
    let known_nodes = check_identifier_uniqueness(&graph, &mut d);
    check_references(&graph, &known_nodes, &mut d);
    check_region_nesting(&graph, options, &mut d);
    check_numbers_and_spans(&graph, &mut d);
    check_unknown_kinds(&graph, options, &mut d);

    // Only meaningful once references resolve; a dangling edge would otherwise
    // report every node it should have reached as unreachable too.
    if !d.has_errors() {
        check_reachability(&graph, &mut d);
    }

    normalize_in_place(&mut graph, options, &mut d);

    Validated {
        graph,
        diagnostics: d,
    }
}

fn check_schema(graph: &SigilGraph, d: &mut Diagnostics) {
    if graph.schema != GRAPH_SCHEMA_NAME {
        d.push(
            SigilDiagnostic::error(
                SIGIL_V001_UNSUPPORTED_GRAPH_SCHEMA,
                GraphRef::Graph,
                format!(
                    "graph schema `{}`, expected `{}`",
                    graph.schema, GRAPH_SCHEMA_NAME
                ),
            )
            .with_note("a Cant graph is adapted into this shape; it is not read directly"),
        );
    }
    // A newer *minor* version is readable — unknown fields were already dropped
    // by serde. A newer major is not, and there is no major yet, so any
    // difference is a refusal.
    if graph.version != GRAPH_SCHEMA_VERSION {
        d.push(SigilDiagnostic::error(
            SIGIL_V002_UNSUPPORTED_SCHEMA_VERSION,
            GraphRef::Graph,
            format!(
                "graph schema version `{}`, expected `{}`",
                graph.version, GRAPH_SCHEMA_VERSION
            ),
        ));
    }
}

fn check_limits(graph: &SigilGraph, options: &NormalizeOptions, d: &mut Diagnostics) {
    let l = &options.limits;
    if graph.nodes.is_empty() {
        d.push(SigilDiagnostic::error(
            SIGIL_G009_EMPTY_GRAPH,
            GraphRef::Graph,
            "the graph has no nodes",
        ));
    }
    if graph.nodes.len() > l.max_nodes {
        d.push(
            SigilDiagnostic::error(
                SIGIL_S001_TOO_MANY_NODES,
                GraphRef::Graph,
                format!("{} nodes, cap is {}", graph.nodes.len(), l.max_nodes),
            )
            .with_note(
                "try `--simplify`, raise `--max-nodes`, or use `cant graph` for a technical view",
            ),
        );
    } else if graph.nodes.len() > l.soft_node_warning {
        d.push(
            SigilDiagnostic::warning(
                SIGIL_S007_LARGE_GRAPH,
                GraphRef::Graph,
                format!(
                    "{} nodes is past the size where a sigil stays legible",
                    graph.nodes.len()
                ),
            )
            .with_note("`--simplify` collapses linear stage chains"),
        );
    }
    if graph.edges.len() > l.max_edges {
        d.push(SigilDiagnostic::error(
            SIGIL_S002_TOO_MANY_EDGES,
            GraphRef::Graph,
            format!("{} edges, cap is {}", graph.edges.len(), l.max_edges),
        ));
    }
}

/// Duplicate identifiers, and the set of node IDs everything else is checked
/// against.
fn check_identifier_uniqueness(graph: &SigilGraph, d: &mut Diagnostics) -> BTreeSet<NodeId> {
    let mut nodes = BTreeSet::new();
    for node in &graph.nodes {
        if !nodes.insert(node.id.clone()) {
            d.push(SigilDiagnostic::error(
                SIGIL_G003_DUPLICATE_ID,
                GraphRef::Node(node.id.0.clone()),
                format!("two nodes share the identifier `{}`", node.id),
            ));
        }
    }
    let mut edges = BTreeSet::new();
    for edge in &graph.edges {
        if !edges.insert(edge.id.clone()) {
            d.push(SigilDiagnostic::error(
                SIGIL_G003_DUPLICATE_ID,
                GraphRef::Edge(edge.id.0.clone()),
                format!("two edges share the identifier `{}`", edge.id),
            ));
        }
    }
    let mut regions = BTreeSet::new();
    for region in &graph.regions {
        if !regions.insert(region.id.clone()) {
            d.push(SigilDiagnostic::error(
                SIGIL_G003_DUPLICATE_ID,
                GraphRef::Region(region.id.0.clone()),
                format!("two regions share the identifier `{}`", region.id),
            ));
        }
    }
    nodes
}

fn check_references(graph: &SigilGraph, nodes: &BTreeSet<NodeId>, d: &mut Diagnostics) {
    let regions: BTreeSet<&RegionId> = graph.regions.iter().map(|r| &r.id).collect();

    if !nodes.contains(&graph.entry) {
        d.push(
            SigilDiagnostic::error(
                SIGIL_G001_NO_ENTRY,
                GraphRef::Graph,
                format!("the entry names node `{}`, which is not here", graph.entry),
            )
            .with_note("the entry is the centre of the composition; there is nothing to draw from"),
        );
    }
    if graph.exits.is_empty() {
        d.push(
            SigilDiagnostic::warning(
                SIGIL_G008_NO_EXIT,
                GraphRef::Graph,
                "the graph names no exit",
            )
            .with_note("the composition will have no closing seal"),
        );
    }
    for exit in &graph.exits {
        if !nodes.contains(exit) {
            d.push(SigilDiagnostic::error(
                SIGIL_G002_UNKNOWN_NODE,
                GraphRef::Node(exit.0.clone()),
                format!("the exit names node `{exit}`, which is not here"),
            ));
        }
    }

    for edge in &graph.edges {
        for (end, port) in [("from", &edge.from), ("to", &edge.to)] {
            if !nodes.contains(&port.node) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G002_UNKNOWN_NODE,
                    GraphRef::Edge(edge.id.0.clone()),
                    format!("`{end}` names node `{}`, which is not here", port.node),
                ));
            }
        }
        if let Some(region) = &edge.region {
            if !regions.contains(region) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G004_UNKNOWN_REGION,
                    GraphRef::Edge(edge.id.0.clone()),
                    format!("names region `{region}`, which is not here"),
                ));
            }
        }
    }

    for node in &graph.nodes {
        if let Some(region) = &node.region {
            if !regions.contains(region) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G004_UNKNOWN_REGION,
                    GraphRef::Node(node.id.0.clone()),
                    format!("names region `{region}`, which is not here"),
                ));
            }
        }
    }

    // A node claimed by two regions has no owning ring, so there is no
    // well-defined band to place it in.
    let mut claimed: BTreeMap<&NodeId, &RegionId> = BTreeMap::new();
    for region in &graph.regions {
        for member in &region.members {
            if !nodes.contains(member) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G002_UNKNOWN_NODE,
                    GraphRef::Region(region.id.0.clone()),
                    format!("claims node `{member}`, which is not here"),
                ));
                continue;
            }
            if let Some(first) = claimed.insert(member, &region.id) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G010_DUPLICATE_REGION_MEMBER,
                    GraphRef::Node(member.0.clone()),
                    format!("claimed by regions `{first}` and `{}`", region.id),
                ));
            }
        }
        for referenced in region.entry.iter().chain(region.exits.iter()) {
            if !nodes.contains(referenced) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G002_UNKNOWN_NODE,
                    GraphRef::Region(region.id.0.clone()),
                    format!("names node `{referenced}`, which is not here"),
                ));
            }
        }
        if let Some(owner) = &region.owner {
            if !nodes.contains(owner) {
                d.push(SigilDiagnostic::error(
                    SIGIL_G002_UNKNOWN_NODE,
                    GraphRef::Region(region.id.0.clone()),
                    format!("names owner `{owner}`, which is not here"),
                ));
            }
        }
    }
}

/// Region parenthood must be a forest, and no deeper than the cap.
///
/// A cycle is an error rather than something to break arbitrarily: concentric
/// rings need an outermost one, and picking a place to cut would mean the
/// picture depends on which node the walk started from.
fn check_region_nesting(graph: &SigilGraph, options: &NormalizeOptions, d: &mut Diagnostics) {
    let by_id: BTreeMap<&RegionId, &SigilRegion> =
        graph.regions.iter().map(|r| (&r.id, r)).collect();

    for region in &graph.regions {
        let mut seen = BTreeSet::new();
        seen.insert(&region.id);
        let mut depth = 0usize;
        let mut current = region.parent.as_ref();

        while let Some(parent_id) = current {
            depth += 1;
            if depth > options.limits.max_region_depth {
                d.push(SigilDiagnostic::error(
                    SIGIL_S003_NESTING_TOO_DEEP,
                    GraphRef::Region(region.id.0.clone()),
                    format!(
                        "region nesting deeper than {}",
                        options.limits.max_region_depth
                    ),
                ));
                break;
            }
            if !seen.insert(parent_id) {
                d.push(
                    SigilDiagnostic::error(
                        SIGIL_G005_REGION_CYCLE,
                        GraphRef::Region(region.id.0.clone()),
                        format!("region parenthood loops through `{parent_id}`"),
                    )
                    .with_note("concentric rings need an outermost one"),
                );
                break;
            }
            match by_id.get(parent_id) {
                // Reported by `check_references`; stop rather than say it twice.
                None => break,
                Some(parent) => current = parent.parent.as_ref(),
            }
        }
    }
}

/// Impossible spans.
///
/// **Non-finite numbers are not checked here, and the omission is deliberate.**
/// §6.4 asks for them to be rejected, and they are — by `serde_json`, before
/// this function can see one. `1e400` fails to parse with "number out of range",
/// `NaN` and `Infinity` are not JSON at all, and `serde_json::Number::from_f64`
/// returns `None` for both, so a non-finite value cannot be *constructed* in a
/// `serde_json::Value` let alone deserialized into one. A check here would be
/// unreachable code wearing the appearance of a safety net, which is worse than
/// no check because it invites trust.
///
/// [`crate::diagnostic::SIGIL_S006_NON_FINITE_NUMBER`] is kept for the place
/// non-finite numbers can genuinely appear: scene coordinates, where `f64`
/// arithmetic over a degenerate graph can produce one and it would reach an SVG
/// attribute as the empty string, looking like a layout bug rather than an input
/// one. That check belongs to the scene bounds pass.
fn check_numbers_and_spans(graph: &SigilGraph, d: &mut Diagnostics) {
    let source_length = graph.metadata.source_length;

    for node in &graph.nodes {
        // A weight is an observed count: non-negative and finite, or it is not
        // an observation. Rejected rather than clamped — a NaN that became 0
        // would render as "this node never ran", which is a claim.
        if let Some(weight) = node.weight {
            if !weight.is_finite() || weight < 0.0 {
                d.push(SigilDiagnostic::error(
                    SIGIL_S008_MALFORMED_SPAN,
                    GraphRef::Node(node.id.0.clone()),
                    format!("weight `{weight}` is not a finite non-negative count"),
                ));
            }
        }
    }

    for node in &graph.nodes {
        let Some(source) = &node.source else { continue };
        let (start, end) = (source.span.start.as_usize(), source.span.end.as_usize());
        if end < start {
            d.push(SigilDiagnostic::error(
                SIGIL_S008_MALFORMED_SPAN,
                GraphRef::Node(node.id.0.clone()),
                format!("span ends at {end}, before it starts at {start}"),
            ));
        } else if let Some(length) = source_length {
            if end > length as usize {
                d.push(
                    SigilDiagnostic::warning(
                        SIGIL_S008_MALFORMED_SPAN,
                        GraphRef::Node(node.id.0.clone()),
                        format!("span ends at {end}, past the {length}-byte source"),
                    )
                    .with_note("the snippet will not be shown"),
                );
            }
        }
    }
}

fn check_unknown_kinds(graph: &SigilGraph, options: &NormalizeOptions, d: &mut Diagnostics) {
    for node in &graph.nodes {
        let Some(name) = node.kind.unknown_name() else {
            continue;
        };
        let message = format!("node kind `{name}` is not one this renderer knows");
        d.push(if options.strict_unknown_kinds {
            SigilDiagnostic::error(
                SIGIL_G007_UNKNOWN_NODE_KIND,
                GraphRef::Node(node.id.0.clone()),
                message,
            )
        } else {
            SigilDiagnostic::warning(
                SIGIL_G007_UNKNOWN_NODE_KIND,
                GraphRef::Node(node.id.0.clone()),
                message,
            )
            .with_note("drawn with the unknown mark; connectivity is preserved")
        });
    }
}

/// Nodes the entry cannot reach.
///
/// A warning, not an error. A detached node is drawable — it goes outside the
/// flow bands and is reported — and refusing would mean a partially-built graph
/// could not be looked at, which is exactly when looking at it helps.
///
/// The walk follows every edge direction, including feedback, because
/// reachability here is "is this part of the same picture", not "can control
/// arrive here".
fn check_reachability(graph: &SigilGraph, d: &mut Diagnostics) {
    let mut adjacency: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for edge in &graph.edges {
        adjacency
            .entry(&edge.from.node)
            .or_default()
            .push(&edge.to.node);
    }
    // Region ownership is a connection too: an orbit body's nodes are part of
    // the orbit's picture whether or not an edge was recorded into each.
    for region in &graph.regions {
        if let Some(owner) = &region.owner {
            adjacency
                .entry(owner)
                .or_default()
                .extend(region.members.iter());
        }
    }

    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    seen.insert(&graph.entry);
    queue.push_back(&graph.entry);
    while let Some(current) = queue.pop_front() {
        for next in adjacency.get(current).into_iter().flatten() {
            if seen.insert(*next) {
                queue.push_back(*next);
            }
        }
    }

    for node in &graph.nodes {
        if !seen.contains(&node.id) {
            d.push(
                SigilDiagnostic::warning(
                    SIGIL_G006_UNREACHABLE_NODE,
                    GraphRef::Node(node.id.0.clone()),
                    format!("node `{}` cannot be reached from the entry", node.id),
                )
                .with_note("it is drawn outside the flow bands"),
            );
        }
    }
}

/// Bound labels, drop snippets that were not asked for.
///
/// Truncation rather than rejection: losing the tail of one 8 KiB label is a
/// better outcome than refusing to draw the program it belongs to. The
/// truncation is at a character boundary and marked with an ellipsis, so nobody
/// reads a cut label as the whole thing.
fn normalize_in_place(graph: &mut SigilGraph, options: &NormalizeOptions, d: &mut Diagnostics) {
    let max = options.limits.max_label_bytes;
    for node in &mut graph.nodes {
        for (which, label) in [
            ("label", node.label.as_mut()),
            ("short_label", node.short_label.as_mut()),
        ] {
            let Some(label) = label else { continue };
            if label.len() > max {
                d.push(SigilDiagnostic::warning(
                    SIGIL_S004_LABEL_TOO_LONG,
                    GraphRef::Node(node.id.0.clone()),
                    format!("{which} is {} bytes, cap is {max}; truncated", label.len()),
                ));
                truncate_on_char_boundary(label, max);
            }
        }

        if let Some(source) = &mut node.source {
            if !options.keep_snippets {
                source.snippet = None;
            } else if let Some(snippet) = &mut source.snippet {
                if snippet.len() > max {
                    truncate_on_char_boundary(snippet, max);
                }
            }
        }
    }
}

/// Cut to at most `max` bytes without splitting a character, and mark the cut.
///
/// The ellipsis is inside the budget, so the result never exceeds `max` — a
/// truncation that overshoots its own limit is the sort of thing that turns a
/// cap into a suggestion.
fn truncate_on_char_boundary(s: &mut String, max: usize) {
    const MARK: &str = "…";
    if s.len() <= max {
        return;
    }
    let budget = max.saturating_sub(MARK.len());
    let mut cut = budget;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
    s.push_str(MARK);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear() -> SigilGraph {
        let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
        g.nodes.push(SigilNode::new("n0", SigilNodeKind::Source));
        g.nodes.push(SigilNode::new("n1", SigilNodeKind::Stage));
        g.nodes.push(SigilNode::new("n2", SigilNodeKind::Output));
        g.exits.push("n2".into());
        for (i, (from, to)) in [("n0", "n1"), ("n1", "n2")].into_iter().enumerate() {
            g.edges.push(SigilEdge {
                id: EdgeId::new(format!("e{i}")),
                from: PortRef::new(from, 0),
                to: PortRef::new(to, 0),
                ordinal: 0,
                kind: EdgeKind::Flow,
                region: None,
            });
        }
        g
    }

    fn codes(v: &Validated) -> Vec<String> {
        v.diagnostics.iter().map(|d| d.code.to_string()).collect()
    }

    #[test]
    fn a_well_formed_graph_passes_clean() {
        let v = validate(linear(), &NormalizeOptions::default());
        assert!(!v.has_errors(), "{}", v.diagnostics);
        assert!(v.diagnostics.is_empty(), "{}", v.diagnostics);
    }

    #[test]
    fn a_dangling_edge_is_an_error_naming_the_edge() {
        let mut g = linear();
        g.edges[1].to = PortRef::new("nowhere", 0);
        let v = validate(g, &NormalizeOptions::default());
        assert!(v.has_errors());
        let d = v
            .diagnostics
            .errors()
            .find(|d| d.code == SIGIL_G002_UNKNOWN_NODE)
            .expect("SIGIL-G002");
        assert_eq!(d.graph_ref, GraphRef::Edge("e1".into()));
        assert!(d.message.contains("nowhere"), "{}", d.message);
    }

    #[test]
    fn a_duplicate_identifier_is_an_error() {
        let mut g = linear();
        g.nodes.push(SigilNode::new("n1", SigilNodeKind::Ward));
        let v = validate(g, &NormalizeOptions::default());
        assert!(codes(&v).contains(&"SIGIL-G003".to_string()));
    }

    #[test]
    fn a_missing_entry_is_an_error() {
        let mut g = linear();
        g.entry = NodeId::new("not-here");
        let v = validate(g, &NormalizeOptions::default());
        assert!(codes(&v).contains(&"SIGIL-G001".to_string()));
    }

    /// Concentric rings need an outermost one, so a parenthood cycle cannot be
    /// drawn — and must terminate rather than loop while discovering that.
    #[test]
    fn a_region_parenthood_cycle_is_an_error_and_terminates() {
        let mut g = linear();
        let mut a = SigilRegion::new("r0", RegionKind::Branch);
        let mut b = SigilRegion::new("r1", RegionKind::Branch);
        a.parent = Some("r1".into());
        b.parent = Some("r0".into());
        g.regions.push(a);
        g.regions.push(b);
        let v = validate(g, &NormalizeOptions::default());
        assert!(codes(&v).contains(&"SIGIL-G005".to_string()));
    }

    #[test]
    fn a_node_claimed_by_two_regions_is_an_error() {
        let mut g = linear();
        for (id, member) in [("r0", "n1"), ("r1", "n1")] {
            let mut r = SigilRegion::new(id, RegionKind::Branch);
            r.members.push(NodeId::new(member));
            g.regions.push(r);
        }
        let v = validate(g, &NormalizeOptions::default());
        assert!(codes(&v).contains(&"SIGIL-G010".to_string()));
    }

    /// Drawable, so a warning — refusing would mean a half-built graph could not
    /// be looked at, which is when looking helps most.
    #[test]
    fn an_unreachable_node_is_a_warning_not_an_error() {
        let mut g = linear();
        g.nodes.push(SigilNode::new("orphan", SigilNodeKind::Stage));
        let v = validate(g, &NormalizeOptions::default());
        assert!(!v.has_errors(), "{}", v.diagnostics);
        let d = v
            .diagnostics
            .warnings()
            .find(|d| d.code == SIGIL_G006_UNREACHABLE_NODE)
            .expect("SIGIL-G006");
        assert_eq!(d.graph_ref, GraphRef::Node("orphan".into()));
    }

    /// §6.3: degrade, do not fail — unless the caller asked to be strict.
    #[test]
    fn an_unknown_kind_warns_by_default_and_errors_when_strict() {
        let mut g = linear();
        g.nodes.push(SigilNode::new(
            "n3",
            SigilNodeKind::Unknown("portal".into()),
        ));
        g.edges.push(SigilEdge {
            id: EdgeId::new("e2"),
            from: PortRef::new("n2", 0),
            to: PortRef::new("n3", 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });

        let lenient = validate(g.clone(), &NormalizeOptions::default());
        assert!(!lenient.has_errors(), "{}", lenient.diagnostics);
        assert!(codes(&lenient).contains(&"SIGIL-G007".to_string()));

        let strict = validate(
            g,
            &NormalizeOptions {
                strict_unknown_kinds: true,
                ..Default::default()
            },
        );
        assert!(strict.has_errors());
    }

    #[test]
    fn a_graph_over_the_node_cap_is_refused_with_a_way_out() {
        let mut g = linear();
        for i in 0..20 {
            g.nodes
                .push(SigilNode::new(format!("x{i}"), SigilNodeKind::Stage));
        }
        let mut options = NormalizeOptions::default();
        options.limits.max_nodes = 10;
        options.limits.soft_node_warning = 5;
        let v = validate(g, &options);
        let d = v
            .diagnostics
            .errors()
            .find(|d| d.code == SIGIL_S001_TOO_MANY_NODES)
            .expect("SIGIL-S001");
        assert!(
            d.notes.iter().any(|n| n.contains("cant graph")),
            "the refusal must name an alternative: {:?}",
            d.notes
        );
    }

    #[test]
    fn a_large_but_legal_graph_warns_once() {
        let mut g = linear();
        for i in 0..20 {
            g.nodes
                .push(SigilNode::new(format!("x{i}"), SigilNodeKind::Stage));
        }
        let mut options = NormalizeOptions::default();
        options.limits.soft_node_warning = 5;
        let v = validate(g, &options);
        assert!(!v.has_errors());
        assert_eq!(
            v.diagnostics
                .iter()
                .filter(|d| d.code == SIGIL_S007_LARGE_GRAPH)
                .count(),
            1
        );
    }

    /// §6.4 asks for non-finite numbers to be rejected. They are — one layer
    /// down, by the deserializer, which is why `check_numbers_and_spans` has no
    /// branch for them. This test is what makes that claim checkable rather than
    /// a comment: if `serde_json` ever starts accepting `1e400`, it fails and the
    /// missing check becomes a real gap to fill.
    #[test]
    fn non_finite_numbers_cannot_reach_validation_at_all() {
        for text in ["1e400", "-1e400", "NaN", "Infinity"] {
            assert!(
                serde_json::from_str::<serde_json::Value>(text).is_err(),
                "serde_json now accepts {text}; validation needs a finiteness check"
            );
        }
        assert!(
            serde_json::Number::from_f64(f64::INFINITY).is_none(),
            "a non-finite JSON number became constructible"
        );
        assert!(serde_json::Number::from_f64(f64::NAN).is_none());
    }

    #[test]
    fn a_backwards_span_is_an_error() {
        let mut g = linear();
        g.nodes[1].source = Some(SourceRef {
            span: rite_core::Span::from_range(40, 10),
            snippet: None,
        });
        let v = validate(g, &NormalizeOptions::default());
        assert!(codes(&v).contains(&"SIGIL-S008".to_string()));
        assert!(v.has_errors());
    }

    #[test]
    fn a_span_past_the_end_of_the_source_is_a_warning() {
        let mut g = linear();
        g.metadata.source_length = Some(20);
        g.nodes[1].source = Some(SourceRef {
            span: rite_core::Span::from_range(10, 400),
            snippet: None,
        });
        let v = validate(g, &NormalizeOptions::default());
        assert!(!v.has_errors(), "{}", v.diagnostics);
        assert!(codes(&v).contains(&"SIGIL-S008".to_string()));
    }

    #[test]
    fn a_long_label_is_truncated_within_its_own_budget_not_rejected() {
        let mut g = linear();
        g.nodes[1].label = Some("é".repeat(4000));
        let mut options = NormalizeOptions::default();
        options.limits.max_label_bytes = 64;
        let v = validate(g, &options);
        assert!(!v.has_errors(), "{}", v.diagnostics);
        let label = v.graph.nodes[1].label.as_ref().expect("still there");
        assert!(
            label.len() <= 64,
            "truncation overshot its own cap: {} bytes",
            label.len()
        );
        assert!(label.ends_with('…'), "a cut label must say it was cut");
        assert!(codes(&v).contains(&"SIGIL-S004".to_string()));
    }

    /// The privacy separation starts at normalization, not at the serializer.
    #[test]
    fn snippets_are_dropped_unless_asked_for() {
        let mut g = linear();
        g.nodes[1].source = Some(SourceRef {
            span: rite_core::Span::from_range(0, 5),
            snippet: Some("secret".into()),
        });

        let dropped = validate(g.clone(), &NormalizeOptions::default());
        assert_eq!(
            dropped.graph.nodes[1].source.as_ref().unwrap().snippet,
            None
        );

        let kept = validate(g, &NormalizeOptions::default().with_snippets());
        assert_eq!(
            kept.graph.nodes[1]
                .source
                .as_ref()
                .unwrap()
                .snippet
                .as_deref(),
            Some("secret")
        );
    }

    /// A graph with errors still comes back, so a caller can name things in it.
    #[test]
    fn a_broken_graph_is_still_returned() {
        let mut g = linear();
        g.entry = NodeId::new("gone");
        let v = validate(g, &NormalizeOptions::default());
        assert!(v.has_errors());
        assert_eq!(v.graph.nodes.len(), 3);
    }
}
