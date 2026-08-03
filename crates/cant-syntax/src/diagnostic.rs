//! Cant diagnostics.
//!
//! Cant has its own code namespace (`CANT-L001`, `CANT-P004`, …) because it is a
//! separate language, not a Rite dialect — see
//! `docs/adr/0001-cant-sibling-frontend.md`. Everything else is Rite's:
//! [`rite_core::Label`] for spans and messages, [`rite_core::Severity`], and
//! [`rite_core::render_snippet`] for the caret-underlined excerpt, so a Cant
//! error looks like a Rite error and lines up the same way over glyphs.
//!
//! A diagnostic that originated in generated Rite carries where it came from in
//! [`CantDiagnostic::rite`]. The primary label still points at `.cant` source;
//! the Rite code and generated span are related metadata, never the headline.

use rite_core::{render_snippet, Label, Severity, SourceMap, SourceSpan};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The letter group a Cant code belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CantCategory {
    /// `CANT-Lxxx` — lexical.
    Lex,
    /// `CANT-Pxxx` — parser.
    Parse,
    /// `CANT-Gxxx` — graph validation.
    Graph,
    /// `CANT-Sxxx` — semantic.
    Semantic,
    /// `CANT-Oxxx` — orbit.
    Orbit,
    /// `CANT-Xxxx` — expansion / lowering.
    Expand,
    /// `CANT-Rxxx` — runtime, including diagnostics remapped from Rite.
    Runtime,
    /// `CANT-Vxxx` — version.
    Version,
}

impl CantCategory {
    pub fn letter(self) -> char {
        match self {
            CantCategory::Lex => 'L',
            CantCategory::Parse => 'P',
            CantCategory::Graph => 'G',
            CantCategory::Semantic => 'S',
            CantCategory::Orbit => 'O',
            CantCategory::Expand => 'X',
            CantCategory::Runtime => 'R',
            CantCategory::Version => 'V',
        }
    }

    /// The process exit status a rejection in this category ends with.
    ///
    /// Aligned with Rite's published contract (0 success, 1 runtime, 2 usage, 3
    /// parse, 4 resolve, 5 permission, 6 compile, 7 test, 8 budget) rather than
    /// invented: a script rejected for a syntax error should exit 3 whichever
    /// language wrote it. The mapping is fixed in `docs/cant/internals.md`.
    pub fn exit_code(self) -> u8 {
        match self {
            CantCategory::Lex | CantCategory::Parse => 3,
            CantCategory::Graph | CantCategory::Semantic | CantCategory::Expand => 4,
            CantCategory::Orbit => 8,
            CantCategory::Runtime => 1,
            CantCategory::Version => 2,
        }
    }
}

/// A stable Cant diagnostic code, rendered as `CANT-P004`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CantCode {
    pub category: CantCategory,
    pub number: u16,
}

impl CantCode {
    pub const fn new(category: CantCategory, number: u16) -> Self {
        Self { category, number }
    }

    pub fn exit_code(self) -> u8 {
        self.category.exit_code()
    }
}

impl fmt::Display for CantCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CANT-{}{:03}", self.category.letter(), self.number)
    }
}

macro_rules! codes {
    ($($name:ident = ($cat:ident, $n:literal, $doc:literal);)*) => {
        $(
            #[doc = $doc]
            pub const $name: CantCode = CantCode::new(CantCategory::$cat, $n);
        )*
        /// Every code this crate can emit, for the documentation generator and
        /// for a test that no two share a number.
        pub const ALL_CODES: &[(CantCode, &str)] = &[$(($name, $doc)),*];
    };
}

codes! {
    CANT_L001_UNEXPECTED_CHARACTER = (Lex, 1, "A character that cannot begin any Cant token.");
    CANT_L002_UNTERMINATED_STRING = (Lex, 2, "A string literal reached the end of the source unclosed.");
    CANT_L003_UNTERMINATED_COMMENT = (Lex, 3, "A `/* … */` comment reached the end of the source unclosed.");

    CANT_P001_EMPTY_PROGRAM = (Parse, 1, "The source contains no stages.");
    CANT_P002_EXPECTED_STAGE = (Parse, 2, "A flow arrow, separator, or block is not followed by a stage.");
    CANT_P003_UNCLOSED_BLOCK = (Parse, 3, "A ward, fork, or orbit was never closed.");
    CANT_P004_UNEXPECTED_BLOCK_CLOSE = (Parse, 4, "A block close with no matching ward, fork, or orbit.");
    CANT_P005_TRAILING_FLOW = (Parse, 5, "A flow arrow with nothing after it.");
    CANT_P006_UNEXPECTED_SEPARATOR = (Parse, 6, "A `;` outside a fork.");
    CANT_P007_GLYPH_ONLY_OPERATOR_IN_LEAF = (Parse, 7, "A glyph-only operator (`⋇`, `⌁`) used inside a leaf expression.");
    CANT_P008_MODIFIER_WITHOUT_FORM = (Parse, 8, "A `:name value` modifier that follows no structural form.");
    CANT_P009_MODIFIER_NEEDS_NAME = (Parse, 9, "A modifier `:` not followed by a name.");
    CANT_P010_MODIFIER_NEEDS_VALUE = (Parse, 10, "A modifier name not followed by a value.");
    CANT_P011_EMPTY_FORK_BRANCH = (Parse, 11, "A fork branch with no stages.");
    CANT_P012_WARD_IS_NOT_A_FLOW = (Parse, 12, "A ward predicate is one expression, not a flow.");
    CANT_P013_NESTING_TOO_DEEP = (Parse, 13, "Structural blocks nested past the supported depth.");

    CANT_G001_NO_ENTRY = (Graph, 1, "The graph has no entry node.");
    CANT_G002_DANGLING_EDGE = (Graph, 2, "An edge names a node that is not in the graph.");
    CANT_G003_INVALID_PORT = (Graph, 3, "An edge attaches to a port the node does not have.");
    CANT_G004_BRANCH_JOIN = (Graph, 4, "A fork branch does not rejoin the fork that opened it.");
    CANT_G005_SCATTER_HAS_NO_INPUT = (Graph, 5, "Scatter used where nothing has been emitted yet.");
    CANT_G006_COLLECT_HAS_NO_INPUT = (Graph, 6, "Collect used where nothing has been emitted yet.");
    CANT_G007_ORBIT_LIMIT = (Graph, 7, "An orbit `:max` that is not a positive integer.");
    CANT_G008_ORBIT_IDENTITY_EFFECTFUL = (Graph, 8, "An orbit `:by` function that performs an effect.");
    CANT_G009_UNSUPPORTED_CYCLE = (Graph, 9, "A cycle that is not owned by an orbit.");
    CANT_G010_UNKNOWN_MODIFIER = (Graph, 10, "A `:name` the form it is attached to does not accept.");
    CANT_G011_DUPLICATE_MODIFIER = (Graph, 11, "The same modifier given twice on one form.");
    CANT_G012_DUPLICATE_NODE_ID = (Graph, 12, "Two nodes in a deserialized graph share an identifier.");
    CANT_G013_UNREACHABLE_NODE = (Graph, 13, "A node no edge can reach from the entry.");
    CANT_G014_WARD_PREDICATE_EFFECTFUL = (Graph, 14, "A ward predicate that performs an effect.");
    CANT_G015_EMPTY_SUBGRAPH = (Graph, 15, "A fork branch or orbit body with no nodes.");

    CANT_S001_EFFECT_REQUIRED = (Semantic, 1, "A host call without a `!` marker, as Rite requires.");
    CANT_S002_UNDEFINED_NAME = (Semantic, 2, "A name that does not resolve.");
    CANT_S003_RITE_SEMANTIC = (Semantic, 3, "A Rite semantic error, remapped onto Cant source.");
    CANT_S004_LEAF_NOT_VALID_RITE = (Semantic, 4, "A leaf expression that Cant accepted but Rite cannot parse.");

    CANT_X001_GENERATED_RITE_INVALID = (Expand, 1, "Generated Rite that Rite's own parser rejected — always a bug in Cant.");

    CANT_O001_BUDGET_EXHAUSTED = (Orbit, 1, "One of Rite's global budgets — steps, time, collection or string size — was exhausted.");
    CANT_O002_ORBIT_LIMIT_REACHED = (Orbit, 2, "An orbit accepted its `:max` candidates and stopped.");

    CANT_R001_RITE_RUNTIME = (Runtime, 1, "A Rite runtime failure, remapped onto Cant source.");
    CANT_R002_PERMISSION_DENIED = (Runtime, 2, "A capability the run was not granted.");
    CANT_R003_SCATTER_NOT_A_LIST = (Runtime, 3, "Scatter applied to something that is not a list.");
}

/// Where in generated Rite a remapped diagnostic came from.
///
/// Populated when lowering lands (Phase 4). Present now because it is part of
/// the published JSON diagnostic shape, and a consumer should not have to handle
/// the field appearing later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiteOrigin {
    /// The underlying Rite code, e.g. `E021`.
    pub code: String,
    /// The span in the generated Rite this was reported at, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    /// The Rite diagnostic's own title, kept verbatim.
    pub title: String,
}

// No `PartialEq`: `rite_core::Label` does not implement it, and reaching into
// Rite to add a derive is not worth it — nothing compares two diagnostics, and a
// test that wants to should be asserting on the code and the span, not on the
// whole struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CantDiagnostic {
    pub code: CantCode,
    pub severity: Severity,
    pub title: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rite: Option<RiteOrigin>,
}

impl CantDiagnostic {
    pub fn error(code: CantCode, title: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            title: title.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
            rite: None,
        }
    }

    pub fn warning(code: CantCode, title: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code, title)
        }
    }

    pub fn with_primary(mut self, span: SourceSpan, message: impl Into<String>) -> Self {
        self.labels.push(Label::primary(span, message));
        self
    }

    pub fn with_secondary(mut self, span: SourceSpan, message: impl Into<String>) -> Self {
        self.labels.push(Label::secondary(span, message));
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_rite_origin(mut self, origin: RiteOrigin) -> Self {
        self.rite = Some(origin);
        self
    }

    pub fn primary_span(&self) -> Option<SourceSpan> {
        self.labels.iter().find(|l| l.primary).map(|l| l.span)
    }

    pub fn render(&self, sources: &SourceMap) -> String {
        render_snippet(
            &format!("{}[{}]: {}", self.severity, self.code, self.title),
            &self.labels,
            &self.notes,
            self.help.as_deref(),
            sources,
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        // `code` is a struct in Rust and a string on the wire: a consumer wants
        // `"CANT-P004"`, not `{"category":"parse","number":4}`.
        let mut v = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "code".into(),
                serde_json::Value::String(self.code.to_string()),
            );
        }
        v
    }
}

impl fmt::Display for CantDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.title)
    }
}

impl std::error::Error for CantDiagnostic {}

#[derive(Debug, Clone, Default)]
pub struct CantDiagnostics {
    items: Vec<CantDiagnostic>,
}

impl CantDiagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: CantDiagnostic) {
        self.items.push(d);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = CantDiagnostic>) {
        self.items.extend(other);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &CantDiagnostic> {
        self.items.iter()
    }

    pub fn errors(&self) -> impl Iterator<Item = &CantDiagnostic> {
        self.items.iter().filter(|d| d.severity == Severity::Error)
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn into_vec(self) -> Vec<CantDiagnostic> {
        self.items
    }

    /// The exit status a rejection ends with, taken from the **first** error.
    ///
    /// Diagnostics accumulate in phase order — lex, then parse, then graph, then
    /// expansion — so the first one is the earliest thing that went wrong, and
    /// that is what a caller acting on the status wants to know. Ranking by
    /// numeric exit code instead would report a later runtime failure (1) over
    /// the syntax error (3) that caused it. `0` when there are no errors, since
    /// a caller only asks this about a rejection.
    pub fn rejection_exit_code(&self) -> u8 {
        self.errors()
            .next()
            .map(|d| d.code.exit_code())
            .unwrap_or(0)
    }

    pub fn render_all(&self, sources: &SourceMap) -> String {
        self.items
            .iter()
            .map(|d| d.render(sources))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Array(self.items.iter().map(|d| d.to_json()).collect())
    }
}

impl IntoIterator for CantDiagnostics {
    type Item = CantDiagnostic;
    type IntoIter = std::vec::IntoIter<CantDiagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_render_in_the_stable_cant_form() {
        assert_eq!(CANT_L001_UNEXPECTED_CHARACTER.to_string(), "CANT-L001");
        assert_eq!(CANT_P013_NESTING_TOO_DEEP.to_string(), "CANT-P013");
        assert_eq!(
            CantCode::new(CantCategory::Orbit, 7).to_string(),
            "CANT-O007"
        );
    }

    #[test]
    fn no_two_codes_collide() {
        let mut seen = std::collections::HashSet::new();
        for (code, _) in ALL_CODES {
            assert!(seen.insert(code.to_string()), "duplicate code {code}");
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }

    #[test]
    fn categories_map_onto_rites_exit_contract() {
        assert_eq!(CANT_L001_UNEXPECTED_CHARACTER.exit_code(), 3);
        assert_eq!(CANT_P001_EMPTY_PROGRAM.exit_code(), 3);
        assert_eq!(CantCategory::Graph.exit_code(), 4);
        assert_eq!(CantCategory::Orbit.exit_code(), 8);
        assert_eq!(CantCategory::Runtime.exit_code(), 1);
    }

    #[test]
    fn json_renders_the_code_as_a_string() {
        let d = CantDiagnostic::error(CANT_P004_UNEXPECTED_BLOCK_CLOSE, "unexpected `}`");
        let j = d.to_json();
        assert_eq!(j["code"], serde_json::json!("CANT-P004"));
        assert_eq!(j["severity"], serde_json::json!("error"));
        assert!(j.get("rite").is_none(), "absent origin should be omitted");
    }

    #[test]
    fn an_empty_collection_has_no_rejection() {
        assert_eq!(CantDiagnostics::new().rejection_exit_code(), 0);
    }
}
