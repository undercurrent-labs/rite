//! Lexer, parser, AST, and operator manifest for Cant.
//!
//! Cant is a sibling front end to Rite, not a Rite dialect: it has its own
//! composition semantics (zero-or-more emissions, scatter, collect, ward, fork,
//! bounded orbit) and therefore its own syntax tree. It reuses Rite's spans,
//! source files, labels and diagnostic rendering, and nothing else — see
//! `docs/adr/0001-cant-sibling-frontend.md`.
//!
//! ```
//! use rite_core::{FileId, SourceFile};
//! let file = SourceFile::new(FileId(0), "demo.cant", "[1, 2, 3] -> * -> square -> []");
//! let parsed = cant_syntax::parse(&file);
//! assert!(!parsed.has_errors());
//! assert_eq!(parsed.program.unwrap().flow.stages.len(), 4);
//! ```

pub mod ast;
pub mod diagnostic;
pub mod fmt;
pub mod lexer;
pub mod manifest;
pub mod parser;
pub mod token;

pub use ast::{structure, CantProgramAst, Flow, Leaf, Modifier, Stage, StageKind};
pub use diagnostic::{
    CantCategory, CantCode, CantDiagnostic, CantDiagnostics, RiteOrigin, ALL_CODES,
};
pub use fmt::{convert, detect, format, FormatError, FormatOptions, FormatResult};
pub use lexer::{lex, span_of, Lexer};
pub use manifest::{manifest, OperatorManifest, OperatorSpec};
pub use parser::{parse, ParseResult, StructuralToken, MAX_NESTING};
pub use token::{CantToken, CantTokenKind, Spelling};

use rite_core::{FileId, SourceFile, SourceMap};

/// The Cant language version this crate implements.
///
/// Separate from the crate version: the tooling can move without the language
/// doing so, and `cant version` reports both.
pub const CANT_LANGUAGE_VERSION: &str = "0";

/// Parse a named source string, returning the result alongside a [`SourceMap`]
/// that can render its diagnostics.
pub fn parse_source(name: &str, text: &str) -> (ParseResult, SourceMap) {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, text);
    let file = sources.get(id).expect("just added").clone();
    (parse(&file), sources)
}

/// Which spelling a Cant source is written in.
///
/// Deliberately *not* `rite_fmt::Dialect`: that enum means "which spelling of
/// Rite", its `Mixed` and `Preserve` variants are about Rite's formatter, and
/// adding Cant to it is prohibited by ADR 0001.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    /// The canonical, typeable form.
    #[default]
    Ascii,
    /// The presentation form. Accepted on input; never required.
    Glyph,
}

/// Report which spelling a source predominantly uses.
///
/// Counts structural tokens only, so a Rite glyph inside a leaf does not make a
/// program "glyph Cant".
pub fn detect_dialect(text: &str) -> Dialect {
    let file = SourceFile::new(FileId(0), "detect.cant", text);
    let (tokens, _) = lex(&file);
    let glyphs = tokens
        .iter()
        .filter(|t| t.spelling == Spelling::Glyph)
        .count();
    if glyphs > 0 {
        Dialect::Glyph
    } else {
        Dialect::Ascii
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_ascii_until_a_structural_glyph_appears() {
        assert_eq!(detect_dialect("a -> b"), Dialect::Ascii);
        assert_eq!(detect_dialect("a → b"), Dialect::Glyph);
        assert_eq!(detect_dialect(""), Dialect::Ascii);
    }

    #[test]
    fn parse_source_gives_back_a_renderable_source_map() {
        let (result, sources) = parse_source("t.cant", "-> f");
        assert!(result.has_errors());
        let rendered = result.diagnostics.render_all(&sources);
        assert!(rendered.contains("t.cant"), "{rendered}");
        assert!(rendered.contains("CANT-P002"), "{rendered}");
    }
}
