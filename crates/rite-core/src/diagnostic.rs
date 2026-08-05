use crate::error_codes::ErrorCode;
use crate::source::SourceMap;
use crate::span::{SourceSpan, Span};
use serde::{Deserialize, Serialize};
use std::fmt;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
            Severity::Help => write!(f, "help"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub span: SourceSpan,
    pub message: String,
    pub primary: bool,
}

impl Label {
    pub fn primary(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            primary: true,
        }
    }

    pub fn secondary(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            primary: false,
        }
    }
}

/// Structured diagnostic with stable code and source labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: ErrorCode,
    pub severity: Severity,
    pub title: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: ErrorCode, title: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            title: title.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn warning(code: ErrorCode, title: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            title: title.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: None,
        }
    }

    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
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

    pub fn primary_span(&self) -> Option<SourceSpan> {
        self.labels.iter().find(|l| l.primary).map(|l| l.span)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Render a human-readable diagnostic with source excerpts.
    pub fn render(&self, sources: &SourceMap) -> String {
        render_snippet(
            &format!("{}[{}]: {}", self.severity, self.code, self.title),
            &self.labels,
            &self.notes,
            self.help.as_deref(),
            sources,
        )
    }
}

/// Render a labelled source excerpt under a header line.
///
/// This is [`Diagnostic::render`] with the header lifted out. Everything below
/// the header — resolving each label's file, sizing the caret by *display* width
/// rather than byte count, printing the excerpt, then the notes and the help — is
/// independent of what kind of code the diagnostic carries, and a tool that has
/// spans and labels but not a [`ErrorCode`] had no way to reach it.
///
/// `header` is the entire first line, without its newline: callers own their own
/// code namespace. Rite passes `"error[E021]: …"`; a tool with codes of its own
/// passes whatever it uses.
pub fn render_snippet(
    header: &str,
    labels: &[Label],
    notes: &[String],
    help: Option<&str>,
    sources: &SourceMap,
) -> String {
    let mut out = String::new();
    out.push_str(header);
    out.push('\n');

    for label in labels {
        if let Some(file) = sources.get(label.span.file) {
            let lc = file.line_col(label.span.span.start);
            let line_text = file.line_text(lc.line).unwrap_or("");
            let path = file
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| file.name.clone());

            out.push_str(&format!("\n  --> {}:{}:{}\n", path, lc.line, lc.column));
            out.push_str("   |\n");
            out.push_str(&format!("{:4} | {}\n", lc.line, line_text));

            // Pad by the *display width* of the text before the span, so the caret
            // lines up under a proportional-width glyph as well as under ASCII.
            // `lc.column` counts characters; a wide character (CJK, some symbols)
            // occupies two terminal cells, which only `unicode-width` knows.
            let chars_before = (lc.column as usize).saturating_sub(1);
            let prefix: String = line_text.chars().take(chars_before).collect();
            let marker_start = UnicodeWidthStr::width(prefix.as_str());
            // The span is a byte range; the caret length is how many characters of
            // this line it actually covers.
            let span_chars = line_text
                .get(prefix.len()..)
                .map(|rest| {
                    rest.char_indices()
                        .take_while(|(i, _)| *i < label.span.span.len().max(1))
                        .count()
                })
                .unwrap_or(1)
                .max(1);
            let line_chars = line_text.chars().count();
            let mut underline = String::new();
            underline.push_str(&" ".repeat(marker_start));
            if label.primary {
                underline.push_str(
                    &"^".repeat(span_chars.min(line_chars.saturating_sub(chars_before).max(1))),
                );
            } else {
                underline.push_str(
                    &"-".repeat(span_chars.min(line_chars.saturating_sub(chars_before).max(1))),
                );
            }
            out.push_str(&format!("   | {}\n", underline));
            if !label.message.is_empty() {
                out.push_str(&format!(
                    "   | {} {}\n",
                    " ".repeat(marker_start),
                    label.message
                ));
            }
        } else if !label.span.span.is_dummy() {
            out.push_str(&format!("  at {}\n", label.span.span));
        }
    }

    for note in notes {
        out.push_str(&format!("\nnote: {}\n", note));
    }

    if let Some(help) = help {
        out.push_str(&format!("\nhelp: {}\n", help));
    }

    out
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.severity, self.code, self.title)
    }
}

impl std::error::Error for Diagnostic {}

/// Convenience: build error with a simple span in one file.
pub fn simple_error(
    code: ErrorCode,
    title: impl Into<String>,
    file: crate::span::FileId,
    span: Span,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(code, title).with_primary(SourceSpan::new(file, span), message)
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::error_codes::E021_EFFECT_REQUIRED;
    use crate::source::SourceMap;
    use crate::span::{SourceSpan, Span};

    /// Pins the rendered form so lifting the body into [`render_snippet`] cannot
    /// have changed a caret, a column, or a blank line.
    #[test]
    fn renders_header_excerpt_caret_note_and_help() {
        let mut sources = SourceMap::new();
        let id = sources.add_file("t.rite", "def f() [[\n  @fs.read(\"x\")\n]]\n");
        let d = Diagnostic::error(E021_EFFECT_REQUIRED, "effect marker required")
            .with_primary(SourceSpan::new(id, Span::from_range(13, 16)), "add `!`")
            .with_note("reads are effects")
            .with_help("write `! @fs.read(\"x\")`");
        assert_eq!(
            d.render(&sources),
            concat!(
                "error[E021]: effect marker required\n",
                "\n  --> t.rite:2:3\n",
                "   |\n",
                "   2 |   @fs.read(\"x\")\n",
                "   |   ^^^\n",
                "   |    add `!`\n",
                "\nnote: reads are effects\n",
                "\nhelp: write `! @fs.read(\"x\")`\n",
            )
        );
    }

    /// The header is the caller's, so a namespace Rite does not own renders too.
    #[test]
    fn render_snippet_takes_any_header() {
        let mut sources = SourceMap::new();
        let id = sources.add_file("t.txt", "alpha beta\n");
        let labels = vec![Label::primary(
            SourceSpan::new(id, Span::from_range(6, 10)),
            "here",
        )];
        let out = render_snippet("error[LINT-0042]: no", &labels, &[], None, &sources);
        assert!(out.starts_with("error[LINT-0042]: no\n"));
        assert!(out.contains("   |       ^^^^\n"), "{out}");
    }
}
