//! The normalized Sigil graph: what the renderer draws.
//!
//! This is *not* Cant's graph. It is a renderer-facing model that Cant adapts
//! into, for the reasons in `docs/adr/0006-sigil-consumes-a-normalized-graph.md`:
//! Cant's type would drag a parser into every build that wants to draw a picture
//! from a JSON file, the two shapes genuinely differ, and untrusted input needs a
//! boundary that exists somewhere specific.
//!
//! # What it deliberately does not have
//!
//! **No coordinates.** Not as an option, not as a reserved field, not as a hint
//! Sigil might honour later. Geometry is computed from topology by the layout
//! engine and lives in the scene. A graph that could carry a position would be a
//! graph in which position could start to matter, which is the thing
//! `docs/adr/0004-sigil-layout-is-non-semantic.md` exists to prevent. Cant's
//! `LayoutHint` round-trips through Cant's own JSON; it does not arrive here.
//!
//! **No source text beyond labels.** A label is a string to draw or withhold. It
//! is never parsed, never matched against, and never used to decide what a node
//! means — that is what [`SigilNodeKind`] and [`SigilNode::effect`] are for.
//!
//! # Where the kinds came from
//!
//! Seven of them map one-to-one onto Cant's node kinds. Three do not:
//!
//! * [`SigilNodeKind::Effect`] — Cant records effects as a `!` on a leaf plus the
//!   capabilities that leaf names. Sigil needs an *invocation* to place on the
//!   outer boundary, so the adapter promotes an effectful node to one.
//! * [`SigilNodeKind::Output`] — Cant's `exit` is an identifier pointing at
//!   whatever kind happens to be last. Sigil needs a closing seal.
//! * [`SigilNodeKind::Unknown`] — Cant's `NodeKind` is a closed enum and should
//!   stay closed. Sigil must render a graph written by a newer producer rather
//!   than refuse it, so the unknown kind is carried as a string and drawn with
//!   the fallback mark.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use rite_core::Span;

/// The schema name a normalized Sigil graph carries.
pub const GRAPH_SCHEMA_NAME: &str = "rite.sigil.graph";

/// The schema version. Bumped when the shape changes in a way a consumer would
/// notice. Independent of the crate version, of `cant.graph`, and of the scene
/// schema — all four move on their own terms.
pub const GRAPH_SCHEMA_VERSION: u32 = 1;

/// A node identifier, stable within one graph and not globally unique.
///
/// A `String` rather than an integer because Sigil takes graphs from more than
/// one producer, and requiring every producer to number its nodes densely from
/// zero is a constraint the renderer has no reason to impose. Sanitized into an
/// SVG-safe element ID at serialization time; the raw value is preserved here so
/// a diagnostic names what the author wrote.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

/// An edge identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeId(pub String);

/// A region identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegionId(pub String);

macro_rules! id_impls {
    ($($t:ident),*) => {$(
        impl $t {
            pub fn new(s: impl Into<String>) -> Self { $t(s.into()) }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
        }
        impl From<&str> for $t {
            fn from(s: &str) -> Self { $t(s.to_string()) }
        }
        impl From<String> for $t {
            fn from(s: String) -> Self { $t(s) }
        }
    )*};
}
id_impls!(NodeId, EdgeId, RegionId);

/// Which language a graph came from. Recorded so a renderer can note provenance
/// in the Codex; it never changes how anything is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceLanguage {
    Cant,
    Rite,
    /// A producer Sigil does not know by name.
    Other(String),
}

impl SourceLanguage {
    pub fn as_str(&self) -> &str {
        match self {
            SourceLanguage::Cant => "cant",
            SourceLanguage::Rite => "rite",
            SourceLanguage::Other(name) => name,
        }
    }
}

/// The schema a normalized graph was adapted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSchema {
    pub name: String,
    pub version: String,
}

/// What a node does, in the vocabulary the visual grammar is written in.
///
/// The order of the variants is the order §9 of the specification introduces
/// them, and it is also the radial order of the composition — centre outward.
/// Not load-bearing, but a reader comparing the two should not have to sort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SigilNodeKind {
    /// Where values come from. Drawn at the centre.
    Source,
    /// An ordinary stage: a mark on a flow path.
    Stage,
    /// A gate across a flow path.
    Ward,
    /// One value becoming many.
    Scatter,
    /// Many values becoming one, and sealed.
    Collect,
    /// Ordered branches from one input.
    Fork,
    /// The bounded fixed point: a closed ring.
    Orbit,
    /// A capability invocation, on the outer boundary.
    Effect,
    /// The closing seal.
    Output,
    /// A constant. Distinguished from `Source` because a literal has no upstream
    /// and a source may.
    Literal,
    /// A kind this renderer version does not know. Carried rather than rejected
    /// so a graph from a newer producer renders with a fallback mark instead of
    /// failing — the string is what the producer called it.
    Unknown(String),
}

impl SigilNodeKind {
    /// The stable name used in scene element classes, legend keys, and
    /// diagnostics. `Unknown` reports `"unknown"` rather than the producer's
    /// string, because the *class* is what a stylesheet targets — the specific
    /// name lives in [`SigilNodeKind::unknown_name`] and in the Codex.
    pub fn name(&self) -> &'static str {
        match self {
            SigilNodeKind::Source => "source",
            SigilNodeKind::Stage => "stage",
            SigilNodeKind::Ward => "ward",
            SigilNodeKind::Scatter => "scatter",
            SigilNodeKind::Collect => "collect",
            SigilNodeKind::Fork => "fork",
            SigilNodeKind::Orbit => "orbit",
            SigilNodeKind::Effect => "effect",
            SigilNodeKind::Output => "output",
            SigilNodeKind::Literal => "literal",
            SigilNodeKind::Unknown(_) => "unknown",
        }
    }

    /// What the producer called a kind this renderer does not know.
    pub fn unknown_name(&self) -> Option<&str> {
        match self {
            SigilNodeKind::Unknown(name) => Some(name),
            _ => None,
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, SigilNodeKind::Unknown(_))
    }

    /// Every kind the renderer knows, for exhaustiveness tests and for the
    /// legend. `Unknown` is absent: it has no single value.
    pub const KNOWN: &'static [SigilNodeKind] = &[
        SigilNodeKind::Source,
        SigilNodeKind::Stage,
        SigilNodeKind::Ward,
        SigilNodeKind::Scatter,
        SigilNodeKind::Collect,
        SigilNodeKind::Fork,
        SigilNodeKind::Orbit,
        SigilNodeKind::Effect,
        SigilNodeKind::Output,
        SigilNodeKind::Literal,
    ];
}

/// Which host family a capability belongs to.
///
/// A closed set with an escape hatch, because the visual grammar gives each
/// family its own invocation mark (§9.10) and an open string would mean a mark
/// per spelling. `Other` gets the generic altar and a Codex note, which is the
/// honest rendering of "a capability this renderer has no symbol for".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityFamily {
    Fs,
    Net,
    Db,
    Console,
    Clock,
    Random,
    Env,
    Process,
    Mcp,
    Other(String),
}

impl CapabilityFamily {
    /// The family a namespace belongs to.
    ///
    /// The mapping is over Rite's actual capability namespaces rather than
    /// invented: `@http` is the network family because `@http` is what Rite
    /// calls it, and a renderer that only knew `net` would draw every HTTP call
    /// with the generic mark.
    pub fn from_namespace(namespace: &str) -> Self {
        match namespace.trim_start_matches('@') {
            "fs" => CapabilityFamily::Fs,
            "http" | "net" => CapabilityFamily::Net,
            "db" => CapabilityFamily::Db,
            "console" => CapabilityFamily::Console,
            "clock" | "time" => CapabilityFamily::Clock,
            "random" | "rand" => CapabilityFamily::Random,
            "env" => CapabilityFamily::Env,
            "process" | "proc" => CapabilityFamily::Process,
            "mcp" => CapabilityFamily::Mcp,
            other => CapabilityFamily::Other(other.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            CapabilityFamily::Fs => "fs",
            CapabilityFamily::Net => "net",
            CapabilityFamily::Db => "db",
            CapabilityFamily::Console => "console",
            CapabilityFamily::Clock => "clock",
            CapabilityFamily::Random => "random",
            CapabilityFamily::Env => "env",
            CapabilityFamily::Process => "process",
            CapabilityFamily::Mcp => "mcp",
            CapabilityFamily::Other(name) => name,
        }
    }

    /// Every family with its own mark. `Other` is absent for the same reason
    /// `Unknown` is absent from [`SigilNodeKind::KNOWN`].
    pub const KNOWN: &'static [CapabilityFamily] = &[
        CapabilityFamily::Fs,
        CapabilityFamily::Net,
        CapabilityFamily::Db,
        CapabilityFamily::Console,
        CapabilityFamily::Clock,
        CapabilityFamily::Random,
        CapabilityFamily::Env,
        CapabilityFamily::Process,
        CapabilityFamily::Mcp,
    ];
}

/// What a node does to the world outside the program.
///
/// Read from graph fields, never from a label. `performs` is the distinction
/// that matters: a node can *name* a capability without invoking it, and only an
/// invocation earns a place on the outer boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectMetadata {
    /// The node performs the effect — Cant's `!`.
    pub performs: bool,
    /// The capabilities the node names, in source order, deduplicated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,
}

/// One host capability a node names.
///
/// The **family is always present; the name may not be.** They are different
/// kinds of fact, and conflating them leaks source.
///
/// A family is a semantic classification this renderer invented — `fs`, `net`,
/// `db` — and it is what decides which invocation mark a node gets, so layout
/// cannot work without it. A *name* is `@fs.read`: text the user wrote, which
/// appears verbatim in their program. Carrying the name unconditionally would
/// put the user's source in the Codex of every Veiled render, which is the leak
/// `docs/adr/0007-veil-and-source-privacy.md` exists to prevent.
///
/// So the adapter carries the family always and the name only when labels were
/// asked for. The privacy decision is made once, at the boundary, rather than
/// filtered out at each of the places that might display it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// The full name as the producer wrote it: `@fs.read`. Untrusted text.
    /// `None` unless labels were requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub family: CapabilityFamily,
}

impl Capability {
    /// A capability known only by its family — the Veiled default.
    pub fn anonymous(family: CapabilityFamily) -> Self {
        Capability { name: None, family }
    }

    /// What is safe to show in any mode: the family, never the name.
    pub fn safe_summary(&self) -> &str {
        self.family.name()
    }
}

/// Where a node came from in the original source.
///
/// Optional at every level, because a graph read from JSON may have been written
/// by something with no source at all. A renderer that required a span could not
/// draw such a graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    pub span: Span,
    /// The source text the span covers, when the producer included it. Untrusted,
    /// length-bounded, and removed entirely under `--metadata none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// A node in the normalized graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigilNode {
    pub id: NodeId,
    #[serde(flatten)]
    pub kind: SigilNodeKind,
    /// Full human-readable text. Drawn only in Revealed mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// An abbreviated form for Inscribed mode, where space is tight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<EffectMetadata>,
    /// The region that owns this node, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    /// Producer-specific extras. A `BTreeMap` so serialization has one possible
    /// order — the same reason Cant's graph is all `Vec`s. Values are carried
    /// through to the Codex and never interpreted as semantics.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

impl SigilNode {
    pub fn new(id: impl Into<NodeId>, kind: SigilNodeKind) -> Self {
        SigilNode {
            id: id.into(),
            kind,
            label: None,
            short_label: None,
            source: None,
            effect: None,
            region: None,
            attributes: BTreeMap::new(),
        }
    }

    /// Whether this node belongs on the outer invocation boundary.
    ///
    /// One question, answered one way, so the layout engine and the legend
    /// cannot disagree about what an invocation is.
    pub fn is_invocation(&self) -> bool {
        matches!(self.kind, SigilNodeKind::Effect)
            || self.effect.as_ref().is_some_and(|e| e.performs)
    }

    /// The families this node touches, for choosing its capability mark.
    pub fn families(&self) -> Vec<&CapabilityFamily> {
        self.effect
            .as_ref()
            .map(|e| e.capabilities.iter().map(|c| &c.family).collect())
            .unwrap_or_default()
    }
}

/// What an edge is for.
///
/// Carried rather than inferred, on exactly Cant's reasoning: "is this cycle
/// allowed?" is answered by asking whether every back edge is
/// [`EdgeKind::Feedback`], and working that out from the shape of the graph is
/// the analysis the label makes unnecessary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// One stage to the next along a flow.
    Flow,
    /// Into a nested region — a fork branch or an orbit body.
    Enter,
    /// A nested region returning to the node that opened it.
    Join,
    /// A region returning candidates to its own worklist. The only cycle.
    Feedback,
}

/// One end of an edge.
///
/// Ports are numbered rather than anonymous because a fork's edge into branch 2
/// has to be distinguishable from its edge carrying the concatenated result
/// onward, and because multi-output nodes are representable in Cant's graph
/// already.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    #[serde(default)]
    pub port: u32,
}

impl PortRef {
    pub fn new(node: impl Into<NodeId>, port: u32) -> Self {
        PortRef {
            node: node.into(),
            port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigilEdge {
    pub id: EdgeId,
    pub from: PortRef,
    pub to: PortRef,
    /// Order among edges leaving the same port — branch order, in a fork.
    ///
    /// **Authoritative over array position.** The layout engine sorts by this,
    /// so a consumer that reorders the edge list still gets the same sectors.
    #[serde(default)]
    pub ordinal: u32,
    pub kind: EdgeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
}

/// What a region is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    /// One ordered branch of a fork. Gets an angular sector.
    Branch,
    /// A bounded fixed point. Gets a closed ring.
    Orbit,
    /// A grouping with no ring or sector semantics of its own.
    Group,
}

/// A nested semantic region: a fork branch, an orbit body, a group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigilRegion {
    pub id: RegionId,
    pub kind: RegionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<RegionId>,
    /// The node that opens this region — the fork or the orbit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<NodeId>,
    /// Members in flow order.
    pub members: Vec<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exits: Vec<NodeId>,
    /// Order among sibling regions — a fork's branch order.
    #[serde(default)]
    pub ordinal: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

impl SigilRegion {
    pub fn new(id: impl Into<RegionId>, kind: RegionKind) -> Self {
        SigilRegion {
            id: id.into(),
            kind,
            parent: None,
            owner: None,
            members: Vec::new(),
            entry: None,
            exits: Vec::new(),
            ordinal: 0,
            attributes: BTreeMap::new(),
        }
    }
}

/// Graph-level metadata. Provenance and display text, never semantics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// The source file's name. **Excluded from the fingerprint** — renaming a
    /// file must not change the artifact it renders to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    /// Length of the original source in bytes, so a consumer can tell whether a
    /// span it holds is in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_length: Option<u32>,
    /// What produced the graph this was adapted from. Diagnostic only, and
    /// excluded from the fingerprint for the reason `cant.graph` gives: keying
    /// an artifact on a producer version invalidates every cached render on a
    /// release that changed no graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A whole program as a graph the renderer can draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigilGraph {
    #[serde(default = "default_schema_name")]
    pub schema: String,
    #[serde(default = "default_schema_version")]
    pub version: u32,
    pub source_language: SourceLanguage,
    /// The schema this was adapted from, so the Codex can say where it came
    /// from and a bug report names both formats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_schema: Option<SourceSchema>,
    /// The centre of the composition.
    pub entry: NodeId,
    /// Closing seals, in the order they should occupy cardinal points.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exits: Vec<NodeId>,
    pub nodes: Vec<SigilNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<SigilEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<SigilRegion>,
    #[serde(default)]
    pub metadata: GraphMetadata,
}

fn default_schema_name() -> String {
    GRAPH_SCHEMA_NAME.to_string()
}

const fn default_schema_version() -> u32 {
    GRAPH_SCHEMA_VERSION
}

impl SigilGraph {
    /// An empty graph with the current schema stamped on it.
    pub fn new(source_language: SourceLanguage, entry: impl Into<NodeId>) -> Self {
        SigilGraph {
            schema: GRAPH_SCHEMA_NAME.to_string(),
            version: GRAPH_SCHEMA_VERSION,
            source_language,
            source_schema: None,
            entry: entry.into(),
            exits: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            regions: Vec::new(),
            metadata: GraphMetadata::default(),
        }
    }

    /// A linear scan rather than an index, for the reason Cant's graph gives:
    /// identifiers are dense in practice, and keeping it a scan means a
    /// *deserialized* graph with duplicates is handled by validation rather than
    /// by whichever entry an index happened to keep.
    pub fn node(&self, id: &NodeId) -> Option<&SigilNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    pub fn edge(&self, id: &EdgeId) -> Option<&SigilEdge> {
        self.edges.iter().find(|e| &e.id == id)
    }

    pub fn region(&self, id: &RegionId) -> Option<&SigilRegion> {
        self.regions.iter().find(|r| &r.id == id)
    }

    /// Edges leaving a node, in ordinal order.
    ///
    /// Sorted here rather than trusted from the array, because branch order is
    /// what the fork sector allocation reads and a consumer that reordered the
    /// list must not silently reorder the picture.
    pub fn edges_from(&self, id: &NodeId) -> Vec<&SigilEdge> {
        let mut out: Vec<&SigilEdge> = self.edges.iter().filter(|e| &e.from.node == id).collect();
        out.sort_by_key(|e| (e.ordinal, e.from.port, e.id.0.clone()));
        out
    }

    /// Edges arriving at a node, in ordinal order.
    pub fn edges_to(&self, id: &NodeId) -> Vec<&SigilEdge> {
        let mut out: Vec<&SigilEdge> = self.edges.iter().filter(|e| &e.to.node == id).collect();
        out.sort_by_key(|e| (e.ordinal, e.to.port, e.id.0.clone()));
        out
    }

    /// Nodes that belong on the outer invocation boundary, in graph order.
    pub fn invocations(&self) -> Vec<&SigilNode> {
        self.nodes.iter().filter(|n| n.is_invocation()).collect()
    }

    /// Every capability family the program touches, deduplicated, in graph
    /// order. What the legend groups invocation marks by.
    pub fn capability_families(&self) -> Vec<CapabilityFamily> {
        let mut out: Vec<CapabilityFamily> = Vec::new();
        for node in &self.nodes {
            for family in node.families() {
                if !out.contains(family) {
                    out.push(family.clone());
                }
            }
        }
        out
    }

    /// Regions whose parent is `parent`, in ordinal order.
    pub fn child_regions(&self, parent: Option<&RegionId>) -> Vec<&SigilRegion> {
        let mut out: Vec<&SigilRegion> = self
            .regions
            .iter()
            .filter(|r| r.parent.as_ref() == parent)
            .collect();
        out.sort_by_key(|r| (r.ordinal, r.id.0.clone()));
        out
    }

    /// A count of each node kind, for the accessible text summary required by
    /// §23 — "one source, seven stages, one ward…".
    pub fn kind_census(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for node in &self.nodes {
            *out.entry(node.kind.name().to_string()).or_insert(0) += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_families_cover_rites_actual_namespaces() {
        // `@http` rather than `@net` is what Rite calls the network capability,
        // and a renderer that only knew `net` would draw every HTTP call with
        // the generic mark.
        assert_eq!(
            CapabilityFamily::from_namespace("@http"),
            CapabilityFamily::Net
        );
        assert_eq!(CapabilityFamily::from_namespace("fs"), CapabilityFamily::Fs);
        assert_eq!(
            CapabilityFamily::from_namespace("@db"),
            CapabilityFamily::Db
        );
        assert_eq!(
            CapabilityFamily::from_namespace("@mcp"),
            CapabilityFamily::Mcp
        );
        assert_eq!(
            CapabilityFamily::from_namespace("@weather"),
            CapabilityFamily::Other("weather".into())
        );
    }

    /// An unknown kind reports the generic class for styling and keeps the
    /// producer's word for the Codex. Conflating the two would either style by a
    /// name no stylesheet knows or lose what the producer said.
    #[test]
    fn an_unknown_kind_keeps_both_names() {
        let kind = SigilNodeKind::Unknown("quantum_gate".into());
        assert_eq!(kind.name(), "unknown");
        assert_eq!(kind.unknown_name(), Some("quantum_gate"));
        assert!(kind.is_unknown());
        assert!(!SigilNodeKind::Stage.is_unknown());
        assert_eq!(SigilNodeKind::Stage.unknown_name(), None);
    }

    /// Branch order is `ordinal`, not array position — the rule Cant's schema
    /// insists on, restated here because this is the accessor the layout engine
    /// uses and it is the one that must not be wrong.
    #[test]
    fn edges_come_back_in_ordinal_order_whatever_the_array_says() {
        let mut graph = SigilGraph::new(SourceLanguage::Cant, "n0");
        graph.nodes.push(SigilNode::new("n0", SigilNodeKind::Fork));
        for (id, ordinal) in [("e2", 2u32), ("e0", 0), ("e1", 1)] {
            graph.edges.push(SigilEdge {
                id: EdgeId::new(id),
                from: PortRef::new("n0", ordinal + 1),
                to: PortRef::new(format!("b{ordinal}"), 0),
                ordinal,
                kind: EdgeKind::Enter,
                region: None,
            });
        }
        let order: Vec<u32> = graph
            .edges_from(&NodeId::new("n0"))
            .iter()
            .map(|e| e.ordinal)
            .collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn an_effectful_node_is_an_invocation_however_its_kind_is_spelled() {
        let mut stage = SigilNode::new("n1", SigilNodeKind::Stage);
        assert!(!stage.is_invocation());
        stage.effect = Some(EffectMetadata {
            performs: true,
            capabilities: vec![Capability::anonymous(CapabilityFamily::Fs)],
        });
        assert!(stage.is_invocation());
        assert_eq!(stage.families(), vec![&CapabilityFamily::Fs]);

        // Naming a capability without performing it is not an invocation: only
        // a `!` earns a place on the outer boundary.
        let mut names_only = SigilNode::new("n2", SigilNodeKind::Stage);
        names_only.effect = Some(EffectMetadata {
            performs: false,
            capabilities: vec![Capability::anonymous(CapabilityFamily::Fs)],
        });
        assert!(!names_only.is_invocation());
    }

    /// The type has no field for a coordinate and must not grow one — ADR 0004.
    /// A serialized graph is the enforcement surface, because that is what an
    /// adapter or a hostile input would use to smuggle one in.
    #[test]
    fn a_serialized_graph_carries_no_geometry() {
        let mut graph = SigilGraph::new(SourceLanguage::Cant, "n0");
        graph
            .nodes
            .push(SigilNode::new("n0", SigilNodeKind::Source));
        let json = serde_json::to_string(&graph).expect("serializes");
        for banned in [
            "\"x\"",
            "\"y\"",
            "\"width\"",
            "\"height\"",
            "\"layout\"",
            "\"angle\"",
            "\"radius\"",
        ] {
            assert!(
                !json.contains(banned),
                "the normalized graph gained {banned}; geometry belongs in the scene (ADR 0004)"
            );
        }
    }
}
