//! Cant's semantic layer: the flow graph, its validation, its lowering to
//! canonical Rite, and the source maps that tie the three together.
//!
//! # Status
//!
//! Phase 3: the graph, its lowering from the AST, its validation, and JSON and
//! DOT export are here. Lowering to Rite lands in Phase 4 — see
//! `docs/cant/checklist.md`. The dependency direction is fixed and enforced:
//! `cant-sem` may depend on `cant-syntax`, `rite-core`, `rite-syntax`,
//! `rite-sem` and `rite-fmt`, and nothing in Rite may depend on it.
//!
//! # What the graph is for
//!
//! It is not a visualization artifact. It is the normalized semantic
//! representation of a Cant program: nodes, directed edges, ports, ordered
//! branches, source spans, effectful-leaf metadata, orbit policies, collection
//! boundaries, and stable identifiers. Lowering reads the graph, not the AST, so
//! that a future Sigil tool consuming the graph is consuming exactly what
//! executes rather than a picture drawn beside it.
//!
//! Layout metadata is reserved but **non-semantic**: a renderer may attach
//! positions, and removing every one of them must not change what a program
//! does. That is what keeps geometry out of the language.

pub mod dot;
pub mod expand;
pub mod explain;
pub mod graph;
pub mod lower;
pub mod sigil;
pub mod validate;

pub use dot::to_dot;
pub use expand::{expand, remap_diagnostic, ExpandOptions, Expansion, Mapping, SourceMap};
pub use explain::{explain, Explanation, Step};
pub use graph::{
    CantProgram, CapabilityRef, Edge, EdgeRole, LayoutHint, LeafExpr, Node, NodeKind, Producer,
    SourceInfo, Subgraph, SubgraphId,
};
pub use lower::lower;
pub use sigil::{to_sigil_graph, AdaptOptions};
pub use validate::{analyze, validate, validate_deserialized, validate_modifiers, Analysis};

use serde::{Deserialize, Serialize};

/// The name of the serialized Cant graph schema.
///
/// Constant, and separate from the version because a consumer that reads more
/// than one graph format has to know *which* format before a version number
/// means anything. Sigil records it as the source schema of the normalized graph
/// it builds.
pub const GRAPH_SCHEMA_NAME: &str = "cant.graph";

/// The version of the serialized Cant graph schema.
///
/// Bumped when the JSON shape changes in a way a consumer would notice.
/// Independent of both the crate version and the language version: a tooling
/// release that does not touch the graph must not invalidate a stored one.
///
/// **1** — added `schema`, `producer`, and per-node `capabilities`, so a
/// renderer can tell which host family a node touches without pattern-matching
/// its leaf text. See `docs/cant/graph-schema.md`.
pub const GRAPH_SCHEMA_VERSION: &str = "1";

/// Re-exported so a consumer needs one crate to know which language and which
/// graph shape it is looking at.
pub use cant_syntax::CANT_LANGUAGE_VERSION;

/// A stable identifier for a node within one parse-and-lower operation.
///
/// "Stable" means deterministic for a given source and tool version, not
/// globally unique: two runs over the same `.cant` file must produce the same
/// IDs so that graph JSON is snapshot-testable and a diff between two versions
/// of a program is readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "n{}", self.0)
    }
}

/// Which side of a node an edge attaches to.
///
/// Cant v0 nodes have one input and one output, but the graph records the port
/// anyway: fork branches, error routing and multi-output nodes are all in the
/// deferred design space, and an edge model that assumes a single anonymous
/// port would have to be re-serialized to admit any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    In,
    Out,
}

/// One end of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    pub kind: PortKind,
    /// Which port of that kind, from zero. Ordered, so fork branch order is
    /// carried by the graph rather than by the order edges happen to be listed.
    pub index: u32,
}

/// The default `:max` for an orbit when none is written.
///
/// Conservative on purpose: an orbit that hits this has a bug or needs to say
/// out loud that it is large, and either way a bounded failure beats an
/// unbounded traversal. Rite's global step and time budgets still apply on top.
pub const DEFAULT_ORBIT_MAX: u64 = 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_render_readably_for_dot_output() {
        assert_eq!(NodeId(0).to_string(), "n0");
        assert_eq!(NodeId(42).to_string(), "n42");
    }

    #[test]
    fn the_graph_schema_and_the_language_are_versioned_separately() {
        assert_eq!(GRAPH_SCHEMA_VERSION, "1");
        assert_eq!(CANT_LANGUAGE_VERSION, "0");
    }
}
