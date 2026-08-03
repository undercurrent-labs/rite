//! `cant explain` — what a program does, in prose.
//!
//! Explicitly **not** a `Debug` dump. The specification is blunt about this
//! (§12.1) and it is the right call: a pretty-printed AST tells you what the
//! parser built, which is only useful if you already understand the language.
//! What someone wants from `explain` is the thing they would have written in a
//! commit message.
//!
//! Read from the graph rather than the AST, because the graph is what executes.
//! An explanation derived from a different representation than the one that runs
//! is a second description free to be wrong.
//!
//! The structure is produced first as [`Explanation`] and rendered second, so a
//! consumer that wants the steps as data — an editor hover, a Studio panel — gets
//! them without parsing prose back out.

use crate::graph::{CantProgram, NodeKind, SubgraphId};
use crate::NodeId;

/// One numbered step, possibly with sub-steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub text: String,
    /// Nested steps, lettered — the body of an orbit or the branches of a fork.
    pub children: Vec<Step>,
    pub node: NodeId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub steps: Vec<Step>,
    /// Capabilities the program names, in source order.
    pub capabilities: Vec<String>,
    /// Stages that perform an effect, described.
    pub effects: Vec<String>,
    /// The largest `:max` any orbit will accept, if there are orbits.
    pub max_orbit_items: Option<u64>,
    /// Things worth knowing before running it.
    pub hazards: Vec<String>,
}

pub fn explain(program: &CantProgram) -> Explanation {
    let top: Vec<NodeId> = program
        .nodes
        .iter()
        .filter(|n| n.subgraph.is_none())
        .map(|n| n.id)
        .collect();

    let steps = top.iter().map(|id| step(program, *id)).collect();

    Explanation {
        steps,
        capabilities: program.capabilities(),
        effects: effect_descriptions(program),
        max_orbit_items: program.max_orbit_items(),
        hazards: hazards(program),
    }
}

fn step(program: &CantProgram, id: NodeId) -> Step {
    let Some(node) = program.node(id) else {
        return Step {
            text: format!("({id} is missing from the graph)"),
            children: Vec::new(),
            node: id,
        };
    };
    let (text, children) = match &node.kind {
        NodeKind::Source { expr } => (format!("Evaluate `{}`.", expr.text), Vec::new()),
        NodeKind::Stage { expr } => (describe_stage(&expr.text), Vec::new()),
        NodeKind::Scatter => (
            "Scatter the list into one emission per element, in order.".to_string(),
            Vec::new(),
        ),
        NodeKind::Collect => (
            "Collect every emission so far into one list, in emission order.".to_string(),
            Vec::new(),
        ),
        NodeKind::Ward { predicate } => (
            format!(
                "Keep only the emissions where `{}` holds; the rest emit nothing.",
                predicate.text
            ),
            Vec::new(),
        ),
        NodeKind::Fork { branches } => (
            format!(
                "Fork into {} branch{}, each seeing the same value, and concatenate \
                 their emissions in order:",
                branches.len(),
                if branches.len() == 1 { "" } else { "es" }
            ),
            branches
                .iter()
                .enumerate()
                .map(|(i, id)| Step {
                    text: format!("Branch {}:", i + 1),
                    children: subgraph_steps(program, *id),
                    node: node.id,
                })
                .collect(),
        ),
        NodeKind::Orbit {
            body,
            identity,
            max_items,
        } => {
            let by = match identity {
                Some(identity) => format!("identified by `{}`", identity.text),
                None => "identified by value".to_string(),
            };
            let mut children = vec![Step {
                text: "For each candidate not seen before:".to_string(),
                children: subgraph_steps(program, *body),
                node: node.id,
            }];
            children.push(Step {
                text: format!(
                    "Whatever that emits joins the back of the worklist, {by}, \
                     first occurrence winning."
                ),
                children: Vec::new(),
                node: node.id,
            });
            children.push(Step {
                text: format!(
                    "Stop when the worklist is empty, or fail after {max_items} \
                     accepted candidates."
                ),
                children: Vec::new(),
                node: node.id,
            });
            (
                "Enter a breadth-first orbit, seeded with the current emissions:".to_string(),
                children,
            )
        }
    };
    Step {
        text,
        children,
        node: id,
    }
}

fn subgraph_steps(program: &CantProgram, id: SubgraphId) -> Vec<Step> {
    program
        .subgraph(id)
        .map(|s| s.nodes.iter().map(|n| step(program, *n)).collect())
        .unwrap_or_default()
}

/// A stage's description, which depends on how the value reaches it.
///
/// The distinction is the one thing about a stage that is not obvious from
/// reading it, so it is the thing worth saying.
fn describe_stage(text: &str) -> String {
    if text.contains('$') {
        format!("Evaluate `{text}`, with `$` bound to each emission.")
    } else if text.starts_with('.') {
        format!("Take `{text}` from each emission.")
    } else {
        format!("Pass each emission through `{text}`.")
    }
}

fn effect_descriptions(program: &CantProgram) -> Vec<String> {
    program
        .nodes
        .iter()
        .filter_map(|node| {
            let leaf = node.kind.leaf()?;
            leaf.effectful
                .then(|| format!("`{}` — {}", leaf.text, node.kind.name()))
        })
        .collect()
}

/// What to know before running it.
///
/// Only things that are true of *this* program. A generic list of everything
/// that could ever go wrong is noise, and noise in a hazards section is worse
/// than an empty one because it trains people to skip it.
fn hazards(program: &CantProgram) -> Vec<String> {
    let mut out = Vec::new();

    if program
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Scatter))
    {
        out.push("Scatter fails at run time if what reaches it is not a list.".to_string());
    }
    if let Some(max) = program.max_orbit_items() {
        out.push(format!(
            "An orbit stops with an error after {max} accepted candidates rather than \
             returning a partial result."
        ));
    }
    if !program.effectful_nodes().is_empty() {
        out.push(
            "This program performs host effects, so it needs the matching permissions \
             granted with `--allow`."
                .to_string(),
        );
    }
    let forks = program
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Fork { .. }))
        .count();
    if forks > 0 && !program.effectful_nodes().is_empty() {
        out.push(
            "Fork branches run one after another, left to right — so do the effects \
             inside them."
                .to_string(),
        );
    }
    out
}

/// Render an explanation the way `cant explain` prints it.
pub fn render(explanation: &Explanation, verbose: bool) -> String {
    let mut out = String::new();

    out.push_str("What this program does\n\n");
    let mut number = 0;
    for step in &explanation.steps {
        number += 1;
        out.push_str(&format!("{number}. {}\n", step.text));
        render_children(&mut out, &step.children, 1);
    }
    if explanation.steps.is_empty() {
        out.push_str("Nothing: it has no stages.\n");
    }

    out.push_str(
        "\nThe result is whatever is emitted at the end: nothing becomes `none`, \
         one value becomes that value, and several become a list in emission order.\n",
    );

    if !explanation.capabilities.is_empty() {
        out.push_str("\nCapabilities it needs\n\n");
        for capability in &explanation.capabilities {
            out.push_str(&format!("  {capability}\n"));
        }
    }

    if !explanation.effects.is_empty() {
        out.push_str("\nWhere it touches the world\n\n");
        for effect in &explanation.effects {
            out.push_str(&format!("  {effect}\n"));
        }
    }

    if !explanation.hazards.is_empty() {
        out.push_str("\nWorth knowing\n\n");
        for hazard in &explanation.hazards {
            out.push_str(&format!("  - {hazard}\n"));
        }
    }

    out.push_str(
        "\nOrder\n\n  \
         Everything here is deterministic. Stages run in source order, fork branches\n  \
         left to right, scatter preserves list order, collect preserves emission order,\n  \
         and an orbit is breadth-first with the first occurrence of a value winning.\n  \
         Effects happen in exactly that order. Nothing runs in parallel.\n",
    );

    if verbose {
        out.push_str(
            "\n  Run `cant expand` to see the Rite this becomes, or `cant graph` for its\n  \
             topology.\n",
        );
    }

    out
}

fn render_children(out: &mut String, steps: &[Step], depth: usize) {
    for (index, step) in steps.iter().enumerate() {
        let indent = "   ".repeat(depth);
        let marker = match depth {
            1 => format!("{}.", (b'a' + (index as u8 % 26)) as char),
            _ => "-".to_string(),
        };
        out.push_str(&format!("{indent}{marker} {}\n", step.text));
        render_children(out, &step.children, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cant_syntax::parse_source;

    fn explain_source(source: &str) -> Explanation {
        let (parsed, _) = parse_source("t.cant", source);
        let graph = crate::lower::lower(&parsed.program.expect("program"), "t.cant", source.len());
        explain(&graph)
    }

    fn text(source: &str) -> String {
        render(&explain_source(source), false)
    }

    #[test]
    fn a_flow_reads_as_numbered_steps() {
        let out = text("[1, 2] -> * -> ?{ $ > 1 } -> []");
        assert!(out.contains("1. Evaluate `[1, 2]`."), "{out}");
        assert!(out.contains("2. Scatter the list"), "{out}");
        assert!(
            out.contains("3. Keep only the emissions where `$ > 1` holds"),
            "{out}"
        );
        assert!(out.contains("4. Collect every emission"), "{out}");
    }

    #[test]
    fn a_stage_is_described_by_how_the_value_reaches_it() {
        assert!(text("x -> square").contains("Pass each emission through `square`."));
        assert!(text("x -> $ + 1").contains("with `$` bound to each emission"));
        assert!(text("x -> .message").contains("Take `.message` from each emission."));
    }

    #[test]
    fn an_orbit_explains_its_policy() {
        let out = text("r -> ~{ d -> * } :by canonical :max 4096");
        assert!(out.contains("breadth-first orbit"), "{out}");
        assert!(out.contains("identified by `canonical`"), "{out}");
        assert!(out.contains("4096 accepted candidates"), "{out}");
        assert!(out.contains("first occurrence winning"), "{out}");
    }

    #[test]
    fn a_fork_lists_its_branches() {
        let out = text("x -> |{ a ; b -> c }");
        assert!(out.contains("Fork into 2 branches"), "{out}");
        assert!(out.contains("a. Branch 1:"), "{out}");
        assert!(out.contains("b. Branch 2:"), "{out}");
    }

    #[test]
    fn capabilities_and_effects_are_reported() {
        let out = text("\"p\" -> !@fs.read -> @json.decode");
        assert!(out.contains("Capabilities it needs"), "{out}");
        assert!(out.contains("@fs.read"), "{out}");
        assert!(out.contains("@json.decode"), "{out}");
        assert!(out.contains("Where it touches the world"), "{out}");
        assert!(out.contains("needs the matching permissions"), "{out}");
    }

    /// Hazards are about *this* program. A pure one has none, and saying so by
    /// omission is better than a generic list nobody reads.
    #[test]
    fn a_pure_program_has_no_hazards_section() {
        let out = text("3 -> $ + 1");
        assert!(!out.contains("Worth knowing"), "{out}");
        assert!(!out.contains("Capabilities it needs"), "{out}");
    }

    #[test]
    fn every_explanation_states_the_ordering_rules() {
        for source in ["3", "x -> |{ a ; b }", "r -> ~{ d } :max 2"] {
            assert!(text(source).contains("deterministic"), "{source}");
        }
    }

    #[test]
    fn verbose_points_at_the_other_two_views() {
        let out = render(&explain_source("a -> b"), true);
        assert!(out.contains("cant expand"), "{out}");
        assert!(out.contains("cant graph"), "{out}");
    }

    /// The thing the specification forbids.
    #[test]
    fn the_output_is_never_a_debug_dump() {
        for source in [
            "[1, 2] -> * -> []",
            "x -> |{ a ; b }",
            "r -> ~{ d } :by k :max 8",
            "\"p\" -> !@fs.read",
        ] {
            let out = text(source);
            for tell in ["NodeKind", "Span {", "LeafExpr", "SubgraphId", "{ id:"] {
                assert!(!out.contains(tell), "{source:?} leaked `{tell}`:\n{out}");
            }
        }
    }
}
