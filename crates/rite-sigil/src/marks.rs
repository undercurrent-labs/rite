//! The procedural mark generator.
//!
//! A mark is the symbol drawn at a node. It is what makes a Veiled render
//! readable: with every label removed, the *shape* has to say whether a thing is
//! a gate, a divergence, or a boundary crossing.
//!
//! # A grammar, not noise
//!
//! Marks are generated from a constrained grammar (§10.3), not from random path
//! data:
//!
//! ```text
//! mark = skeleton + terminal_pair + optional crossbar + optional satellites
//!                 + optional notch pattern
//! ```
//!
//! The **skeleton** is fixed per node kind and is the part a viewer learns. The
//! rest varies deterministically per node, so two stages are visibly different
//! individuals without either stopping being a stage. That is the constraint
//! §10.2 states as "variation must not erase the base semantic skeleton", and it
//! is why variation only ever *adds* strokes to a skeleton it never edits.
//!
//! # Why the shapes are what they are
//!
//! Each skeleton is chosen so the kind survives three things at once: being
//! drawn small, being drawn in one colour, and being drawn next to its
//! neighbours. Colour is not permitted to carry the distinction (§4.6), so every
//! pair of kinds differs in topology — a count of strokes, a closed versus open
//! form, a symmetry class — rather than in weight or hue.
//!
//! | Kind | Skeleton | What makes it that kind |
//! |---|---|---|
//! | Source | nested core | the only concentric-closed form; reads as an origin |
//! | Stage | rune spine | one bar, two terminals — the least mark that is still a mark |
//! | Ward | gate | a bar *across* the flow, with a gap: a thing you pass through |
//! | Scatter | flare | rays diverging from one point |
//! | Collect | knot | rays converging into a closed ring: Scatter, inverted |
//! | Fork | trident | discrete ordered tines, so branch count is countable |
//! | Orbit | circular lock | a ring with a break and an inward key |
//! | Effect | altar | an open bracket facing outward, touching nothing inward |
//! | Output | seal | a closed polygon with an inner mark: nothing leaves it |
//! | Literal | dot cluster | no strokes at all; a value, not an operation |
//! | Unknown | broken hex | a familiar form with a piece missing |
//!
//! Scatter and Collect are deliberately inverses, because §9.7 requires Collect
//! to visually oppose Scatter and a viewer who has learned one gets the other.
//!
//! # Coordinates
//!
//! Everything is generated in a normalized box from `-1` to `1` and scaled by
//! the mark's size at the end. Generating in place would mean every skeleton
//! carrying the scale through its own arithmetic, and a bug in one of them would
//! be a mark that overflows its own bounds — which
//! [`bounds_of`] exists to make a test rather than a review.

use std::f64::consts::{PI, TAU};

use crate::canonical::Prng;
use crate::graph::{CapabilityFamily, SigilNodeKind};
use crate::scene::{PathCommand, Point};
use crate::trig::DeterministicTrig;

/// How elaborate a mark should be.
///
/// Not an ornament level — this is the semantic mark itself, and even `Minimal`
/// draws the full skeleton. It controls how much *variation* is layered on, so a
/// dense graph can be simplified without any node changing what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkDetail {
    /// Skeleton only. What `--simplify` uses, and what a 500-node graph gets.
    Minimal,
    #[default]
    Full,
}

/// A generated mark: path data in a normalized box, plus what it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub commands: Vec<PathCommand>,
    /// The largest distance from the centre any point reaches, in normalized
    /// units. Always ≤ 1.
    pub extent: f64,
}

/// Generate the mark for a node.
///
/// `seed_label` is the node's identity — the per-node PRNG stream is derived
/// from it, so a mark depends on *which* node it is and not on when it was
/// generated or in what order.
pub fn generate(
    kind: &SigilNodeKind,
    family: Option<&CapabilityFamily>,
    seed: &Prng,
    seed_label: &str,
    detail: MarkDetail,
) -> Mark {
    let mut prng = seed.derive(seed_label);
    let mut b = Builder::new();

    match kind {
        SigilNodeKind::Source => source(&mut b, &mut prng),
        SigilNodeKind::Stage => stage(&mut b, &mut prng),
        SigilNodeKind::Ward => ward(&mut b, &mut prng),
        SigilNodeKind::Scatter => scatter(&mut b, &mut prng),
        SigilNodeKind::Collect => collect(&mut b, &mut prng),
        SigilNodeKind::Fork => fork(&mut b, &mut prng),
        SigilNodeKind::Orbit => orbit(&mut b, &mut prng),
        SigilNodeKind::Effect => effect(&mut b, &mut prng, family),
        SigilNodeKind::Output => output(&mut b, &mut prng),
        SigilNodeKind::Literal => literal(&mut b, &mut prng),
        SigilNodeKind::Unknown(_) => unknown(&mut b, &mut prng),
    }

    if detail == MarkDetail::Full {
        satellites(&mut b, &mut prng);
    }

    b.finish()
}

/// The extent of a generated mark, for the bounds test.
pub fn bounds_of(mark: &Mark) -> f64 {
    mark.extent
}

/// Accumulates path commands and tracks how far out they reach.
struct Builder {
    commands: Vec<PathCommand>,
    extent: f64,
}

impl Builder {
    fn new() -> Self {
        Builder {
            commands: Vec::new(),
            extent: 0.0,
        }
    }

    fn note(&mut self, p: Point) -> Point {
        self.extent = self.extent.max(p.x.hypot(p.y));
        p
    }

    fn move_to(&mut self, x: f64, y: f64) {
        let p = self.note(Point::new(x, y));
        self.commands.push(PathCommand::MoveTo(p));
    }

    fn line_to(&mut self, x: f64, y: f64) {
        let p = self.note(Point::new(x, y));
        self.commands.push(PathCommand::LineTo(p));
    }

    fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    /// A line segment as its own subpath, so strokes never join accidentally.
    fn segment(&mut self, a: (f64, f64), b: (f64, f64)) {
        self.move_to(a.0, a.1);
        self.line_to(b.0, b.1);
    }

    /// A regular polygon, closed.
    fn polygon(&mut self, radius: f64, sides: usize, phase: f64) {
        let sides = sides.max(3);
        for i in 0..sides {
            let theta = phase + i as f64 * TAU / sides as f64;
            let (x, y) = (radius * theta.dcos(), radius * theta.dsin());
            if i == 0 {
                self.move_to(x, y);
            } else {
                self.line_to(x, y);
            }
        }
        self.close();
    }

    /// A circle, as four cubic segments. `k` is the standard circular constant.
    fn circle(&mut self, radius: f64) {
        const K: f64 = 0.552_284_749_83;
        let r = radius;
        let c = r * K;
        self.move_to(r, 0.0);
        for (c1, c2, to) in [
            ((r, c), (c, r), (0.0, r)),
            ((-c, r), (-r, c), (-r, 0.0)),
            ((-r, -c), (-c, -r), (0.0, -r)),
            ((c, -r), (r, -c), (r, 0.0)),
        ] {
            let c1 = self.note(Point::new(c1.0, c1.1));
            let c2 = self.note(Point::new(c2.0, c2.1));
            let to = self.note(Point::new(to.0, to.1));
            self.commands.push(PathCommand::CubicTo { c1, c2, to });
        }
    }

    /// An arc from `start` to `end` radians, as line segments.
    ///
    /// Segments rather than an SVG arc command: the step count is a function of
    /// the sweep, so the same arc produces the same points on every platform,
    /// and there is no arc-flag arithmetic to get wrong at the wrap-around.
    fn arc(&mut self, radius: f64, start: f64, end: f64) {
        let steps = ((end - start).abs() / 0.22).ceil().max(2.0) as usize;
        for i in 0..=steps {
            let theta = start + (end - start) * i as f64 / steps as f64;
            let (x, y) = (radius * theta.dcos(), radius * theta.dsin());
            if i == 0 {
                self.move_to(x, y);
            } else {
                self.line_to(x, y);
            }
        }
    }

    /// A dot, as a tiny closed square. Round dots would need another circle's
    /// worth of commands each, and at mark scale the difference is invisible.
    fn dot(&mut self, x: f64, y: f64, r: f64) {
        self.move_to(x - r, y - r);
        self.line_to(x + r, y - r);
        self.line_to(x + r, y + r);
        self.line_to(x - r, y + r);
        self.close();
    }

    fn finish(self) -> Mark {
        Mark {
            commands: self.commands,
            // Clamped rather than asserted: a mark that reached past its box
            // would be a bug, and the test asserts it — but a renderer that
            // panicked on one would take the whole picture down for a symbol.
            extent: self.extent.min(1.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Skeletons
// ---------------------------------------------------------------------------

/// Nested core: concentric closed forms. The only skeleton that nests, which is
/// what makes the centre of the composition unmistakable even at thumbnail size.
fn source(b: &mut Builder, prng: &mut Prng) {
    b.circle(0.86);
    let inner_sides = prng.range(3, 7) as usize;
    b.polygon(0.52, inner_sides, -PI / 2.0);
    b.circle(0.2);
    // A cardinal notch: the canonical orientation, readable from the mark alone.
    b.segment((0.0, -0.86), (0.0, -0.98));
}

/// Rune spine: one bar with two terminals. The least mark that is still a mark,
/// because a stage is the least a node can do.
fn stage(b: &mut Builder, prng: &mut Prng) {
    b.segment((0.0, -0.78), (0.0, 0.78));
    let terminal = prng.range(0, 3);
    match terminal {
        0 => {
            b.segment((-0.34, -0.5), (0.0, -0.78));
            b.segment((0.34, 0.5), (0.0, 0.78));
        }
        1 => {
            b.segment((-0.3, -0.78), (0.3, -0.78));
            b.segment((-0.3, 0.78), (0.3, 0.78));
        }
        _ => {
            b.dot(0.0, -0.78, 0.11);
            b.dot(0.0, 0.78, 0.11);
        }
    }
    if prng.chance(0.55) {
        let y = (prng.next_f64() - 0.5) * 0.7;
        b.segment((-0.42, y), (0.42, y));
    }
}

/// Gate: a bar across the flow with a gap in it. A thing values pass *through*,
/// and the gap is what says the passage is conditional.
fn ward(b: &mut Builder, prng: &mut Prng) {
    // The crossing bar, perpendicular to the flow, broken in the middle.
    b.segment((-0.92, 0.0), (-0.22, 0.0));
    b.segment((0.22, 0.0), (0.92, 0.0));
    // The jambs, so the gap reads as a threshold rather than a broken line.
    b.segment((-0.22, -0.44), (-0.22, 0.44));
    b.segment((0.22, -0.44), (0.22, 0.44));
    // The predicate mark, centred in the gap.
    let facets = prng.range(3, 6) as usize;
    b.polygon(0.17, facets, PI / 2.0);
    if prng.chance(0.5) {
        b.segment((-0.92, -0.2), (-0.92, 0.2));
        b.segment((0.92, -0.2), (0.92, 0.2));
    }
}

/// Flare: rays diverging from one point. Deliberately the inverse of Collect.
fn scatter(b: &mut Builder, prng: &mut Prng) {
    let rays = prng.range(5, 9) as usize;
    let spread = 2.1;
    for i in 0..rays {
        let t = i as f64 / (rays - 1).max(1) as f64;
        let theta = -PI / 2.0 - spread / 2.0 + t * spread;
        b.segment(
            (0.18 * theta.dcos(), 0.18 * theta.dsin()),
            (0.95 * theta.dcos(), 0.95 * theta.dsin()),
        );
        // Multiplicity ticks: repeated, small, and countable — §9.6's "no
        // implication that branch count equals runtime cardinality" is why they
        // are ticks on the rays rather than a number.
        if prng.chance(0.6) {
            let m = 0.62;
            let (nx, ny) = (-theta.dsin() * 0.09, theta.dcos() * 0.09);
            b.segment(
                (m * theta.dcos() - nx, m * theta.dsin() - ny),
                (m * theta.dcos() + nx, m * theta.dsin() + ny),
            );
        }
    }
    b.dot(0.0, 0.32, 0.1);
}

/// Knot: rays converging into a closed ring. Scatter, inverted, so a viewer who
/// has learned one gets the other.
fn collect(b: &mut Builder, prng: &mut Prng) {
    let rays = prng.range(4, 8) as usize;
    let spread = 2.1;
    for i in 0..rays {
        let t = i as f64 / (rays - 1).max(1) as f64;
        let theta = PI / 2.0 - spread / 2.0 + t * spread;
        b.segment(
            (0.92 * theta.dcos(), 0.92 * theta.dsin()),
            (0.3 * theta.dcos(), 0.3 * theta.dsin()),
        );
    }
    // The sealing ring, and the braid inside it.
    b.circle(0.3);
    b.segment((-0.21, -0.21), (0.21, 0.21));
    b.segment((0.21, -0.21), (-0.21, 0.21));
    if prng.chance(0.5) {
        b.circle(0.46);
    }
}

/// Trident: discrete ordered tines, so a viewer can count the branches.
fn fork(b: &mut Builder, prng: &mut Prng) {
    b.segment((0.0, 0.9), (0.0, 0.16));
    // Three tines is the skeleton whatever the real branch count — the mark says
    // "this divides", and *how many* is what the sectors around it show.
    // Tine spread and reach are chosen against the *corner* they produce:
    // (0.55, -0.72) is 0.905 from the centre, and the optional terminal bar
    // widens it to 0.67 → 0.983. An earlier ±0.62 by -0.86 put the corner at
    // 1.06, outside the box, which is the sort of thing only the bounds test
    // notices.
    for (i, dx) in [-0.55f64, 0.0, 0.55].into_iter().enumerate() {
        b.segment((dx, 0.16), (dx, -0.72));
        if i != 1 {
            b.segment((0.0, 0.16), (dx, 0.16));
        }
        if prng.chance(0.5) {
            b.segment((dx - 0.12, -0.72), (dx + 0.12, -0.72));
        }
    }
}

/// Circular lock: a ring with a break and an inward key. The break is the exit;
/// the key is identity/deduplication.
fn orbit(b: &mut Builder, prng: &mut Prng) {
    // A ring with a gap — the exit §9.9 requires be visible in the ring itself.
    b.arc(0.88, -PI / 2.0 + 0.38, -PI / 2.0 + TAU - 0.38);
    // Directional ticks around the circumference.
    let ticks = prng.range(6, 11) as usize;
    for i in 0..ticks {
        let theta = -PI / 2.0 + 0.5 + i as f64 * (TAU - 1.0) / ticks as f64;
        b.segment(
            (0.88 * theta.dcos(), 0.88 * theta.dsin()),
            (0.7 * theta.dcos(), 0.7 * theta.dsin()),
        );
    }
    // The inner lock: identity, if the orbit deduplicates.
    b.polygon(0.3, prng.range(3, 6) as usize, -PI / 2.0);
    // Re-entry: the returning arc, drawn inward from the gap.
    b.segment((0.0, -0.88), (0.0, -0.42));
}

/// Altar: an open bracket facing outward, closed inward. It touches the boundary
/// and nothing else, which is what a host invocation is.
fn effect(b: &mut Builder, prng: &mut Prng, family: Option<&CapabilityFamily>) {
    // The bracket, opening outward (away from the centre, i.e. up in mark space).
    // Same corner arithmetic as the fork: (0.76, -0.56) is 0.944 out.
    b.move_to(-0.76, 0.5);
    b.line_to(-0.76, -0.56);
    b.line_to(-0.32, -0.88);
    b.move_to(0.76, 0.5);
    b.line_to(0.76, -0.56);
    b.line_to(0.32, -0.88);
    // The inward face: closed, so the mark reads as a threshold rather than a
    // corridor.
    b.segment((-0.76, 0.5), (0.76, 0.5));

    // The family mark, inside the altar. Each family gets its own topology, not
    // its own colour — §4.6 requires the distinction survive monochrome.
    match family {
        Some(CapabilityFamily::Fs) => {
            // Stacked leaves.
            for i in 0..3 {
                let y = -0.28 + i as f64 * 0.24;
                b.segment((-0.36, y), (0.36, y));
            }
        }
        Some(CapabilityFamily::Net) => {
            // Concentric arcs: a signal leaving.
            for r in [0.2f64, 0.36, 0.52] {
                b.arc(r, -PI * 0.85, -PI * 0.15);
            }
        }
        Some(CapabilityFamily::Db) => {
            // A cylinder: two caps and two walls.
            b.arc(0.34, 0.0, PI);
            b.arc(0.34, -PI, 0.0);
            b.segment((-0.34, -0.18), (-0.34, 0.18));
            b.segment((0.34, -0.18), (0.34, 0.18));
        }
        Some(CapabilityFamily::Console) => {
            // A caret and a rule.
            b.segment((-0.3, -0.18), (-0.06, 0.06));
            b.segment((-0.06, 0.06), (-0.3, 0.3));
            b.segment((0.04, 0.3), (0.32, 0.3));
        }
        Some(CapabilityFamily::Clock) => {
            b.circle(0.38);
            b.segment((0.0, 0.0), (0.0, -0.28));
            b.segment((0.0, 0.0), (0.2, 0.1));
        }
        Some(CapabilityFamily::Random) => {
            // Scattered pips: the one family whose mark is deliberately not
            // symmetric, because that is what it means.
            for _ in 0..5 {
                let x = (prng.next_f64() - 0.5) * 0.7;
                let y = (prng.next_f64() - 0.5) * 0.7;
                b.dot(x, y, 0.07);
            }
        }
        Some(CapabilityFamily::Env) => {
            b.polygon(0.36, 6, 0.0);
            b.dot(0.0, 0.0, 0.09);
        }
        Some(CapabilityFamily::Process) => {
            // Interlocking tines.
            b.segment((-0.3, -0.3), (-0.3, 0.3));
            b.segment((0.0, -0.3), (0.0, 0.3));
            b.segment((0.3, -0.3), (0.3, 0.3));
            b.segment((-0.3, 0.0), (0.3, 0.0));
        }
        Some(CapabilityFamily::Mcp) => {
            // Three nodes bridged: a protocol between parties.
            b.dot(-0.32, 0.12, 0.09);
            b.dot(0.0, -0.24, 0.09);
            b.dot(0.32, 0.12, 0.09);
            b.segment((-0.32, 0.12), (0.0, -0.24));
            b.segment((0.0, -0.24), (0.32, 0.12));
        }
        // Unknown or absent: a bare pillar, which is the honest rendering of "a
        // capability this renderer has no symbol for".
        _ => {
            b.segment((0.0, -0.34), (0.0, 0.34));
            b.segment((-0.22, 0.34), (0.22, 0.34));
        }
    }
}

/// Seal: a closed polygon with an inner mark. Nothing leaves it, which is the
/// point.
fn output(b: &mut Builder, prng: &mut Prng) {
    let sides = prng.range(5, 9) as usize;
    b.polygon(0.9, sides, -PI / 2.0);
    b.polygon(0.58, sides, -PI / 2.0 + PI / sides as f64);
    b.circle(0.26);
    // The completion mark: a bar through the centre, terminated both ends.
    b.segment((-0.26, 0.0), (0.26, 0.0));
    if prng.chance(0.6) {
        b.dot(0.0, 0.0, 0.09);
    }
}

/// Dot cluster: no strokes at all. A literal is a value, not an operation, and
/// the absence of a spine is what says so.
fn literal(b: &mut Builder, prng: &mut Prng) {
    let count = prng.range(3, 6) as usize;
    for i in 0..count {
        let theta = -PI / 2.0 + i as f64 * TAU / count as f64;
        b.dot(0.5 * theta.dcos(), 0.5 * theta.dsin(), 0.13);
    }
    b.dot(0.0, 0.0, 0.1);
}

/// Broken hex: a familiar form with a piece missing. Recognizably *a* mark, and
/// recognizably not one of the others.
fn unknown(b: &mut Builder, prng: &mut Prng) {
    // Five of six sides, so the gap is the message.
    let missing = prng.range(0, 6) as usize;
    for i in 0..6 {
        if i == missing {
            continue;
        }
        let a = i as f64 * TAU / 6.0 - PI / 2.0;
        let c = (i + 1) as f64 * TAU / 6.0 - PI / 2.0;
        b.segment(
            (0.85 * a.dcos(), 0.85 * a.dsin()),
            (0.85 * c.dcos(), 0.85 * c.dsin()),
        );
    }
    // A question the mark cannot answer: a stroke that stops.
    b.segment((0.0, -0.3), (0.0, 0.12));
    b.dot(0.0, 0.36, 0.09);
}

/// Deterministic satellites: the variation layer.
///
/// Added at the rim, outside the body of every skeleton, so variation cannot
/// obscure the thing it is varying. This is the mechanical form of §10.2's
/// "variation must not erase the base semantic skeleton".
///
/// The radius band is chosen against the *corner* of the dot, not its centre: a
/// square of half-width `h` at radius `r` reaches `r + h·√2`, and placing
/// centres by the radius alone is how the first version of this overflowed the
/// normalized box by 1.8%.
fn satellites(b: &mut Builder, prng: &mut Prng) {
    if !prng.chance(0.6) {
        return;
    }
    const HALF: f64 = 0.04;
    let count = prng.range(1, 4) as usize;
    for _ in 0..count {
        let theta = prng.next_f64() * TAU;
        let r = 0.88 + prng.next_f64() * 0.05;
        debug_assert!(r + HALF * std::f64::consts::SQRT_2 <= 1.0);
        b.dot(r * theta.dcos(), r * theta.dsin(), HALF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn all_kinds() -> Vec<SigilNodeKind> {
        let mut kinds = SigilNodeKind::KNOWN.to_vec();
        kinds.push(SigilNodeKind::Unknown("portal".into()));
        kinds
    }

    fn mark_for(kind: &SigilNodeKind, label: &str) -> Mark {
        generate(kind, None, &Prng::new(7), label, MarkDetail::Full)
    }

    /// The property the whole renderer rests on.
    #[test]
    fn a_mark_is_the_same_every_time() {
        for kind in all_kinds() {
            let a = mark_for(&kind, "n1");
            let b = mark_for(&kind, "n1");
            assert_eq!(a, b, "{} is not deterministic", kind.name());
        }
    }

    /// §10.3: generated paths stay inside the normalized mark box. Asserted, not
    /// reviewed — a skeleton that overflowed would collide with its neighbours
    /// in a way that looks like a layout bug.
    #[test]
    fn every_mark_stays_inside_its_box() {
        for kind in all_kinds() {
            for i in 0..40 {
                let mark = mark_for(&kind, &format!("n{i}"));
                for point in points(&mark) {
                    let r = point.x.hypot(point.y);
                    assert!(
                        r <= 1.0 + 1e-9,
                        "{} reached {r} outside its box",
                        kind.name()
                    );
                }
                assert!(bounds_of(&mark) <= 1.0);
            }
        }
    }

    /// Every coordinate finite. A `NaN` here reaches an SVG attribute as the
    /// empty string and renders as nothing.
    #[test]
    fn every_coordinate_is_finite() {
        for kind in all_kinds() {
            for i in 0..40 {
                for point in points(&mark_for(&kind, &format!("n{i}"))) {
                    assert!(point.is_finite(), "{} produced {point:?}", kind.name());
                }
            }
        }
    }

    /// §4.6: shape carries meaning, so no two kinds may produce the same paths.
    /// Colour is not permitted to be the difference.
    #[test]
    fn no_two_kinds_produce_the_same_mark() {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for kind in all_kinds() {
            let mark = mark_for(&kind, "same-node");
            let key = format!("{:?}", mark.commands);
            assert!(
                seen.insert(key),
                "{} draws the same paths as another kind",
                kind.name()
            );
        }
    }

    /// Capability families are distinguished by topology, not by colour, so
    /// their marks must differ from each other too.
    #[test]
    fn no_two_capability_families_produce_the_same_mark() {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for family in CapabilityFamily::KNOWN {
            let mark = generate(
                &SigilNodeKind::Effect,
                Some(family),
                &Prng::new(3),
                "node",
                MarkDetail::Full,
            );
            assert!(
                seen.insert(format!("{:?}", mark.commands)),
                "the {} family draws the same mark as another",
                family.name()
            );
        }
    }

    /// §10.2: variation must not erase the skeleton. Mechanically: two nodes of
    /// the same kind share a recognizable core, and the variation layer only
    /// ever adds strokes outside it.
    #[test]
    fn variation_does_not_erase_the_skeleton() {
        for kind in all_kinds() {
            let skeleton = generate(&kind, None, &Prng::new(1), "a", MarkDetail::Minimal);
            let varied = generate(&kind, None, &Prng::new(1), "a", MarkDetail::Full);
            assert!(
                varied.commands.len() >= skeleton.commands.len(),
                "{}: variation removed strokes",
                kind.name()
            );
            assert_eq!(
                &varied.commands[..skeleton.commands.len()],
                &skeleton.commands[..],
                "{}: variation edited the skeleton instead of adding to it",
                kind.name()
            );
        }
    }

    /// Two different nodes of the same kind are visibly different individuals —
    /// otherwise a chain of stages reads as a printed font rather than a set of
    /// inscribed runes.
    #[test]
    fn two_nodes_of_one_kind_differ() {
        let mut distinct = 0;
        for i in 0..20 {
            let a = mark_for(&SigilNodeKind::Stage, &format!("n{i}"));
            let b = mark_for(&SigilNodeKind::Stage, &format!("m{i}"));
            if a != b {
                distinct += 1;
            }
        }
        assert!(
            distinct >= 12,
            "only {distinct}/20 stage pairs differed — variation is too weak"
        );
    }

    /// A mark has to survive being drawn small. Proxy: it is made of few enough
    /// strokes that they do not merge, and enough that it is not a blob.
    #[test]
    fn marks_are_legible_at_small_sizes() {
        for kind in all_kinds() {
            let mark = mark_for(&kind, "n0");
            let count = mark.commands.len();
            assert!(
                count >= 2,
                "{}: {count} commands is not a mark",
                kind.name()
            );
            assert!(
                count <= 140,
                "{}: {count} commands will merge into a blob at 24px",
                kind.name()
            );
        }
    }

    /// Every subpath begins with a move. A stroke that continued from wherever
    /// the previous one ended would join two shapes into one.
    #[test]
    fn every_subpath_starts_with_a_move() {
        for kind in all_kinds() {
            let mark = mark_for(&kind, "n0");
            assert!(
                matches!(mark.commands.first(), Some(PathCommand::MoveTo(_))),
                "{} does not begin with a move",
                kind.name()
            );
        }
    }

    fn points(mark: &Mark) -> Vec<Point> {
        mark.commands
            .iter()
            .flat_map(|c| match c {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => vec![*p],
                PathCommand::CubicTo { c1, c2, to } => vec![*c1, *c2, *to],
                PathCommand::ArcTo { to, .. } => vec![*to],
                PathCommand::Close => vec![],
            })
            .collect()
    }
}
