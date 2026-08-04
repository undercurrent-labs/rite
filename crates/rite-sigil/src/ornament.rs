//! Ornament: everything that makes the artifact beautiful and means nothing.
//!
//! # The rule that governs all of it
//!
//! Ornament must be removable **without relayout** (ADR 0004). Not "without
//! much change" — without any. Turning it off and on must leave every semantic
//! coordinate bit-for-bit identical, which is why nothing here is allowed to
//! run before placement, read a node's position to avoid it, or feed anything
//! back. Ornament is generated *after* the semantic scene is finished, from the
//! seed and the canvas alone, and appended.
//!
//! That is a stronger constraint than it looks. "Draw filigree in the gaps"
//! would need to know where the gaps are, and a gap depends on where the nodes
//! landed — so an ornament that avoided collisions would make the semantic
//! layout depend on the ornament level through the collision pass. The way out
//! is that ornament does not avoid anything: it lives on rings and radii the
//! composition reserves for it, and it is drawn behind or in front.
//!
//! # What it may not do
//!
//! - Carry a graph reference. An ornament element that could be selected would
//!   put a meaningless entry in the Codex.
//! - Receive a hit region.
//! - Resemble a semantic edge closely enough to be mistaken for one — which is
//!   why nothing here draws a stroke between two points that are *near* two
//!   nodes, and why ornament is drawn at a lower opacity from its own palette.
//! - Change identity, bounds, or ordering of anything semantic.
//!
//! # Levels
//!
//! `none` draws nothing. `sparse` is the guide geometry a technical reader
//! wants. `ritual` is the default and the intended look. `maximal` is
//! deliberately excessive and still has to leave the semantics distinguishable
//! — §15.1 says so, and `maximal` is the level most likely to break that, so it
//! is the one the invariance test runs against.

use std::f64::consts::TAU;

use crate::canonical::Prng;
use crate::layout::{CENTER, SAFE_RADIUS, VIEW_SIZE};
use crate::scene::*;
use crate::trig::DeterministicTrig;

/// How much non-semantic geometry to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrnamentLevel {
    None,
    Sparse,
    #[default]
    Ritual,
    Maximal,
}

impl OrnamentLevel {
    pub fn name(self) -> &'static str {
        match self {
            OrnamentLevel::None => "none",
            OrnamentLevel::Sparse => "sparse",
            OrnamentLevel::Ritual => "ritual",
            OrnamentLevel::Maximal => "maximal",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "none" => Some(OrnamentLevel::None),
            "sparse" => Some(OrnamentLevel::Sparse),
            "ritual" => Some(OrnamentLevel::Ritual),
            "maximal" => Some(OrnamentLevel::Maximal),
            _ => None,
        }
    }

    pub const ALL: &'static [OrnamentLevel] = &[
        OrnamentLevel::None,
        OrnamentLevel::Sparse,
        OrnamentLevel::Ritual,
        OrnamentLevel::Maximal,
    ];

    /// How many decorative families are drawn, cumulatively.
    fn density(self) -> u8 {
        match self {
            OrnamentLevel::None => 0,
            OrnamentLevel::Sparse => 1,
            OrnamentLevel::Ritual => 2,
            OrnamentLevel::Maximal => 3,
        }
    }
}

/// Generate the ornament for a scene.
///
/// Takes the seed and nothing from the placed elements, which is what makes the
/// invariance a property of the code rather than of care. Returns elements to
/// append; the caller does not merge them into the semantic layers because they
/// are not on them.
pub fn generate(level: OrnamentLevel, seed: u64) -> Vec<SceneElement> {
    if level == OrnamentLevel::None {
        return Vec::new();
    }
    let mut prng = Prng::new(seed).derive("ornament");
    let mut out = Vec::new();
    let density = level.density();

    // Guide geometry: concentric rings at the band boundaries. Sparse and up,
    // because these are the bones of the composition and a reader who wants to
    // see the structure wants these first.
    guide_rings(&mut out);
    cardinal_ticks(&mut out, &mut prng);

    if density >= 2 {
        radial_filaments(&mut out, &mut prng);
        rune_band(&mut out, &mut prng);
        dot_constellation(&mut out, &mut prng, 40);
    }

    if density >= 3 {
        // Deliberately excessive. Still bounded, still behind the semantics,
        // still on its own layers.
        broken_circuit_ring(&mut out, &mut prng);
        dot_constellation(&mut out, &mut prng, 90);
        outer_containment(&mut out, &mut prng);
    }

    out
}

fn element(id: String, layer: SceneLayerKind, geometry: Geometry, bounds: Rect) -> SceneElement {
    SceneElement {
        id,
        layer,
        semantic: SemanticKind::Ornament,
        // Never. §15.3, and the reason an ornament cannot be selected.
        graph_ref: None,
        geometry,
        // No title either: an accessible name on a decoration is noise in a
        // screen reader's traversal of a diagram.
        title: None,
        legend_key: None,
        ends: None,
        weight: None,
        bounds,
    }
}

fn ring(id: &str, radius: f64, layer: SceneLayerKind) -> SceneElement {
    element(
        format!("ornament/{id}"),
        layer,
        Geometry::Circle {
            center: Point::new(CENTER, CENTER),
            radius,
        },
        Rect::new(CENTER - radius, CENTER - radius, radius * 2.0, radius * 2.0),
    )
}

/// The band boundaries, drawn faintly. These are the composition's own
/// structure made visible — the one ornament that is genuinely informative
/// without being semantic.
fn guide_rings(out: &mut Vec<SceneElement>) {
    for (i, fraction) in [0.15f64, 0.65, 0.85].into_iter().enumerate() {
        out.push(ring(
            &format!("guide-{i}"),
            SAFE_RADIUS * fraction,
            SceneLayerKind::GuideGeometry,
        ));
    }
}

/// Cardinal and intercardinal marks: eight ticks at the rim.
fn cardinal_ticks(out: &mut Vec<SceneElement>, prng: &mut Prng) {
    for i in 0..8 {
        let theta = i as f64 * TAU / 8.0 - std::f64::consts::FRAC_PI_2;
        // Cardinals longer than intercardinals, so orientation is readable.
        let length = if i % 2 == 0 { 34.0 } else { 18.0 };
        let inner = SAFE_RADIUS + 6.0;
        let outer = inner + length + prng.next_f64() * 4.0;
        out.push(segment(
            format!("ornament/cardinal-{i}"),
            polar(inner, theta),
            polar(outer, theta),
            SceneLayerKind::GuideGeometry,
        ));
    }
}

/// Fine radial hairlines between the flow band and the boundary.
fn radial_filaments(out: &mut Vec<SceneElement>, prng: &mut Prng) {
    let count = 48;
    for i in 0..count {
        let theta = i as f64 * TAU / count as f64;
        let jitter = prng.next_f64() * 0.012;
        let inner = SAFE_RADIUS * 0.86;
        let outer = SAFE_RADIUS * (0.94 + prng.next_f64() * 0.05);
        out.push(segment(
            format!("ornament/filament-{i}"),
            polar(inner, theta + jitter),
            polar(outer, theta + jitter),
            SceneLayerKind::OrnamentBack,
        ));
    }
}

/// A band of synthetic runes just inside the boundary.
///
/// Generated tick groups rather than glyphs from any script: §2.2 asks for an
/// original vocabulary, and borrowing real characters would make the artifact
/// say something in a language somebody reads.
fn rune_band(out: &mut Vec<SceneElement>, prng: &mut Prng) {
    let count = 28;
    for i in 0..count {
        let theta = i as f64 * TAU / count as f64 + 0.04;
        let base = SAFE_RADIUS * 0.79;
        let strokes = prng.range(2, 5);
        let mut commands = Vec::new();
        for s in 0..strokes {
            let offset = (s as f64 - strokes as f64 / 2.0) * 0.008;
            let height = 10.0 + prng.next_f64() * 12.0;
            commands.push(PathCommand::MoveTo(polar(base, theta + offset)));
            commands.push(PathCommand::LineTo(polar(base + height, theta + offset)));
        }
        if prng.chance(0.5) {
            let mid = base + 6.0;
            commands.push(PathCommand::MoveTo(polar(mid, theta - 0.012)));
            commands.push(PathCommand::LineTo(polar(mid, theta + 0.012)));
        }
        let anchor = polar(base + 12.0, theta);
        out.push(element(
            format!("ornament/rune-{i}"),
            SceneLayerKind::OrnamentBack,
            Geometry::Path { commands },
            Rect::new(anchor.x - 20.0, anchor.y - 20.0, 40.0, 40.0),
        ));
    }
}

/// Hash-derived dots, scattered inside the safe radius.
fn dot_constellation(out: &mut Vec<SceneElement>, prng: &mut Prng, count: usize) {
    let base = out.len();
    for i in 0..count {
        let theta = prng.next_f64() * TAU;
        // `sqrt` so the dots are uniform over the disc rather than crowding the
        // centre, which is where the semantics are.
        let r = SAFE_RADIUS * (0.25 + 0.72 * prng.next_f64().sqrt());
        let p = polar(r, theta);
        let radius = 1.2 + prng.next_f64() * 1.8;
        out.push(element(
            format!("ornament/dot-{}", base + i),
            SceneLayerKind::OrnamentBack,
            Geometry::Circle { center: p, radius },
            Rect::new(p.x - radius, p.y - radius, radius * 2.0, radius * 2.0),
        ));
    }
}

/// A ring of broken arcs, like a circuit trace.
fn broken_circuit_ring(out: &mut Vec<SceneElement>, prng: &mut Prng) {
    let radius = SAFE_RADIUS * 0.92;
    let segments = 18;
    for i in 0..segments {
        if prng.chance(0.25) {
            continue;
        }
        let start = i as f64 * TAU / segments as f64;
        let end = start + TAU / segments as f64 * (0.55 + prng.next_f64() * 0.3);
        out.push(element(
            format!("ornament/circuit-{i}"),
            SceneLayerKind::OrnamentFront,
            Geometry::Arc {
                center: Point::new(CENTER, CENTER),
                radius,
                start_angle: start,
                end_angle: end,
            },
            Rect::new(CENTER - radius, CENTER - radius, radius * 2.0, radius * 2.0),
        ));
    }
}

/// The outermost containment circles, at `maximal`.
fn outer_containment(out: &mut Vec<SceneElement>, prng: &mut Prng) {
    for i in 0..2 {
        let radius = SAFE_RADIUS + 44.0 + i as f64 * 10.0 + prng.next_f64() * 4.0;
        // Inside the canvas: the safe radius is 700 on an 800 half-canvas, so
        // there is room, but a level that grew could reach the edge and get
        // clipped. Bounded rather than trusted.
        let radius = radius.min(VIEW_SIZE / 2.0 - 8.0);
        out.push(ring(
            &format!("containment-{i}"),
            radius,
            SceneLayerKind::OrnamentFront,
        ));
    }
}

fn polar(radius: f64, theta: f64) -> Point {
    Point::new(
        CENTER + radius * theta.dcos(),
        CENTER + radius * theta.dsin(),
    )
}

fn segment(id: String, a: Point, b: Point, layer: SceneLayerKind) -> SceneElement {
    element(
        id,
        layer,
        Geometry::Path {
            commands: vec![PathCommand::MoveTo(a), PathCommand::LineTo(b)],
        },
        Rect::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_through_their_names() {
        for level in OrnamentLevel::ALL {
            assert_eq!(OrnamentLevel::parse(level.name()), Some(*level));
        }
        assert_eq!(OrnamentLevel::parse("baroque"), None);
    }

    #[test]
    fn none_draws_nothing_and_the_levels_increase() {
        let counts: Vec<usize> = OrnamentLevel::ALL
            .iter()
            .map(|l| generate(*l, 7).len())
            .collect();
        assert_eq!(counts[0], 0, "`none` drew something");
        assert!(
            counts[1] < counts[2] && counts[2] < counts[3],
            "levels do not increase: {counts:?}"
        );
    }

    #[test]
    fn ornament_is_deterministic() {
        for level in OrnamentLevel::ALL {
            assert_eq!(
                generate(*level, 99),
                generate(*level, 99),
                "{}",
                level.name()
            );
        }
    }

    #[test]
    fn a_different_seed_gives_different_ornament() {
        assert_ne!(
            generate(OrnamentLevel::Ritual, 1),
            generate(OrnamentLevel::Ritual, 2)
        );
    }

    /// §15.3, and the reason an ornament cannot be selected: no graph reference,
    /// no title, and it is on an ornament layer.
    #[test]
    fn no_ornament_element_carries_a_graph_reference_or_a_title() {
        for level in OrnamentLevel::ALL {
            for element in generate(*level, 5) {
                assert!(
                    element.graph_ref.is_none(),
                    "{} carries a graph reference",
                    element.id
                );
                assert!(element.title.is_none(), "{} carries a title", element.id);
                assert!(element.legend_key.is_none());
                assert_eq!(element.semantic, SemanticKind::Ornament);
                assert!(
                    element.is_ornament(),
                    "{} is not classified as ornament",
                    element.id
                );
                assert!(
                    !element.layer.is_semantic(),
                    "{} landed on a semantic layer",
                    element.id
                );
            }
        }
    }

    /// Ornament identifiers must not collide with each other or look like a
    /// semantic one — a duplicate element ID is an invalid document.
    #[test]
    fn ornament_identifiers_are_unique_and_prefixed() {
        for level in OrnamentLevel::ALL {
            let elements = generate(*level, 11);
            let mut ids: Vec<&str> = elements.iter().map(|e| e.id.as_str()).collect();
            let count = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), count, "{}: duplicate ornament id", level.name());
            for element in &elements {
                assert!(
                    element.id.starts_with("ornament/"),
                    "{} is not prefixed",
                    element.id
                );
            }
        }
    }

    /// Everything finite, and on the canvas — including `maximal`, whose outer
    /// containment circles are the ones most likely to escape.
    #[test]
    fn every_ornament_coordinate_is_finite_and_on_the_canvas() {
        let canvas = Rect::new(0.0, 0.0, VIEW_SIZE, VIEW_SIZE);
        for level in OrnamentLevel::ALL {
            for element in generate(*level, 3) {
                let b = element.bounds;
                assert!(
                    [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite()),
                    "{}",
                    element.id
                );
                let center = Point::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
                assert!(
                    canvas.contains(center),
                    "{} is centred off-canvas at {center:?}",
                    element.id
                );
                if let Geometry::Circle { radius, .. } = element.geometry {
                    assert!(
                        radius <= VIEW_SIZE / 2.0,
                        "{} has radius {radius}, past the canvas",
                        element.id
                    );
                }
            }
        }
    }
}
