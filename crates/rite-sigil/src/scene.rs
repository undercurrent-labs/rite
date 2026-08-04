//! The scene: geometry, layered, with every element traceable to the graph.
//!
//! The scene exists so that layout regressions and serialization regressions are
//! different test failures. `cant sigil --format scene-json` is the fast loop
//! for the first, and it is why an aesthetic change to the SVG writer cannot
//! silently move a node.
//!
//! # Why layers rather than draw order
//!
//! Ornament has to be removable *without relayout* (ADR 0004). If ornament were
//! interleaved with semantic geometry by z-index alone, "remove the ornament"
//! would mean filtering by class and hoping nothing else changed. A layer is an
//! explicit container: drop [`SceneLayerKind::OrnamentBack`] and
//! [`SceneLayerKind::OrnamentFront`], and every remaining coordinate is bit-for-bit
//! what it was. That is a property a test can state.
//!
//! # Why every element carries a graph reference
//!
//! Three things need it and none of them can reconstruct it: the Codex maps a
//! click back to a node, the accessible summary needs to say what a shape is,
//! and the SVG serializer needs a stable element ID that survives a re-render.
//! Deriving IDs from draw order would make them change whenever anything moved.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::graph::{NodeId, SigilNodeKind};

/// The scene schema name.
pub const SCENE_SCHEMA_NAME: &str = "rite.sigil.scene";

/// The scene schema version. Experimental in v0 — the specification says so, and
/// it moves independently of both graph schemas.
pub const SCENE_SCHEMA_VERSION: u32 = 1;

/// A point in scene space.
///
/// `f64` throughout, rounded only at serialization. Rounding during layout would
/// make collision resolution depend on accumulated rounding error, which is the
/// kind of thing that differs between a native and a wasm32 build.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.width && p.y >= self.y && p.y <= self.y + self.height
    }

    /// Grown by `margin` on every side. Used for the bounds assertion, which
    /// allows a stroke to overhang the viewBox slightly without failing.
    pub fn expanded(&self, margin: f64) -> Rect {
        Rect::new(
            self.x - margin,
            self.y - margin,
            self.width + 2.0 * margin,
            self.height + 2.0 * margin,
        )
    }
}

/// Which layer an element belongs to.
///
/// The order here is the paint order, back to front, and
/// [`SceneLayerKind::ALL`] relies on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SceneLayerKind {
    Background,
    GuideGeometry,
    OrnamentBack,
    SemanticRegions,
    SemanticEdges,
    SemanticNodes,
    Inscriptions,
    OrnamentFront,
    Interaction,
}

impl SceneLayerKind {
    pub const ALL: &'static [SceneLayerKind] = &[
        SceneLayerKind::Background,
        SceneLayerKind::GuideGeometry,
        SceneLayerKind::OrnamentBack,
        SceneLayerKind::SemanticRegions,
        SceneLayerKind::SemanticEdges,
        SceneLayerKind::SemanticNodes,
        SceneLayerKind::Inscriptions,
        SceneLayerKind::OrnamentFront,
        SceneLayerKind::Interaction,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SceneLayerKind::Background => "background",
            SceneLayerKind::GuideGeometry => "guide-geometry",
            SceneLayerKind::OrnamentBack => "ornament-back",
            SceneLayerKind::SemanticRegions => "semantic-regions",
            SceneLayerKind::SemanticEdges => "semantic-edges",
            SceneLayerKind::SemanticNodes => "semantic-nodes",
            SceneLayerKind::Inscriptions => "inscriptions",
            SceneLayerKind::OrnamentFront => "ornament-front",
            SceneLayerKind::Interaction => "interaction",
        }
    }

    /// Whether this layer carries meaning.
    ///
    /// The single answer to "is this ornament?", so the invariance test, the
    /// hit-region builder, and the eventual SVG writer cannot disagree about
    /// which layers may be dropped.
    pub fn is_semantic(self) -> bool {
        matches!(
            self,
            SceneLayerKind::SemanticRegions
                | SceneLayerKind::SemanticEdges
                | SceneLayerKind::SemanticNodes
                | SceneLayerKind::Inscriptions
        )
    }

    pub fn is_ornament(self) -> bool {
        matches!(
            self,
            SceneLayerKind::OrnamentBack
                | SceneLayerKind::OrnamentFront
                | SceneLayerKind::GuideGeometry
        )
    }
}

/// The two ends of an edge, carried on the element that draws it.
///
/// Without this, a consumer wanting to highlight a path had to parse the
/// endpoints back out of the edge's *identifier* — `e0:n0.0->n1.0` — with a
/// regular expression. That works only because the Cant adapter happens to build
/// the id that way, and it makes a string format a structural dependency: change
/// how edges are named and path highlighting silently stops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeEnds {
    pub from: String,
    pub to: String,
}

/// What a scene element is about.
///
/// `None` for ornament, and that is enforced rather than conventional: ornament
/// must not carry graph node IDs (§15.3), because an ornament element that could
/// be selected would put a meaningless entry in the Codex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum SceneRef {
    Node(String),
    Edge(String),
    Region(String),
}

/// What a semantic element depicts, for CSS classes and the legend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticKind {
    /// A node, of the given kind.
    Node(SigilNodeKind),
    /// A flow trace between two nodes.
    Edge(crate::graph::EdgeKind),
    /// A region ring or sector boundary.
    Region(crate::graph::RegionKind),
    /// The outer host boundary.
    InvocationBoundary,
    /// A non-semantic decoration. Carries no [`SceneRef`].
    Ornament,
}

impl SemanticKind {
    /// The CSS class, without the `sigil-` prefix the serializer adds.
    pub fn class(&self) -> String {
        match self {
            SemanticKind::Node(kind) => format!("node-{}", kind.name()),
            SemanticKind::Edge(kind) => format!(
                "edge-{}",
                match kind {
                    crate::graph::EdgeKind::Flow => "flow",
                    crate::graph::EdgeKind::Enter => "enter",
                    crate::graph::EdgeKind::Join => "join",
                    crate::graph::EdgeKind::Feedback => "feedback",
                }
            ),
            SemanticKind::Region(kind) => format!(
                "region-{}",
                match kind {
                    crate::graph::RegionKind::Branch => "branch",
                    crate::graph::RegionKind::Orbit => "orbit",
                    crate::graph::RegionKind::Group => "group",
                }
            ),
            SemanticKind::InvocationBoundary => "invocation-boundary".into(),
            SemanticKind::Ornament => "ornament".into(),
        }
    }
}

/// The geometry of one element.
///
/// Deliberately few variants. A scene made of a hundred primitive types would be
/// a drawing format; this is a semantic layout, and Phase 3's generated marks
/// arrive as [`Geometry::Mark`] with path data rather than as new variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "geometry", rename_all = "snake_case")]
pub enum Geometry {
    Circle {
        center: Point,
        radius: f64,
    },
    /// A closed or open ring arc — an orbit's circumference, a band boundary.
    Arc {
        center: Point,
        radius: f64,
        /// Radians, clockwise from the canonical axis.
        start_angle: f64,
        end_angle: f64,
    },
    Polygon {
        points: Vec<Point>,
    },
    /// An SVG path, as a sequence of commands the serializer renders.
    ///
    /// Commands rather than a `d` string, so the scene stays inspectable and the
    /// serializer owns number formatting in one place.
    Path {
        commands: Vec<PathCommand>,
    },
    /// A generated semantic mark. In Phase 2 this carries its placement and
    /// size; Phase 3 fills in `path`.
    Mark {
        center: Point,
        /// Half-width of the normalized mark box.
        size: f64,
        /// Rotation in radians, so a mark aligns with local flow direction.
        rotation: f64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        path: Vec<PathCommand>,
    },
    Text {
        anchor: Point,
        content: String,
        size: f64,
        rotation: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    CubicTo {
        c1: Point,
        c2: Point,
        to: Point,
    },
    /// An elliptical arc, as SVG spells it.
    ArcTo {
        radius: f64,
        large: bool,
        sweep: bool,
        to: Point,
    },
    Close,
}

/// One thing in the scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneElement {
    /// Stable across renders of the same graph. Derived from the graph
    /// reference and a role, never from draw order.
    pub id: String,
    pub layer: SceneLayerKind,
    pub semantic: SemanticKind,
    /// The graph object this depicts. Absent exactly when this is ornament.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_ref: Option<SceneRef>,
    #[serde(flatten)]
    pub geometry: Geometry,
    /// Accessible title. Present under `safe` and `full` metadata, stripped
    /// under `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The legend entry that decodes this element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend_key: Option<String>,
    /// For an edge, the nodes it joins. Absent on everything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<EdgeEnds>,
    /// Traced-run weight, normalized to the heaviest node: `0.0` never ran,
    /// `1.0` ran the most. Present only on the edges of a weighted render.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
    /// Axis-aligned bounds, for the bounds pass and for hit testing.
    pub bounds: Rect,
}

impl SceneElement {
    pub fn is_ornament(&self) -> bool {
        self.layer.is_ornament() || self.semantic == SemanticKind::Ornament
    }
}

/// Where a pointer or a focus ring lands.
///
/// Separate from the drawn element because the drawn thing may be a two-pixel
/// stroke and the target has to be reachable — §23's keyboard requirement is not
/// satisfiable by hit-testing a hairline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitRegion {
    pub element_id: String,
    pub graph_ref: SceneRef,
    pub center: Point,
    pub radius: f64,
    /// Tab order. Graph order, so keyboard traversal follows the program rather
    /// than the accident of where things landed.
    pub tab_index: u32,
}

/// One decoded entry in the Codex.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendEntry {
    pub key: String,
    pub graph_ref: SceneRef,
    pub semantic: SemanticKind,
    /// What kind of thing this is, always safe to show: "orbit", "filesystem
    /// invocation". Never the user's source.
    pub summary: String,
    /// The node's label. Present only when the graph carried one, which happens
    /// only when labels were asked for (ADR 0007).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<rite_core::Span>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch_ordinal: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, serde_json::Value>,
    /// Warnings about this element, for the Codex's warning indicator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// What produced this scene, and from what.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneMetadata {
    pub renderer_version: String,
    pub graph_fingerprint: String,
    pub seed: u64,
    /// How traces were drawn. Recorded so a scene says how it was built —
    /// defaulted on deserialization because scenes older than the field exist.
    #[serde(default = "default_tracery")]
    pub tracery: String,
    /// Counts, for the accessible summary and the diagnostics panel.
    pub node_count: usize,
    pub edge_count: usize,
    pub region_count: usize,
    /// One count per node kind — what §23's text summary is generated from.
    pub census: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_schema: Option<String>,
}

fn default_tracery() -> String {
    "flowing".to_string()
}

/// A layout-ready scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigilScene {
    pub schema: String,
    pub version: u32,
    pub view_box: Rect,
    pub center: Point,
    pub elements: Vec<SceneElement>,
    pub hit_regions: Vec<HitRegion>,
    pub legend: Vec<LegendEntry>,
    pub metadata: SceneMetadata,
    /// Layout complaints. Never fatal — a scene with a warning is still a scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl SigilScene {
    /// Elements on the semantic layers, in paint order.
    pub fn semantic_elements(&self) -> impl Iterator<Item = &SceneElement> {
        self.elements.iter().filter(|e| e.layer.is_semantic())
    }

    pub fn ornament_elements(&self) -> impl Iterator<Item = &SceneElement> {
        self.elements.iter().filter(|e| e.is_ornament())
    }

    /// Every element depicting a given graph node.
    pub fn elements_for(&self, node: &NodeId) -> Vec<&SceneElement> {
        self.elements
            .iter()
            .filter(|e| matches!(&e.graph_ref, Some(SceneRef::Node(id)) if id == node.as_str()))
            .collect()
    }

    /// The accessible text summary §23 requires.
    ///
    /// Built from the census rather than from element inspection, so it stays
    /// true when the visual grammar changes.
    pub fn summary(&self) -> String {
        fn plural(n: usize, one: &str, many: &str) -> String {
            let count = match n {
                1 => "one".to_string(),
                2 => "two".to_string(),
                3 => "three".to_string(),
                4 => "four".to_string(),
                5 => "five".to_string(),
                6 => "six".to_string(),
                7 => "seven".to_string(),
                8 => "eight".to_string(),
                9 => "nine".to_string(),
                n => n.to_string(),
            };
            format!("{count} {}", if n == 1 { one } else { many })
        }

        // The order of the visual grammar, centre outward, so the sentence reads
        // in the order someone would trace the picture.
        const ORDER: &[(&str, &str, &str)] = &[
            ("source", "source", "sources"),
            ("literal", "literal", "literals"),
            ("stage", "stage", "stages"),
            ("ward", "ward", "wards"),
            ("scatter", "scatter", "scatters"),
            ("collect", "collect", "collects"),
            ("fork", "fork", "forks"),
            ("orbit", "orbit", "orbits"),
            ("effect", "invocation", "invocations"),
            ("output", "output seal", "output seals"),
            ("unknown", "unknown node", "unknown nodes"),
        ];

        let parts: Vec<String> = ORDER
            .iter()
            .filter_map(|(key, one, many)| {
                let n = *self.metadata.census.get(*key)?;
                (n > 0).then(|| plural(n, one, many))
            })
            .collect();

        match parts.len() {
            0 => "This sigil is empty.".to_string(),
            1 => format!("This sigil contains {}.", parts[0]),
            _ => {
                let (last, rest) = parts.split_last().expect("checked non-empty");
                format!("This sigil contains {}, and {last}.", rest.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_classification_has_one_answer() {
        for layer in SceneLayerKind::ALL {
            assert!(
                !(layer.is_semantic() && layer.is_ornament()),
                "{} is both semantic and ornament",
                layer.name()
            );
        }
        assert!(SceneLayerKind::SemanticNodes.is_semantic());
        assert!(SceneLayerKind::OrnamentFront.is_ornament());
        // Neither: the background is a fill, not a decoration to strip and not a
        // thing that means something.
        assert!(!SceneLayerKind::Background.is_semantic());
        assert!(!SceneLayerKind::Background.is_ornament());
    }

    #[test]
    fn layer_names_are_unique_and_css_safe() {
        let mut seen = std::collections::BTreeSet::new();
        for layer in SceneLayerKind::ALL {
            assert!(seen.insert(layer.name()), "duplicate layer name");
            assert!(
                layer
                    .name()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "{} is not a safe class fragment",
                layer.name()
            );
        }
    }

    #[test]
    fn semantic_classes_are_distinct_per_kind() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in SigilNodeKind::KNOWN {
            assert!(
                seen.insert(SemanticKind::Node(kind.clone()).class()),
                "two node kinds share a class"
            );
        }
        assert_eq!(
            SemanticKind::Node(SigilNodeKind::Orbit).class(),
            "node-orbit"
        );
    }

    #[test]
    fn the_summary_reads_as_a_sentence() {
        let mut census = BTreeMap::new();
        census.insert("source".to_string(), 1);
        census.insert("stage".to_string(), 7);
        census.insert("ward".to_string(), 1);
        census.insert("effect".to_string(), 2);
        census.insert("output".to_string(), 1);
        let scene = SigilScene {
            schema: SCENE_SCHEMA_NAME.into(),
            version: SCENE_SCHEMA_VERSION,
            view_box: Rect::new(0.0, 0.0, 1600.0, 1600.0),
            center: Point::new(800.0, 800.0),
            elements: Vec::new(),
            hit_regions: Vec::new(),
            legend: Vec::new(),
            warnings: Vec::new(),
            metadata: SceneMetadata {
                renderer_version: "0.1.0".into(),
                graph_fingerprint: "0".repeat(32),
                seed: 0,
                tracery: "flowing".into(),
                node_count: 12,
                edge_count: 11,
                region_count: 0,
                census,
                source_schema: None,
            },
        };
        assert_eq!(
            scene.summary(),
            "This sigil contains one source, seven stages, one ward, two invocations, \
             and one output seal."
        );
    }

    #[test]
    fn an_empty_census_still_produces_a_sentence() {
        let scene = SigilScene {
            schema: SCENE_SCHEMA_NAME.into(),
            version: SCENE_SCHEMA_VERSION,
            view_box: Rect::new(0.0, 0.0, 1600.0, 1600.0),
            center: Point::new(800.0, 800.0),
            elements: Vec::new(),
            hit_regions: Vec::new(),
            legend: Vec::new(),
            warnings: Vec::new(),
            metadata: SceneMetadata {
                renderer_version: "0.1.0".into(),
                graph_fingerprint: "0".repeat(32),
                seed: 0,
                tracery: "flowing".into(),
                node_count: 0,
                edge_count: 0,
                region_count: 0,
                census: BTreeMap::new(),
                source_schema: None,
            },
        };
        assert_eq!(scene.summary(), "This sigil is empty.");
    }
}
