//! Measured, not enforced.
//!
//! §24 asks for measurement before hard release gates, and this is the
//! measurement. The assertions here are deliberately loose — an order of
//! magnitude above the target — because a tight timing assertion on shared CI
//! hardware fails for reasons that have nothing to do with the renderer, and a
//! test that fails randomly gets disabled rather than investigated.
//!
//! What it *does* catch is an accidental quadratic: the layout's collision pass
//! is O(n²) in the worst case by construction, and this fails loudly if the rest
//! of the pipeline joins it.
//!
//! Run `cargo test -p rite-sigil --test performance -- --nocapture` to see the
//! numbers.

use std::time::Instant;

use rite_sigil::{
    build_scene, normalize, EdgeId, EdgeKind, LayoutOptions, NodeId, NormalizeOptions, PortRef,
    SigilEdge, SigilGraph, SigilNode, SigilNodeKind, SourceLanguage,
};

/// A chain of `n` nodes with a fork and an orbit in it, so the measurement
/// covers the structures that actually cost something.
fn graph_of_size(n: usize) -> SigilGraph {
    let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
    g.nodes.push(SigilNode::new("n0", SigilNodeKind::Source));
    for i in 1..n {
        let kind = match i % 7 {
            0 => SigilNodeKind::Ward,
            3 => SigilNodeKind::Scatter,
            5 => SigilNodeKind::Collect,
            _ => SigilNodeKind::Stage,
        };
        g.nodes.push(SigilNode::new(format!("n{i}"), kind));
        g.edges.push(SigilEdge {
            id: EdgeId::new(format!("e{i}")),
            from: PortRef::new(format!("n{}", i - 1), 0),
            to: PortRef::new(format!("n{i}"), 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
    }
    g.exits.push(NodeId::new(format!("n{}", n - 1)));
    g
}

fn measure(n: usize) -> (u128, usize) {
    let graph = graph_of_size(n);
    let start = Instant::now();
    let mut options = NormalizeOptions::default();
    options.limits.soft_node_warning = usize::MAX;
    let normalized = normalize(graph, &options).expect("valid");
    let scene = build_scene(&normalized, &LayoutOptions::canonical());
    let json = serde_json::to_string(&scene).expect("serializes");
    (start.elapsed().as_millis(), json.len())
}

#[test]
fn scene_construction_is_within_an_order_of_magnitude_of_the_targets() {
    // (nodes, §24's native target in ms for scene + SVG). Phase 2 has no SVG
    // writer, so the scene alone should be comfortably under.
    let cases = [(25usize, 25u128), (100, 100), (500, 1000)];
    for (n, target) in cases {
        let (elapsed, size) = measure(n);
        println!(
            "{n:>4} nodes: {elapsed:>5} ms, {size:>8} bytes of scene JSON (target {target} ms)"
        );
        assert!(
            elapsed < target * 10,
            "{n} nodes took {elapsed} ms against a {target} ms target — \
             more than an order of magnitude over, which is a real regression \
             rather than a slow machine"
        );
    }
}

/// The shape of the growth, which is what actually matters. Ten times the nodes
/// must not cost a hundred times the work outside the collision pass.
#[test]
fn cost_does_not_grow_catastrophically_with_size() {
    let (small, _) = measure(50);
    let (large, _) = measure(500);
    println!("50 nodes: {small} ms, 500 nodes: {large} ms");
    // Generous: the collision pass is quadratic by construction, so 100× is the
    // honest ceiling. Anything past it means something else went quadratic too.
    assert!(
        large <= (small.max(1)) * 150 + 1000,
        "500 nodes cost {large} ms against {small} ms for 50 — worse than the \
         quadratic collision pass alone explains"
    );
}
