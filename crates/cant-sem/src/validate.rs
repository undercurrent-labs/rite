//! Graph validation.
//!
//! Everything here is checkable **without executing anything**, which is the
//! point: `cant check` should reject a program that cannot work before a
//! capability is granted or a byte is read.
//!
//! Two audiences, one function. A graph that came from [`crate::lower`] cannot
//! have a dangling edge or a duplicate identifier — the builder assigns them —
//! but a graph that came from *JSON* can, and the specification requires that a
//! fuzzed graph cannot smuggle in an unvalidated cycle. So the structural checks
//! run on both rather than being skipped as impossible; on a freshly lowered
//! graph they cost one pass and find nothing, which is the correct price for not
//! having to trust the input.

use cant_syntax::diagnostic::*;
use rite_core::{Severity, SourceSpan};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph::{CantProgram, EdgeRole, NodeKind, SubgraphId};
use crate::{NodeId, PortKind};

/// Modifiers each structural form accepts.
///
/// The parser takes `:name value` after any block and asks no questions, so this
/// is where an unknown one is caught — with a better message than a parser could
/// give, because here we know whether it was attached to a ward or an orbit.
const ORBIT_MODIFIERS: &[&str] = &["by", "max"];

pub fn validate(program: &CantProgram, file: rite_core::FileId) -> CantDiagnostics {
    let mut v = Validator {
        program,
        file,
        diagnostics: CantDiagnostics::new(),
    };
    v.structure();
    v.placement();
    v.orbits();
    v.wards();
    v.cycles();
    v.reachability();
    v.diagnostics
}

struct Validator<'a> {
    program: &'a CantProgram,
    file: rite_core::FileId,
    diagnostics: CantDiagnostics,
}

impl Validator<'_> {
    fn span(&self, span: rite_core::Span) -> SourceSpan {
        SourceSpan::new(self.file, span)
    }

    fn error(
        &mut self,
        code: CantCode,
        title: impl Into<String>,
        span: rite_core::Span,
        label: &str,
    ) {
        let at = self.span(span);
        self.diagnostics
            .push(CantDiagnostic::error(code, title).with_primary(at, label));
    }

    fn node_span(&self, id: NodeId) -> rite_core::Span {
        self.program
            .node(id)
            .map(|n| n.span)
            .unwrap_or(rite_core::Span::DUMMY)
    }

    // ---- structure: true of any graph, however it arrived

    fn structure(&mut self) {
        if self.program.nodes.is_empty() {
            self.diagnostics.push(
                CantDiagnostic::error(CANT_G001_NO_ENTRY, "this graph has no nodes")
                    .with_primary(self.span(rite_core::Span::DUMMY), "nothing to run"),
            );
            return;
        }

        // Duplicate identifiers. Impossible from the builder; entirely possible
        // from JSON, and every lookup below would silently take the first match.
        let mut seen: HashSet<NodeId> = HashSet::new();
        let mut duplicates: Vec<NodeId> = Vec::new();
        for node in &self.program.nodes {
            if !seen.insert(node.id) && !duplicates.contains(&node.id) {
                duplicates.push(node.id);
            }
        }
        for id in duplicates {
            let span = self.node_span(id);
            self.error(
                CANT_G012_DUPLICATE_NODE_ID,
                format!("two nodes share the identifier `{id}`"),
                span,
                "identifiers must be unique within a graph",
            );
        }

        if self.program.node(self.program.entry).is_none() {
            self.diagnostics.push(
                CantDiagnostic::error(
                    CANT_G001_NO_ENTRY,
                    format!(
                        "the entry node `{}` is not in the graph",
                        self.program.entry
                    ),
                )
                .with_primary(self.span(rite_core::Span::DUMMY), "no entry"),
            );
        }

        let ports: HashMap<NodeId, (u32, u32)> = self
            .program
            .nodes
            .iter()
            .map(|n| (n.id, (n.kind.in_ports(), n.kind.out_ports())))
            .collect();

        for edge in &self.program.edges {
            for (end, expect_out) in [(edge.from, true), (edge.to, false)] {
                let Some((ins, outs)) = ports.get(&end.node) else {
                    let span = self.node_span(edge.from.node);
                    self.error(
                        CANT_G002_DANGLING_EDGE,
                        format!("an edge names `{}`, which is not in the graph", end.node),
                        span,
                        "every edge must join two nodes that exist",
                    );
                    continue;
                };
                let limit = match end.kind {
                    PortKind::Out => *outs,
                    PortKind::In => *ins,
                };
                let matches_direction = matches!(
                    (end.kind, expect_out),
                    (PortKind::Out, true) | (PortKind::In, false)
                );
                if !matches_direction {
                    let span = self.node_span(end.node);
                    self.error(
                        CANT_G003_INVALID_PORT,
                        "an edge leaves an input port or arrives at an output port",
                        span,
                        "edges run from an output to an input",
                    );
                    continue;
                }
                if end.index >= limit {
                    let span = self.node_span(end.node);
                    self.error(
                        CANT_G003_INVALID_PORT,
                        format!(
                            "`{}` has no {:?} port {} (it has {limit})",
                            end.node, end.kind, end.index
                        ),
                        span,
                        "port index out of range",
                    );
                }
            }
        }

        self.branch_joins();
    }

    /// Every fork branch must return to the fork that opened it, and every orbit
    /// body to its orbit. A branch that flows somewhere else is a graph that
    /// cannot be executed and cannot be drawn.
    fn branch_joins(&mut self) {
        for subgraph in &self.program.subgraphs {
            let owner = subgraph.owner;
            let Some(node) = self.program.node(owner) else {
                let span = self.node_span(owner);
                self.error(
                    CANT_G002_DANGLING_EDGE,
                    format!(
                        "subgraph `{}` is owned by `{owner}`, which is not in the graph",
                        subgraph.id
                    ),
                    span,
                    "an orphaned branch",
                );
                continue;
            };

            let Some(exit) = subgraph.exit else {
                let span = node.span;
                let what = match node.kind {
                    NodeKind::Fork { .. } => "fork branch",
                    _ => "orbit body",
                };
                self.error(
                    CANT_G015_EMPTY_SUBGRAPH,
                    format!("this {what} has no stages"),
                    span,
                    "an empty branch emits nothing and cannot be lowered",
                );
                continue;
            };

            let expected_role = match node.kind {
                NodeKind::Fork { .. } => EdgeRole::Join,
                _ => EdgeRole::OrbitFeedback,
            };
            let rejoins = self.program.edges.iter().any(|e| {
                e.from.node == exit
                    && e.to.node == owner
                    && e.to.index == 1
                    && e.role == expected_role
            });
            if !rejoins {
                let span = node.span;
                self.error(
                    CANT_G004_BRANCH_JOIN,
                    format!("subgraph `{}` does not rejoin `{owner}`", subgraph.id),
                    span,
                    "a branch's emissions have nowhere to go",
                );
            }
        }
    }

    // ---- placement

    /// Scatter and collect both consume emissions, so neither can be the first
    /// thing a program does — there are none yet.
    fn placement(&mut self) {
        let Some(entry) = self.program.node(self.program.entry) else {
            return;
        };
        match entry.kind {
            NodeKind::Scatter => self.error(
                CANT_G005_SCATTER_HAS_NO_INPUT,
                "a program cannot begin with scatter",
                entry.span,
                "there are no emissions to expand yet",
            ),
            NodeKind::Collect => {
                let span = entry.span;
                self.diagnostics.push(
                    CantDiagnostic::error(
                        CANT_G006_COLLECT_HAS_NO_INPUT,
                        "a program cannot begin with collect",
                    )
                    .with_primary(self.span(span), "there are no emissions to gather yet")
                    .with_help(
                        "if you meant the empty list, write it in ASCII as `[]` — \
                         the glyph `⌁` is always collect",
                    ),
                );
            }
            _ => {}
        }
    }

    // ---- orbits

    fn orbits(&mut self) {
        for node in &self.program.nodes {
            let NodeKind::Orbit {
                identity,
                max_items,
                ..
            } = &node.kind
            else {
                continue;
            };

            if *max_items == 0 {
                self.error(
                    CANT_G007_ORBIT_LIMIT,
                    "an orbit's `:max` must be a positive integer",
                    node.span,
                    "zero accepted candidates would make the orbit emit nothing",
                );
            }

            if let Some(identity) = identity {
                if identity.effectful {
                    let span = identity.span;
                    self.diagnostics.push(
                        CantDiagnostic::error(
                            CANT_G008_ORBIT_IDENTITY_EFFECTFUL,
                            "an orbit's `:by` function must be pure",
                        )
                        .with_primary(self.span(span), "this performs an effect")
                        .with_secondary(self.span(node.span), "for this orbit")
                        .with_note(
                            "identity is computed for every candidate, including ones already \
                             seen, so an effect here would run an unpredictable number of times",
                        )
                        .with_help("compute the value in the orbit body and deduplicate on it"),
                    );
                }
            }
        }
    }

    // ---- wards

    fn wards(&mut self) {
        for node in &self.program.nodes {
            let NodeKind::Ward { predicate } = &node.kind else {
                continue;
            };
            if !predicate.effectful {
                continue;
            }
            let span = predicate.span;
            self.diagnostics.push(
                CantDiagnostic::error(
                    CANT_G014_WARD_PREDICATE_EFFECTFUL,
                    "a ward predicate must be pure in v0",
                )
                .with_primary(self.span(span), "this performs an effect")
                .with_note(
                    "a filter that reads the world needs ordering and failure rules Cant does \
                     not have yet, and an RFC to fix them",
                )
                .with_help("do the effect in a stage before the ward, and test its result"),
            );
        }
    }

    // ---- cycles

    /// The one structural rule v0 has: **every cycle is orbit-owned.**
    ///
    /// Only [`EdgeRole::Flow`] and [`EdgeRole::Enter`] carry control forward, so
    /// only they can form a loop that runs twice. The other two are excluded for
    /// different reasons:
    ///
    /// * [`EdgeRole::OrbitFeedback`] is the sanctioned loop — the whole point of
    ///   an orbit, and bounded by its `:max`.
    /// * [`EdgeRole::Join`] returns a branch's emissions to the fork that opened
    ///   it. It always pairs with an `Enter`, so counting it made **every fork**
    ///   look like an illegal cycle. A branch runs once; the join is a
    ///   concatenation point, not a re-entry.
    ///
    /// Nothing lowering produces can build a cycle in the remaining subgraph,
    /// which is exactly why this has to run on *deserialized* graphs: JSON is the
    /// only way one gets in, and admitting it silently would give Cant unbounded
    /// recursion through the back door.
    fn cycles(&mut self) {
        let mut successors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in &self.program.edges {
            if !matches!(edge.role, EdgeRole::Flow | EdgeRole::Enter) {
                continue;
            }
            successors
                .entry(edge.from.node)
                .or_default()
                .push(edge.to.node);
        }

        // Iterative depth-first search with an explicit stack: a graph from JSON
        // can be arbitrarily deep, and recursing over it would trade one
        // unbounded construct for another.
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Open,
            Done,
        }
        let mut marks: HashMap<NodeId, Mark> = HashMap::new();
        let mut reported: HashSet<NodeId> = HashSet::new();

        for start in self.program.nodes.iter().map(|n| n.id) {
            if marks.contains_key(&start) {
                continue;
            }
            let mut stack: Vec<(NodeId, usize)> = vec![(start, 0)];
            marks.insert(start, Mark::Open);
            while let Some((node, index)) = stack.pop() {
                let next = successors.get(&node).and_then(|s| s.get(index)).copied();
                match next {
                    Some(child) => {
                        stack.push((node, index + 1));
                        match marks.get(&child) {
                            Some(Mark::Open) => {
                                if reported.insert(child) {
                                    let span = self.node_span(child);
                                    self.diagnostics.push(
                                        CantDiagnostic::error(
                                            CANT_G009_UNSUPPORTED_CYCLE,
                                            "this graph contains a cycle that is not an orbit",
                                        )
                                        .with_primary(
                                            self.span(span),
                                            "reachable from itself without passing through an orbit",
                                        )
                                        .with_note(
                                            "orbit is the only cyclic construct in Cant v0, and it \
                                             is bounded; an arbitrary cycle has no termination rule",
                                        ),
                                    );
                                }
                            }
                            Some(Mark::Done) => {}
                            None => {
                                marks.insert(child, Mark::Open);
                                stack.push((child, 0));
                            }
                        }
                    }
                    None => {
                        marks.insert(node, Mark::Done);
                    }
                }
            }
        }
    }

    // ---- reachability

    /// A node nothing can reach is dead weight — reported as a warning, since it
    /// is a smell rather than a failure, and a graph being edited toward
    /// something is allowed to pass through that state.
    fn reachability(&mut self) {
        let mut reached: HashSet<NodeId> = HashSet::new();
        let mut queue: VecDeque<NodeId> = VecDeque::new();
        if self.program.node(self.program.entry).is_some() {
            queue.push_back(self.program.entry);
            reached.insert(self.program.entry);
        }
        while let Some(node) = queue.pop_front() {
            for edge in self.program.edges_from(node) {
                if reached.insert(edge.to.node) {
                    queue.push_back(edge.to.node);
                }
            }
        }
        for node in &self.program.nodes {
            if reached.contains(&node.id) {
                continue;
            }
            let at = SourceSpan::new(self.file, node.span);
            self.diagnostics.push(
                CantDiagnostic::warning(
                    CANT_G013_UNREACHABLE_NODE,
                    format!("`{}` is not reachable from the entry", node.id),
                )
                .with_primary(at, format!("this {} never runs", node.kind.name())),
            );
        }
    }
}

/// Modifier names a stage's kind accepts, checked against the AST rather than
/// the graph.
///
/// It has to be the AST: lowering *consumes* `:by` and `:max` into the orbit's
/// policy and drops everything else, so by the time there is a graph an unknown
/// modifier has already vanished. Reporting it needs the thing that was written.
pub fn validate_modifiers(
    ast: &cant_syntax::CantProgramAst,
    file: rite_core::FileId,
) -> CantDiagnostics {
    let mut diagnostics = CantDiagnostics::new();
    check_flow(&ast.flow, file, &mut diagnostics);
    diagnostics
}

fn check_flow(flow: &cant_syntax::Flow, file: rite_core::FileId, out: &mut CantDiagnostics) {
    for stage in &flow.stages {
        // "an orbit", not "a orbit": the form's name is interpolated into every
        // message below, and a diagnostic that reads like a template is one
        // nobody trusts to have thought about their case.
        let (allowed, form) = match &stage.kind {
            cant_syntax::StageKind::Orbit { .. } => (ORBIT_MODIFIERS, "an orbit"),
            cant_syntax::StageKind::Ward { .. } => (&[][..], "a ward"),
            cant_syntax::StageKind::Fork { .. } => (&[][..], "a fork"),
            _ => (&[][..], "a stage"),
        };

        let mut seen: Vec<&str> = Vec::new();
        for modifier in &stage.modifiers {
            let at = SourceSpan::new(file, modifier.name_span);
            if !allowed.contains(&modifier.name.as_str()) {
                let mut diagnostic = CantDiagnostic::error(
                    CANT_G010_UNKNOWN_MODIFIER,
                    format!("{form} does not take `:{}`", modifier.name),
                )
                .with_primary(at, "unknown modifier");
                diagnostic = if allowed.is_empty() {
                    diagnostic.with_help(format!(
                        "no modifier applies to {form}; only an orbit takes `:by` and `:max`"
                    ))
                } else {
                    diagnostic.with_help(format!(
                        "an orbit takes {}",
                        allowed
                            .iter()
                            .map(|m| format!("`:{m}`"))
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ))
                };
                out.push(diagnostic);
            } else if seen.contains(&modifier.name.as_str()) {
                out.push(
                    CantDiagnostic::error(
                        CANT_G011_DUPLICATE_MODIFIER,
                        format!(
                            "`:{}` is given twice on this {}",
                            modifier.name,
                            form.trim_start_matches("an ").trim_start_matches("a ")
                        ),
                    )
                    .with_primary(at, "the later one wins")
                    .with_help("remove one of them"),
                );
            } else {
                seen.push(&modifier.name);
            }
        }

        // `:max` has to be a positive integer, and the *text* is the only place
        // that survives — the graph stores the default when parsing failed.
        if let cant_syntax::StageKind::Orbit { .. } = &stage.kind {
            if let Some(max) = crate::lower::modifier(&stage.modifiers, "max") {
                let text = max.value.text.trim();
                let ok = text.parse::<u64>().map(|n| n > 0).unwrap_or(false);
                if !ok {
                    out.push(
                        CantDiagnostic::error(
                            CANT_G007_ORBIT_LIMIT,
                            format!("`:max {text}` is not a positive integer"),
                        )
                        .with_primary(SourceSpan::new(file, max.value.span), "expected a count")
                        .with_note(format!(
                            "an orbit accepts at most this many candidates before failing; \
                             the default is {}",
                            crate::DEFAULT_ORBIT_MAX
                        )),
                    );
                }
            }
        }

        match &stage.kind {
            cant_syntax::StageKind::Fork { branches } => {
                for branch in branches {
                    check_flow(branch, file, out);
                }
            }
            cant_syntax::StageKind::Orbit { body } => check_flow(body, file, out),
            _ => {}
        }
    }
}

/// Everything a caller wants after parsing: the graph, and what is wrong with it.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub graph: CantProgram,
    pub diagnostics: CantDiagnostics,
}

impl Analysis {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .errors()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Lower and validate in one step.
pub fn analyze(
    ast: &cant_syntax::CantProgramAst,
    file: rite_core::FileId,
    source_name: &str,
    source_len: usize,
) -> Analysis {
    let graph = crate::lower::lower(ast, source_name, source_len);
    let mut diagnostics = validate_modifiers(ast, file);
    diagnostics.extend(validate(&graph, file));
    Analysis { graph, diagnostics }
}

/// Validate a graph that arrived as JSON.
///
/// The path the specification cares about: a deserialized graph is untrusted
/// input, and the structural checks are the only thing between it and a
/// consumer that assumes edges join real nodes.
pub fn validate_deserialized(json: &str, file: rite_core::FileId) -> Result<Analysis, String> {
    let graph: CantProgram = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if graph.version != crate::GRAPH_SCHEMA_VERSION {
        return Err(format!(
            "graph schema version `{}`, expected `{}`",
            graph.version,
            crate::GRAPH_SCHEMA_VERSION
        ));
    }
    let diagnostics = validate(&graph, file);
    Ok(Analysis { graph, diagnostics })
}

/// Subgraph membership, for a renderer that wants to draw clusters.
pub fn members(program: &CantProgram, id: SubgraphId) -> Vec<NodeId> {
    program
        .subgraph(id)
        .map(|s| s.nodes.clone())
        .unwrap_or_default()
}
