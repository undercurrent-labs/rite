//! AST → [`CantProgram`].
//!
//! A depth-first walk in source order, which keeps identifiers stable:
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
        definitions: ast
            .defs
            .iter()
            .map(|d| (d.name.as_str(), &d.flow))
            .collect(),
        splicing: Vec::new(),
    };

    let flow = builder.flow(&ast.flow, None, true);

    let source = SourceInfo {
        name: source_name.to_string(),
        length: source_len as u32,
    };
    let mut program = CantProgram::new_empty(source);
    program.version = GRAPH_SCHEMA_VERSION.to_string();
    program.uses = ast.uses.iter().map(|u| u.name.clone()).collect();
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

struct Builder<'a> {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    subgraphs: Vec<Subgraph>,
    next_node: u32,
    next_subgraph: u32,
    /// The program's named flows, in source order. First declaration wins; a
    /// duplicate name is validation's to report.
    definitions: Vec<(&'a str, &'a AstFlow)>,
    /// The definitions currently being spliced, innermost last. A name already
    /// on it is a cycle, and lowering leaves it as an ordinary leaf so that this
    /// terminates and `CANT-G020` has a node to point at.
    splicing: Vec<&'a str>,
}

impl<'a> Builder<'a> {
    fn add_node(
        &mut self,
        kind: NodeKind,
        span: rite_core::Span,
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        // Scanned once, here, rather than by every consumer that wants to know
        // which host family a node touches. See `CapabilityRef`.
        let capabilities = kind
            .leaf()
            .map(|leaf| crate::graph::capability_refs(&leaf.text))
            .unwrap_or_default();
        self.nodes.push(Node {
            id,
            kind,
            span,
            subgraph,
            capabilities,
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
    fn flow(
        &mut self,
        flow: &'a AstFlow,
        subgraph: Option<SubgraphId>,
        is_top_level: bool,
    ) -> Wired {
        let mut members = Vec::new();
        let mut previous: Option<NodeId> = None;
        self.stages(flow, subgraph, is_top_level, &mut members, &mut previous);
        Wired {
            entry: members.first().copied(),
            exit: previous,
            members,
        }
    }

    /// Append a flow's stages to the chain being built, splicing definitions.
    ///
    /// A stage that is nothing but a definition's name contributes that
    /// definition's stages instead of a node of its own, at the position it was
    /// named — so a definition is a chain, and the emissions rule at each of its
    /// stages is the ordinary one. Nothing records that a splice happened: the
    /// graph is the program that runs (ADR 0011).
    fn stages(
        &mut self,
        flow: &'a AstFlow,
        subgraph: Option<SubgraphId>,
        is_top_level: bool,
        members: &mut Vec<NodeId>,
        previous: &mut Option<NodeId>,
    ) {
        for stage in &flow.stages {
            if let Some(body) = self.definition_body(stage) {
                let name = definition_use(stage).expect("a body was found for it");
                self.splicing.push(name);
                self.stages(body, subgraph, is_top_level, members, previous);
                self.splicing.pop();
                continue;
            }
            // The first stage of the top-level flow is the source, wherever it
            // came from: a definition spliced in at the front supplies it.
            let id = self.stage(stage, subgraph, is_top_level && members.is_empty());
            members.push(id);
            if let Some(prev) = *previous {
                self.connect(prev, id, EdgeRole::Flow);
            }
            *previous = Some(id);
        }
    }

    /// The flow this stage names, when it names one that is not already being
    /// spliced.
    fn definition_body(&self, stage: &'a Stage) -> Option<&'a AstFlow> {
        let name = definition_use(stage)?;
        if self.splicing.contains(&name) {
            return None;
        }
        self.definitions
            .iter()
            .find(|(defined, _)| *defined == name)
            .map(|(_, flow)| *flow)
    }

    fn stage(&mut self, stage: &'a Stage, subgraph: Option<SubgraphId>, is_source: bool) -> NodeId {
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
            StageKind::Rescue { handler } => self.rescue(stage, handler, subgraph),
        }
    }

    fn fork(
        &mut self,
        stage: &'a Stage,
        branches: &'a [AstFlow],
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        // The node is created before its branches so that its identifier is
        // lower than theirs — a reader of the JSON meets the fork before the
        // things inside it.
        let ids: Vec<SubgraphId> = (0..branches.len())
            .map(|i| SubgraphId(self.next_subgraph + i as u32))
            .collect();
        self.next_subgraph += branches.len() as u32;

        // `:par` is present or it is not; a value on it, or the name on anything
        // other than a fork, is validation's to report from the AST — the graph
        // keeps only the answer.
        let parallel = modifier(&stage.modifiers, "par").is_some();

        let node = self.add_node(
            NodeKind::Fork {
                branches: ids.clone(),
                parallel,
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

    fn orbit(
        &mut self,
        stage: &'a Stage,
        body: &'a AstFlow,
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        let id = SubgraphId(self.next_subgraph);
        self.next_subgraph += 1;

        let identity = modifier(&stage.modifiers, "by")
            .and_then(|m| m.value.as_ref())
            .map(leaf_expr);
        // An unparseable `:max` keeps the default here and is reported by
        // validation, which can point at the modifier's span and say what was
        // written. Failing in the builder would mean no graph to point at.
        let max_items = modifier(&stage.modifiers, "max")
            .and_then(|m| m.value.as_ref())
            .and_then(|v| v.text.trim().parse::<u64>().ok())
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

    /// A rescue: out port 0 continues, out port 1 is the failure path into the
    /// handler, and the handler rejoins at in port 1.
    fn rescue(
        &mut self,
        stage: &'a Stage,
        handler: &'a AstFlow,
        subgraph: Option<SubgraphId>,
    ) -> NodeId {
        let id = SubgraphId(self.next_subgraph);
        self.next_subgraph += 1;

        let node = self.add_node(NodeKind::Rescue { handler: id }, stage.span, subgraph);

        let wired = self.flow(handler, Some(id), false);
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
                role: EdgeRole::Rescue,
            });
        }
        if let Some(exit) = wired.exit {
            self.edges.push(Edge {
                from: port(exit, PortKind::Out, 0),
                to: port(node, PortKind::In, 1),
                ordinal: 0,
                role: EdgeRole::Join,
            });
        }
        node
    }
}

/// The name a stage would be a use of, which is a stage that is nothing but a
/// leaf.
///
/// Whether the name *is* defined is the caller's question. A stage carrying
/// anything else — `clean($)`, `upper -> clean` — is leaf text, so a definition
/// is never callable and never part of an expression.
pub(crate) fn definition_use(stage: &Stage) -> Option<&str> {
    let StageKind::Leaf(leaf) = &stage.kind else {
        return None;
    };
    let text = leaf.text.trim();
    (!text.is_empty()).then_some(text)
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
        let NodeKind::Fork { branches, .. } = &fork.kind else {
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

    /// `:par` is the whole difference in the graph. Everything else about a
    /// parallel fork — branch count, ordinals, ports — is a sequential fork's.
    #[test]
    fn a_par_modifier_becomes_the_forks_parallel_flag() {
        for (source, expected) in [("x -> |{ a ; b }:par", true), ("x -> |{ a ; b }", false)] {
            let g = graph_of(source);
            let fork = g
                .nodes
                .iter()
                .find_map(|n| match n.kind {
                    NodeKind::Fork { parallel, .. } => Some(parallel),
                    _ => None,
                })
                .expect("a fork");
            assert_eq!(fork, expected, "for {source:?}");
        }

        let par = graph_of("x -> |{ a ; b }:par");
        let seq = graph_of("x -> |{ a ; b }");
        assert_eq!(par.edges.len(), seq.edges.len());
        assert_eq!(par.subgraphs.len(), seq.subgraphs.len());
    }

    /// The name is what selects it, not the value the parser did or did not
    /// record — `:par true` is `CANT-G023` and still a parallel fork.
    #[test]
    fn par_on_anything_but_a_fork_leaves_no_trace_in_the_graph() {
        let g = graph_of("roots -> ~{ deps }:par");
        assert_eq!(g.max_orbit_items(), Some(DEFAULT_ORBIT_MAX));
        assert!(!g
            .nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Fork { .. })));
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

    /// The failure path is an edge of its own, and the handler comes back the
    /// way a fork branch does.
    #[test]
    fn a_rescue_wires_its_handler_on_the_second_out_port() {
        let g = graph_of("x -> !{ $.message }");
        let rescue = g
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Rescue { .. }))
            .expect("rescue");

        let failure: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.role == EdgeRole::Rescue)
            .collect();
        assert_eq!(failure.len(), 1);
        assert_eq!(failure[0].from.node, rescue.id);
        assert_eq!(failure[0].from.index, 1, "port 0 is the continuation");

        let joins: Vec<_> = g
            .edges
            .iter()
            .filter(|e| e.role == EdgeRole::Join)
            .collect();
        assert_eq!(joins.len(), 1);
        assert_eq!(joins[0].to.node, rescue.id);
        assert_eq!(joins[0].to.index, 1);
        // A rescue is not a cycle, whatever a naive edge walk makes of it.
        assert!(g.edges.iter().all(|e| e.role != EdgeRole::OrbitFeedback));
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

    // ---- definitions

    #[test]
    fn a_definition_is_spliced_where_it_is_named() {
        let g = graph_of("double:{ $ * 2 }\n[1] -> * -> double -> []");
        // Source, scatter, the definition's one stage, collect: no node stands
        // for the definition itself.
        assert_eq!(g.nodes.len(), 4);
        assert!(matches!(g.nodes[2].kind, NodeKind::Stage { .. }));
        assert_eq!(
            g.nodes[2].kind.leaf().expect("a leaf").text,
            "$ * 2",
            "the definition's stage, not its name"
        );
    }

    /// Two uses are two nodes, which is what makes effect-ness per splice rather
    /// than memoized: each carries its own leaf and is analyzed on its own.
    #[test]
    fn a_definition_used_twice_becomes_two_nodes() {
        let g = graph_of("read:{ !@fs.read($) }\n[\"a\"] -> * -> read -> read -> []");
        let reads: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.kind.leaf().is_some_and(|l| l.effectful))
            .collect();
        assert_eq!(reads.len(), 2);
        assert_ne!(reads[0].id, reads[1].id);
        assert_eq!(g.effectful_nodes().len(), 2);
    }

    #[test]
    fn a_definition_splices_into_the_subgraph_that_used_it() {
        let g = graph_of("clean:{ trim -> upper }\n[\"a\"] -> |{ clean ; lower }");
        let branch: Vec<_> = g.nodes.iter().filter(|n| n.subgraph.is_some()).collect();
        // Two spliced stages in the first branch, one in the second.
        assert_eq!(branch.len(), 3);
        assert_eq!(branch[0].kind.leaf().expect("leaf").text, "trim");
        assert_eq!(branch[0].subgraph, branch[1].subgraph);
        assert_ne!(branch[0].subgraph, branch[2].subgraph);
    }

    /// A definition spliced at the front supplies the program's source, because
    /// the first stage of the top-level flow is the source wherever it came from.
    #[test]
    fn a_definition_used_first_supplies_the_source() {
        let g = graph_of("start:{ [1, 2] -> * }\nstart -> []");
        assert!(matches!(g.nodes[0].kind, NodeKind::Source { .. }));
        assert_eq!(g.nodes[0].kind.leaf().expect("leaf").text, "[1, 2]");
        assert_eq!(g.entry, NodeId(0));
    }

    /// Lowering rejects nothing, so it has to survive a program `CANT-G020`
    /// refuses: a name already being spliced is left as an ordinary leaf, which
    /// terminates and leaves a node for the diagnostic to point at.
    #[test]
    fn a_recursive_definition_terminates_as_a_leaf() {
        let g = graph_of("a:{ trim -> a }\n[\"x\"] -> a");
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.nodes[2].kind.leaf().expect("leaf").text, "a");
    }

    #[test]
    fn an_unused_definition_contributes_no_nodes() {
        let g = graph_of("spare:{ trim }\n[1] -> upper");
        assert_eq!(g.nodes.len(), 2);
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
