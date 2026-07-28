use serde::{Deserialize, Serialize};
use std::fmt;

/// Byte offset into a source file (UTF-8 byte index).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BytePos(pub u32);

impl BytePos {
    pub const ZERO: Self = Self(0);

    pub fn new(pos: usize) -> Self {
        Self(pos as u32)
    }

    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for BytePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Half-open byte range `[start, end)` in a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: BytePos,
    pub end: BytePos,
}

impl Span {
    pub const DUMMY: Self = Self {
        start: BytePos(0),
        end: BytePos(0),
    };

    pub fn new(start: BytePos, end: BytePos) -> Self {
        Self { start, end }
    }

    pub fn from_range(start: usize, end: usize) -> Self {
        Self {
            start: BytePos::new(start),
            end: BytePos::new(end),
        }
    }

    pub fn len(self) -> usize {
        self.end.as_usize().saturating_sub(self.start.as_usize())
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn is_dummy(self) -> bool {
        self == Self::DUMMY
    }

    pub fn merge(self, other: Span) -> Span {
        if self.is_dummy() {
            return other;
        }
        if other.is_dummy() {
            return self;
        }
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn contains(self, pos: BytePos) -> bool {
        pos >= self.start && pos < self.end
    }

    pub fn contains_span(self, other: Span) -> bool {
        other.start >= self.start && other.end <= self.end
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// 1-based line and column (column is UTF-8 byte offset within the line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for LineCol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Source file identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

impl FileId {
    pub const DUMMY: Self = Self(u32::MAX);
}

/// Span with file identity for multi-file diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: FileId,
    pub span: Span,
}

impl SourceSpan {
    pub fn new(file: FileId, span: Span) -> Self {
        Self { file, span }
    }

    pub fn dummy() -> Self {
        Self {
            file: FileId::DUMMY,
            span: Span::DUMMY,
        }
    }

    pub fn merge(self, other: SourceSpan) -> SourceSpan {
        debug_assert_eq!(self.file, other.file);
        SourceSpan {
            file: self.file,
            span: self.span.merge(other.span),
        }
    }
}
