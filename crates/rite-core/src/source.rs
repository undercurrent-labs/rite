use crate::span::{BytePos, FileId, LineCol, SourceSpan, Span};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A single source file.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: FileId,
    pub name: String,
    pub path: Option<PathBuf>,
    pub text: Arc<str>,
    line_starts: Vec<u32>,
}

impl SourceFile {
    pub fn new(id: FileId, name: impl Into<String>, text: impl Into<String>) -> Self {
        let text: Arc<str> = Arc::from(text.into());
        let line_starts = compute_line_starts(&text);
        Self {
            id,
            name: name.into(),
            path: None,
            text,
            line_starts,
        }
    }

    pub fn from_path(id: FileId, path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut file = Self::new(id, name, text);
        file.path = Some(path.to_path_buf());
        Ok(file)
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn slice(&self, span: Span) -> &str {
        let start = span.start.as_usize().min(self.text.len());
        let end = span.end.as_usize().min(self.text.len());
        &self.text[start..end]
    }

    pub fn line_col(&self, pos: BytePos) -> LineCol {
        let offset = pos.as_usize().min(self.text.len()) as u32;
        let line_idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line_idx];
        LineCol {
            line: (line_idx + 1) as u32,
            column: offset - line_start + 1,
        }
    }

    pub fn line_span(&self, line: u32) -> Option<Span> {
        let idx = (line as usize).checked_sub(1)?;
        if idx >= self.line_starts.len() {
            return None;
        }
        let start = self.line_starts[idx];
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.text.len() as u32);
        Some(Span::from_range(start as usize, end as usize))
    }

    pub fn line_text(&self, line: u32) -> Option<&str> {
        let span = self.line_span(line)?;
        let mut text = self.slice(span);
        if text.ends_with('\n') {
            text = &text[..text.len() - 1];
            if text.ends_with('\r') {
                text = &text[..text.len() - 1];
            }
        }
        Some(text)
    }

    pub fn source_span(&self, span: Span) -> SourceSpan {
        SourceSpan::new(self.id, span)
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}

fn compute_line_starts(text: &str) -> Vec<u32> {
    let mut starts = vec![0u32];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push((i + 1) as u32);
        }
    }
    starts
}

/// Map of all loaded source files.
#[derive(Debug, Default, Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(id, name, text));
        id
    }

    pub fn add_path(&mut self, path: impl AsRef<Path>) -> std::io::Result<FileId> {
        let id = FileId(self.files.len() as u32);
        let file = SourceFile::from_path(id, path)?;
        self.files.push(file);
        Ok(id)
    }

    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    pub fn get_mut(&mut self, id: FileId) -> Option<&mut SourceFile> {
        self.files.get_mut(id.0 as usize)
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn slice(&self, ss: SourceSpan) -> Option<&str> {
        self.get(ss.file).map(|f| f.slice(ss.span))
    }

    pub fn line_col(&self, ss: SourceSpan) -> Option<LineCol> {
        self.get(ss.file).map(|f| f.line_col(ss.span.start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_basic() {
        let f = SourceFile::new(FileId(0), "t.rite", "hello\nworld\n");
        assert_eq!(f.line_col(BytePos::new(0)).line, 1);
        assert_eq!(f.line_col(BytePos::new(6)).line, 2);
        assert_eq!(f.line_col(BytePos::new(6)).column, 1);
    }

    #[test]
    fn slice_span() {
        let f = SourceFile::new(FileId(0), "t.rite", "abc def");
        assert_eq!(f.slice(Span::from_range(0, 3)), "abc");
    }
}
