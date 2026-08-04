//! Graphviz DOT export.
//!
//! The technical topology view, and it stays that way. A topology you cannot
//! look at is a topology nobody checks, and `dot -Tsvg` is two seconds away from
//! anyone with a terminal. Sigil is the stylized semantic artifact and does not
//! replace this or take its layout from it — see
//! `docs/adr/0008-graphviz-stays-the-technical-view.md`.
//!
//! Output is deterministic — nodes in identifier order, edges in construction
//! order — so it can be snapshot-tested like the JSON.

use crate::graph::{CantProgram, EdgeRole, NodeKind};

/// Colours, matching the site and `grammar/palette.json` so a graph printed from
/// the CLI and one drawn on the web are recognisably the same object.
const ACCENT: &str = "#ff7edb"; // structural operators
const CAPABILITY: &str = "#7ee0ff"; // effectful nodes
const MUTED: &str = "#8b9bb4";
const BACKGROUND: &str = "#121821";
const TEXT: &str = "#e2e8f0";

pub fn to_dot(program: &CantProgram) -> String {
    let mut out = String::new();
    out.push_str("digraph cant {\n");
    out.push_str("  rankdir=TB;\n");
    out.push_str(&format!(
        "  bgcolor=\"{BACKGROUND}\";\n  fontname=\"IBM Plex Mono\";\n  fontcolor=\"{TEXT}\";\n"
    ));
    out.push_str(&format!(
        "  node [shape=box style=\"rounded,filled\" fillcolor=\"#161d28\" color=\"#1e293b\" \
         fontname=\"IBM Plex Mono\" fontcolor=\"{TEXT}\" fontsize=10];\n"
    ));
    out.push_str(&format!(
        "  edge [color=\"{MUTED}\" fontname=\"IBM Plex Mono\" fontsize=9 fontcolor=\"{MUTED}\"];\n"
    ));
    out.push_str(&format!(
        "  label=\"{}\";\n  labelloc=t;\n\n",
        escape(&program.source.name)
    ));

    // Top-level nodes first, then one cluster per subgraph. A cluster is how DOT
    // draws containment, which is what a fork branch and an orbit body are.
    let mut ids: Vec<_> = program.nodes.iter().map(|n| n.id).collect();
    ids.sort();

    for id in &ids {
        let Some(node) = program.node(*id) else {
            continue;
        };
        if node.subgraph.is_some() {
            continue;
        }
        out.push_str(&node_line(program, node.id));
    }

    let mut subgraphs: Vec<_> = program.subgraphs.iter().collect();
    subgraphs.sort_by_key(|s| s.id.0);
    for subgraph in subgraphs {
        let owner = program
            .node(subgraph.owner)
            .map(|n| n.kind.name())
            .unwrap_or("branch");
        out.push_str(&format!("\n  subgraph cluster_{} {{\n", subgraph.id.0));
        out.push_str(&format!(
            "    label=\"{owner} {}\";\n    color=\"#1e293b\";\n    fontcolor=\"{MUTED}\";\n    fontsize=9;\n",
            subgraph.id
        ));
        for id in &subgraph.nodes {
            out.push_str("  ");
            out.push_str(&node_line(program, *id));
        }
        out.push_str("  }\n");
    }

    out.push('\n');
    for edge in &program.edges {
        let (style, label) = match edge.role {
            EdgeRole::Flow => ("", String::new()),
            EdgeRole::Enter => (" style=dashed", format!(" label=\"{}\"", edge.ordinal)),
            EdgeRole::Join => (" style=dashed", String::new()),
            // The one cycle: drawn distinctly, because "where does this loop"
            // is the first question anyone asks of an orbit.
            EdgeRole::OrbitFeedback => (
                &*format!(" style=bold color=\"{ACCENT}\" constraint=false"),
                " label=\"feedback\"".to_string(),
            ),
        };
        out.push_str(&format!(
            "  {} -> {}[{}{}];\n",
            edge.from.node, edge.to.node, style, label
        ));
    }

    out.push_str("}\n");
    out
}

fn node_line(program: &CantProgram, id: crate::NodeId) -> String {
    let Some(node) = program.node(id) else {
        return String::new();
    };
    let effectful = node.kind.leaf().is_some_and(|l| l.effectful);
    let colour = if effectful {
        CAPABILITY
    } else if matches!(
        node.kind,
        NodeKind::Scatter | NodeKind::Collect | NodeKind::Fork { .. } | NodeKind::Orbit { .. }
    ) {
        ACCENT
    } else {
        TEXT
    };
    // Escape the pieces, then join with a literal `\n` — DOT's line break inside
    // a label. Escaping the assembled label instead turned that break into a
    // backslash and an `n`, which is how an orbit came to be captioned
    // `~orbit\n:max 8` on the page.
    let label = node_label(program, id)
        .iter()
        .map(|piece| escape(piece))
        .collect::<Vec<_>>()
        .join("\\n");
    format!("  {id} [label=\"{label}\" color=\"{colour}\" fontcolor=\"{colour}\"];\n")
}

/// What a node says on the page, one line per element.
///
/// Long leaf text is truncated: a diagram is a topology, and a node containing a
/// forty-character expression stops being readable as a shape. The full text is
/// in the JSON for anyone who needs it.
fn node_label(program: &CantProgram, id: crate::NodeId) -> Vec<String> {
    let Some(node) = program.node(id) else {
        return vec![id.to_string()];
    };
    match &node.kind {
        NodeKind::Source { expr } | NodeKind::Stage { expr } => {
            vec![format!("{id}  {}", truncate(&expr.text))]
        }
        NodeKind::Scatter => vec![format!("{id}  * scatter")],
        NodeKind::Collect => vec![format!("{id}  [] collect")],
        NodeKind::Ward { predicate } => {
            vec![format!("{id}  ?{{ {} }}", truncate(&predicate.text))]
        }
        NodeKind::Fork { branches } => {
            vec![format!("{id}  |{{ {} branches }}", branches.len())]
        }
        NodeKind::Orbit {
            identity,
            max_items,
            ..
        } => {
            let mut lines = vec![format!("{id}  ~orbit")];
            if let Some(identity) = identity {
                lines.push(format!(":by {}", truncate(&identity.text)));
            }
            lines.push(format!(":max {max_items}"));
            lines
        }
    }
}

fn truncate(text: &str) -> String {
    const LIMIT: usize = 28;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= LIMIT {
        return flat;
    }
    let kept: String = flat.chars().take(LIMIT - 1).collect();
    format!("{kept}…")
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cant_syntax::parse_source;

    fn dot_of(source: &str) -> String {
        let (result, _) = parse_source("t.cant", source);
        let graph = crate::lower::lower(&result.program.expect("program"), "t.cant", source.len());
        to_dot(&graph)
    }

    #[test]
    fn a_flow_becomes_a_chain_of_edges() {
        let dot = dot_of("a -> b -> c");
        assert!(dot.starts_with("digraph cant {"));
        assert!(dot.contains("n0 -> n1"), "{dot}");
        assert!(dot.contains("n1 -> n2"), "{dot}");
        assert!(dot.trim_end().ends_with('}'));
    }

    #[test]
    fn subgraphs_become_clusters() {
        let dot = dot_of("x -> |{ a ; b }");
        assert!(dot.contains("subgraph cluster_0"), "{dot}");
        assert!(dot.contains("subgraph cluster_1"), "{dot}");
        assert!(dot.contains("fork s0"), "{dot}");
    }

    #[test]
    fn the_orbit_feedback_edge_is_drawn_distinctly() {
        let dot = dot_of("r -> ~{ d -> * }");
        assert!(dot.contains("feedback"), "{dot}");
        assert!(dot.contains("constraint=false"), "{dot}");
    }

    /// An orbit's caption breaks across lines.
    ///
    /// `escape` used to run over the assembled label and turn DOT's `\n` into a
    /// backslash and an `n`, so the picture read `~orbit\n:max 8` on one line.
    #[test]
    fn an_orbit_caption_uses_a_real_line_break() {
        let dot = dot_of("r -> ~{ d } :by str :max 8");
        assert!(dot.contains(r"~orbit\n:by str\n:max 8"), "{dot}");
        assert!(
            !dot.contains(r"~orbit\\n"),
            "the break was escaped away: {dot}"
        );
    }

    /// A backslash in leaf text still has to be escaped, or it would be read as
    /// the start of one of DOT's own escapes.
    #[test]
    fn a_backslash_in_leaf_text_is_still_escaped() {
        let dot = dot_of(r#""a\\nb" -> f"#);
        assert!(dot.contains(r"\\"), "{dot}");
    }

    #[test]
    fn output_is_deterministic() {
        let source = "roots -> * -> |{ a ; b -> c } -> ~{ d } :max 8 -> []";
        assert_eq!(dot_of(source), dot_of(source));
    }

    #[test]
    fn long_leaf_text_is_truncated_so_the_shape_stays_readable() {
        let dot = dot_of("some_extremely_long_identifier_that_will_not_fit_on_a_node -> b");
        assert!(dot.contains('…'), "{dot}");
    }

    #[test]
    fn quotes_in_leaf_text_do_not_break_the_output() {
        let dot = dot_of("\"a\\\"b\" -> f");
        // Every `"` that is not a delimiter must be escaped, so the number of
        // unescaped quotes stays even on every line.
        for line in dot.lines() {
            let unescaped = line
                .char_indices()
                .filter(|(i, c)| *c == '"' && (*i == 0 || line.as_bytes()[i - 1] != b'\\'))
                .count();
            assert_eq!(unescaped % 2, 0, "unbalanced quotes in: {line}");
        }
    }
}
