//! The Cant flow graph.
//!
//! This is the normalized semantic representation of a Cant program — not a
//! picture drawn beside one. Lowering to Rite reads *this*, not the AST, so that
//! what a future Sigil renderer displays is what actually executes.
//!
//! # What it has to carry
//!
//! Nodes, directed edges, ports, ordered branches, source spans, effectful-leaf
//! metadata, orbit policies, collection boundaries, and stable identifiers. Plus
//! [`LayoutHint`], which is **reserved and non-semantic**: a renderer may attach
//! positions, and deleting every one of them must not change what a program
//! does. That is what keeps geometry out of the language.
//!
//! # Why the cycle is in the graph
//!
//! An orbit's body loops back to the orbit node, and that back edge is a real
//! edge. It would be easier to leave it out and let the `Orbit` node imply it,
//! but then "reject every cycle that is not orbit-owned" — the one structural
//! rule v0 has to enforce — would be checking a graph in which no cycle can
//! appear. The edge is present so validation is a genuine question.
//!
//! # Determinism
//!
//! Identifiers are assigned by a depth-first walk in source order, so the same
//! source and tool version always produce the same graph. Everything is a `Vec`
//! rather than a map, so serialization has one possible order. Both are what
//! make `cant graph` snapshot-testable and its diffs readable.

use rite_core::Span;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{
    NodeId, PortKind, PortRef, DEFAULT_ORBIT_MAX, GRAPH_SCHEMA_NAME, GRAPH_SCHEMA_VERSION,
};

/// Serde default for [`CantProgram::schema`], so a version-0 graph — which had
/// no such field — still deserializes to something the version check can reject
/// with a useful message instead of a serde error about a missing key.
fn default_schema() -> String {
    GRAPH_SCHEMA_NAME.to_string()
}

/// A fork branch or an orbit body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SubgraphId(pub u32);

impl fmt::Display for SubgraphId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s{}", self.0)
    }
}

/// A host capability a node's leaf names, recorded rather than re-derived.
///
/// [`CantProgram::capabilities`] used to scan leaf text on every call, which is
/// the right thing for a producer summarizing its own source and the wrong thing
/// for a *consumer*. A renderer that has to decide "is this node a filesystem
/// invocation or a network one?" by pattern-matching a label is inferring
/// semantics from a label, which
/// `docs/adr/0006-sigil-consumes-a-normalized-graph.md` forbids by name. So the
/// scan happens once, during lowering, and the answer is in the JSON.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CapabilityRef {
    /// The full name as written, including the `@`: `@fs.read`.
    pub name: String,
    /// The namespace before the first dot: `fs`. A consumer groups by this —
    /// it is what decides which invocation mark a capability gets — so it is
    /// stored rather than left to be re-split by every reader, each of whom
    /// would have to agree about `@fs` with no dot at all.
    pub family: String,
}

impl CapabilityRef {
    /// `@fs.read` → family `fs`; a bare `@fs` → family `fs`.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let family = name
            .trim_start_matches('@')
            .split('.')
            .next()
            .unwrap_or_default()
            .to_string();
        CapabilityRef { name, family }
    }
}

/// What produced a graph, so a consumer reading a stored one knows whose bug it
/// is looking at.
///
/// The version is `cant-sem`'s own — Cant's number, not Rite's (ADR 0001,
/// Amendment 2). It is deliberately **not** part of anything a consumer hashes:
/// a renderer keying its output on the producer version would invalidate every
/// cached artifact on a release that changed nothing about the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    pub name: String,
    pub version: String,
}

impl Default for Producer {
    fn default() -> Self {
        Producer {
            name: "cant".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// A run of Rite expression text, with what Cant can tell about it on its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeafExpr {
    pub text: String,
    pub span: Span,
    /// The leaf carries a Cant effect marker (`!`).
    ///
    /// This is all Cant knows and all it needs: whether a *name* in the leaf
    /// resolves to something effectful is Rite's question, asked by its resolver
    /// after expansion. Cant only enforces the two rules it owns — a ward
    /// predicate and an orbit `:by` must not be effectful.
    pub effectful: bool,
    /// The leaf places the current emission explicitly with `$`.
    pub placeholder: bool,
}

/// What a node does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// The first stage: where values come from.
    Source {
        expr: LeafExpr,
    },
    /// An ordinary stage.
    Stage {
        expr: LeafExpr,
    },
    Scatter,
    Collect,
    Ward {
        predicate: LeafExpr,
    },
    Fork {
        branches: Vec<SubgraphId>,
    },
    Orbit {
        body: SubgraphId,
        #[serde(skip_serializing_if = "Option::is_none")]
        identity: Option<LeafExpr>,
        max_items: u64,
    },
}

impl NodeKind {
    pub fn name(&self) -> &'static str {
        match self {
            NodeKind::Source { .. } => "source",
            NodeKind::Stage { .. } => "stage",
            NodeKind::Scatter => "scatter",
            NodeKind::Collect => "collect",
            NodeKind::Ward { .. } => "ward",
            NodeKind::Fork { .. } => "fork",
            NodeKind::Orbit { .. } => "orbit",
        }
    }

    /// How many output ports this node has.
    ///
    /// Port 0 is always the continuation — the value leaving the node along the
    /// main flow. A fork adds one port per branch and an orbit one for its body,
    /// so an edge into a branch is distinguishable from the edge that carries the
    /// concatenated result onward.
    pub fn out_ports(&self) -> u32 {
        match self {
            NodeKind::Fork { branches } => 1 + branches.len() as u32,
            NodeKind::Orbit { .. } => 2,
            _ => 1,
        }
    }

    /// How many input ports. Port 0 is the incoming value; port 1, where it
    /// exists, is the join a branch or an orbit body returns to.
    pub fn in_ports(&self) -> u32 {
        match self {
            NodeKind::Fork { .. } | NodeKind::Orbit { .. } => 2,
            _ => 1,
        }
    }

    /// The leaf this node evaluates, if it has one.
    pub fn leaf(&self) -> Option<&LeafExpr> {
        match self {
            NodeKind::Source { expr } | NodeKind::Stage { expr } => Some(expr),
            NodeKind::Ward { predicate } => Some(predicate),
            _ => None,
        }
    }
}

/// Position and size a renderer may attach.
///
/// **Never semantic.** Nothing in lowering, validation or execution reads it,
/// and a graph with every hint stripped must behave identically. It is here so
/// that a Sigil editor has somewhere to put layout without inventing a sidecar
/// format that can drift from the graph it describes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutHint {
    pub x: f32,
    pub y: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(flatten)]
    pub kind: NodeKind,
    pub span: Span,
    /// Which subgraph this node belongs to; `None` for the top-level flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subgraph: Option<SubgraphId>,
    /// Host capabilities this node's leaf names, in source order, deduplicated.
    ///
    /// Empty for a node with no leaf and for a leaf that names none, and omitted
    /// from the JSON when empty — so the common case costs nothing. Pair it with
    /// `leaf().effectful` to tell "names a capability" from "performs an effect":
    /// a node can do the first without the second, and only the second is a `!`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CapabilityRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutHint>,
}

/// What an edge is for.
///
/// Recorded rather than inferred, because "is this cycle allowed?" is answered
/// by asking whether every back edge is a [`EdgeRole::OrbitFeedback`] — and
/// working that out from the shape of the graph is exactly the analysis the
/// label makes unnecessary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeRole {
    /// One stage to the next along a flow.
    Flow,
    /// A fork or orbit into its branch or body.
    Enter,
    /// A fork branch returning its emissions to the fork that opened it.
    Join,
    /// An orbit body returning candidates to the orbit's worklist. **The only
    /// cycle v0 permits.**
    OrbitFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: PortRef,
    pub to: PortRef,
    /// Order among edges leaving the same port — branch order, in a fork.
    pub ordinal: u32,
    pub role: EdgeRole,
}

/// A fork branch or an orbit body: a flow nested inside a node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subgraph {
    pub id: SubgraphId,
    /// The node that owns it.
    pub owner: NodeId,
    /// First and last node of the nested flow. `None` when the branch is empty,
    /// which is a parse error but still has to be representable — a graph that
    /// cannot hold a broken program cannot report on one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<NodeId>,
    /// Members in flow order.
    pub nodes: Vec<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    /// Bytes, so a consumer can tell whether a span it holds is in range.
    pub length: u32,
}

/// A whole Cant program as a graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CantProgram {
    /// Which schema this is. Constant, and present because a consumer that
    /// accepts more than one graph format needs to dispatch on something before
    /// it trusts `version` — a bare integer says nothing about whose integer it
    /// is. Sigil normalizes from several producers and records this as the
    /// source schema of what it built.
    #[serde(default = "default_schema")]
    pub schema: String,
    /// Schema version, so a stored graph can be recognised or rejected.
    pub version: String,
    pub language_version: String,
    /// What wrote this graph. Diagnostic metadata, not part of its meaning.
    #[serde(default)]
    pub producer: Producer,
    /// The first node of the top-level flow.
    pub entry: NodeId,
    /// The last node of the top-level flow — where program-boundary collection
    /// happens.
    pub exit: NodeId,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
    pub source: SourceInfo,
}

impl CantProgram {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        // A linear scan rather than an index: identifiers are dense and assigned
        // in order, so this is `nodes[id]` in practice, and keeping it a scan
        // means a *deserialized* graph with holes or duplicates is handled by
        // validation rather than by panicking here.
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn subgraph(&self, id: SubgraphId) -> Option<&Subgraph> {
        self.subgraphs.iter().find(|s| s.id == id)
    }

    /// Edges leaving a node, in ordinal order.
    pub fn edges_from(&self, id: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.from.node == id)
    }

    /// Edges arriving at a node.
    pub fn edges_to(&self, id: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.to.node == id)
    }

    /// Every capability the program names, deduplicated, in source order.
    ///
    /// Read off the per-node [`Node::capabilities`] rather than by re-scanning
    /// leaf text, so that this and a consumer walking the nodes cannot disagree.
    /// Still answered before anything is expanded or run, which is what
    /// `cant explain` and `cant graph` need it for.
    pub fn capabilities(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for node in &self.nodes {
            for capability in &node.capabilities {
                if !out.contains(&capability.name) {
                    out.push(capability.name.clone());
                }
            }
        }
        out
    }

    /// Every capability family the program touches, deduplicated, in source
    /// order — `["fs", "http"]`. What a renderer groups outer-boundary
    /// invocation marks by.
    pub fn capability_families(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for node in &self.nodes {
            for capability in &node.capabilities {
                if !out.contains(&capability.family) {
                    out.push(capability.family.clone());
                }
            }
        }
        out
    }

    /// Nodes that perform an effect, in graph order.
    pub fn effectful_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|n| n.kind.leaf().is_some_and(|l| l.effectful))
            .map(|n| n.id)
            .collect()
    }

    /// The largest number of candidates any orbit will accept, if there are any.
    pub fn max_orbit_items(&self) -> Option<u64> {
        self.nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Orbit { max_items, .. } => Some(*max_items),
                _ => None,
            })
            .max()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn new_empty(source: SourceInfo) -> Self {
        Self {
            schema: GRAPH_SCHEMA_NAME.to_string(),
            producer: Producer::default(),
            version: GRAPH_SCHEMA_VERSION.to_string(),
            language_version: crate::CANT_LANGUAGE_VERSION.to_string(),
            entry: NodeId(0),
            exit: NodeId(0),
            nodes: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
            source,
        }
    }
}

/// The capabilities one leaf names, deduplicated, in source order.
///
/// Called once per node during lowering. Everything downstream reads the stored
/// [`Node::capabilities`] instead of re-running this, which is the whole point:
/// one scan, one answer, and no consumer deciding for itself what a leaf means.
pub(crate) fn capability_refs(text: &str) -> Vec<CapabilityRef> {
    let mut out: Vec<CapabilityRef> = Vec::new();
    for name in capabilities_in(text) {
        let capability = CapabilityRef::new(name);
        if !out.contains(&capability) {
            out.push(capability);
        }
    }
    out
}

/// `@fs.read` out of `! @fs.read(path)`.
///
/// Textual, and only over leaf text the Cant lexer already separated from
/// strings and comments, so a `"@fs.read"` inside a string is not reported.
fn capabilities_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_string => i += 1,
            b'"' => in_string = !in_string,
            b'@' if !in_string => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
                {
                    i += 1;
                }
                let name = text[start..i].trim_end_matches('.');
                if name.len() > 1 {
                    out.push(name.to_string());
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The default `:max` an orbit gets when none is written.
pub const fn default_orbit_max() -> u64 {
    DEFAULT_ORBIT_MAX
}

/// A port on a node, for building edges.
pub fn port(node: NodeId, kind: PortKind, index: u32) -> PortRef {
    PortRef { node, kind, index }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_counts_reflect_what_a_node_can_be_wired_to() {
        assert_eq!(NodeKind::Scatter.out_ports(), 1);
        assert_eq!(NodeKind::Scatter.in_ports(), 1);
        let fork = NodeKind::Fork {
            branches: vec![SubgraphId(0), SubgraphId(1), SubgraphId(2)],
        };
        // One continuation plus one per branch.
        assert_eq!(fork.out_ports(), 4);
        assert_eq!(fork.in_ports(), 2, "value in, and the branches' join");
        let orbit = NodeKind::Orbit {
            body: SubgraphId(0),
            identity: None,
            max_items: 8,
        };
        assert_eq!(orbit.out_ports(), 2);
        assert_eq!(orbit.in_ports(), 2);
    }

    #[test]
    fn capabilities_are_read_out_of_leaf_text_but_not_out_of_strings() {
        assert_eq!(capabilities_in("!@fs.read"), vec!["@fs.read"]);
        assert_eq!(
            capabilities_in("@json.decode(@fs.read(p))"),
            vec!["@json.decode", "@fs.read"]
        );
        assert!(
            capabilities_in(r#"replace($, "@fs.read", "x")"#).is_empty(),
            "a capability inside a string is text"
        );
        assert!(capabilities_in("a + b").is_empty());
    }

    #[test]
    fn subgraph_ids_render_readably() {
        assert_eq!(SubgraphId(3).to_string(), "s3");
        assert_eq!(NodeId(3).to_string(), "n3");
    }
}
