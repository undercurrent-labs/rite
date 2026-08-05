//! Graph validation.
//!
//! Everything here is checkable **without executing anything**, which is the
//! point: `cant check` should reject a program that cannot work before a
//! capability is granted or a byte is read.
//!
//! Two audiences, one function. A graph that came from [`crate::lower`] cannot
//! have a dangling edge or a duplicate identifier, since the builder assigns
//! them,
//! but a graph that came from *JSON* can, and the specification requires that a
//! fuzzed graph cannot smuggle in an unvalidated cycle. So the structural checks
//! run on both instead of being skipped as impossible; on a freshly lowered
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
const FORK_MODIFIERS: &[&str] = &["par"];

/// Modifiers that configure a form by being present, with nothing after them.
///
/// The parser records `:name` and `:name value` alike and asks no questions, so
/// this is the only place that knows `:par` takes nothing and `:max` takes a
/// count. Both directions are checked: a value on `:par` and a missing one on
/// `:max` are each a mistake with a message of its own.
const VALUELESS_MODIFIERS: &[&str] = &["par"];

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
    v.rescues();
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
                    NodeKind::Rescue { .. } => "rescue handler",
                    _ => "orbit body",
                };
                let label = format!("an empty {what} emits nothing and cannot be lowered");
                self.error(
                    CANT_G015_EMPTY_SUBGRAPH,
                    format!("this {what} has no stages"),
                    span,
                    &label,
                );
                continue;
            };

            let expected_role = match node.kind {
                NodeKind::Fork { .. } | NodeKind::Rescue { .. } => EdgeRole::Join,
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

    /// Scatter, collect and rescue all consume emissions, so none of them can be
    /// the first thing a program does — there are none yet.
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
            NodeKind::Rescue { .. } => self.error(
                CANT_G016_RESCUE_HAS_NO_INPUT,
                "a program cannot begin with a rescue",
                entry.span,
                "nothing has been emitted yet, so nothing can have failed",
            ),
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

    // ---- rescues

    /// A `?` on the stage feeding a rescue removes the failure the rescue exists
    /// to route.
    ///
    /// `?` unwraps the `ok` and returns from the loop body Cant generates for a
    /// stage, so the failed emission is dropped before the rescue sees anything
    /// and the handler can never run. The program works, silently does no error
    /// handling, and looks like it does — which is why this is an error and not a
    /// warning.
    fn rescues(&mut self) {
        let rescues: Vec<NodeId> = self
            .program
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Rescue { .. }))
            .map(|n| n.id)
            .collect();

        for rescue in rescues {
            let sources: Vec<NodeId> = self
                .program
                .edges_to(rescue)
                .filter(|e| e.role == EdgeRole::Flow)
                .map(|e| e.from.node)
                .collect();
            for source in sources {
                let Some(leaf) = self.program.node(source).and_then(|n| n.kind.leaf()) else {
                    continue;
                };
                let text = leaf.text.trim_end();
                if !text.ends_with('?') || text.ends_with("??") {
                    continue;
                }
                let (leaf_span, rescue_span) = (leaf.span, self.node_span(rescue));
                self.diagnostics.push(
                    CantDiagnostic::error(
                        CANT_G017_RESCUE_AFTER_TRY,
                        "this `?` removes the failure the rescue would route",
                    )
                    .with_primary(self.span(leaf_span), "`?` fails the whole run here")
                    .with_secondary(self.span(rescue_span), "so this handler never runs")
                    .with_help("drop the `?` and let the rescue take the `err`"),
                );
            }
        }
    }

    // ---- cycles

    /// The one structural rule v0 has: **every cycle is orbit-owned.**
    ///
    /// [`EdgeRole::Flow`], [`EdgeRole::Enter`] and [`EdgeRole::Rescue`] carry
    /// control forward, so only they can form a loop that runs twice. The other
    /// two are excluded for different reasons:
    ///
    /// * [`EdgeRole::OrbitFeedback`] is the sanctioned loop — the whole point of
    ///   an orbit, and bounded by its `:max`.
    /// * [`EdgeRole::Join`] returns a branch's emissions to the fork that opened
    ///   it, or a handler's to its rescue. It always pairs with an `Enter` or a
    ///   `Rescue`, so counting it made **every fork** look like an illegal cycle.
    ///   A branch runs once; the join is a concatenation point, not a re-entry.
    ///
    /// Nothing lowering produces can build a cycle in the remaining subgraph,
    /// which is why this has to run on *deserialized* graphs: JSON is the
    /// only way one gets in, and admitting it silently would give Cant unbounded
    /// recursion through the back door.
    fn cycles(&mut self) {
        let mut successors: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
        for edge in &self.program.edges {
            if !matches!(
                edge.role,
                EdgeRole::Flow | EdgeRole::Enter | EdgeRole::Rescue
            ) {
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
            cant_syntax::StageKind::Fork { .. } => (FORK_MODIFIERS, "a fork"),
            cant_syntax::StageKind::Rescue { .. } => (&[][..], "a rescue"),
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
                        "no modifier applies to {form}; an orbit takes `:by` and `:max`, \
                         and a fork takes `:par`"
                    ))
                } else {
                    diagnostic.with_help(format!("{form} takes {}", spelled(allowed)))
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
                check_modifier_value(modifier, file, out);
            }
        }

        // `:par` on a fork with one branch runs that branch concurrently with
        // nothing. A warning rather than an error: unlike a `?` before a rescue,
        // it neither changes what the program emits nor makes a stage silently
        // unreachable, and a fork is a thing people edit branches out of.
        if let cant_syntax::StageKind::Fork { branches } = &stage.kind {
            if branches.len() < 2 {
                if let Some(par) = crate::lower::modifier(&stage.modifiers, "par") {
                    out.push(
                        CantDiagnostic::warning(
                            CANT_G024_PARALLEL_SINGLE_BRANCH,
                            "`:par` on a fork with one branch",
                        )
                        .with_primary(
                            SourceSpan::new(file, par.name_span),
                            "there is nothing to run it alongside",
                        )
                        .with_help("add a branch, or drop the `:par`"),
                    );
                }
            }
        }

        // `:max` has to be a positive integer, and the *text* is the only place
        // that survives — the graph stores the default when parsing failed.
        if let cant_syntax::StageKind::Orbit { .. } = &stage.kind {
            // A `:max` with no value at all is reported by `check_modifier_value`
            // above; this is about the ones that carry something unusable.
            if let Some(value) =
                crate::lower::modifier(&stage.modifiers, "max").and_then(|m| m.value.as_ref())
            {
                let text = value.text.trim();
                let ok = text.parse::<u64>().map(|n| n > 0).unwrap_or(false);
                if !ok {
                    out.push(
                        CantDiagnostic::error(
                            CANT_G007_ORBIT_LIMIT,
                            format!("`:max {text}` is not a positive integer"),
                        )
                        .with_primary(SourceSpan::new(file, value.span), "expected a count")
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
            cant_syntax::StageKind::Rescue { handler } => check_flow(handler, file, out),
            _ => {}
        }
    }
}

/// `` `:by` and `:max` `` — a list of modifier names for a help line.
fn spelled(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|m| format!("`:{m}`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// A modifier carries a value exactly when its name takes one.
///
/// Called only for names that are allowed on the form, so both messages can name
/// the modifier and say what it wants.
fn check_modifier_value(
    modifier: &cant_syntax::Modifier,
    file: rite_core::FileId,
    out: &mut CantDiagnostics,
) {
    let valueless = VALUELESS_MODIFIERS.contains(&modifier.name.as_str());
    match (&modifier.value, valueless) {
        (None, false) => out.push(
            CantDiagnostic::error(
                CANT_G022_MODIFIER_NEEDS_VALUE,
                format!("`:{}` needs a value", modifier.name),
            )
            .with_primary(
                SourceSpan::new(file, modifier.name_span),
                "expected a value after the modifier name",
            ),
        ),
        (Some(value), true) => out.push(
            CantDiagnostic::error(
                CANT_G023_MODIFIER_TAKES_NO_VALUE,
                format!("`:{}` takes no value", modifier.name),
            )
            .with_primary(SourceSpan::new(file, value.span), "nothing goes here")
            .with_help(format!(
                "write it on its own: `:{}` configures the form by being present",
                modifier.name
            )),
        ),
        _ => {}
    }
}

/// Named flows, checked against the AST rather than the graph.
///
/// It has to be the AST for the same reason modifier validation is: lowering
/// *splices* a definition into the flow that used it, so by the time there is a
/// graph there is no definition left to point at, and no name either.
///
/// Four rules, in the order a program hits them (ADR 0011).
pub fn validate_definitions(
    ast: &cant_syntax::CantProgramAst,
    file: rite_core::FileId,
) -> CantDiagnostics {
    let mut out = CantDiagnostics::new();
    if ast.defs.is_empty() {
        return out;
    }
    let defined: Vec<&str> = ast.defs.iter().map(|d| d.name.as_str()).collect();

    // Two definitions with one name: the first wins when lowering splices, so
    // the second is dead and the program says two things.
    for (index, def) in ast.defs.iter().enumerate() {
        let Some(first) = ast.defs[..index].iter().find(|d| d.name == def.name) else {
            continue;
        };
        out.push(
            CantDiagnostic::error(
                CANT_G018_DUPLICATE_DEFINITION,
                format!("`{}` is defined twice", def.name),
            )
            .with_primary(SourceSpan::new(file, def.name_span), "defined again here")
            .with_secondary(SourceSpan::new(file, first.name_span), "first defined here")
            .with_help("give one of them another name, or delete it"),
        );
    }

    // A name Rite already binds means one thing as a stage and another inside a
    // leaf, in the same program.
    for def in &ast.defs {
        let clash = if rite_sem::resolve::BUILTIN_NAMES.contains(&def.name.as_str())
            || rite_sem::resolve::EFFECTFUL_BUILTINS.contains(&def.name.as_str())
        {
            Some("a Rite builtin")
        } else if ast.uses.iter().any(|u| u.name == def.name) {
            Some("a module this program imports")
        } else {
            None
        };
        let Some(what) = clash else { continue };
        out.push(
            CantDiagnostic::error(
                CANT_G019_DEFINITION_SHADOWS,
                format!("`{}` is already {what}", def.name),
            )
            .with_primary(
                SourceSpan::new(file, def.name_span),
                "defined here as a flow",
            )
            .with_note(format!(
                "`{0}` as a whole stage would be this flow, and `{0}` inside a leaf would \
                 stay {what}",
                def.name
            ))
            .with_help("name the flow something the program does not already use"),
        );
    }

    // Every cycle in Cant is orbit-owned, and splicing a definition that reaches
    // itself would not terminate.
    let mut reported: Vec<&str> = Vec::new();
    for def in &ast.defs {
        if reported.contains(&def.name.as_str()) {
            continue;
        }
        let Some(cycle) = cycle_from(&def.name, &ast.defs, &defined) else {
            continue;
        };
        reported.extend(cycle.iter().copied());
        let route = cycle.join("` -> `");
        let title = if cycle.len() == 1 {
            format!("the definition `{}` uses itself", def.name)
        } else {
            format!("the definition `{}` reaches itself", def.name)
        };
        out.push(
            CantDiagnostic::error(CANT_G020_RECURSIVE_DEFINITION, title)
                .with_primary(
                    SourceSpan::new(file, def.name_span),
                    format!("`{route}` -> `{}`", def.name),
                )
                .with_note(
                    "a definition is spliced in where it is named, so a recursive one has \
                     no end; an orbit is the only construct that repeats",
                )
                .with_help("write the repetition as an orbit: `~{ … } :max n`"),
        );
    }

    // A definition nothing names is dead, and the usual reason is a typo at the
    // use site, which became an ordinary Rite name and was reported as one.
    let mut live: Vec<&str> = Vec::new();
    referenced(&ast.flow, &defined, &mut live);
    let mut index = 0;
    while index < live.len() {
        let name = live[index];
        index += 1;
        if let Some(def) = ast.defs.iter().find(|d| d.name == name) {
            referenced(&def.flow, &defined, &mut live);
        }
    }
    for def in &ast.defs {
        if live.contains(&def.name.as_str()) {
            continue;
        }
        out.push(
            CantDiagnostic::error(
                CANT_G021_UNUSED_DEFINITION,
                format!("`{}` is defined and never used", def.name),
            )
            .with_primary(SourceSpan::new(file, def.name_span), "nothing names this")
            .with_note(
                "a definition is used by naming it as a whole stage; `clean($)` is Rite \
                 expression text, not a use",
            )
            .with_help("use it in the flow, or delete it"),
        );
    }

    out
}

/// The route from `start` back to itself through other definitions, if there is
/// one.
///
/// Depth-first over a graph with one node per definition, which is small enough
/// that plain recursion is bounded by the number of definitions in the file.
fn cycle_from<'a>(
    start: &'a str,
    defs: &'a [cant_syntax::FlowDef],
    defined: &[&'a str],
) -> Option<Vec<&'a str>> {
    fn walk<'a>(
        start: &'a str,
        current: &'a str,
        defs: &'a [cant_syntax::FlowDef],
        defined: &[&'a str],
        seen: &mut Vec<&'a str>,
        route: &mut Vec<&'a str>,
    ) -> bool {
        let Some(def) = defs.iter().find(|d| d.name == current) else {
            return false;
        };
        let mut names = Vec::new();
        referenced(&def.flow, defined, &mut names);
        for name in names {
            if name == start {
                return true;
            }
            if seen.contains(&name) {
                continue;
            }
            seen.push(name);
            route.push(name);
            if walk(start, name, defs, defined, seen, route) {
                return true;
            }
            route.pop();
        }
        false
    }

    let mut seen = vec![start];
    let mut route = vec![start];
    walk(start, start, defs, defined, &mut seen, &mut route).then_some(route)
}

/// Every definition name this flow uses as a whole stage, deduplicated, in
/// source order and including the ones inside forks, orbits and rescues.
fn referenced<'a>(flow: &'a cant_syntax::Flow, defined: &[&'a str], out: &mut Vec<&'a str>) {
    for stage in &flow.stages {
        if let Some(name) = crate::lower::definition_use(stage) {
            if let Some(found) = defined.iter().find(|d| **d == name) {
                if !out.contains(found) {
                    out.push(*found);
                }
            }
        }
        match &stage.kind {
            cant_syntax::StageKind::Fork { branches } => {
                for branch in branches {
                    referenced(branch, defined, out);
                }
            }
            cant_syntax::StageKind::Orbit { body } => referenced(body, defined, out),
            cant_syntax::StageKind::Rescue { handler } => referenced(handler, defined, out),
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
    // Definitions first: lowering leaves a recursive one as a leaf so that it
    // terminates, and the graph that results is not worth describing until the
    // recursion is reported.
    let mut diagnostics = validate_definitions(ast, file);
    diagnostics.extend(validate_modifiers(ast, file));
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
    // Name before version: a version number is only meaningful once it is known
    // whose it is, and "expected 1, got 3" is a confusing thing to say about a
    // document that was never a Cant graph.
    if graph.schema != crate::GRAPH_SCHEMA_NAME {
        return Err(format!(
            "graph schema `{}`, expected `{}`",
            graph.schema,
            crate::GRAPH_SCHEMA_NAME
        ));
    }
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
