//! AST → [`CantProgram`].
//!
//! A depth-first walk in source order, which is what makes identifiers stable:
//! the same source and tool version always assign the same numbers, so graph
//! JSON is snapshot-testable and a diff between two versions of a program reads
//! like a diff of the program.
//!
//! Lowering is total. It does not reject anything — a `:max` of `"eight"` and a
//! ward that reads a file both produce a graph, and [`crate::validate`] is where
//! they are refused. Two reasons: the graph is what a diagnostic points *at*, so
//! it has to exist before one can be written; and `cant graph` on a broken
//! program is more useful than a refusal, because seeing the shape is usually how
//! you work out what went wrong.

use cant_syntax::{CantProgramAst, Flow as AstFlow, Leaf as AstLeaf, Modifier, Stage, StageKind};

use crate::graph::{
    port, CantProgram, Edge, EdgeRole, LeafExpr, Node, NodeKind, SourceInfo, Subgraph, SubgraphId,
};
use crate::{NodeId, PortKind, DEFAULT_ORBIT_MAX, GRAPH_SCHEMA_VERSION};

/// Lower a parsed program into its graph.
pub fn lower(ast: &CantProgramAst, source_name: &str, source_len: usize) -> CantProgram {
    let mut builder = Builder {
        nodes: Vec::new(),
        edges: Vec::new(),
        subgraphs: Vec::new(),
        next_node: 0,
        next_subgraph: 0,
    };

    let flow = builder.flow(&ast.flow, None, true);

    let source = SourceInfo {
        name: source_name.to_string(),
        length: source_len as u32,
    };
    let mut program = CantProgram::new_empty(source);
    program.version = GRAPH_SCHEMA_VERSION.to_string();
    program.entry = flow.entry.unwrap_or(NodeId(0));
    program.exit = flow.exit.unwrap_or(program.entry);
    program.nodes = builder.nodes;
    program.edges = builder.edges;
    program.subgraphs = builder.subgraphs;
    program
}

/// Where a lowered flow begins and ends, for wiring it to whatever contains it.
struct Wired {
    entry: Option<NodeId>,
    exit: Option<NodeId>,
    members: Vec<NodeId>,
}

struct Builder {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    subgraphs: Vec<Subgraph>,
    next_node: u32,
    next_subgraph: u32,
}

impl Builder {
    fn add_node(
        &mut self,
        kind: NodeKind,
        span: rite_core::Span,
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        self.nodes.push(Node {
            id,
            kind,
            span,
            subgraph,
            label: None,
            layout: None,
        });
        id
    }

    fn connect(&mut self, from: NodeId, to: NodeId, role: EdgeRole) {
        self.edges.push(Edge {
            from: port(from, PortKind::Out, 0),
            to: port(to, PortKind::In, 0),
            ordinal: 0,
            role,
        });
    }

    /// Lower a flow, returning its ends.
    ///
    /// `is_top_level` decides whether the first stage becomes a `Source` node.
    /// Inside a fork branch or an orbit body the first stage receives a value
    /// from the enclosing node, so it is an ordinary `Stage` — calling it a
    /// source would say the branch invents its input, which is the opposite of
    /// what a fork does.
    fn flow(&mut self, flow: &AstFlow, subgraph: Option<SubgraphId>, is_top_level: bool) -> Wired {
        let mut members = Vec::new();
        let mut previous: Option<NodeId> = None;

        for (index, stage) in flow.stages.iter().enumerate() {
            let id = self.stage(stage, subgraph, is_top_level && index == 0);
            members.push(id);
            if let Some(prev) = previous {
                self.connect(prev, id, EdgeRole::Flow);
            }
            previous = Some(id);
        }

        Wired {
            entry: members.first().copied(),
            exit: previous,
            members,
        }
    }

    fn stage(&mut self, stage: &Stage, subgraph: Option<SubgraphId>, is_source: bool) -> NodeId {
        match &stage.kind {
            StageKind::Leaf(leaf) => {
                let expr = leaf_expr(leaf);
                let kind = if is_source {
                    NodeKind::Source { expr }
                } else {
                    NodeKind::Stage { expr }
                };
                self.add_node(kind, stage.span, subgraph)
            }
            StageKind::Scatter => self.add_node(NodeKind::Scatter, stage.span, subgraph),
            StageKind::Collect => self.add_node(NodeKind::Collect, stage.span, subgraph),
            StageKind::Ward { predicate } => self.add_node(
                NodeKind::Ward {
                    predicate: leaf_expr(predicate),
                },
                stage.span,
                subgraph,
            ),
            StageKind::Fork { branches } => self.fork(stage, branches, subgraph),
            StageKind::Orbit { body } => self.orbit(stage, body, subgraph),
        }
    }

    fn fork(
        &mut self,
        stage: &Stage,
        branches: &[AstFlow],
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        // The node is created before its branches so that its identifier is
        // lower than theirs — a reader of the JSON meets the fork before the
        // things inside it.
        let ids: Vec<SubgraphId> = (0..branches.len())
            .map(|i| SubgraphId(self.next_subgraph + i as u32))
            .collect();
        self.next_subgraph += branches.len() as u32;

        let node = self.add_node(
            NodeKind::Fork {
                branches: ids.clone(),
            },
            stage.span,
            subgraph,
        );

        for (index, (branch, id)) in branches.iter().zip(ids).enumerate() {
            let wired = self.flow(branch, Some(id), false);
            self.subgraphs.push(Subgraph {
                id,
                owner: node,
                entry: wired.entry,
                exit: wired.exit,
                nodes: wired.members,
            });
            // Branch order is carried by the edge ordinal, not by the order the
            // edges happen to appear in the list — a consumer that sorts the
            // edges must still get the branches right.
            if let Some(entry) = wired.entry {
                self.edges.push(Edge {
                    from: port(node, PortKind::Out, index as u32 + 1),
                    to: port(entry, PortKind::In, 0),
                    ordinal: index as u32,
                    role: EdgeRole::Enter,
                });
            }
            if let Some(exit) = wired.exit {
                self.edges.push(Edge {
                    from: port(exit, PortKind::Out, 0),
                    to: port(node, PortKind::In, 1),
                    ordinal: index as u32,
                    role: EdgeRole::Join,
                });
            }
        }
        node
    }

    fn orbit(&mut self, stage: &Stage, body: &AstFlow, subgraph: Option<SubgraphId>) -> NodeId {
        let id = SubgraphId(self.next_subgraph);
        self.next_subgraph += 1;

        let identity = modifier(&stage.modifiers, "by").map(|m| leaf_expr(&m.value));
        // An unparseable `:max` keeps the default here and is reported by
        // validation, which can point at the modifier's span and say what was
        // written. Failing in the builder would mean no graph to point at.
        let max_items = modifier(&stage.modifiers, "max")
            .and_then(|m| m.value.text.trim().parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_ORBIT_MAX);

        let node = self.add_node(
            NodeKind::Orbit {
                body: id,
                identity,
                max_items,
            },
            stage.span,
            subgraph,
        );

        let wired = self.flow(body, Some(id), false);
        self.subgraphs.push(Subgraph {
            id,
            owner: node,
            entry: wired.entry,
            exit: wired.exit,
            nodes: wired.members,
        });

        if let Some(entry) = wired.entry {
            self.edges.push(Edge {
                from: port(node, PortKind::Out, 1),
                to: port(entry, PortKind::In, 0),
                ordinal: 0,
                role: EdgeRole::Enter,
            });
        }
        if let Some(exit) = wired.exit {
            // The one cycle v0 permits, labelled as such so validation can tell
            // it from a cycle nobody asked for.
            self.edges.push(Edge {
                from: port(exit, PortKind::Out, 0),
                to: port(node, PortKind::In, 1),
                ordinal: 0,
                role: EdgeRole::OrbitFeedback,
            });
        }
        node
    }
}

fn leaf_expr(leaf: &AstLeaf) -> LeafExpr {
    LeafExpr {
        text: leaf.text.clone(),
        span: leaf.span,
        effectful: leaf.has_effect_marker,
        placeholder: leaf.has_placeholder,
    }
}

/// The last modifier with this name.
///
/// Last rather than first so that a repeated `:max` behaves like a later
/// assignment; validation reports the duplicate separately, so neither reading
/// is silently accepted.
pub(crate) fn modifier<'a>(modifiers: &'a [Modifier], name: &str) -> Option<&'a Modifier> {
    modifiers.iter().rev().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cant_syntax::parse_source;

    fn graph_of(source: &str) -> CantProgram {
        let (result, _) = parse_source("t.cant", source);
        assert!(!result.has_errors(), "{source:?} should parse");
        lower(&result.program.expect("program"), "t.cant", source.len())
    }

    #[test]
    fn a_flow_becomes_a_chain() {
        let g = graph_of("a -> b -> c");
        assert_eq!(g.nodes.len(), 3);
        assert!(matches!(g.nodes[0].kind, NodeKind::Source { .. }));
        assert!(matches!(g.nodes[1].kind, NodeKind::Stage { .. }));
        assert_eq!(g.entry, NodeId(0));
        assert_eq!(g.exit, NodeId(2));
        assert_eq!(g.edges.len(), 2);
        assert!(g.edges.iter().all(|e| e.role == EdgeRole::Flow));
    }

    #[test]
    fn identifiers_are_stable_across_runs() {
        let source = "roots -> * -> |{ a ; b -> c } -> ~{ d -> * } :max 8 -> []";
        assert_eq!(graph_of(source).to_json(), graph_of(source).to_json());
    }

    #[test]
    fn a_fork_wires_each_branch_in_and_back() {
        let g = graph_of("x -> |{ a ; b }");
        let fork = g
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Fork { .. }))
            .expect("fork");
        let NodeKind::Fork { branches } = &fork.kind else {
            unreachable!()
        };
        assert_eq!(branches.len(), 2);

        let enters: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.role == EdgeRole::Enter)
            .collect();
        assert_eq!(enters.len(), 2);
        // Branch order lives in the ordinal, and in the out-port index.
        assert_eq!(enters[0].ordinal, 0);
        assert_eq!(enters[0].from.index, 1);
        assert_eq!(enters[1].ordinal, 1);
        assert_eq!(enters[1].from.index, 2);

        let joins: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.role == EdgeRole::Join)
            .collect();
        assert_eq!(joins.len(), 2);
        assert!(joins
            .iter()
            .all(|e| e.to.node == fork.id && e.to.index == 1));
    }

    #[test]
    fn a_branchs_first_stage_is_not_a_source() {
        // It receives the fork's input; calling it a source would say the branch
        // invents its own.
        let g = graph_of("x -> |{ a ; b }");
        let sources = g
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Source { .. }))
            .count();
        assert_eq!(sources, 1, "only the program's first stage is a source");
    }

    #[test]
    fn an_orbit_has_a_feedback_edge_and_that_is_the_only_cycle() {
        let g = graph_of("roots -> ~{ deps -> * }");
        let feedback: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.role == EdgeRole::OrbitFeedback)
            .collect();
        assert_eq!(feedback.len(), 1);
        let orbit = g
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Orbit { .. }))
            .expect("orbit");
        assert_eq!(feedback[0].to.node, orbit.id);
        assert_eq!(feedback[0].to.index, 1, "feedback arrives at the join port");
    }

    #[test]
    fn orbit_modifiers_become_policy() {
        let g = graph_of("roots -> ~{ deps } :by canonical :max 4096");
        let orbit = g
            .nodes
            .iter()
            .find_map(|n| match &n.kind {
                NodeKind::Orbit {
                    identity,
                    max_items,
                    ..
                } => Some((identity.clone(), *max_items)),
                _ => None,
            })
            .expect("orbit");
        assert_eq!(orbit.0.expect("identity").text, "canonical");
        assert_eq!(orbit.1, 4096);
    }

    #[test]
    fn an_orbit_without_a_limit_gets_the_conservative_default() {
        let g = graph_of("roots -> ~{ deps }");
        assert_eq!(g.max_orbit_items(), Some(DEFAULT_ORBIT_MAX));
    }

    #[test]
    fn an_unusable_limit_keeps_the_default_for_validation_to_report() {
        for source in [
            "r -> ~{ d } :max 0",
            "r -> ~{ d } :max eight",
            "r -> ~{ d } :max -3",
        ] {
            let g = graph_of(source);
            assert_eq!(
                g.max_orbit_items(),
                Some(DEFAULT_ORBIT_MAX),
                "for {source:?}"
            );
        }
    }

    #[test]
    fn effectful_leaves_and_capabilities_are_visible_without_running_anything() {
        let g = graph_of("\"p\" -> !@fs.read -> @json.decode");
        assert_eq!(g.effectful_nodes().len(), 1);
        assert_eq!(g.capabilities(), vec!["@fs.read", "@json.decode"]);
    }

    #[test]
    fn nesting_puts_nodes_in_their_subgraph() {
        let g = graph_of("x -> |{ a ; b }");
        let inner: Vec<_> = g.nodes.iter().filter(|n| n.subgraph.is_some()).collect();
        assert_eq!(inner.len(), 2);
        for node in inner {
            let sub = g
                .subgraph(node.subgraph.expect("subgraph"))
                .expect("present");
            assert!(sub.nodes.contains(&node.id));
        }
    }
}
