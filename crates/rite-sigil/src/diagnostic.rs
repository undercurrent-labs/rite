//! Sigil diagnostics.
//!
//! Sigil has its own code namespace (`SIGIL-G001`, `SIGIL-S004`, …) for the same
//! reason Cant has one: it is a separate tool with its own failure modes, and
//! folding them into Rite's `E0xx` space would make Rite's error table describe
//! things Rite cannot do. The shape is deliberately the same as
//! `cant_syntax::diagnostic` so that a Sigil error reads like a Cant error reads
//! like a Rite error.
//!
//! Everything except the code is borrowed: [`rite_core::Span`] for source
//! positions, [`rite_core::Severity`] for how bad it is.
//!
//! A Sigil diagnostic points at two things at once and both matter. The **graph
//! reference** — a node, edge or region ID — is what makes a message actionable
//! about a graph read from JSON, where there is no source text at all. The
//! **span** points into the original program, when the graph carried one. A
//! renderer that could only say "something is wrong somewhere in this picture"
//! would be useless, so a diagnostic that has neither is a bug.

use rite_core::{Severity, Span};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The letter group a Sigil code belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SigilCategory {
    /// `SIGIL-Gxxx` — graph structure and schema.
    Graph,
    /// `SIGIL-Lxxx` — layout.
    Layout,
    /// `SIGIL-Mxxx` — mark generation.
    Mark,
    /// `SIGIL-Rxxx` — rendering and serialization.
    Render,
    /// `SIGIL-Txxx` — theme.
    Theme,
    /// `SIGIL-Wxxx` — wasm / browser boundary.
    Wasm,
    /// `SIGIL-Cxxx` — CLI and configuration.
    Cli,
    /// `SIGIL-Sxxx` — security and input validation.
    Security,
    /// `SIGIL-Vxxx` — version compatibility.
    Version,
}

impl SigilCategory {
    pub fn letter(self) -> char {
        match self {
            SigilCategory::Graph => 'G',
            SigilCategory::Layout => 'L',
            SigilCategory::Mark => 'M',
            SigilCategory::Render => 'R',
            SigilCategory::Theme => 'T',
            SigilCategory::Wasm => 'W',
            SigilCategory::Cli => 'C',
            SigilCategory::Security => 'S',
            SigilCategory::Version => 'V',
        }
    }

    /// The process exit status a rejection in this category ends with.
    ///
    /// Taken from Rite's published contract (0 success, 1 runtime, 2 usage, 3
    /// parse, 4 resolve, 5 permission, 6 compile, 7 test, 8 budget) rather than
    /// invented, on the same reasoning Cant used: a caller scripting `cant sigil`
    /// should not have to learn a third table. A malformed graph is a resolve-class
    /// failure (4) because that is what "your input does not hang together" means
    /// here; a bad flag is usage (2); a graph over the node cap is budget (8).
    pub fn exit_code(self) -> u8 {
        match self {
            SigilCategory::Graph | SigilCategory::Security => 4,
            SigilCategory::Cli | SigilCategory::Version | SigilCategory::Theme => 2,
            SigilCategory::Layout | SigilCategory::Mark | SigilCategory::Render => 1,
            SigilCategory::Wasm => 1,
        }
    }
}

/// A stable Sigil diagnostic code, rendered as `SIGIL-G004`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SigilCode {
    pub category: SigilCategory,
    pub number: u16,
}

impl SigilCode {
    pub const fn new(category: SigilCategory, number: u16) -> Self {
        Self { category, number }
    }

    pub fn exit_code(self) -> u8 {
        self.category.exit_code()
    }
}

impl fmt::Display for SigilCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SIGIL-{}{:03}", self.category.letter(), self.number)
    }
}

macro_rules! codes {
    ($($name:ident = ($cat:ident, $n:literal, $doc:literal);)*) => {
        $(
            #[doc = $doc]
            pub const $name: SigilCode = SigilCode::new(SigilCategory::$cat, $n);
        )*
        /// Every code this crate can emit, for the documentation generator and
        /// for the test that no two share a number within a category.
        pub const ALL_CODES: &[(SigilCode, &str)] = &[$(($name, $doc)),*];
    };
}

codes! {
    // Graph structure and schema. Everything that makes a graph unusable as a
    // graph, before any geometry is considered.
    SIGIL_G001_NO_ENTRY = (Graph, 1, "The graph names no entry node, so there is no centre to draw from.");
    SIGIL_G002_UNKNOWN_NODE = (Graph, 2, "An edge, region, entry or exit names a node that is not in the graph.");
    SIGIL_G003_DUPLICATE_ID = (Graph, 3, "Two nodes, edges or regions share an identifier.");
    SIGIL_G004_UNKNOWN_REGION = (Graph, 4, "An edge or region parent names a region that is not in the graph.");
    SIGIL_G005_REGION_CYCLE = (Graph, 5, "Region parenthood forms a cycle, so nesting has no outermost ring.");
    SIGIL_G006_UNREACHABLE_NODE = (Graph, 6, "A node cannot be reached from the entry; it is drawn detached and reported.");
    SIGIL_G007_UNKNOWN_NODE_KIND = (Graph, 7, "A node kind this renderer version does not know; drawn with the unknown mark.");
    SIGIL_G008_NO_EXIT = (Graph, 8, "The graph names no exit node, so the composition has no closing seal.");
    SIGIL_G009_EMPTY_GRAPH = (Graph, 9, "The graph contains no nodes.");
    SIGIL_G010_DUPLICATE_REGION_MEMBER = (Graph, 10, "A node is claimed as a member by more than one region.");

    // Security and input validation. Limits, and anything a hostile input does.
    SIGIL_S001_TOO_MANY_NODES = (Security, 1, "The graph exceeds the hard node cap.");
    SIGIL_S002_TOO_MANY_EDGES = (Security, 2, "The graph exceeds the hard edge cap.");
    SIGIL_S003_NESTING_TOO_DEEP = (Security, 3, "Region nesting exceeds the maximum depth.");
    SIGIL_S004_LABEL_TOO_LONG = (Security, 4, "A label exceeds the maximum length and was truncated.");
    SIGIL_S005_INPUT_TOO_LARGE = (Security, 5, "The serialized input exceeds the maximum accepted size.");
    SIGIL_S006_NON_FINITE_NUMBER = (Security, 6, "A numeric value in the input is not finite.");
    SIGIL_S007_LARGE_GRAPH = (Security, 7, "The graph is past the size where a sigil stays legible; consider `--simplify`.");
    SIGIL_S008_MALFORMED_SPAN = (Security, 8, "A source span ends before it starts, or runs past the source length.");
    SIGIL_S009_ID_NOT_REPRESENTABLE = (Security, 9, "An identifier contains characters that cannot be made into a stable element ID.");

    // Version compatibility.
    SIGIL_V001_UNSUPPORTED_GRAPH_SCHEMA = (Version, 1, "The input names a graph schema this renderer does not read.");
    SIGIL_V002_UNSUPPORTED_SCHEMA_VERSION = (Version, 2, "The input names a major schema version this renderer does not read.");
    SIGIL_V003_NEWER_MINOR_SCHEMA = (Version, 3, "The input is a newer minor version; unknown fields were ignored.");
}

/// What a diagnostic is about, when there is no source text to point at.
///
/// A graph read from JSON has no `.cant` file behind it, so "line 4" is not
/// available and "node `n7`" is the only thing that identifies the problem. Both
/// are carried when both exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum GraphRef {
    Node(String),
    Edge(String),
    Region(String),
    /// The graph as a whole — a schema or limit complaint.
    Graph,
}

/// One thing that went wrong, or one thing worth saying.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigilDiagnostic {
    /// Rendered as the string `"SIGIL-G004"`, not as a structure.
    #[serde(
        serialize_with = "serialize_code",
        deserialize_with = "deserialize_code"
    )]
    pub code: SigilCode,
    pub severity: Severity,
    pub message: String,
    /// What in the graph this is about. Always present — see the module docs.
    pub graph_ref: GraphRef,
    /// Where in the original source, when the graph carried a span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub notes: Vec<String>,
}

fn serialize_code<S: serde::Serializer>(code: &SigilCode, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&code.to_string())
}

fn deserialize_code<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SigilCode, D::Error> {
    use serde::de::Error;
    let text = String::deserialize(d)?;
    parse_code(&text).ok_or_else(|| D::Error::custom(format!("not a Sigil code: {text}")))
}

/// `"SIGIL-G004"` back into a code. Only used when reading a diagnostic someone
/// else serialized, which is why an unknown letter is an error rather than a
/// guess.
pub fn parse_code(text: &str) -> Option<SigilCode> {
    let rest = text.strip_prefix("SIGIL-")?;
    let mut chars = rest.chars();
    let letter = chars.next()?;
    let number: u16 = chars.as_str().parse().ok()?;
    let category = match letter {
        'G' => SigilCategory::Graph,
        'L' => SigilCategory::Layout,
        'M' => SigilCategory::Mark,
        'R' => SigilCategory::Render,
        'T' => SigilCategory::Theme,
        'W' => SigilCategory::Wasm,
        'C' => SigilCategory::Cli,
        'S' => SigilCategory::Security,
        'V' => SigilCategory::Version,
        _ => return None,
    };
    Some(SigilCode::new(category, number))
}

impl SigilDiagnostic {
    pub fn error(code: SigilCode, graph_ref: GraphRef, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            graph_ref,
            span: None,
            notes: Vec::new(),
        }
    }

    pub fn warning(code: SigilCode, graph_ref: GraphRef, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            message: message.into(),
            graph_ref,
            span: None,
            notes: Vec::new(),
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for SigilDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.message)?;
        match &self.graph_ref {
            GraphRef::Node(id) => write!(f, " (node `{id}`)")?,
            GraphRef::Edge(id) => write!(f, " (edge `{id}`)")?,
            GraphRef::Region(id) => write!(f, " (region `{id}`)")?,
            GraphRef::Graph => {}
        }
        Ok(())
    }
}

/// A run of diagnostics, in the order they were produced.
///
/// Order is production order rather than sorted, deliberately: validation walks
/// the graph in identifier order, so the output is already stable, and re-sorting
/// would separate a complaint from the note that explains it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Diagnostics(pub Vec<SigilDiagnostic>);

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: SigilDiagnostic) {
        self.0.push(diagnostic);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.0.extend(other.0);
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(SigilDiagnostic::is_error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &SigilDiagnostic> {
        self.0.iter().filter(|d| d.is_error())
    }

    pub fn warnings(&self) -> impl Iterator<Item = &SigilDiagnostic> {
        self.0.iter().filter(|d| !d.is_error())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, SigilDiagnostic> {
        self.0.iter()
    }

    /// The exit status a CLI should end with: the worst category present, or 0.
    pub fn exit_code(&self) -> u8 {
        self.errors().map(|d| d.code.exit_code()).max().unwrap_or(0)
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a SigilDiagnostic;
    type IntoIter = std::slice::Iter<'a, SigilDiagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{d}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn codes_render_in_the_documented_form() {
        assert_eq!(SIGIL_G001_NO_ENTRY.to_string(), "SIGIL-G001");
        assert_eq!(SIGIL_S007_LARGE_GRAPH.to_string(), "SIGIL-S007");
        assert_eq!(
            SIGIL_V002_UNSUPPORTED_SCHEMA_VERSION.to_string(),
            "SIGIL-V002"
        );
    }

    /// A duplicate number means two different failures rendering as the same
    /// code, which makes the code useless for the thing codes are for.
    #[test]
    fn no_two_codes_collide() {
        let mut seen = BTreeSet::new();
        for (code, _) in ALL_CODES {
            assert!(
                seen.insert((code.category, code.number)),
                "duplicate code {code}"
            );
        }
        assert_eq!(seen.len(), ALL_CODES.len());
    }

    #[test]
    fn every_code_has_documentation() {
        for (code, doc) in ALL_CODES {
            assert!(!doc.trim().is_empty(), "{code} has no documentation");
            assert!(
                doc.trim_end().ends_with('.'),
                "{code}'s documentation is not a sentence: {doc:?}"
            );
        }
    }

    #[test]
    fn codes_round_trip_through_their_rendered_form() {
        for (code, _) in ALL_CODES {
            assert_eq!(parse_code(&code.to_string()), Some(*code));
        }
        assert_eq!(parse_code("E001"), None);
        assert_eq!(parse_code("SIGIL-Z001"), None);
        assert_eq!(parse_code("CANT-G001"), None);
    }

    /// A diagnostic serialized for a machine carries the string, not a struct —
    /// a consumer greps `"SIGIL-G002"` and would not find `{"category":"graph"}`.
    #[test]
    fn a_serialized_diagnostic_carries_the_rendered_code() {
        let d = SigilDiagnostic::error(
            SIGIL_G002_UNKNOWN_NODE,
            GraphRef::Edge("e3".into()),
            "edge names a node that is not here",
        );
        let json = serde_json::to_value(&d).expect("serializes");
        assert_eq!(json["code"], serde_json::json!("SIGIL-G002"));
        assert_eq!(json["graph_ref"]["kind"], serde_json::json!("edge"));
        assert_eq!(json["graph_ref"]["id"], serde_json::json!("e3"));
        let back: SigilDiagnostic = serde_json::from_value(json).expect("round trips");
        assert_eq!(back, d);
    }

    #[test]
    fn exit_status_is_the_worst_error_present() {
        let mut d = Diagnostics::new();
        assert_eq!(d.exit_code(), 0);
        d.push(SigilDiagnostic::warning(
            SIGIL_S007_LARGE_GRAPH,
            GraphRef::Graph,
            "large",
        ));
        assert_eq!(d.exit_code(), 0, "a warning is not a failure");
        d.push(SigilDiagnostic::error(
            SIGIL_G001_NO_ENTRY,
            GraphRef::Graph,
            "no entry",
        ));
        assert_eq!(d.exit_code(), 4);
        assert!(d.has_errors());
    }
}
