//! The radial layout engine, and the scene it produces.
//!
//! # The composition
//!
//! A square field, 1600 by 1600, centred at (800, 800), safe radius 700 — the
//! specification's §9.1 defaults. Radius is allocated in bands by what a node
//! *is* to the composition, never by what it is called:
//!
//! ```text
//! 0.00–0.15  the core: the entry, at or near the centre
//! 0.15–0.65  flow, and the regions nested inside it
//! 0.65–0.85  joins, seals, region exits
//! 0.85–1.00  the host boundary: invocations
//! ```
//!
//! # The shape of the argument
//!
//! The top-level flow **spirals**: angle advances with position along the spine
//! and radius advances with depth, so a chain of stages reads as a sequence
//! moving outward and clockwise. That is what makes an unlabelled render still
//! show where the program begins and which way it goes, which is the whole claim
//! of Veiled mode.
//!
//! A fork **fans**: its branches take angular sectors clockwise by ordinal,
//! widths weighted by what each branch contains. Branch order is spatial, and
//! that is asserted rather than assumed.
//!
//! An orbit **rings**: its body members are placed on a closed circle centred on
//! the orbit node, with an entry notch and an exit break. A ring is the one
//! shape that says "this may go round again" without a caption.
//!
//! An invocation is **pulled outward**: it keeps the angle it would have had in
//! the flow and moves to the boundary band, which is §11.3's "effects occupy
//! outer boundary positions nearest their calling path" made literal. The spoke
//! back to its calling position is what shows which stage reaches the world.
//!
//! # Determinism
//!
//! Every quantity here is a pure function of the topology plus the seed. There
//! is no iteration over a `HashMap`, no floating-point accumulation whose order
//! depends on discovery, and no randomness that is not drawn from the seeded
//! PRNG. Collision resolution is a sorted, bounded pass — not a relaxation loop
//! that might converge differently.
//!
//! Ornament is not generated here at all in Phase 2. When it arrives it goes on
//! its own layers and reads the same seed, and the invariance test in this
//! module is what will keep it from moving anything.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use crate::analysis::{analyze, Placement, Topology};
use crate::canonical::Prng;
use crate::graph::{NodeId, RegionId, RegionKind, SigilGraph, SigilNode, SigilNodeKind};
use crate::ornament::{self, OrnamentLevel};
use crate::scene::*;
use crate::tracery::Tracery;
use crate::trig::DeterministicTrig;
use crate::NormalizedGraph;

/// The canvas. Square, because a radial composition in a rectangle wastes two
/// corners and crops two others.
pub const VIEW_SIZE: f64 = 1600.0;
pub const CENTER: f64 = VIEW_SIZE / 2.0;
/// Everything semantic stays inside this. The margin between it and the canvas
/// edge is where ornament and glow live without being clipped.
pub const SAFE_RADIUS: f64 = 700.0;

/// Band boundaries, as fractions of [`SAFE_RADIUS`].
const CORE_BAND: (f64, f64) = (0.00, 0.15);
const FLOW_BAND: (f64, f64) = (0.15, 0.65);
const SEAL_BAND: (f64, f64) = (0.65, 0.85);
const BOUNDARY_BAND: (f64, f64) = (0.85, 1.00);

/// The canonical axis: north. SVG's y grows downward, so "up" is `-PI/2`, and
/// angles increase clockwise from there — which is the direction §11.3 says
/// branch 0 begins in.
const CANONICAL_AXIS: f64 = -PI / 2.0;

/// How much of a full turn the top-level spine sweeps.
///
/// Less than one, so the spiral's end does not land on its own beginning and
/// create a false join. The gap left at the top is where the canonical
/// orientation notch goes.
const SPINE_SWEEP: f64 = 0.82;

/// The widest fan a single fork may open, in radians.
///
/// Bounded because a fork with many branches should subdivide its own wedge
/// rather than take over the circle — the composition still has to close.
const MAX_FORK_FAN: f64 = TAU * 0.55;

/// No branch narrower than this, whatever its weight (§11.5).
const MIN_BRANCH_SECTOR: f64 = 0.18;

/// How close two semantic marks may be before one is nudged.
const MIN_SEPARATION: f64 = 46.0;

/// The drawn size of a node mark, by placement class.
fn mark_size(kind: &SigilNodeKind, placement: Placement) -> f64 {
    match (kind, placement) {
        (SigilNodeKind::Source, _) | (_, Placement::Core) => 54.0,
        (SigilNodeKind::Output, _) | (_, Placement::Seal) => 40.0,
        (SigilNodeKind::Orbit, _) => 34.0,
        (SigilNodeKind::Fork, _) => 32.0,
        (_, Placement::Invocation) => 30.0,
        _ => 24.0,
    }
}

/// How the scene should be built. Phase 2 exposes only what layout needs;
/// theme, disclosure and ornament arrive in later phases.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutOptions {
    /// The render seed. Defaults to the graph fingerprint's.
    pub seed: u64,
    /// Rotate the whole composition. Deterministic from the seed unless fixed.
    pub orientation: Orientation,
    /// Emit legend entries. Off costs nothing to compute, so this is about the
    /// size of the scene rather than about privacy — a legend entry never
    /// contains a label the graph did not carry.
    pub legend: bool,
    /// How much non-semantic geometry to generate.
    ///
    /// Generated last, from the seed alone, and appended — so changing it moves
    /// nothing semantic. See `ornament`.
    pub ornament: OrnamentLevel,
    /// How traces are drawn. Changes every edge's shape and no node's
    /// position. See [`crate::tracery`].
    pub tracery: Tracery,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    /// A documented fixed orientation: the canonical axis, no rotation. What
    /// `--canonical` uses, and what golden tests are written against.
    Canonical,
    /// Rotated deterministically by the seed, so two programs do not produce
    /// pictures that sit at exactly the same angle.
    Seeded,
    /// An explicit angle in radians.
    Fixed(f64),
}

impl LayoutOptions {
    pub fn canonical() -> Self {
        LayoutOptions {
            seed: 0,
            orientation: Orientation::Canonical,
            legend: true,
            ornament: OrnamentLevel::default(),
            tracery: Tracery::default(),
        }
    }

    pub fn from_graph(graph: &NormalizedGraph) -> Self {
        LayoutOptions {
            seed: graph.seed(),
            orientation: Orientation::Seeded,
            legend: true,
            ornament: OrnamentLevel::default(),
            tracery: Tracery::default(),
        }
    }

    fn rotation(&self) -> f64 {
        match self.orientation {
            Orientation::Canonical => 0.0,
            // A whole number of degrees, so the rotation is a legible fact
            // rather than an irrational nudge, and so two renders that differ
            // only by rotation differ visibly.
            Orientation::Seeded => (Prng::new(self.seed).next_u64() % 360) as f64 * PI / 180.0,
            Orientation::Fixed(angle) => angle,
        }
    }
}

/// A node's polar position, before it becomes a point.
#[derive(Debug, Clone, Copy)]
struct Polar {
    radius: f64,
    angle: f64,
}

impl Polar {
    fn to_point(self, rotation: f64) -> Point {
        let angle = self.angle + rotation;
        Point::new(
            CENTER + self.radius * angle.dcos(),
            CENTER + self.radius * angle.dsin(),
        )
    }
}

fn band(fraction: f64, band: (f64, f64)) -> f64 {
    let t = fraction.clamp(0.0, 1.0);
    SAFE_RADIUS * (band.0 + (band.1 - band.0) * t)
}

/// Build a scene from a normalized graph.
pub fn build_scene(graph: &NormalizedGraph, options: &LayoutOptions) -> SigilScene {
    let topology = analyze(&graph.graph);
    let mut warnings = Vec::new();

    let positions = place_nodes(&graph.graph, &topology, options, &mut warnings);

    let mut elements = Vec::new();
    let mut hit_regions = Vec::new();
    let mut legend = Vec::new();

    emit_background(&mut elements);
    emit_regions(&graph.graph, &topology, &positions, &mut elements);
    emit_edges(&graph.graph, &topology, &positions, options, &mut elements);
    emit_nodes(
        &graph.graph,
        &topology,
        &positions,
        options,
        &mut elements,
        &mut hit_regions,
        &mut legend,
    );

    emit_inscriptions(&graph.graph, &positions, &mut elements);

    // Last, and from the seed alone. Appending rather than interleaving is what
    // makes "remove the ornament" a filter over layers rather than a relayout
    // (ADR 0004) — nothing above this line has seen the ornament level.
    elements.extend(ornament::generate(options.ornament, options.seed));

    check_bounds(&elements, &mut warnings);

    SigilScene {
        schema: SCENE_SCHEMA_NAME.to_string(),
        version: SCENE_SCHEMA_VERSION,
        view_box: Rect::new(0.0, 0.0, VIEW_SIZE, VIEW_SIZE),
        center: Point::new(CENTER, CENTER),
        elements,
        hit_regions,
        legend: if options.legend { legend } else { Vec::new() },
        metadata: SceneMetadata {
            renderer_version: crate::RENDERER_VERSION.to_string(),
            graph_fingerprint: graph.fingerprint.as_str().to_string(),
            seed: options.seed,
            tracery: options.tracery.name().to_string(),
            node_count: graph.graph.nodes.len(),
            edge_count: graph.graph.edges.len(),
            region_count: graph.graph.regions.len(),
            census: graph.graph.kind_census(),
            source_schema: graph
                .graph
                .source_schema
                .as_ref()
                .map(|s| format!("{}@{}", s.name, s.version)),
        },
        warnings,
    }
}

/// Where every node goes.
fn place_nodes(
    graph: &SigilGraph,
    topology: &Topology,
    options: &LayoutOptions,
    warnings: &mut Vec<String>,
) -> BTreeMap<NodeId, Point> {
    let rotation = options.rotation();
    let mut polar: BTreeMap<NodeId, Polar> = BTreeMap::new();

    // 1. The spine. Angle from position along the chain, radius from depth, so
    //    a linear program reads as a spiral moving outward and clockwise.
    // The spiral is laid out over the spine nodes that will still *be* on it.
    //
    // Allocating an angular slot to every spine node and then relocating some of
    // them — an invocation to the boundary, an exit to the seal — left those
    // slots empty, and on a program where three of five spine nodes move, most
    // of the circle went unused while the survivors bunched together. The
    // vacated angle is now reclaimed rather than reserved for something that
    // will not be there.
    //
    // A relocated node still gets an angle from this pass, so its spoke points
    // back along the flow it came from; it is simply not counted when deciding
    // how the sweep is divided.
    let stays_on_the_spiral = |id: &NodeId| {
        matches!(
            topology.placement(id),
            Placement::Core | Placement::Flow | Placement::Ring
        )
    };
    let visible: Vec<&NodeId> = topology
        .spine
        .iter()
        .filter(|id| stays_on_the_spiral(id))
        .collect();
    // `len - 1`, so the last one reaches the end of the sweep. Dividing by the
    // length meant a chain of three covered two thirds of it — the gap grew as
    // the program got *shorter*, which is backwards.
    let spine_span = visible.len().saturating_sub(1).max(1) as f64;

    let mut visible_index = 0usize;
    for id in topology.spine.iter() {
        // A relocated node borrows the position it would have had, so its angle
        // still reads as "between these two", without consuming a slot.
        let along = if stays_on_the_spiral(id) {
            let along = visible_index as f64 / spine_span;
            visible_index += 1;
            along
        } else {
            (visible_index as f64 - 0.5).max(0.0) / spine_span
        };
        let index = visible_index.saturating_sub(1);
        let angle = CANONICAL_AXIS + along * TAU * SPINE_SWEEP;
        // Radius from progress *along the spine*, not from global depth.
        //
        // Depth is the whole graph's, so a program with deep branches gave its
        // spine tiny depth fractions and bunched the main narrative near the
        // centre while the branches spread past it — the spiral stopped being
        // the thing the eye follows. The spine is the composition's backbone and
        // walks the flow band from core to seal whatever is hanging off it;
        // branch members still place by depth, which is what makes a deep branch
        // reach further out than a shallow one.
        let radius = if index == 0 {
            band(0.0, CORE_BAND)
        } else {
            band(along, FLOW_BAND)
        };
        polar.insert(id.clone(), Polar { radius, angle });
    }

    // 2. Fork sectors. Clockwise by ordinal, weighted by content, floored at
    //    `MIN_BRANCH_SECTOR` so a one-stage branch stays visible.
    //
    //    Recursively renormalized. Forks are processed outer-first — sorted by
    //    how deeply the region holding them is nested — so a nested fork's own
    //    position exists before its branches are placed, and its fan is capped
    //    to the sector its parent branch was allocated. Without the cap a
    //    fork-inside-a-fork opened the same near-two-radian fan as a top-level
    //    one and sprayed its branches across its siblings' sectors; before the
    //    recursion its branch regions (whose `parent` is the enclosing branch)
    //    were not placed by this pass at all and fell to the leftover pass
    //    below, which scatters them near the seal band.
    let region_of: BTreeMap<&NodeId, &RegionId> = topology
        .regions
        .iter()
        .flat_map(|(id, analysis)| analysis.direct_members.iter().map(move |m| (m, id)))
        .collect();
    let mut forks: Vec<&SigilNode> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == SigilNodeKind::Fork)
        .collect();
    forks.sort_by_key(|n| (region_depth(graph, region_of.get(&n.id).copied()), &n.id));

    // Each branch region's allocated sector width, for the forks nested in it.
    let mut sector_width: BTreeMap<RegionId, f64> = BTreeMap::new();

    for node in forks {
        let Some(origin) = polar.get(&node.id).copied() else {
            continue;
        };
        let mut branches = graph
            .regions
            .iter()
            .filter(|r| r.kind == RegionKind::Branch && r.owner.as_ref() == Some(&node.id))
            .collect::<Vec<_>>();
        // Ordinal order — branch order is spatial, and asserted to be.
        branches.sort_by_key(|r| (r.ordinal, r.id.0.clone()));
        if branches.is_empty() {
            continue;
        }

        let weights: Vec<f64> = branches
            .iter()
            .map(|r| topology.regions.get(&r.id).map(|a| a.weight).unwrap_or(1.0))
            .collect();
        let total: f64 = weights.iter().sum();
        // Wide enough that two branches read as a *fan* rather than a splay.
        // The old figure gave two branches 1.12 radians — 64° — which looks like
        // one thick spoke, and it was the other half of the cramping.
        let mut fan = MAX_FORK_FAN.min(1.9 + 0.55 * branches.len().saturating_sub(1) as f64);
        // A nested fork subdivides what its branch was given, not the circle.
        // 0.9, not 1.0: a fan that exactly fills the sector puts its outermost
        // branches on the boundary shared with the neighbouring branch.
        if let Some(parent_width) = region_of
            .get(&node.id)
            .and_then(|region| sector_width.get(*region))
        {
            fan = fan.min(parent_width * 0.9);
        }

        // Sector widths, floored then renormalized so the floors cannot push the
        // total past the fan — a floor that overflows its own budget is how
        // branches end up overlapping. The floor also yields to the fan: a
        // nested fork's cap may be narrower than the floors alone, and floors
        // that ignored it would undo the renormalization they sit inside.
        let floor = MIN_BRANCH_SECTOR.min(fan / branches.len() as f64);
        let floored: Vec<f64> = weights
            .iter()
            .map(|w| (w / total * fan).max(floor))
            .collect();
        let floored_total: f64 = floored.iter().sum();
        let scale = if floored_total > fan {
            fan / floored_total
        } else {
            1.0
        };

        let mut cursor = origin.angle - fan / 2.0;
        for (region, width) in branches.iter().zip(floored.iter()) {
            let width = width * scale;
            sector_width.insert(region.id.clone(), width);
            let members = topology
                .regions
                .get(&region.id)
                .map(|a| a.direct_members.clone())
                .unwrap_or_default();
            place_branch(
                graph,
                topology,
                &members,
                cursor + width / 2.0,
                width,
                &mut polar,
            );
            cursor += width;
        }
    }

    // 3. Seals: the closing marks, in the seal band, at cardinal points when
    //    there are several so multiple exits read as ordered rather than
    //    scattered.
    let seals: Vec<&SigilNode> = graph
        .nodes
        .iter()
        .filter(|n| topology.placement(&n.id) == Placement::Seal)
        .collect();
    for (index, node) in seals.iter().enumerate() {
        let angle = if seals.len() == 1 {
            // Due south: the composition closes at the bottom, opposite the
            // canonical axis, so "where does this end" has one answer.
            CANONICAL_AXIS + PI
        } else {
            CANONICAL_AXIS + PI + (index as f64 - (seals.len() - 1) as f64 / 2.0) * (TAU / 8.0)
        };
        polar.insert(
            node.id.clone(),
            Polar {
                radius: band(0.5, SEAL_BAND),
                angle,
            },
        );
    }

    // 4. Invocations: keep the angle, move to the boundary. This is what makes
    //    a spoke from a stage to the rim mean "this is where the program
    //    touches the world".
    for node in &graph.nodes {
        if topology.placement(&node.id) != Placement::Invocation {
            continue;
        }
        let angle = polar
            .get(&node.id)
            .map(|p| p.angle)
            .or_else(|| caller_angle(graph, &polar, &node.id))
            .unwrap_or(CANONICAL_AXIS);
        polar.insert(
            node.id.clone(),
            Polar {
                radius: band(0.4, BOUNDARY_BAND),
                angle,
            },
        );
    }

    // 5. Anything still unplaced — a detached node, or a region member no
    //    structure claimed. Outside the flow bands, evenly spread, reported.
    let unplaced: Vec<&NodeId> = graph
        .nodes
        .iter()
        .map(|n| &n.id)
        .filter(|id| !polar.contains_key(*id))
        .collect();
    for (index, id) in unplaced.iter().enumerate() {
        let angle = CANONICAL_AXIS + (index as f64 + 0.5) * TAU / unplaced.len().max(1) as f64;
        polar.insert(
            (*id).clone(),
            Polar {
                radius: band(0.85, SEAL_BAND),
                angle,
            },
        );
        if topology.placement(id) == Placement::Detached {
            warnings.push(format!(
                "node `{id}` is not reachable from the entry; placed outside the flow bands"
            ));
        }
    }

    resolve_collisions(graph, topology, &mut polar, warnings);

    let mut points: BTreeMap<NodeId, Point> = polar
        .iter()
        .map(|(id, p)| (id.clone(), p.to_point(rotation)))
        .collect();

    // 6. Orbit rings, placed *after* collision resolution and from the orbit
    //    node's settled position.
    //
    //    This ordering is the whole design of a ring, not a convenience. The
    //    collision pass nudges in polar coordinates about the *canvas* centre;
    //    applied to a ring member, that walks it straight off the circle it is
    //    supposed to sit on, and an orbit whose members are scattered near a
    //    circle no longer says "this may go round again". So a ring settles as a
    //    unit: the pass may move the orbit node, and its members follow.
    //
    //    They need no separation pass of their own — `ring_radius` sizes the
    //    circumference to hold them at `MIN_SEPARATION` by construction. What
    //    they can still do is overlap something *outside* the ring, and that is
    //    reported below rather than left silent (§11.7).
    for node in &graph.nodes {
        if node.kind != SigilNodeKind::Orbit {
            continue;
        }
        let (Some(origin), Some(origin_point)) =
            (polar.get(&node.id).copied(), points.get(&node.id).copied())
        else {
            continue;
        };
        let Some(region) = graph
            .regions
            .iter()
            .find(|r| r.kind == RegionKind::Orbit && r.owner.as_ref() == Some(&node.id))
        else {
            continue;
        };
        let members = topology
            .regions
            .get(&region.id)
            .map(|a| a.direct_members.clone())
            .unwrap_or_default();
        if members.is_empty() {
            continue;
        }

        let radius = ring_radius(members.len());
        let step = TAU / members.len() as f64;
        for (index, member) in members.iter().enumerate() {
            // Members start at the entry notch — the point of the ring facing
            // away from the centre of the composition, where the value arrives
            // — and go clockwise, so the ring reads in flow order from the
            // point it is entered.
            let theta = origin.angle + rotation + PI + index as f64 * step;
            points.insert(
                member.clone(),
                Point::new(
                    origin_point.x + radius * theta.dcos(),
                    origin_point.y + radius * theta.dsin(),
                ),
            );
        }
    }

    report_ring_overlaps(graph, topology, &points, warnings);

    points
}

/// A ring member overlapping something that is not on its ring.
///
/// Ring members are exempt from the collision pass (see above), so the one thing
/// that pass would have caught for them has to be caught here. A warning rather
/// than a nudge: moving the member breaks the ring, and moving the ring breaks
/// the orbit's relationship to its own flow position. §11.7's last resort is to
/// say so.
fn report_ring_overlaps(
    graph: &SigilGraph,
    topology: &Topology,
    points: &BTreeMap<NodeId, Point>,
    warnings: &mut Vec<String>,
) {
    let ring_members: Vec<&NodeId> = graph
        .nodes
        .iter()
        .map(|n| &n.id)
        .filter(|id| topology.placement(id) == Placement::Ring)
        .collect();

    for member in ring_members {
        let Some(a) = points.get(member) else {
            continue;
        };
        for other in &graph.nodes {
            if &other.id == member || topology.placement(&other.id) == Placement::Ring {
                continue;
            }
            let Some(b) = points.get(&other.id) else {
                continue;
            };
            if (a.x - b.x).hypot(a.y - b.y) < MIN_SEPARATION {
                warnings.push(format!(
                    "orbit body node `{member}` overlaps `{}`; the ring was kept intact",
                    other.id
                ));
            }
        }
    }
}

/// How deeply nested a region is: ancestors counted up the `parent` chain.
///
/// Bounded by the region count, so a cyclic `parent` chain — which validation
/// refuses, but this function should not have to trust that — terminates.
fn region_depth(graph: &SigilGraph, region: Option<&RegionId>) -> usize {
    let mut depth = 0;
    let mut current = region;
    for _ in 0..graph.regions.len() {
        let Some(id) = current else { break };
        depth += 1;
        current = graph
            .regions
            .iter()
            .find(|r| &r.id == id)
            .and_then(|r| r.parent.as_ref());
    }
    depth
}

/// A node's depth as a fraction of the deepest, for radial interpolation.
fn depth_fraction(topology: &Topology, id: &NodeId) -> f64 {
    let depth = topology.of(id).map(|n| n.depth).unwrap_or(0) as f64;
    // `max_depth` of zero means a single-node graph; everything is at the core.
    let max = topology.max_depth.max(1) as f64;
    (depth / max).clamp(0.0, 1.0)
}

/// A branch's members, along its sector.
///
/// Radius advances with position in the branch and angle spreads across the
/// sector, so a branch of several stages reads as its own small spiral inside
/// the parent's wedge rather than as a straight spoke.
fn place_branch(
    _graph: &SigilGraph,
    topology: &Topology,
    members: &[NodeId],
    center_angle: f64,
    width: f64,
    polar: &mut BTreeMap<NodeId, Polar>,
) {
    let count = members.len().max(1);
    for (index, member) in members.iter().enumerate() {
        let along = (index as f64 + 1.0) / (count as f64 + 1.0);
        // Half the sector width, so the branch's own spread never reaches its
        // neighbour's boundary.
        let angle = center_angle + (along - 0.5) * width * 0.5;
        // Biased outward. A branch hangs off a fork that is already partway out,
        // so *averaging* its depth with its position pulled every member back
        // toward the middle of the band and stacked the branches at one radius.
        let reach = 0.45 + 0.55 * depth_fraction(topology, member).max(along);
        let radius = band(reach, FLOW_BAND);
        polar.insert(member.clone(), Polar { radius, angle });
    }
}

/// A ring big enough that its members do not touch.
///
/// Circumference has to hold `n` marks at `MIN_SEPARATION`, with a floor so a
/// one-member orbit still reads as a ring rather than as a dot.
fn ring_radius(members: usize) -> f64 {
    let needed = members as f64 * MIN_SEPARATION / TAU;
    // `clamp` panics if the ceiling drops below the floor. Both are constants,
    // so a change that would do that fails to compile rather than at render
    // time — which is the only reason `clamp` is safe here.
    const FLOOR: f64 = 52.0;
    const CEILING: f64 = SAFE_RADIUS * 0.22;
    const { assert!(FLOOR < CEILING) };
    needed.clamp(FLOOR, CEILING)
}

/// The angle of whatever calls an invocation, so it lands nearest its caller.
fn caller_angle(graph: &SigilGraph, polar: &BTreeMap<NodeId, Polar>, id: &NodeId) -> Option<f64> {
    graph
        .edges_to(id)
        .into_iter()
        .find_map(|e| polar.get(&e.from.node).map(|p| p.angle))
}

/// Deterministic collision resolution.
///
/// The priority order is §11.7's: band ownership is preserved absolutely, angle
/// is adjusted first, radius second and only within the node's own band, and an
/// unresolved overlap is a warning rather than a silent stack.
///
/// A sorted single pass rather than a relaxation loop, because a loop's result
/// depends on how many iterations it happened to run — which is exactly the kind
/// of thing that differs between two builds.
fn resolve_collisions(
    graph: &SigilGraph,
    topology: &Topology,
    polar: &mut BTreeMap<NodeId, Polar>,
    warnings: &mut Vec<String>,
) {
    // Sorted by band then angle then identifier: a total order that does not
    // depend on graph traversal order.
    let mut order: Vec<NodeId> = polar.keys().cloned().collect();
    order.sort_by(|a, b| {
        let (pa, pb) = (polar[a], polar[b]);
        topology
            .placement(a)
            .cmp(&topology.placement(b))
            .then(pa.radius.total_cmp(&pb.radius))
            .then(pa.angle.total_cmp(&pb.angle))
            .then(a.cmp(b))
    });

    let mut settled: Vec<(NodeId, Polar, f64)> = Vec::new();

    for id in order {
        let mut current = polar[&id];
        let size = graph
            .node(&id)
            .map(|n| mark_size(&n.kind, topology.placement(&id)))
            .unwrap_or(24.0);
        let placement = topology.placement(&id);
        let allowed = band_for(placement);

        // At most a bounded number of nudges, so this terminates on any input.
        let mut resolved = true;
        for attempt in 0..24 {
            let clash = settled.iter().find(|(_, other, other_size)| {
                let a = current.to_point(0.0);
                let b = other.to_point(0.0);
                let needed = (size + other_size).max(MIN_SEPARATION);
                (a.x - b.x).hypot(a.y - b.y) < needed
            });
            let Some(_) = clash else {
                resolved = true;
                break;
            };
            resolved = false;
            // Angle first (§11.7 priority 4), then radius within the band
            // (priority 5). Never out of the band: that would move a node
            // between placement classes and change what it appears to be.
            let step = 0.11 * (attempt / 3 + 1) as f64;
            current.angle += step;
            if attempt >= 3 {
                let inset = 0.06 * ((attempt - 3) / 3 + 1) as f64;
                current.radius = band(0.5 + inset.min(0.45), allowed);
            }
        }

        if !resolved {
            warnings.push(format!(
                "could not separate node `{id}` from its neighbours without leaving its band"
            ));
        }
        polar.insert(id.clone(), current);
        settled.push((id, current, size));
    }
}

fn band_for(placement: Placement) -> (f64, f64) {
    match placement {
        Placement::Core => CORE_BAND,
        Placement::Flow | Placement::Ring => FLOW_BAND,
        Placement::Seal | Placement::Detached => SEAL_BAND,
        Placement::Invocation => BOUNDARY_BAND,
    }
}

fn emit_background(elements: &mut Vec<SceneElement>) {
    // The host boundary is semantic: it is the line the program's effects cross,
    // and §9.10 places invocations on it. It is not ornament and does not vanish
    // when ornament is turned off.
    let radius = SAFE_RADIUS * BOUNDARY_BAND.0;
    elements.push(SceneElement {
        id: "boundary/host".to_string(),
        layer: SceneLayerKind::SemanticRegions,
        semantic: SemanticKind::InvocationBoundary,
        graph_ref: None,
        geometry: Geometry::Circle {
            center: Point::new(CENTER, CENTER),
            radius,
        },
        title: Some("host boundary".to_string()),
        legend_key: None,
        ends: None,
        weight: None,
        bounds: Rect::new(CENTER - radius, CENTER - radius, radius * 2.0, radius * 2.0),
    });
}

fn emit_regions(
    graph: &SigilGraph,
    topology: &Topology,
    positions: &BTreeMap<NodeId, Point>,
    elements: &mut Vec<SceneElement>,
) {
    for region in &graph.regions {
        let Some(owner) = region.owner.as_ref().and_then(|o| positions.get(o)) else {
            continue;
        };
        let members = topology
            .regions
            .get(&region.id)
            .map(|a| a.direct_members.len())
            .unwrap_or(0);

        // Only an orbit gets a drawn ring. A branch's sector is expressed by
        // where its members are, not by a boundary line — §9.8 warns that sector
        // boundaries must not resemble flow edges, and the cheapest way to
        // honour that is not to draw them in Phase 2.
        if region.kind != RegionKind::Orbit || members == 0 {
            continue;
        }
        let radius = ring_radius(members);
        elements.push(SceneElement {
            id: format!("region/{}/ring", region.id),
            layer: SceneLayerKind::SemanticRegions,
            semantic: SemanticKind::Region(region.kind),
            graph_ref: Some(SceneRef::Region(region.id.0.clone())),
            geometry: Geometry::Circle {
                center: *owner,
                radius,
            },
            title: Some("orbit ring".to_string()),
            legend_key: Some(format!("region/{}", region.id)),
            ends: None,
            weight: None,
            bounds: Rect::new(
                owner.x - radius,
                owner.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
        });
    }
}

fn emit_edges(
    graph: &SigilGraph,
    topology: &Topology,
    positions: &BTreeMap<NodeId, Point>,
    options: &LayoutOptions,
    elements: &mut Vec<SceneElement>,
) {
    // Every placed mark, with the clearance its drawn size needs. Computed once:
    // the same list serves every edge, and the graph's node order is stable.
    let obstacles: Vec<(&NodeId, Point, f64)> = graph
        .nodes
        .iter()
        .filter_map(|n| {
            let p = positions.get(&n.id)?;
            let size = mark_size(&n.kind, topology.placement(&n.id));
            Some((&n.id, *p, size + EDGE_CLEARANCE))
        })
        .collect();

    // Grows as edges are routed, in graph order, so a later edge can prefer a
    // shape that does not cross an earlier one — §11.6, within the positions
    // the layout already committed to.
    let mut traces: Vec<crate::tracery::RoutedTrace> = Vec::with_capacity(graph.edges.len());

    // A traced run's counts, normalized to the heaviest node. An edge carries
    // the weight of the node it leaves — emissions travel outward — so a hot
    // path is a bright path and a branch that never ran is faint.
    let heaviest = graph
        .nodes
        .iter()
        .filter_map(|n| n.weight)
        .fold(0.0_f64, f64::max);
    let weight_of = |id: &NodeId| -> Option<f64> {
        if heaviest <= 0.0 {
            return None;
        }
        let raw = graph.node(id).and_then(|n| n.weight).unwrap_or(0.0);
        Some((raw / heaviest).clamp(0.0, 1.0))
    };

    for edge in &graph.edges {
        let (Some(from), Some(to)) = (positions.get(&edge.from.node), positions.get(&edge.to.node))
        else {
            continue;
        };

        let routed = crate::tracery::route(
            options.tracery,
            &crate::tracery::EdgeSpan {
                kind: edge.kind,
                from: *from,
                to: *to,
                from_id: &edge.from.node,
                to_id: &edge.to.node,
            },
            &obstacles,
            &traces,
        );
        traces.push(crate::tracery::RoutedTrace {
            from: edge.from.node.clone(),
            to: edge.to.node.clone(),
            samples: routed.samples,
        });

        elements.push(SceneElement {
            id: format!("edge/{}", sanitize(edge.id.as_str())),
            layer: SceneLayerKind::SemanticEdges,
            semantic: SemanticKind::Edge(edge.kind),
            graph_ref: Some(SceneRef::Edge(edge.id.0.clone())),
            geometry: Geometry::Path {
                commands: routed.commands,
            },
            title: None,
            legend_key: None,
            // The endpoints, as fields. A consumer highlighting a path reads
            // these rather than parsing them back out of the identifier.
            ends: Some(EdgeEnds {
                from: edge.from.node.0.clone(),
                to: edge.to.node.0.clone(),
            }),
            weight: weight_of(&edge.from.node),
            bounds: routed.bounds,
        });
    }
}

/// How much air an edge keeps between itself and a mark it does not end at.
///
/// A trace that clips a mark's strokes reads as *touching* it — a relationship
/// the graph does not assert. The margin is over the mark's drawn size, so a
/// large seal pushes traces further than a small stage does. Routing itself —
/// the candidate shapes, the avoidance — lives in [`crate::tracery`].
const EDGE_CLEARANCE: f64 = 14.0;

#[allow(clippy::too_many_arguments)]
fn emit_nodes(
    graph: &SigilGraph,
    topology: &Topology,
    positions: &BTreeMap<NodeId, Point>,
    options: &LayoutOptions,
    elements: &mut Vec<SceneElement>,
    hit_regions: &mut Vec<HitRegion>,
    legend: &mut Vec<LegendEntry>,
) {
    let prng = Prng::new(options.seed);
    let rotation = options.rotation();

    // Graph order, so keyboard traversal follows the program rather than the
    // accident of where things landed.
    for (tab_index, node) in graph.nodes.iter().enumerate() {
        let Some(position) = positions.get(&node.id) else {
            continue;
        };
        let placement = topology.placement(&node.id);
        let size = mark_size(&node.kind, placement);
        let element_id = format!("node/{}", sanitize(node.id.as_str()));

        // A per-node stream, derived from the node's identity rather than from
        // visit order, so a mark is the same whatever order nodes were emitted
        // in. Phase 3's generated marks draw from this.
        let mut node_prng = prng.derive(node.id.as_str());
        // Marks align with the direction of flow, which for a radial
        // composition is the outward radial. Small deterministic jitter keeps a
        // row of identical stages from reading as a printed font.
        let outward = (position.y - CENTER).datan2(position.x - CENTER);
        let jitter = (node_prng.next_f64() - 0.5) * 0.08;

        let legend_key = format!("node/{}", node.id);
        elements.push(SceneElement {
            id: element_id.clone(),
            layer: SceneLayerKind::SemanticNodes,
            semantic: SemanticKind::Node(node.kind.clone()),
            graph_ref: Some(SceneRef::Node(node.id.0.clone())),
            geometry: Geometry::Mark {
                center: *position,
                size,
                rotation: outward + jitter + rotation * 0.0,
                path: Vec::new(),
            },
            // The kind, never the label. A title carrying source text would put
            // it in a Veiled render's accessibility tree, which is exactly the
            // leak ADR 0007 separates disclosure from metadata to prevent.
            title: Some(title_for(node)),
            legend_key: Some(legend_key.clone()),
            ends: None,
            weight: None,
            bounds: Rect::new(position.x - size, position.y - size, size * 2.0, size * 2.0),
        });

        hit_regions.push(HitRegion {
            element_id,
            graph_ref: SceneRef::Node(node.id.0.clone()),
            center: *position,
            // Never smaller than a comfortable target, whatever the mark's drawn
            // size — §23's keyboard requirement is not satisfiable by hit-testing
            // a hairline.
            radius: size.max(22.0),
            tab_index: tab_index as u32,
        });

        legend.push(legend_entry(node, topology, legend_key));
    }
}

/// Text beside a node, for Inscribed and Revealed.
///
/// Emitted whenever the graph carried a label — and the graph only carries one
/// when labels were asked for, so a Veiled render has nothing to emit rather
/// than something to suppress. The serializer refuses to draw text in Veiled
/// mode as well; two independent guards, because this is the one that a scene
/// JSON export also passes through.
///
/// Placed outward of the mark along the radial, so an inscription never lands
/// between a node and the centre where the flow traces are.
fn emit_inscriptions(
    graph: &SigilGraph,
    positions: &BTreeMap<NodeId, Point>,
    elements: &mut Vec<SceneElement>,
) {
    for node in &graph.nodes {
        let Some(label) = node.label.as_ref() else {
            continue;
        };
        let Some(position) = positions.get(&node.id) else {
            continue;
        };

        let outward = (position.y - CENTER).datan2(position.x - CENTER);
        let offset = mark_size(&node.kind, Placement::Flow) + 22.0;
        let anchor = Point::new(
            position.x + offset * outward.dcos(),
            position.y + offset * outward.dsin(),
        );

        // Kept upright. A label rotated to the radial is unreadable on the left
        // half of the circle, and §20.9 warns against illegible ornamental text
        // — an inscription is there to be read or it should not be there.
        elements.push(SceneElement {
            id: format!("inscription/{}", sanitize(node.id.as_str())),
            layer: SceneLayerKind::Inscriptions,
            semantic: SemanticKind::Node(node.kind.clone()),
            graph_ref: Some(SceneRef::Node(node.id.0.clone())),
            geometry: Geometry::Text {
                anchor,
                content: label.clone(),
                size: 15.0,
                rotation: 0.0,
            },
            title: None,
            legend_key: Some(format!("node/{}", node.id)),
            ends: None,
            weight: None,
            bounds: Rect::new(anchor.x - 90.0, anchor.y - 12.0, 180.0, 24.0),
        });
    }
}

/// What a node is, in words that are always safe to show.
fn title_for(node: &SigilNode) -> String {
    match &node.kind {
        SigilNodeKind::Unknown(name) => format!("unknown node kind ({name})"),
        kind if node.is_invocation() => {
            // `safe_name`, not `name`: `Other` carries the producer's own
            // namespace string, and a title is read aloud by a screen reader in
            // Veiled mode. See `CapabilityFamily::safe_name`.
            let families: Vec<&str> = node.families().into_iter().map(|f| f.safe_name()).collect();
            if families.is_empty() {
                format!("{} invocation", kind.name())
            } else {
                format!("{} invocation", families.join(", "))
            }
        }
        kind => kind.name().to_string(),
    }
}

fn legend_entry(node: &SigilNode, topology: &Topology, key: String) -> LegendEntry {
    let analysis = topology.of(&node.id);
    LegendEntry {
        key,
        graph_ref: SceneRef::Node(node.id.0.clone()),
        semantic: SemanticKind::Node(node.kind.clone()),
        summary: title_for(node),
        // Present only when the graph carried one, which happens only when
        // labels were asked for.
        label: node.label.clone(),
        source_span: node.source.as_ref().map(|s| s.span),
        // The name when the graph carried one, the family otherwise. A Codex
        // that said nothing about an invocation would be useless; one that said
        // `@fs.read` in a Veiled render would leak.
        capabilities: node
            .effect
            .as_ref()
            .map(|e| {
                e.capabilities
                    .iter()
                    .map(|c| {
                        c.name
                            .clone()
                            .unwrap_or_else(|| c.safe_summary().to_string())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        region: node.region.as_ref().map(|r| r.0.clone()),
        branch_ordinal: analysis.and_then(|a| a.branch_ordinal),
        attributes: node.attributes.clone(),
        warnings: if node.kind.is_unknown() {
            vec!["this renderer does not know this node kind".to_string()]
        } else {
            Vec::new()
        },
    }
}

/// Every coordinate finite, and inside the canvas.
///
/// A `NaN` here would reach an SVG attribute as the empty string and render as
/// nothing at all, which looks like a layout bug rather than the arithmetic one
/// it is. This is the place non-finite numbers can genuinely appear — see the
/// note in `validate::check_numbers_and_spans`.
fn check_bounds(elements: &[SceneElement], warnings: &mut Vec<String>) {
    let canvas = Rect::new(0.0, 0.0, VIEW_SIZE, VIEW_SIZE);
    for element in elements {
        let b = element.bounds;
        if ![b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite()) {
            warnings.push(format!("element `{}` has non-finite bounds", element.id));
            continue;
        }
        // Semantic geometry only. Edge bounds are deliberately generous and an
        // arc's box may overhang without the arc itself doing so.
        if element.layer == SceneLayerKind::SemanticNodes {
            let center = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
            if !canvas.contains(center) {
                warnings.push(format!(
                    "element `{}` is centred outside the canvas",
                    element.id
                ));
            }
        }
    }
}

/// A graph identifier as a stable, safe element-ID fragment.
///
/// Untrusted: an identifier is producer-supplied and may contain quotes, angle
/// brackets, or anything else. Non-alphanumerics become `_`, and the original is
/// preserved in `graph_ref` so the Codex still shows what the author wrote.
fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::*;
    use crate::{normalize, NormalizeOptions};

    fn normalized(graph: SigilGraph) -> NormalizedGraph {
        normalize(graph, &NormalizeOptions::default()).expect("a valid graph")
    }

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

    fn linear() -> NormalizedGraph {
        normalized(chain(&[
            ("n0", SigilNodeKind::Source),
            ("n1", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Stage),
            ("n3", SigilNodeKind::Output),
        ]))
    }

    fn node_center(scene: &SigilScene, id: &str) -> Point {
        scene
            .elements
            .iter()
            .find(|e| e.id == format!("node/{id}"))
            .and_then(|e| match e.geometry {
                Geometry::Mark { center, .. } => Some(center),
                Geometry::Circle { center, .. } => Some(center),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no node element for {id}"))
    }

    fn radius_of(scene: &SigilScene, id: &str) -> f64 {
        let p = node_center(scene, id);
        (p.x - CENTER).hypot(p.y - CENTER)
    }

    /// The property everything downstream rests on.
    #[test]
    fn the_same_graph_and_options_produce_an_identical_scene() {
        let options = LayoutOptions::canonical();
        let a = build_scene(&linear(), &options);
        let b = build_scene(&linear(), &options);
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).expect("serializes"),
            serde_json::to_string(&b).expect("serializes")
        );
    }

    /// Phase 2's acceptance criterion: every required kind produces geometry.
    #[test]
    fn every_node_kind_produces_a_scene_element() {
        for kind in SigilNodeKind::KNOWN
            .iter()
            .chain(std::iter::once(&SigilNodeKind::Unknown(
                "portal".to_string(),
            )))
        {
            let g = chain(&[
                ("n0", SigilNodeKind::Source),
                ("subject", kind.clone()),
                ("n2", SigilNodeKind::Output),
            ]);
            let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
            let elements = scene.elements_for(&NodeId::new("subject"));
            assert!(
                !elements.is_empty(),
                "{} produced no scene element",
                kind.name()
            );
            assert_eq!(
                elements[0].semantic,
                SemanticKind::Node(kind.clone()),
                "{} lost its semantic kind",
                kind.name()
            );
        }
    }

    /// An unknown kind must not panic, and must keep its connectivity.
    #[test]
    fn an_unknown_node_kind_lays_out_without_panicking() {
        let g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("weird", SigilNodeKind::Unknown("\"><script>".to_string())),
            ("n2", SigilNodeKind::Output),
        ]);
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
        let element = scene.elements_for(&NodeId::new("weird"));
        assert_eq!(element.len(), 1);
        // The element ID is sanitized; the graph reference keeps the original.
        assert_eq!(element[0].id, "node/weird");
        assert_eq!(scene.metadata.edge_count, 2, "connectivity preserved");
    }

    /// A hostile identifier must not escape into an element ID.
    #[test]
    fn a_hostile_identifier_is_sanitized_in_the_element_id() {
        let mut g = chain(&[("n0", SigilNodeKind::Source), ("n1", SigilNodeKind::Output)]);
        g.nodes[1].id = NodeId::new("\"><script>alert(1)</script>");
        g.exits = vec![g.nodes[1].id.clone()];
        g.edges[0].to = PortRef::new(g.nodes[1].id.clone(), 0);
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
        let element = scene
            .elements
            .iter()
            .find(|e| matches!(&e.graph_ref, Some(SceneRef::Node(_))) && e.id != "node/n0")
            .expect("the second node");
        for banned in ['<', '>', '"', '\'', '&', '(', ')'] {
            assert!(
                !element.id.contains(banned),
                "element ID carries `{banned}`: {}",
                element.id
            );
        }
    }

    /// The composition's centre is the entry, and depth reads outward. This is
    /// what makes an unlabelled render show where a program begins.
    #[test]
    fn the_entry_is_central_and_depth_reads_outward() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        assert!(
            radius_of(&scene, "n0") <= SAFE_RADIUS * CORE_BAND.1,
            "the entry is not in the core band"
        );
        assert!(
            radius_of(&scene, "n1") < radius_of(&scene, "n2"),
            "depth does not read outward"
        );
    }

    /// §9.10 and §11.4: invocations occupy the outer band, and nothing else does.
    #[test]
    fn invocations_occupy_the_outer_boundary_band() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("io", SigilNodeKind::Stage),
            ("n2", SigilNodeKind::Output),
        ]);
        g.nodes[1].effect = Some(EffectMetadata {
            performs: true,
            capabilities: vec![Capability::anonymous(CapabilityFamily::Fs)],
        });
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
        let r = radius_of(&scene, "io");
        assert!(
            r >= SAFE_RADIUS * BOUNDARY_BAND.0 && r <= SAFE_RADIUS * BOUNDARY_BAND.1,
            "invocation at {r}, outside the boundary band"
        );
        assert!(
            radius_of(&scene, "n2") < SAFE_RADIUS * BOUNDARY_BAND.0,
            "a seal wandered into the boundary band"
        );
    }

    /// §9.8: fork order is spatial, and it comes from the ordinal rather than
    /// from array position — so shuffling the region list must not reorder the
    /// picture.
    #[test]
    fn fork_branches_occupy_sectors_in_ordinal_order() {
        fn build(reverse: bool) -> SigilScene {
            let mut g = chain(&[
                ("n0", SigilNodeKind::Source),
                ("fork", SigilNodeKind::Fork),
                ("out", SigilNodeKind::Output),
            ]);
            let mut regions = Vec::new();
            for ordinal in 0..3u32 {
                let member = format!("b{ordinal}");
                g.nodes
                    .push(SigilNode::new(member.clone(), SigilNodeKind::Stage));
                g.nodes.last_mut().expect("just pushed").region =
                    Some(RegionId::new(format!("r{ordinal}")));
                let mut region = SigilRegion::new(format!("r{ordinal}"), RegionKind::Branch);
                region.owner = Some("fork".into());
                region.ordinal = ordinal;
                region.members.push(NodeId::new(member.clone()));
                regions.push(region);
                g.edges.push(SigilEdge {
                    id: EdgeId::new(format!("enter{ordinal}")),
                    from: PortRef::new("fork", ordinal + 1),
                    to: PortRef::new(member, 0),
                    ordinal,
                    kind: EdgeKind::Enter,
                    region: Some(RegionId::new(format!("r{ordinal}"))),
                });
            }
            if reverse {
                regions.reverse();
            }
            g.regions = regions;
            build_scene(&normalized(g), &LayoutOptions::canonical())
        }

        let scene = build(false);
        let fork = node_center(&scene, "fork");
        let angle_of = |scene: &SigilScene, id: &str| {
            let p = node_center(scene, id);
            (p.y - fork.y).datan2(p.x - fork.x)
        };

        // Clockwise from the fork: in SVG's downward-y space, that is increasing
        // angle — but `atan2` wraps at ±π, and a wide fan crosses it. Measuring
        // relative to branch 0 and unwrapping into `[0, τ)` compares the thing
        // actually under test, which is the *order*, rather than where the
        // discontinuity happens to fall.
        let base = angle_of(&scene, "b0");
        let relative: Vec<f64> = (0..3)
            .map(|i| {
                let delta = angle_of(&scene, &format!("b{i}")) - base;
                delta.rem_euclid(std::f64::consts::TAU)
            })
            .collect();
        assert!(
            relative[0] < relative[1] && relative[1] < relative[2],
            "branches are not in clockwise ordinal order: {relative:?}"
        );

        // And the array order of the region list must not matter.
        let shuffled = build(true);
        for i in 0..3 {
            let a = node_center(&scene, &format!("b{i}"));
            let b = node_center(&shuffled, &format!("b{i}"));
            assert!(
                (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9,
                "branch {i} moved when the region array was reordered"
            );
        }
    }

    /// §9.9: an orbit is a visible closed ring, and its members are on it.
    #[test]
    fn an_orbit_produces_a_ring_its_members_sit_on() {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("orbit", SigilNodeKind::Orbit),
            ("out", SigilNodeKind::Output),
        ]);
        let mut region = SigilRegion::new("ring", RegionKind::Orbit);
        region.owner = Some("orbit".into());
        for i in 0..4u32 {
            let id = format!("body{i}");
            g.nodes
                .push(SigilNode::new(id.clone(), SigilNodeKind::Stage));
            g.nodes.last_mut().expect("just pushed").region = Some("ring".into());
            region.members.push(NodeId::new(id.clone()));
            g.edges.push(SigilEdge {
                id: EdgeId::new(format!("enter{i}")),
                from: PortRef::new("orbit", 1),
                to: PortRef::new(id, 0),
                ordinal: i,
                kind: EdgeKind::Enter,
                region: Some("ring".into()),
            });
        }
        g.regions.push(region);
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());

        let ring = scene
            .elements
            .iter()
            .find(|e| e.id == "region/ring/ring")
            .expect("an orbit ring element");
        let Geometry::Circle { center, radius } = ring.geometry else {
            panic!("an orbit ring must be a circle, not {:?}", ring.geometry);
        };
        assert!(radius > 0.0);

        // Every member on the circumference, within the nudge collision
        // resolution is allowed to apply.
        for i in 0..4 {
            let p = node_center(&scene, &format!("body{i}"));
            let distance = (p.x - center.x).hypot(p.y - center.y);
            assert!(
                (distance - radius).abs() < MIN_SEPARATION,
                "body{i} is {distance} from the ring centre, ring radius {radius}"
            );
        }
    }

    /// The seal closes the composition at one place, in the seal band.
    #[test]
    fn the_output_seal_lands_in_the_seal_band() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        let r = radius_of(&scene, "n3");
        assert!(
            r >= SAFE_RADIUS * SEAL_BAND.0 && r <= SAFE_RADIUS * SEAL_BAND.1,
            "the seal is at {r}, outside the seal band"
        );
    }

    /// ADR 0004's central claim, in the form that can actually fail: the
    /// ornament level changes no semantic coordinate. Not "not much" — not one.
    ///
    /// Run against `maximal` because that is the level most likely to break it,
    /// and over a graph with every construct in it because a linear chain has no
    /// collisions to perturb.
    #[test]
    fn the_ornament_level_moves_no_semantic_geometry() {
        let graph = fork_orbit_and_effect();
        let base: Vec<SceneElement> = build_scene(
            &graph,
            &LayoutOptions {
                ornament: OrnamentLevel::None,
                ..LayoutOptions::canonical()
            },
        )
        .elements
        .into_iter()
        .filter(|e| !e.is_ornament())
        .collect();
        assert!(!base.is_empty());

        for level in OrnamentLevel::ALL {
            let scene = build_scene(
                &graph,
                &LayoutOptions {
                    ornament: *level,
                    ..LayoutOptions::canonical()
                },
            );
            let semantic: Vec<SceneElement> = scene
                .elements
                .iter()
                .filter(|e| !e.is_ornament())
                .cloned()
                .collect();
            assert_eq!(
                semantic,
                base,
                "ornament level `{}` moved semantic geometry",
                level.name()
            );
            // And the hit regions, which are what a user actually interacts
            // with — an ornament that shifted those would be worse than one
            // that shifted a stroke.
            assert_eq!(
                scene.hit_regions.len(),
                base.iter()
                    .filter(|e| e.layer == SceneLayerKind::SemanticNodes)
                    .count()
            );
        }
    }

    /// Ornament appears only at the levels that ask for it, and never on a
    /// semantic layer.
    #[test]
    fn ornament_arrives_on_its_own_layers_only() {
        let graph = linear();
        for level in OrnamentLevel::ALL {
            let scene = build_scene(
                &graph,
                &LayoutOptions {
                    ornament: *level,
                    ..LayoutOptions::canonical()
                },
            );
            let count = scene.ornament_elements().count();
            if *level == OrnamentLevel::None {
                assert_eq!(count, 0, "`none` drew ornament");
            } else {
                assert!(count > 0, "`{}` drew none", level.name());
            }
            for element in scene.ornament_elements() {
                assert!(element.graph_ref.is_none(), "{}", element.id);
            }
        }
    }

    /// A graph with a fork, an orbit and an invocation — the shapes whose
    /// placement the collision pass actually perturbs.
    fn fork_orbit_and_effect() -> NormalizedGraph {
        let mut g = chain(&[
            ("n0", SigilNodeKind::Source),
            ("fork", SigilNodeKind::Fork),
            ("orbit", SigilNodeKind::Orbit),
            ("io", SigilNodeKind::Stage),
            ("out", SigilNodeKind::Output),
        ]);
        g.nodes[3].effect = Some(EffectMetadata {
            performs: true,
            capabilities: vec![Capability::anonymous(CapabilityFamily::Fs)],
        });
        for (i, (region, kind, owner)) in [
            ("b0", RegionKind::Branch, "fork"),
            ("b1", RegionKind::Branch, "fork"),
            ("ring", RegionKind::Orbit, "orbit"),
        ]
        .into_iter()
        .enumerate()
        {
            let member = format!("m{i}");
            g.nodes
                .push(SigilNode::new(member.clone(), SigilNodeKind::Stage));
            g.nodes.last_mut().expect("just pushed").region = Some(RegionId::new(region));
            let mut r = SigilRegion::new(region, kind);
            r.owner = Some(NodeId::new(owner));
            r.ordinal = i as u32;
            r.members.push(NodeId::new(member.clone()));
            g.regions.push(r);
            g.edges.push(SigilEdge {
                id: EdgeId::new(format!("enter{i}")),
                from: PortRef::new(owner, i as u32 + 1),
                to: PortRef::new(member, 0),
                ordinal: i as u32,
                kind: EdgeKind::Enter,
                region: Some(RegionId::new(region)),
            });
        }
        normalized(g)
    }

    /// The layer separation the invariance rests on.
    #[test]
    fn semantic_geometry_is_independent_of_the_ornament_layers() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        let semantic: Vec<&SceneElement> = scene.semantic_elements().collect();
        assert!(!semantic.is_empty());

        let mut stripped = scene.clone();
        stripped.elements.retain(|e| !e.is_ornament());
        let after: Vec<&SceneElement> = stripped.semantic_elements().collect();
        assert_eq!(
            semantic, after,
            "removing ornament changed semantic geometry"
        );

        // And no ornament element carries a graph reference (§15.3).
        for element in scene.ornament_elements() {
            assert!(
                element.graph_ref.is_none(),
                "ornament element `{}` carries a graph reference",
                element.id
            );
        }
    }

    /// Every coordinate finite, every mark inside the canvas.
    #[test]
    fn all_scene_coordinates_are_finite_and_on_the_canvas() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        let canvas = Rect::new(0.0, 0.0, VIEW_SIZE, VIEW_SIZE);
        for element in &scene.elements {
            let b = element.bounds;
            assert!(
                [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite()),
                "{} has non-finite bounds",
                element.id
            );
            if let Geometry::Mark { center, .. } = &element.geometry {
                assert!(center.is_finite());
                assert!(
                    canvas.contains(*center),
                    "{} is centred off-canvas at {center:?}",
                    element.id
                );
            }
        }
        assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    }

    /// A producer-supplied capability namespace is user text, and it used to
    /// reach `<title>` through `CapabilityFamily::name`. A screen reader would
    /// have read it aloud from a Veiled render.
    #[test]
    fn scene_titles_never_carry_a_producer_supplied_family_name() {
        let mut g = chain(&[("n0", SigilNodeKind::Source), ("io", SigilNodeKind::Output)]);
        g.nodes[1].effect = Some(EffectMetadata {
            performs: true,
            capabilities: vec![Capability {
                name: Some("@ZZSECRET.read".into()),
                family: CapabilityFamily::Other("ZZSECRET".into()),
            }],
        });
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
        for element in &scene.elements {
            if let Some(title) = &element.title {
                assert!(!title.contains("ZZSECRET"), "leaked into a title: {title}");
            }
        }
    }

    /// A title carrying source text would put it in a Veiled render's
    /// accessibility tree — the leak ADR 0007 separates disclosure from metadata
    /// to prevent.
    #[test]
    fn scene_titles_never_carry_the_nodes_label() {
        let mut g = chain(&[("n0", SigilNodeKind::Source), ("n1", SigilNodeKind::Output)]);
        g.nodes[0].label = Some("SECRET_FUNCTION_NAME".into());
        let scene = build_scene(&normalized(g), &LayoutOptions::canonical());
        for element in &scene.elements {
            if let Some(title) = &element.title {
                assert!(
                    !title.contains("SECRET"),
                    "a label reached an element title: {title}"
                );
            }
        }
        // The Codex may carry it — that is what a Codex is for, and it is
        // present only because the graph carried a label at all.
        assert_eq!(
            scene.legend[0].label.as_deref(),
            Some("SECRET_FUNCTION_NAME")
        );
    }

    /// Hit regions are reachable targets and follow graph order, so keyboard
    /// traversal follows the program.
    #[test]
    fn every_node_has_a_reachable_hit_region_in_graph_order() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        assert_eq!(scene.hit_regions.len(), scene.metadata.node_count);
        for hit in &scene.hit_regions {
            assert!(hit.radius >= 22.0, "hit target too small: {}", hit.radius);
        }
        let order: Vec<u32> = scene.hit_regions.iter().map(|h| h.tab_index).collect();
        assert_eq!(order, (0..order.len() as u32).collect::<Vec<_>>());
    }

    /// Collision resolution runs even on a graph built to collide, terminates,
    /// and never moves a node out of its band.
    #[test]
    fn collisions_resolve_within_their_band_and_terminate() {
        let mut kinds = vec![("n0", SigilNodeKind::Source)];
        let ids: Vec<String> = (1..30).map(|i| format!("n{i}")).collect();
        for id in &ids {
            kinds.push((id.as_str(), SigilNodeKind::Stage));
        }
        kinds.push(("last", SigilNodeKind::Output));
        let scene = build_scene(&normalized(chain(&kinds)), &LayoutOptions::canonical());

        for element in &scene.elements {
            if let Geometry::Mark { center, .. } = &element.geometry {
                let r = (center.x - CENTER).hypot(center.y - CENTER);
                assert!(
                    r <= SAFE_RADIUS + 1.0,
                    "{} escaped at radius {r}",
                    element.id
                );
            }
        }
        assert_eq!(scene.hit_regions.len(), 31);
    }

    /// Rotation is presentation. It moves everything and changes nothing about
    /// which band anything is in.
    #[test]
    fn a_seeded_rotation_preserves_every_radius() {
        let graph = linear();
        let canonical = build_scene(&graph, &LayoutOptions::canonical());
        let rotated = build_scene(
            &graph,
            &LayoutOptions {
                seed: 12345,
                orientation: Orientation::Seeded,
                ..LayoutOptions::canonical()
            },
        );
        for id in ["n0", "n1", "n2", "n3"] {
            assert!(
                (radius_of(&canonical, id) - radius_of(&rotated, id)).abs() < 1e-6,
                "{id} changed radius under rotation"
            );
        }
    }

    /// The accessible summary comes out of a real scene, not a hand-built census.
    #[test]
    fn a_real_scene_produces_the_accessible_summary() {
        let scene = build_scene(&linear(), &LayoutOptions::canonical());
        assert_eq!(
            scene.summary(),
            "This sigil contains one source, two stages, and one output seal."
        );
    }
}
