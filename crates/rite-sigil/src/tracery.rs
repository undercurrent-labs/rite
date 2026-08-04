//! How traces are drawn.
//!
//! The layout decides where every mark *is*; this module decides what the line
//! between two of them looks like. Three traceries:
//!
//! - **Flowing** (the default): a cubic bowed toward the canvas centre, so a
//!   trace reads as an arc of the composition rather than a chord across it.
//! - **Concentric**: a radial run, an arc of a circle centred on the
//!   composition, and a radial run — angular at the joints, circular in the
//!   travel, the way an astrolabe is.
//! - **Circuit**: orthogonal runs at right angles, with a small via dot at
//!   every bend — and one on a straight run, so even a bendless trace reads as
//!   a conductor rather than a wire.
//!
//! Every tracery routes around marks. A trace that clips a mark it does not
//! end at reads as *touching* it — a relationship the graph never asserted —
//! so each router tries its house shape first and then walks a fixed candidate
//! order until one clears every mark, falling back to the least-bad candidate
//! when nothing does (§11.7's posture, applied to traces). Everything here is
//! a pure function of the endpoints, the obstacle list and the edge kind:
//! no PRNG, no iteration order that depends on discovery.

use std::f64::consts::{PI, TAU};

use crate::graph::{EdgeKind, NodeId};
use crate::layout::{CENTER, SAFE_RADIUS};
use crate::scene::{PathCommand, Point, Rect};

/// The trace style for a whole render. An axis like theme or ornament: it
/// changes how every edge is drawn and nothing about where any node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tracery {
    #[default]
    Flowing,
    Concentric,
    Circuit,
}

impl Tracery {
    pub const ALL: &'static [Tracery] = &[Tracery::Flowing, Tracery::Concentric, Tracery::Circuit];

    pub fn parse(name: &str) -> Option<Tracery> {
        match name {
            "flowing" => Some(Tracery::Flowing),
            "concentric" => Some(Tracery::Concentric),
            "circuit" => Some(Tracery::Circuit),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tracery::Flowing => "flowing",
            Tracery::Concentric => "concentric",
            Tracery::Circuit => "circuit",
        }
    }
}

/// How much air an edge keeps between itself and a mark it does not end at.
/// The margin is over the mark's drawn size, so a large seal pushes traces
/// further than a small stage does.
const EDGE_CLEARANCE: f64 = 14.0;

/// The bow magnitude an adjusted flowing edge may not exceed. A cubic with
/// both controls at 0.66 toward the control point stays within `0.75 · pull`
/// of the chord, so this keeps every candidate within half a chord of it.
const MAX_BOW: f64 = 0.48;

/// The drawn radius of a circuit via dot.
const VIA_RADIUS: f64 = 4.5;

/// A routed edge: the path, a bounding box computed from the points the
/// router actually sampled — so a detour is inside its own bounds by
/// construction rather than by a generous guess — and the samples themselves,
/// which the caller feeds back as [`RoutedTrace`]s so later edges can avoid
/// crossing earlier ones.
pub struct RoutedEdge {
    pub commands: Vec<PathCommand>,
    pub bounds: Rect,
    pub samples: Vec<Point>,
}

/// An already-routed edge, as the crossing check sees it.
pub struct RoutedTrace {
    pub from: NodeId,
    pub to: NodeId,
    pub samples: Vec<Point>,
}

/// One candidate route: its path, and the points clearance is checked at.
struct Candidate {
    commands: Vec<PathCommand>,
    samples: Vec<Point>,
}

/// One edge, as the router sees it: what kind of trace, between which points,
/// connecting which marks.
#[derive(Clone, Copy)]
pub struct EdgeSpan<'a> {
    pub kind: EdgeKind,
    pub from: Point,
    pub to: Point,
    pub from_id: &'a NodeId,
    pub to_id: &'a NodeId,
}

pub fn route(
    tracery: Tracery,
    span: &EdgeSpan,
    obstacles: &[(&NodeId, Point, f64)],
    routed: &[RoutedTrace],
) -> RoutedEdge {
    let EdgeSpan {
        kind,
        from,
        to,
        from_id,
        to_id,
    } = *span;
    let chord = (from.x - to.x).hypot(from.y - to.y);

    // Only marks near enough to this edge to matter. Every router's detours
    // stay within roughly half a chord of the direct line, except a concentric
    // feedback return, which is bounded separately below.
    let reach = chord * 0.6 + EDGE_CLEARANCE + 120.0;
    let near: Vec<(Point, f64)> = obstacles
        .iter()
        .filter(|(id, _, _)| *id != from_id && *id != to_id)
        .filter(|(_, p, clearance)| distance_to_segment(*p, from, to) <= reach + clearance)
        .map(|(_, p, clearance)| (*p, *clearance))
        .collect();

    // Earlier traces this one could cross. Traces sharing an endpoint are
    // excluded — they *meet* at the shared mark, and counting that meeting as
    // a crossing would penalize every fan-out a fork produces.
    let crossable: Vec<&RoutedTrace> = routed
        .iter()
        .filter(|t| &t.from != from_id && &t.from != to_id && &t.to != from_id && &t.to != to_id)
        .collect();

    let candidates: Vec<Candidate> = match tracery {
        Tracery::Flowing => flowing_candidates(kind, from, to),
        Tracery::Concentric => concentric_candidates(kind, from, to),
        Tracery::Circuit => circuit_candidates(from, to),
    };

    // Marks are a hard constraint, crossings a soft one: among candidates that
    // clear every mark, the fewest crossings of earlier traces wins, earliest
    // in the candidate order on a tie — so the house shape still wins whenever
    // it is as clean as anything else (§11.6, within fixed positions). A
    // candidate that cannot clear the marks is only ever the last resort.
    let mut clear_best: Option<(usize, Candidate)> = None;
    let mut dirty_best: Option<(f64, Candidate)> = None;
    for candidate in candidates {
        let worst = worst_clearance(&candidate.samples, &near);
        if worst >= 0.0 {
            let crossings = crossings_of(&candidate.samples, &crossable);
            if crossings == 0 {
                return finish(candidate);
            }
            if clear_best.as_ref().is_none_or(|(c, _)| crossings < *c) {
                clear_best = Some((crossings, candidate));
            }
        } else if dirty_best.as_ref().is_none_or(|(b, _)| worst > *b) {
            dirty_best = Some((worst, candidate));
        }
    }
    let candidate = clear_best
        .map(|(_, c)| c)
        .or(dirty_best.map(|(_, c)| c))
        .expect("every tracery yields at least one candidate");
    finish(candidate)
}

/// How many times a candidate polyline properly crosses earlier traces.
fn crossings_of(samples: &[Point], routed: &[&RoutedTrace]) -> usize {
    let bounds = |points: &[Point]| {
        points.iter().fold(
            (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        )
    };
    let (ax0, ay0, ax1, ay1) = bounds(samples);

    let mut crossings = 0;
    for trace in routed {
        let (bx0, by0, bx1, by1) = bounds(&trace.samples);
        if ax1 < bx0 || bx1 < ax0 || ay1 < by0 || by1 < ay0 {
            continue;
        }
        for a in samples.windows(2) {
            for b in trace.samples.windows(2) {
                if segments_cross(a[0], a[1], b[0], b[1]) {
                    crossings += 1;
                }
            }
        }
    }
    crossings
}

/// Proper crossing only: shared endpoints and touches do not count, so two
/// traces meeting at a mark are a junction rather than a crossing.
fn segments_cross(a1: Point, a2: Point, b1: Point, b2: Point) -> bool {
    let orient =
        |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let d1 = orient(b1, b2, a1);
    let d2 = orient(b1, b2, a2);
    let d3 = orient(a1, a2, b1);
    let d4 = orient(a1, a2, b2);
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

fn finish(candidate: Candidate) -> RoutedEdge {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in &candidate.samples {
        min.x = min.x.min(p.x);
        min.y = min.y.min(p.y);
        max.x = max.x.max(p.x);
        max.y = max.y.max(p.y);
    }
    RoutedEdge {
        commands: candidate.commands,
        // Padded past the stroke and a via dot, so hit testing and bounds
        // checks see the ink rather than the centreline.
        bounds: Rect::new(min.x, min.y, max.x - min.x, max.y - min.y).expanded(VIA_RADIUS + 6.0),
        samples: candidate.samples,
    }
}

/// The smallest margin between the sampled points and any obstacle.
fn worst_clearance(samples: &[Point], obstacles: &[(Point, f64)]) -> f64 {
    let mut worst = f64::INFINITY;
    for s in samples {
        for (p, clearance) in obstacles {
            let margin = (s.x - p.x).hypot(s.y - p.y) - clearance;
            if margin < worst {
                worst = margin;
            }
        }
    }
    worst
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq <= 1e-12 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_sq).clamp(0.0, 1.0)
    };
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

// ---------------------------------------------------------------- flowing

/// Candidate bowed cubics, the house curve first, then deeper and slid bows in
/// a fixed order — never flipping the bow's side, because a feedback arc that
/// crossed to the inside would stop *reading* as feedback.
fn flowing_candidates(kind: EdgeKind, from: Point, to: Point) -> Vec<Candidate> {
    // Curved toward the centre; feedback bows the other way, which is what
    // makes a returning arc distinguishable from the outgoing one it parallels.
    let bow = match kind {
        EdgeKind::Feedback => -0.34,
        EdgeKind::Enter | EdgeKind::Join => 0.16,
        EdgeKind::Flow => 0.22,
    };

    const SCALES: &[f64] = &[1.0, 0.7, 1.35, 0.45, 1.7, 0.2, 2.05];
    const SLIDES: &[f64] = &[0.0, 0.18, -0.18, 0.36, -0.36];

    let mut out = Vec::with_capacity(SCALES.len() * SLIDES.len());
    for scale in SCALES {
        for slide in SLIDES {
            let bowed = (bow * scale).clamp(-MAX_BOW, MAX_BOW);
            out.push(bowed_cubic(from, to, bowed, *slide));
        }
    }
    out
}

/// A cubic bowed toward the canvas centre by `bow` of the chord length, its
/// apex slid `slide` of the chord toward `to`. Sliding moves where the curve
/// reaches its deepest point without changing which side it bows to.
fn bowed_cubic(from: Point, to: Point, bow: f64, slide: f64) -> Candidate {
    let mid = Point::new(
        (from.x + to.x) / 2.0 + (to.x - from.x) * slide,
        (from.y + to.y) / 2.0 + (to.y - from.y) * slide,
    );
    let (tx, ty) = (CENTER - mid.x, CENTER - mid.y);
    let length = tx.hypot(ty).max(1e-9);
    let pull = (from.x - to.x).hypot(from.y - to.y) * bow;
    let control = Point::new(mid.x + tx / length * pull, mid.y + ty / length * pull);

    let c1 = Point::new(
        from.x + (control.x - from.x) * 0.66,
        from.y + (control.y - from.y) * 0.66,
    );
    let c2 = Point::new(
        to.x + (control.x - to.x) * 0.66,
        to.y + (control.y - to.y) * 0.66,
    );

    // Sixteen segments resolves a curve a few hundred units long to well under
    // a mark's size.
    let mut samples = Vec::with_capacity(17);
    for i in 0..=16 {
        let t = i as f64 / 16.0;
        let u = 1.0 - t;
        samples.push(Point::new(
            u * u * u * from.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * to.x,
            u * u * u * from.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * to.y,
        ));
    }

    Candidate {
        commands: vec![
            PathCommand::MoveTo(from),
            PathCommand::CubicTo { c1, c2, to },
        ],
        samples,
    }
}

// ------------------------------------------------------------- concentric

/// Radial run, concentric arc, radial run.
///
/// The travel happens on a circle centred on the composition — the ring the
/// whole layout is already organized around — and the joints where radius
/// becomes angle are left sharp on purpose. Candidates vary the ring the
/// travel happens on; feedback tries rings *outside* both endpoints first, so
/// a return reads as the long way round rather than a shortcut through.
fn concentric_candidates(kind: EdgeKind, from: Point, to: Point) -> Vec<Candidate> {
    let (ra, aa) = polar(from);
    let (rb, ab) = polar(to);
    let delta = wrap_angle(ab - aa);

    // A near-radial edge — a spoke to an invocation, a plunge to the seal —
    // has no meaningful arc travel; it is drawn as the straight radial it is.
    if delta.abs() < 0.05 || (ra - rb).abs() < 1.0 && delta.abs() < 0.15 {
        return vec![segments_candidate(&[from, to])];
    }

    let (lo, hi) = (ra.min(rb), ra.max(rb));
    let mut rings: Vec<f64> = Vec::new();
    if kind == EdgeKind::Feedback {
        // The return loop travels outside what it returns across.
        rings.extend([hi + 52.0, hi + 96.0, hi + 24.0]);
    }
    rings.extend([
        lo + (hi - lo) * 0.5,
        lo + (hi - lo) * 0.3,
        lo + (hi - lo) * 0.7,
        lo + (hi - lo) * 0.12,
        lo + (hi - lo) * 0.88,
        (lo - 46.0).max(30.0),
        hi + 46.0,
        (lo - 92.0).max(30.0),
        hi + 92.0,
    ]);

    rings
        .into_iter()
        .map(|r| r.clamp(26.0, SAFE_RADIUS * 0.99))
        .map(|r| ring_candidate(from, to, r, aa, delta))
        .collect()
}

/// One concentric route at ring radius `r`.
fn ring_candidate(from: Point, to: Point, r: f64, aa: f64, delta: f64) -> Candidate {
    let (ra, _) = polar(from);
    let (rb, ab_end) = (polar(to).0, aa + delta);

    let enter = at_polar(r, aa);
    let exit = at_polar(r, ab_end);

    let mut commands = vec![PathCommand::MoveTo(from)];
    if (ra - r).abs() > 1.0 {
        commands.push(PathCommand::LineTo(enter));
    }
    commands.push(PathCommand::ArcTo {
        radius: r,
        // The travel is always the short way; `delta` is already wrapped.
        large: false,
        sweep: delta > 0.0,
        to: exit,
    });
    if (rb - r).abs() > 1.0 {
        commands.push(PathCommand::LineTo(to));
    }

    // Samples: both radial runs, and the arc walked in angle.
    let mut samples = vec![from];
    sample_segment(&mut samples, from, enter, 6);
    let steps = ((delta.abs() / TAU * 48.0).ceil() as usize).clamp(4, 24);
    for i in 0..=steps {
        let a = aa + delta * i as f64 / steps as f64;
        samples.push(at_polar(r, a));
    }
    sample_segment(&mut samples, exit, to, 6);
    samples.push(to);

    Candidate { commands, samples }
}

// ---------------------------------------------------------------- circuit

/// Orthogonal runs with via dots.
///
/// Candidates in a fixed order: the two single-bend routes, then double-bend
/// routes whose middle run slides across the span. A bend gets a via dot; a
/// straight run gets one at its midpoint, so every trace carries the mark of
/// the style.
fn circuit_candidates(from: Point, to: Point) -> Vec<Candidate> {
    let (dx, dy) = (to.x - from.x, to.y - from.y);

    // Already orthogonal (or as good as): a straight run with a via on it.
    if dx.abs() < 6.0 || dy.abs() < 6.0 {
        let mid = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
        let mut candidate = segments_candidate(&[from, to]);
        candidate.commands.extend(via_dot(mid));
        return vec![candidate];
    }

    let mut out = Vec::new();
    // Single bends.
    out.push(corners_candidate(&[from, Point::new(to.x, from.y), to]));
    out.push(corners_candidate(&[from, Point::new(from.x, to.y), to]));
    // Double bends, middle run sliding across the span.
    for f in [0.5, 0.3, 0.7, 0.15, 0.85] {
        let mx = from.x + dx * f;
        out.push(corners_candidate(&[
            from,
            Point::new(mx, from.y),
            Point::new(mx, to.y),
            to,
        ]));
        let my = from.y + dy * f;
        out.push(corners_candidate(&[
            from,
            Point::new(from.x, my),
            Point::new(to.x, my),
            to,
        ]));
    }
    out
}

/// A polyline through `points`, with a via dot at every interior corner.
fn corners_candidate(points: &[Point]) -> Candidate {
    let mut candidate = segments_candidate(points);
    for corner in &points[1..points.len() - 1] {
        candidate.commands.extend(via_dot(*corner));
    }
    candidate
}

/// A bare polyline candidate: MoveTo, LineTo…, sampled per segment.
fn segments_candidate(points: &[Point]) -> Candidate {
    let mut commands = vec![PathCommand::MoveTo(points[0])];
    let mut samples = vec![points[0]];
    for window in points.windows(2) {
        commands.push(PathCommand::LineTo(window[1]));
        sample_segment(&mut samples, window[0], window[1], 10);
        samples.push(window[1]);
    }
    Candidate { commands, samples }
}

/// A via: a small circle drawn as its own sub-path, two half arcs.
fn via_dot(center: Point) -> Vec<PathCommand> {
    let east = Point::new(center.x + VIA_RADIUS, center.y);
    let west = Point::new(center.x - VIA_RADIUS, center.y);
    vec![
        PathCommand::MoveTo(east),
        PathCommand::ArcTo {
            radius: VIA_RADIUS,
            large: false,
            sweep: true,
            to: west,
        },
        PathCommand::ArcTo {
            radius: VIA_RADIUS,
            large: false,
            sweep: true,
            to: east,
        },
        PathCommand::Close,
    ]
}

// ------------------------------------------------------------------ shared

fn polar(p: Point) -> (f64, f64) {
    (
        (p.x - CENTER).hypot(p.y - CENTER),
        (p.y - CENTER).atan2(p.x - CENTER),
    )
}

fn at_polar(radius: f64, angle: f64) -> Point {
    Point::new(CENTER + radius * angle.cos(), CENTER + radius * angle.sin())
}

/// Wrap to `(-PI, PI]`, so "the short way round" is a sign and a magnitude.
fn wrap_angle(a: f64) -> f64 {
    let wrapped = (a + PI).rem_euclid(TAU) - PI;
    if wrapped <= -PI {
        wrapped + TAU
    } else {
        wrapped
    }
}

fn sample_segment(samples: &mut Vec<Point>, a: Point, b: Point, steps: usize) {
    for i in 1..steps {
        let t = i as f64 / steps as f64;
        samples.push(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracery_parses_its_own_names() {
        for tracery in Tracery::ALL {
            assert_eq!(Tracery::parse(tracery.name()), Some(*tracery));
        }
        assert_eq!(Tracery::parse("cursive"), None);
    }

    #[test]
    fn wrap_angle_stays_in_range_and_keeps_direction() {
        assert!((wrap_angle(0.1) - 0.1).abs() < 1e-12);
        assert!((wrap_angle(TAU + 0.1) - 0.1).abs() < 1e-12);
        assert!((wrap_angle(-0.1) + 0.1).abs() < 1e-12);
        assert!(wrap_angle(PI) <= PI);
        assert!(wrap_angle(-PI) > -PI);
    }

    #[test]
    fn a_straight_circuit_edge_still_carries_a_via() {
        let from = Point::new(100.0, 100.0);
        let to = Point::new(400.0, 100.0);
        let candidates = circuit_candidates(from, to);
        assert_eq!(candidates.len(), 1);
        // MoveTo + LineTo + a four-command via.
        assert_eq!(candidates[0].commands.len(), 6);
    }

    #[test]
    fn concentric_route_travels_on_one_radius() {
        let from = at_polar(200.0, 0.3);
        let to = at_polar(340.0, 1.9);
        let candidate = ring_candidate(from, to, 260.0, 0.3, 1.6);
        let on_ring = candidate
            .samples
            .iter()
            .filter(|p| (polar(**p).0 - 260.0).abs() < 0.5)
            .count();
        assert!(on_ring >= 4, "the arc travel is missing its ring samples");
    }
}
