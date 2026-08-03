//! The Cant AST.
//!
//! Deliberately thin. It records what was written and where, and leaves every
//! question about *meaning* to the graph in `cant-sem` (Phase 3) and to Rite's
//! resolver after expansion (Phase 4). In particular a [`Leaf`] is Rite
//! expression text plus a span, not a parsed Rite expression: Cant does not
//! re-specify Rite's expression grammar, and the only facts about a leaf that
//! Cant itself needs — does it perform an effect, does it name the current value
//! — are visible from Cant's own tokens.

use rite_core::Span;
use serde::{Deserialize, Serialize};

/// A whole `.cant` source: one flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CantProgramAst {
    pub flow: Flow,
    pub span: Span,
}

/// `stage -> stage -> stage`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flow {
    pub stages: Vec<Stage>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub kind: StageKind,
    pub span: Span,
    /// `:name value` forms attached to this stage.
    ///
    /// Only a structural stage can carry them, which the parser enforces; which
    /// names are *meaningful* (`:by` and `:max` on an orbit) is graph validation's
    /// call in Phase 3, so an unknown modifier parses and is rejected later with
    /// a better message than the parser could give.
    pub modifiers: Vec<Modifier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum StageKind {
    /// Rite expression text: a value, a call, a projection, a capability call.
    Leaf(Leaf),
    /// `*` — expand a list into one emission per element.
    Scatter,
    /// `[]` — materialize the current emissions as one list.
    Collect,
    /// `?{ predicate }` — pass the input through only when the predicate holds.
    Ward { predicate: Leaf },
    /// `|{ a ; b ; c }` — ordered branches from the same input, concatenated.
    Fork { branches: Vec<Flow> },
    /// `~{ body }` — bounded breadth-first fixed point.
    Orbit { body: Flow },
}

impl StageKind {
    /// The name used in diagnostics and in `cant explain`.
    pub fn name(&self) -> &'static str {
        match self {
            StageKind::Leaf(_) => "stage",
            StageKind::Scatter => "scatter",
            StageKind::Collect => "collect",
            StageKind::Ward { .. } => "ward",
            StageKind::Fork { .. } => "fork",
            StageKind::Orbit { .. } => "orbit",
        }
    }

    /// Can a `:name value` modifier attach to this stage?
    pub fn takes_modifiers(&self) -> bool {
        matches!(
            self,
            StageKind::Ward { .. } | StageKind::Fork { .. } | StageKind::Orbit { .. }
        )
    }
}

/// A run of Rite expression text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Leaf {
    /// The source slice, with surrounding whitespace trimmed. Reaches generated
    /// Rite unchanged.
    pub text: String,
    pub span: Span,
    /// The leaf contains a Cant effect marker (`!`).
    ///
    /// Enough for the two v0 rules Cant owns — a ward predicate and an orbit
    /// `:by` function must not be effectful — without Cant having to decide
    /// anything about *names*, which only Rite's resolver can answer.
    pub has_effect_marker: bool,
    /// The leaf contains an explicit `$`, so the current emission goes there
    /// rather than into the first argument position.
    pub has_placeholder: bool,
}

/// `:name value`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Modifier {
    pub name: String,
    /// The whole `:name value`.
    pub span: Span,
    /// Just `:name`, for a diagnostic that should point at the name.
    pub name_span: Span,
    pub value: Leaf,
}

/// A span-free view of a program, for comparing two spellings of the same
/// source.
///
/// ASCII and glyph forms of one program differ in byte offsets everywhere, so
/// `PartialEq` on the AST cannot answer "are these the same program". This can:
/// it keeps structure, leaf text and modifier names, and drops every span.
/// Rite's `parse_both_equivalent` does the same thing for the same reason.
pub fn structure(program: &CantProgramAst) -> serde_json::Value {
    flow_structure(&program.flow)
}

fn flow_structure(flow: &Flow) -> serde_json::Value {
    serde_json::Value::Array(flow.stages.iter().map(stage_structure).collect())
}

fn stage_structure(stage: &Stage) -> serde_json::Value {
    let kind = match &stage.kind {
        StageKind::Leaf(leaf) => serde_json::json!({"leaf": leaf.text}),
        StageKind::Scatter => serde_json::json!("scatter"),
        StageKind::Collect => serde_json::json!("collect"),
        StageKind::Ward { predicate } => serde_json::json!({"ward": predicate.text}),
        StageKind::Fork { branches } => serde_json::json!({
            "fork": branches.iter().map(flow_structure).collect::<Vec<_>>(),
        }),
        StageKind::Orbit { body } => serde_json::json!({"orbit": flow_structure(body)}),
    };
    if stage.modifiers.is_empty() {
        return kind;
    }
    serde_json::json!({
        "kind": kind,
        "modifiers": stage.modifiers.iter()
            .map(|m| serde_json::json!({"name": m.name, "value": m.value.text}))
            .collect::<Vec<_>>(),
    })
}
