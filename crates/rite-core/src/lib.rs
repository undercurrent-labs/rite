//! Core source model, spans, and diagnostics for Rite.

mod diagnostic;
mod error_codes;
mod source;
mod span;

pub use diagnostic::*;
pub use error_codes::*;
pub use source::*;
pub use span::*;

/// Result type used across Rite front-end crates.
pub type RiteResult<T> = Result<T, Diagnostic>;

/// Collection of diagnostics that can still allow partial success.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(other);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter()
    }

    /// Drop everything recorded after `len`.
    ///
    /// For speculative parses: a parser that tries one interpretation, fails, and
    /// re-parses the same tokens another way must not leave the abandoned attempt's
    /// complaints behind. Rewinding the token position is not enough on its own.
    pub fn rewind(&mut self, len: usize) {
        self.items.truncate(len);
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.severity == Severity::Error)
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

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
