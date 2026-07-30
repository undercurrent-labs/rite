use crate::error_codes::ErrorCode;
use crate::source::SourceMap;
use crate::span::{SourceSpan, Span};
use serde::{Deserialize, Serialize};
use std::fmt;

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
        let mut out = String::new();
        out.push_str(&format!(
            "{}[{}]: {}\n",
            self.severity, self.code, self.title
        ));

        for label in &self.labels {
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

                let col = lc.column as usize;
                let span_len = label.span.span.len().max(1);
                let marker_start = col.saturating_sub(1);
                let mut underline = String::new();
                underline.push_str(&" ".repeat(marker_start));
                if label.primary {
                    underline.push_str(
                        &"^".repeat(
                            span_len.min(line_text.len().saturating_sub(marker_start).max(1)),
                        ),
                    );
                } else {
                    underline.push_str(
                        &"-".repeat(
                            span_len.min(line_text.len().saturating_sub(marker_start).max(1)),
                        ),
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

        for note in &self.notes {
            out.push_str(&format!("\nnote: {}\n", note));
        }

        if let Some(help) = &self.help {
            out.push_str(&format!("\nhelp: {}\n", help));
        }

        out
    }
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
