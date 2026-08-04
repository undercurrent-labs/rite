//! Sigil — a program's semantic topology as a ritual artifact.
//!
//! ```
//! use rite_sigil::{normalize_json, NormalizeOptions};
//!
//! let json = r#"{
//!   "schema": "rite.sigil.graph",
//!   "version": 1,
//!   "source_language": "cant",
//!   "entry": "n0",
//!   "exits": ["n1"],
//!   "nodes": [
//!     { "id": "n0", "kind": "source" },
//!     { "id": "n1", "kind": "output" }
//!   ],
//!   "edges": [
//!     { "id": "e0", "from": {"node": "n0"}, "to": {"node": "n1"}, "kind": "flow" }
//!   ]
//! }"#;
//!
//! let normalized = normalize_json(json, &NormalizeOptions::default()).expect("a valid graph");
//! assert_eq!(normalized.graph.nodes.len(), 2);
//! // The default render seed, and the same value every time.
//! assert_eq!(normalized.fingerprint.as_str().len(), 32);
//! ```
//!
//! # What this crate is
//!
//! A deterministic renderer: normalized graph in, scene out, SVG after that. It
//! does not parse a language, execute a program, invoke a capability, or open a
//! file. The dependency list in `Cargo.toml` is the enforcement point and
//! `tests/boundaries.rs` reads it.
//!
//! The decisions that constrain everything here are recorded as ADRs:
//!
//! | | |
//! |---|---|
//! | [0003] | Sigil is a semantic renderer, not a runtime |
//! | [0004] | Layout is non-semantic |
//! | [0005] | One renderer, in Rust, shared by the CLI and the browser |
//! | [0006] | Sigil consumes a normalized adapter graph |
//! | [0007] | Veiled rendering and source privacy are first-class |
//! | [0008] | Graphviz stays the technical view |
//!
//! [0003]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0003-sigil-is-a-renderer-not-a-runtime.md
//! [0004]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0004-sigil-layout-is-non-semantic.md
//! [0005]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0005-one-renderer-in-rust.md
//! [0006]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0006-sigil-consumes-a-normalized-graph.md
//! [0007]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0007-veil-and-source-privacy.md
//! [0008]: https://github.com/undercurrent-labs/rite/blob/main/docs/adr/0008-graphviz-stays-the-technical-view.md
//!
//! # The boundary
//!
//! Everything entering this crate is untrusted: graph JSON pasted into a web
//! page, a label written by someone else, a node kind from a producer that has
//! run ahead of this renderer. [`normalize`] and [`normalize_json`] are where
//! that stops being true. Nothing downstream re-checks, so nothing downstream
//! may be reached without going through one of them.

pub mod analysis;
pub mod canonical;
pub mod diagnostic;
pub mod graph;
pub mod layout;
pub mod limits;
pub mod marks;
pub mod ornament;
pub mod scene;
pub mod svg;
pub mod theme;
pub mod validate;

pub use analysis::{analyze, Placement, Topology};
pub use canonical::{
    canonical_json, fingerprint, fingerprint_of_bytes, semantic_json, GraphFingerprint, Prng,
};
pub use diagnostic::{
    parse_code, Diagnostics, GraphRef, SigilCategory, SigilCode, SigilDiagnostic, ALL_CODES,
};
pub use graph::{
    Capability, CapabilityFamily, EdgeId, EdgeKind, EffectMetadata, GraphMetadata, NodeId, PortRef,
    RegionId, RegionKind, SigilEdge, SigilGraph, SigilNode, SigilNodeKind, SigilRegion,
    SourceLanguage, SourceRef, SourceSchema, GRAPH_SCHEMA_NAME, GRAPH_SCHEMA_VERSION,
};
pub use layout::{build_scene, LayoutOptions, Orientation};
pub use limits::{NormalizeOptions, RenderLimits};
pub use marks::{Mark, MarkDetail};
pub use ornament::OrnamentLevel;
pub use scene::{
    Geometry, HitRegion, LegendEntry, PathCommand, Point, Rect, SceneElement, SceneLayerKind,
    SceneMetadata, SceneRef, SemanticKind, SigilScene, SCENE_SCHEMA_NAME, SCENE_SCHEMA_VERSION,
};
#[cfg(feature = "png")]
pub use svg::render_png;
pub use svg::{
    render_svg, Background, DisclosureMode, MetadataMode, RenderFingerprint, RenderedSvg,
    SvgOptions,
};
pub use theme::{Theme, ThemeId, THEME_VERSION};
pub use validate::Validated;

/// This renderer's version, reported in every render fingerprint.
///
/// The crate's own number, not the workspace's: Sigil renders graphs from more
/// than one producer, and tying its version to either language's would make a
/// renderer release imply a language one.
pub const RENDERER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A graph that has been validated and fingerprinted, ready to lay out.
///
/// Holding the fingerprint alongside the graph rather than recomputing it is
/// what makes "the default seed is the graph" cheap enough to be the default:
/// the hash is over a canonical serialization, so doing it once per render
/// rather than once per consumer matters.
#[derive(Debug, Clone)]
pub struct NormalizedGraph {
    pub graph: SigilGraph,
    pub fingerprint: GraphFingerprint,
    /// Warnings. Errors never reach here — [`normalize`] returns them instead.
    pub diagnostics: Diagnostics,
}

impl NormalizedGraph {
    /// The default render seed.
    pub fn seed(&self) -> u64 {
        self.fingerprint.seed()
    }
}

/// Validate a graph and fingerprint it.
///
/// `Err` carries every diagnostic, warnings included, so a caller reporting a
/// failure shows the whole picture rather than only the fatal part.
pub fn normalize(
    graph: SigilGraph,
    options: &NormalizeOptions,
) -> Result<NormalizedGraph, Diagnostics> {
    let validated = validate::validate(graph, options);
    if validated.diagnostics.has_errors() {
        return Err(validated.diagnostics);
    }
    let fingerprint = canonical::fingerprint(&validated.graph);
    Ok(NormalizedGraph {
        graph: validated.graph,
        fingerprint,
        diagnostics: validated.diagnostics,
    })
}

/// The same, from JSON.
///
/// The size check happens before the parse, because everything the parse does is
/// proportional to the input and a 400 MiB document should be refused rather
/// than allocated.
pub fn normalize_json(
    json: &str,
    options: &NormalizeOptions,
) -> Result<NormalizedGraph, Diagnostics> {
    if json.len() > options.limits.max_input_bytes {
        let mut d = Diagnostics::new();
        d.push(SigilDiagnostic::error(
            diagnostic::SIGIL_S005_INPUT_TOO_LARGE,
            GraphRef::Graph,
            format!(
                "{} bytes of input, cap is {}",
                json.len(),
                options.limits.max_input_bytes
            ),
        ));
        return Err(d);
    }
    let graph: SigilGraph = serde_json::from_str(json).map_err(|e| {
        let mut d = Diagnostics::new();
        d.push(
            SigilDiagnostic::error(
                diagnostic::SIGIL_G002_UNKNOWN_NODE,
                GraphRef::Graph,
                format!("could not read the graph: {e}"),
            )
            .with_note("expected a `rite.sigil.graph` document"),
        );
        d
    })?;
    normalize(graph, options)
}

/// Read a graph without validating it, for a tool that wants to inspect a
/// broken one.
///
/// Not a rendering path. Everything that draws goes through [`normalize`].
pub fn parse_graph_json(json: &str) -> Result<SigilGraph, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> SigilGraph {
        let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
        g.nodes.push(SigilNode::new("n0", SigilNodeKind::Source));
        g.nodes.push(SigilNode::new("n1", SigilNodeKind::Output));
        g.exits.push("n1".into());
        g.edges.push(SigilEdge {
            id: EdgeId::new("e0"),
            from: PortRef::new("n0", 0),
            to: PortRef::new("n1", 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
        g
    }

    #[test]
    fn normalizing_produces_a_stable_fingerprint_and_seed() {
        let a = normalize(graph(), &NormalizeOptions::default()).expect("valid");
        let b = normalize(graph(), &NormalizeOptions::default()).expect("valid");
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_eq!(a.seed(), b.seed());
    }

    #[test]
    fn a_graph_with_errors_comes_back_as_diagnostics() {
        let mut g = graph();
        g.entry = NodeId::new("missing");
        let err = normalize(g, &NormalizeOptions::default()).expect_err("invalid");
        assert!(err.has_errors());
        assert_eq!(err.exit_code(), 4);
    }

    /// Warnings do not stop a render — they travel with it.
    #[test]
    fn warnings_survive_into_the_normalized_graph() {
        let mut g = graph();
        g.nodes.push(SigilNode::new(
            "n2",
            SigilNodeKind::Unknown("portal".into()),
        ));
        let normalized = normalize(g, &NormalizeOptions::default()).expect("still renders");
        assert!(!normalized.diagnostics.is_empty());
        assert!(!normalized.diagnostics.has_errors());
    }

    #[test]
    fn json_round_trips_through_the_normalizer() {
        let json = serde_json::to_string(&graph()).expect("serializes");
        let normalized = normalize_json(&json, &NormalizeOptions::default()).expect("valid");
        assert_eq!(normalized.graph, graph());
    }

    /// The size check runs before the parse, so an oversized document is
    /// refused rather than allocated.
    #[test]
    fn oversized_input_is_refused_before_it_is_parsed() {
        let mut options = NormalizeOptions::default();
        options.limits.max_input_bytes = 16;
        let json = serde_json::to_string(&graph()).expect("serializes");
        assert!(json.len() > 16);
        let err = normalize_json(&json, &options).expect_err("too large");
        assert_eq!(
            err.iter().next().expect("one").code.to_string(),
            "SIGIL-S005"
        );
    }

    #[test]
    fn malformed_json_is_a_diagnostic_not_a_panic() {
        let err = normalize_json("{ not json", &NormalizeOptions::default()).expect_err("bad");
        assert!(err.has_errors());
    }

    /// The doc example's document is the one a caller would actually write:
    /// optional fields omitted, defaults implied.
    #[test]
    fn a_minimal_document_omitting_defaults_is_accepted() {
        let json = r#"{
          "source_language": "cant",
          "entry": "n0",
          "nodes": [{ "id": "n0", "kind": "source" }]
        }"#;
        let normalized = normalize_json(json, &NormalizeOptions::default()).expect("valid");
        assert_eq!(normalized.graph.schema, GRAPH_SCHEMA_NAME);
        assert_eq!(normalized.graph.version, GRAPH_SCHEMA_VERSION);
    }
}
