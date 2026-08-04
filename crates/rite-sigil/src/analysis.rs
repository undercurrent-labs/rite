//! Topology analysis: everything the layout engine needs to know before it
//! decides where anything goes.
//!
//! Separated from layout because these are answers about the *graph*, and a
//! layout that recomputed them inline would recompute them differently in each
//! of the places it needed them. Depth is the clearest case: fork sector width
//! and ring radius both depend on it, and two implementations that disagreed by
//! one would put a branch's nodes in a band its own boundary did not cover.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::graph::{EdgeKind, NodeId, RegionId, RegionKind, SigilGraph, SigilNodeKind};

/// Where a node belongs in the composition, before any coordinate exists.
///
/// This is the one decision that determines a node's radial band, and it is made
/// once. A node is classified by what it *is* to the composition, which is not
/// always what its kind says — an invocation is an invocation whatever stage it
/// was written as, and the exit is a seal whatever it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Placement {
    /// The entry. Centre.
    Core,
    /// Ordinary flow, in the middle bands.
    Flow,
    /// A member of an orbit's body, on that orbit's ring.
    Ring,
    /// A capability invocation, on the outer boundary.
    Invocation,
    /// A closing seal, in the outer semantic band.
    Seal,
    /// Reachable from nothing. Drawn outside the flow bands and reported.
    Detached,
}

/// One node's position in the topology.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeAnalysis {
    pub placement: Placement,
    /// Longest path from the entry, over non-feedback edges.
    ///
    /// Longest rather than shortest, because shortest-path depth lets a node sit
    /// at the same radius as its own predecessor whenever a shortcut edge
    /// exists — and two nodes at the same radius on the same spoke overlap.
    pub depth: u32,
    /// The region that owns this node, if any.
    pub region: Option<RegionId>,
    /// The branch ordinal of the owning region chain's outermost branch, which
    /// is what decides an angular sector.
    pub branch_ordinal: Option<u32>,
    /// Position among the members of the owning region, in flow order.
    pub index_in_region: usize,
    /// Position along the top-level spine, for a node that is on it.
    pub spine_index: Option<usize>,
}

/// One region's shape.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionAnalysis {
    pub kind: RegionKind,
    pub depth: u32,
    /// Members that are actually drawn inside this region — its own, not a
    /// nested region's.
    pub direct_members: Vec<NodeId>,
    /// How much angular room this region should get relative to its siblings.
    ///
    /// Node count plus a premium for nested structure. A branch containing an
    /// orbit needs room for a ring; a branch containing one stage does not, and
    /// giving them equal sectors wastes the circle on the second and crushes the
    /// first.
    pub weight: f64,
}

/// The whole analysis.
#[derive(Debug, Clone)]
pub struct Topology {
    pub nodes: BTreeMap<NodeId, NodeAnalysis>,
    pub regions: BTreeMap<RegionId, RegionAnalysis>,
    /// The top-level flow, in order: the chain the composition spirals along.
    pub spine: Vec<NodeId>,
    /// The deepest node, which sets the radial scale.
    pub max_depth: u32,
    /// Nodes unreachable from the entry, in graph order.
    pub detached: Vec<NodeId>,
}

impl Topology {
    pub fn of(&self, node: &NodeId) -> Option<&NodeAnalysis> {
        self.nodes.get(node)
    }

    pub fn placement(&self, node: &NodeId) -> Placement {
        self.nodes
            .get(node)
            .map(|n| n.placement)
            .unwrap_or(Placement::Detached)
    }

    /// Regions whose parent is `parent`, in ordinal order — the order that
    /// decides clockwise sector allocation.
    pub fn children_of<'a>(
        &self,
        graph: &'a SigilGraph,
        parent: Option<&RegionId>,
    ) -> Vec<&'a crate::graph::SigilRegion> {
        graph.child_regions(parent)
    }
}

/// Analyse a validated graph.
///
/// Takes a graph that has already been through [`crate::normalize`], so every
/// reference resolves and there are no duplicate identifiers. That is what lets
/// this be total: no lookup here can fail in a way that needs reporting.
pub fn analyze(graph: &SigilGraph) -> Topology {
    let depths = longest_path_depths(graph);
    let reachable = reachable_from_entry(graph);
    let spine = trace_spine(graph);
    let spine_index: BTreeMap<&NodeId, usize> =
        spine.iter().enumerate().map(|(i, id)| (id, i)).collect();

    let regions_by_id: BTreeMap<&RegionId, &crate::graph::SigilRegion> =
        graph.regions.iter().map(|r| (&r.id, r)).collect();

    let mut nodes = BTreeMap::new();
    let mut detached = Vec::new();

    for node in &graph.nodes {
        let depth = depths.get(&node.id).copied().unwrap_or(0);
        let is_reachable = reachable.contains(&node.id);
        if !is_reachable {
            detached.push(node.id.clone());
        }

        let in_orbit = node.region.as_ref().is_some_and(|r| {
            regions_by_id
                .get(r)
                .is_some_and(|region| region.kind == RegionKind::Orbit)
        });

        // The order of these tests is the priority order, and it is deliberate.
        // An invocation outranks everything because reaching the host world is
        // the strongest thing a node does — an effectful node inside an orbit
        // body still belongs on the boundary, since that is where "this touches
        // the outside" is legible. A seal outranks ordinary flow for the same
        // reason in reverse: the composition has to close somewhere visible.
        let placement = if !is_reachable {
            Placement::Detached
        } else if node.is_invocation() {
            Placement::Invocation
        } else if node.id == graph.entry {
            Placement::Core
        } else if graph.exits.contains(&node.id) || node.kind == SigilNodeKind::Output {
            Placement::Seal
        } else if in_orbit {
            Placement::Ring
        } else {
            Placement::Flow
        };

        let branch_ordinal = outermost_branch_ordinal(node.region.as_ref(), &regions_by_id);
        let index_in_region = node
            .region
            .as_ref()
            .and_then(|r| regions_by_id.get(r))
            .and_then(|region| region.members.iter().position(|m| *m == node.id))
            .unwrap_or(0);

        nodes.insert(
            node.id.clone(),
            NodeAnalysis {
                placement,
                depth,
                region: node.region.clone(),
                branch_ordinal,
                index_in_region,
                spine_index: spine_index.get(&node.id).copied(),
            },
        );
    }

    let regions = graph
        .regions
        .iter()
        .map(|region| {
            let depth = region_depth(&region.id, &regions_by_id);
            let direct_members = region
                .members
                .iter()
                .filter(|m| {
                    graph
                        .node(m)
                        .and_then(|n| n.region.as_ref())
                        .is_some_and(|r| *r == region.id)
                })
                .cloned()
                .collect::<Vec<_>>();
            let weight = region_weight(region, graph);
            (
                region.id.clone(),
                RegionAnalysis {
                    kind: region.kind,
                    depth,
                    direct_members,
                    weight,
                },
            )
        })
        .collect();

    let max_depth = depths.values().copied().max().unwrap_or(0);

    Topology {
        nodes,
        regions,
        spine,
        max_depth,
        detached,
    }
}

/// Longest-path depth from the entry, ignoring feedback edges.
///
/// Feedback is the one cycle the graph permits, and following it would make
/// "longest path" unbounded. Ignoring it is exactly right for layout: an orbit's
/// re-entry arc returns to a ring that has already been placed, so it adds no
/// depth.
///
/// Computed by relaxation with an iteration bound rather than by a topological
/// sort, because the bound makes it total on a graph containing a cycle that is
/// *not* feedback — which validation rejects, but which a caller using
/// [`analyze`] directly could still hand over.
fn longest_path_depths(graph: &SigilGraph) -> BTreeMap<NodeId, u32> {
    let mut depths: BTreeMap<NodeId, u32> =
        graph.nodes.iter().map(|n| (n.id.clone(), 0u32)).collect();

    let forward: Vec<(&NodeId, &NodeId)> = graph
        .edges
        .iter()
        .filter(|e| e.kind != EdgeKind::Feedback)
        .map(|e| (&e.from.node, &e.to.node))
        .collect();

    // A node's depth can grow by at most one per pass, so `nodes.len()` passes
    // is a hard bound on convergence for any graph, cyclic or not.
    for _ in 0..graph.nodes.len().max(1) {
        let mut changed = false;
        for (from, to) in &forward {
            let candidate = depths.get(*from).copied().unwrap_or(0) + 1;
            let current = depths.get(*to).copied().unwrap_or(0);
            if candidate > current {
                depths.insert((*to).clone(), candidate);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    depths
}

fn reachable_from_entry(graph: &SigilGraph) -> BTreeSet<NodeId> {
    let mut adjacency: BTreeMap<&NodeId, Vec<&NodeId>> = BTreeMap::new();
    for edge in &graph.edges {
        adjacency
            .entry(&edge.from.node)
            .or_default()
            .push(&edge.to.node);
    }
    // Region ownership connects too: an orbit body's members belong to the
    // orbit's picture whether or not an edge was recorded into each of them.
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
    seen.insert(graph.entry.clone());
    queue.push_back(&graph.entry);
    while let Some(current) = queue.pop_front() {
        for next in adjacency.get(current).into_iter().flatten() {
            if seen.insert((*next).clone()) {
                queue.push_back(next);
            }
        }
    }
    seen
}

/// The top-level flow: the chain the composition spirals along.
///
/// Follows `Flow` edges from the entry, staying outside regions. A node inside a
/// fork branch or an orbit body is not on the spine — it is placed relative to
/// the node that opened its region, which is.
///
/// Ties are broken by ordinal then by identifier, so the spine is the same every
/// time whatever order the edge list happens to be in.
fn trace_spine(graph: &SigilGraph) -> Vec<NodeId> {
    let mut spine = vec![graph.entry.clone()];
    let mut seen: BTreeSet<NodeId> = [graph.entry.clone()].into_iter().collect();
    let mut current = graph.entry.clone();

    // Bounded by node count: a repeated node stops the walk, so this terminates
    // even if a caller bypassed validation and handed over a cycle.
    for _ in 0..graph.nodes.len() {
        let next = graph
            .edges_from(&current)
            .into_iter()
            .filter(|e| e.kind == EdgeKind::Flow)
            .find(|e| graph.node(&e.to.node).is_some_and(|n| n.region.is_none()))
            .map(|e| e.to.node.clone());

        match next {
            Some(node) if seen.insert(node.clone()) => {
                spine.push(node.clone());
                current = node;
            }
            _ => break,
        }
    }

    spine
}

/// The branch ordinal of the outermost branch region in a node's ancestry.
///
/// Outermost rather than nearest, because it is the top-level branch that owns
/// an angular sector — a nested region subdivides its parent's sector, and a
/// node's place in the circle is decided by which top-level wedge it fell into.
fn outermost_branch_ordinal(
    region: Option<&RegionId>,
    regions: &BTreeMap<&RegionId, &crate::graph::SigilRegion>,
) -> Option<u32> {
    let mut current = region;
    let mut outermost = None;
    let mut guard = 0;
    while let Some(id) = current {
        guard += 1;
        if guard > 256 {
            break;
        }
        let Some(r) = regions.get(id) else { break };
        if r.kind == RegionKind::Branch {
            outermost = Some(r.ordinal);
        }
        current = r.parent.as_ref();
    }
    outermost
}

fn region_depth(id: &RegionId, regions: &BTreeMap<&RegionId, &crate::graph::SigilRegion>) -> u32 {
    let mut depth = 0;
    let mut current = regions.get(id).and_then(|r| r.parent.as_ref());
    while let Some(parent) = current {
        depth += 1;
        if depth > 256 {
            break;
        }
        current = regions.get(parent).and_then(|r| r.parent.as_ref());
    }
    depth
}

/// How much angular room a region deserves relative to its siblings.
///
/// A branch containing an orbit needs room for a ring; a branch containing one
/// stage does not. Equal sectors would waste the circle on the second and crush
/// the first, so weight is member count plus a premium for the structures that
/// actually need space.
fn region_weight(region: &crate::graph::SigilRegion, graph: &SigilGraph) -> f64 {
    let mut weight = region.members.len() as f64;

    for member in &region.members {
        let Some(node) = graph.node(member) else {
            continue;
        };
        // A ring needs circumference, and an invocation needs a spoke reaching
        // the boundary. Both are wider than a stage.
        weight += match node.kind {
            SigilNodeKind::Orbit => 3.0,
            SigilNodeKind::Fork => 2.0,
            _ => 0.0,
        };
        if node.is_invocation() {
            weight += 1.0;
        }
    }

    // Nested regions inside this one, recursively — bounded by the fact that
    // parenthood is a forest, which validation has already established.
    for child in graph
        .regions
        .iter()
        .filter(|r| r.parent.as_ref() == Some(&region.id))
    {
        weight += region_weight(child, graph) * 0.5;
    }

    weight.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;

    fn chain(kinds: &[(&str, SigilNodeKind)]) -> SigilGraph {
        let mut g = SigilGraph::new(SourceLanguage::Cant, kinds[0].0);
        for (id, kind) in kinds {
            g.nodes.push(SigilNode::new(*id, kind.clone()));
        }
        for (i, pair) in kinds.windows(2).enumerate() {
            g.edges.push(SigilEdge {
                id: EdgeId::new(format!("e{i}")),
                from: PortRef::new(pair[0].0, 0),
                to: PortRef::new(pair[1].0, 0),
                ordinal: 0,
                kind: EdgeKind::Flow,
                region: None,
            });
        }
        g.exits.push(kinds.last().expect("non-empty").0.into());
        g
    }

    #[test]
    fn depth_follows_the_chain() {
        let g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Output),
        ]);
        let t = analyze(&g);
        assert_eq!(t.of(&"n0".into()).unwrap().depth, 0);
        assert_eq!(t.of(&"n1".into()).unwrap().depth, 1);
        assert_eq!(t.of(&"n2".into()).unwrap().depth, 2);
        assert_eq!(t.max_depth, 2);
        assert_eq!(t.spine.len(), 3);
    }

    /// Shortest-path depth would put `n3` at 1, level with `n1` — and two nodes
    /// at the same radius on the same spoke overlap. Longest path is what keeps
    /// a node outside its own predecessor.
    #[test]
    fn depth_is_longest_path_so_a_shortcut_does_not_pull_a_node_inward() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Stage),
            ("n3", SigilNodeKind::Output),
        ]);
        g.edges.push(SigilEdge {
            id: EdgeId::new("shortcut"),
            from: PortRef::new("n0", 0),
            to: PortRef::new("n3", 0),
            ordinal: 1,
            kind: EdgeKind::Flow,
            region: None,
        });
        let t = analyze(&g);
        assert_eq!(t.of(&"n3".into()).unwrap().depth, 3, "not 1");
    }

    /// Feedback is the only cycle, and following it would make longest path
    /// unbounded. It must terminate and add no depth.
    #[test]
    fn a_feedback_edge_adds_no_depth_and_terminates() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Orbit),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes.push(SigilNode::new("body", SigilNodeKind::Stage));
        g.edges.push(SigilEdge {
            id: EdgeId::new("enter"),
            from: PortRef::new("n1", 1),
            to: PortRef::new("body", 0),
            ordinal: 0,
            kind: EdgeKind::Enter,
            region: None,
        });
        g.edges.push(SigilEdge {
            id: EdgeId::new("back"),
            from: PortRef::new("body", 0),
            to: PortRef::new("n1", 1),
            ordinal: 0,
            kind: EdgeKind::Feedback,
            region: None,
        });
        let t = analyze(&g);
        assert_eq!(t.of(&"n1".into()).unwrap().depth, 1);
        assert_eq!(t.of(&"body".into()).unwrap().depth, 2);
    }

    /// Reaching the host world is the strongest thing a node does, so it
    /// outranks every other placement — including membership of an orbit body.
    #[test]
    fn an_invocation_goes_to_the_boundary_from_wherever_it_was() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Output),
        ]);
        let mut region = SigilRegion::new("r0", RegionKind::Orbit);
        region.members.push("n1".into());
        region.owner = Some("n0".into());
        g.regions.push(region);
        g.nodes[1].region = Some("r0".into());
        g.nodes[1].effect = Some(EffectMetadata {
            performs: true,
            capabilities: vec![Capability::anonymous(CapabilityFamily::Fs)],
        });
        let t = analyze(&g);
        assert_eq!(t.placement(&"n1".into()), Placement::Invocation);
    }

    #[test]
    fn placements_cover_every_node_exactly_once() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes.push(SigilNode::new("lost", SigilNodeKind::Stage));
        let t = analyze(&g);
        assert_eq!(t.nodes.len(), 4);
        assert_eq!(t.placement(&"n0".into()), Placement::Core);
        assert_eq!(t.placement(&"n1".into()), Placement::Flow);
        assert_eq!(t.placement(&"n2".into()), Placement::Seal);
        assert_eq!(t.placement(&"lost".into()), Placement::Detached);
        assert_eq!(t.detached, vec![NodeId::new("lost")]);
    }

    /// The spine stays outside regions: a node inside a branch is placed
    /// relative to the fork that opened it, not on the main spiral.
    #[test]
    fn the_spine_does_not_wander_into_a_region() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Fork),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes.push(SigilNode::new("b0", SigilNodeKind::Stage));
        g.nodes[3].region = Some("r0".into());
        let mut region = SigilRegion::new("r0", RegionKind::Branch);
        region.members.push("b0".into());
        region.owner = Some("n1".into());
        g.regions.push(region);
        g.edges.push(SigilEdge {
            id: EdgeId::new("enter"),
            from: PortRef::new("n1", 1),
            to: PortRef::new("b0", 0),
            ordinal: 0,
            kind: EdgeKind::Enter,
            region: Some("r0".into()),
        });
        let t = analyze(&g);
        assert_eq!(
            t.spine,
            vec![NodeId::new("n0"), NodeId::new("n1"), NodeId::new("n2")]
        );
        assert_eq!(t.of(&"b0".into()).unwrap().spine_index, None);
    }

    /// Equal sectors would crush a branch holding an orbit and waste the circle
    /// on one holding a single stage.
    #[test]
    fn a_branch_with_structure_weighs_more_than_a_bare_one() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Fork),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes.push(SigilNode::new("plain", SigilNodeKind::Stage));
        g.nodes.push(SigilNode::new("ring", SigilNodeKind::Orbit));
        g.nodes[3].region = Some("r0".into());
        g.nodes[4].region = Some("r1".into());
        for (id, member, ordinal) in [("r0", "plain", 0u32), ("r1", "ring", 1)] {
            let mut region = SigilRegion::new(id, RegionKind::Branch);
            region.members.push(member.into());
            region.owner = Some("n1".into());
            region.ordinal = ordinal;
            g.regions.push(region);
        }
        let t = analyze(&g);
        let plain = t.regions[&RegionId::new("r0")].weight;
        let structured = t.regions[&RegionId::new("r1")].weight;
        assert!(
            structured > plain,
            "a branch holding an orbit ({structured}) must outweigh a bare one ({plain})"
        );
    }

    /// A node's wedge is decided by the top-level branch it fell into, not by
    /// the innermost region that happens to contain it.
    #[test]
    fn branch_ordinal_comes_from_the_outermost_branch() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Fork),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes.push(SigilNode::new("deep", SigilNodeKind::Stage));
        g.nodes[3].region = Some("inner".into());

        let mut outer = SigilRegion::new("outer", RegionKind::Branch);
        outer.ordinal = 2;
        outer.owner = Some("n1".into());
        let mut inner = SigilRegion::new("inner", RegionKind::Branch);
        inner.ordinal = 0;
        inner.parent = Some("outer".into());
        inner.members.push("deep".into());
        g.regions.push(outer);
        g.regions.push(inner);

        let t = analyze(&g);
        assert_eq!(t.of(&"deep".into()).unwrap().branch_ordinal, Some(2));
    }

    /// Analysis runs over graphs a caller may have built by hand, so it must be
    /// total rather than trusting validation to have run.
    #[test]
    fn analysis_terminates_on_a_graph_validation_would_reject() {
        let mut g = chain(&[("n0", SigilNodeKind::Source), ("n1", SigilNodeKind::Stage)]);
        // A non-feedback cycle: rejected by validation, but `analyze` is called
        // directly here and must still return.
        g.edges.push(SigilEdge {
            id: EdgeId::new("loop"),
            from: PortRef::new("n1", 0),
            to: PortRef::new("n0", 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
        let t = analyze(&g);
        assert_eq!(t.nodes.len(), 2);
    }
}
