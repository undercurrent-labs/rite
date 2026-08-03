//! Cant lexer.
//!
//! Context-free and lossless: every byte of the source ends up in exactly one
//! token, trivia included, so concatenating [`CantToken::text`] in order
//! reproduces the input. That is what lets the formatter and the ASCII/glyph
//! converter work on tokens rather than on a regular expression, and it is why
//! a string or a comment containing `->` or `?{` survives untouched — the lexer
//! consumes it whole before any operator is considered.
//!
//! The lexer makes no structural decisions. `*` is a [`CantTokenKind::Star`]
//! whether it means scatter or multiplication; `:` is a
//! [`CantTokenKind::Colon`] whether it introduces a modifier or a Rite atom.
//! Both are resolved by the parser from position. See `docs/cant/internals.md`.

use crate::diagnostic::{
    CantDiagnostic, CantDiagnostics, CANT_L001_UNEXPECTED_CHARACTER, CANT_L002_UNTERMINATED_STRING,
    CANT_L003_UNTERMINATED_COMMENT,
};
use crate::token::{CantToken, CantTokenKind as K, Spelling};
use rite_core::{SourceFile, SourceSpan, Span};

/// Multi-character ASCII spellings of structural operators, longest first.
///
/// Order is load-bearing: `->` must be tried before `-`, `[]` before `[`, and
/// `?{` before `?`. Kept next to the manifest's ASCII column, and checked
/// against it by `manifest_sync`.
const ASCII_STRUCTURAL: &[(&str, K)] = &[
    ("->", K::Flow),
    ("?{", K::WardOpen),
    ("|{", K::ForkOpen),
    ("~{", K::OrbitOpen),
    ("[]", K::Collect),
];

/// Glyph spellings, longest first. `⊣⟦` is two characters and must beat `⊣`.
const GLYPH_STRUCTURAL: &[(&str, K)] = &[
    ("⊣⟦", K::WardOpen),
    ("⫴⟦", K::ForkOpen),
    ("⟲⟦", K::OrbitOpen),
    ("→", K::Flow),
    ("⋇", K::Star),
    ("⌁", K::Collect),
];

/// Glyph delimiters that belong to a *Rite* leaf, not to Cant.
///
/// `⟦ ⟧` are Rite's block delimiters and `⟨ ⟩` its record delimiters, so a leaf
/// may contain them. They are mapped onto the same depth-counting tokens as
/// `{` and `}`, which makes them harmless: a closer only ends a Cant block when
/// the parser sees it at leaf-depth zero, and inside a leaf these are always
/// balanced by their own opener.
const GLYPH_DELIMITERS: &[(&str, K)] = &[
    ("⟦", K::LBrace),
    ("⟧", K::BlockClose),
    ("⟨", K::LBrace),
    ("⟩", K::BlockClose),
];

/// Two-character Rite operators that must not be split.
///
/// `!=` is here so that `!` is only ever an effect marker; `??`, `:=` and `..`
/// so that `?`, `:` and `.` keep their Cant meanings elsewhere.
const RITE_OPERATORS: &[&str] = &[
    "...", "..=", "..", "<-", "<~", "<<", ">>", "??", ":=", "**", "!=", "==", "<=", ">=", "+=",
    "-=", "*=", "/=", "%=",
];

/// Single characters that continue a Rite operator token.
const OPERATOR_CHARS: &[char] = &[
    '+', '-', '/', '%', '=', '<', '>', '&', '^', '?', '~', '|', '#',
];

pub struct Lexer<'a> {
    file: &'a SourceFile,
    text: &'a str,
    pos: usize,
    diagnostics: CantDiagnostics,
}

/// Tokenize a Cant source file.
///
/// Always returns a complete token stream ending in [`CantTokenKind::Eof`],
/// however malformed the input: an unreadable character becomes a
/// [`CantTokenKind::Error`] token with a diagnostic and lexing continues, so a
/// caller always gets something to report positions against.
pub fn lex(file: &SourceFile) -> (Vec<CantToken>, CantDiagnostics) {
    let mut lexer = Lexer {
        file,
        text: file.as_str(),
        pos: 0,
        diagnostics: CantDiagnostics::new(),
    };
    let tokens = lexer.run();
    (tokens, lexer.diagnostics)
}

impl Lexer<'_> {
    fn run(&mut self) -> Vec<CantToken> {
        let mut out = Vec::new();
        if self.text.starts_with("#!") {
            let end = self.text.find('\n').unwrap_or(self.text.len());
            out.push(self.make(K::Shebang, 0, end, Spelling::Ascii));
            self.pos = end;
        }
        while self.pos < self.text.len() {
            let before = self.pos;
            let token = self.next_token();
            debug_assert!(
                self.pos > before,
                "lexer made no progress at byte {before} on {:?}",
                &self.text[before..]
            );
            out.push(token);
        }
        out.push(self.make(K::Eof, self.text.len(), self.text.len(), Spelling::Ascii));
        out
    }

    fn next_token(&mut self) -> CantToken {
        let start = self.pos;
        let rest = &self.text[start..];
        let c = rest.chars().next().expect("caller checked for input");

        // Newlines before other whitespace: the formatter needs blank-line
        // structure, so a line break is its own token.
        if c == '\n' {
            return self.advance_by(K::Newline, 1);
        }
        if rest.starts_with("\r\n") {
            return self.advance_by(K::Newline, 2);
        }
        if c.is_whitespace() {
            let len = rest
                .char_indices()
                .take_while(|(_, ch)| ch.is_whitespace() && *ch != '\n' && *ch != '\r')
                .map(|(i, ch)| i + ch.len_utf8())
                .last()
                .unwrap_or(c.len_utf8());
            return self.advance_by(K::Whitespace, len);
        }

        // Comments and strings are consumed whole, before any operator is
        // considered. This is the entire reason `"a -> b"` is a string.
        if rest.starts_with("//") {
            let end = rest.find('\n').unwrap_or(rest.len());
            return self.advance_by(K::Comment, end);
        }
        if rest.starts_with("/*") {
            return self.block_comment(start);
        }
        if rest.starts_with("r\"") {
            return self.raw_string(start);
        }
        if c == '"' {
            return self.string(start);
        }

        for (lexeme, kind) in GLYPH_STRUCTURAL {
            if rest.starts_with(lexeme) {
                return self.advance_with(*kind, lexeme.len(), Spelling::Glyph);
            }
        }
        for (lexeme, kind) in GLYPH_DELIMITERS {
            if rest.starts_with(lexeme) {
                return self.advance_with(*kind, lexeme.len(), Spelling::Glyph);
            }
        }
        for (lexeme, kind) in ASCII_STRUCTURAL {
            if rest.starts_with(lexeme) {
                return self.advance_by(*kind, lexeme.len());
            }
        }
        for lexeme in RITE_OPERATORS {
            if rest.starts_with(lexeme) {
                return self.advance_by(K::Op, lexeme.len());
            }
        }

        if c.is_ascii_digit() {
            return self.number(start);
        }
        if c == '_' || c.is_alphabetic() {
            let len = rest
                .char_indices()
                .take_while(|(_, ch)| *ch == '_' || ch.is_alphanumeric())
                .map(|(i, ch)| i + ch.len_utf8())
                .last()
                .unwrap_or(c.len_utf8());
            return self.advance_by(K::Ident, len);
        }

        let single = match c {
            '(' => K::LParen,
            ')' => K::RParen,
            '[' => K::LBracket,
            ']' => K::RBracket,
            '{' => K::LBrace,
            '}' => K::BlockClose,
            ',' => K::Comma,
            '.' => K::Dot,
            ';' => K::Semi,
            '$' => K::Dollar,
            '!' => K::Bang,
            '@' => K::At,
            ':' => K::Colon,
            '*' => K::Star,
            other if OPERATOR_CHARS.contains(&other) => K::Op,
            _ => {
                let token = self.advance_by(K::Error, c.len_utf8());
                self.diagnostics.push(
                    CantDiagnostic::error(
                        CANT_L001_UNEXPECTED_CHARACTER,
                        format!("`{c}` cannot begin a Cant token"),
                    )
                    .with_primary(token.source_span(), "not part of any operator or literal")
                    .with_help(
                        "Cant's operators are `-> * [] ?{ |{ ~{ } ; $ ! @ :`; \
                         run `cant version` for the manifest they come from",
                    ),
                );
                return token;
            }
        };
        self.advance_by(single, c.len_utf8())
    }

    fn block_comment(&mut self, start: usize) -> CantToken {
        // Not nested, matching Rite's lexer: `/* /* */` ends at the first `*/`.
        match self.text[start + 2..].find("*/") {
            Some(offset) => self.advance_by(K::Comment, 2 + offset + 2),
            None => {
                let token = self.advance_by(K::Comment, self.text.len() - start);
                self.diagnostics.push(
                    CantDiagnostic::error(CANT_L003_UNTERMINATED_COMMENT, "unterminated comment")
                        .with_primary(token.source_span(), "this comment is never closed")
                        .with_help("close it with `*/`"),
                );
                token
            }
        }
    }

    fn raw_string(&mut self, start: usize) -> CantToken {
        match self.text[start + 2..].find('"') {
            Some(offset) => self.advance_by(K::RawStr, 2 + offset + 1),
            None => self.unterminated_string(start, K::RawStr),
        }
    }

    fn string(&mut self, start: usize) -> CantToken {
        let body = &self.text[start + 1..];
        let mut escaped = false;
        for (i, ch) in body.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => return self.advance_by(K::Str, 1 + i + 1),
                _ => {}
            }
        }
        self.unterminated_string(start, K::Str)
    }

    fn unterminated_string(&mut self, start: usize, kind: K) -> CantToken {
        let token = self.advance_by(kind, self.text.len() - start);
        self.diagnostics.push(
            CantDiagnostic::error(CANT_L002_UNTERMINATED_STRING, "unterminated string")
                .with_primary(token.source_span(), "this string is never closed")
                .with_help("close it with `\"`"),
        );
        token
    }

    fn number(&mut self, start: usize) -> CantToken {
        let rest = &self.text[start..];
        let bytes = rest.as_bytes();
        let mut i = 0;
        let mut float = false;
        if rest.starts_with("0x") || rest.starts_with("0X") || rest.starts_with("0b") {
            i = 2;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            return self.advance_by(K::Int, i);
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
            i += 1;
        }
        // `1.5` is a float; `1..5` is a range whose `..` belongs to the leaf, so
        // a dot only continues the number when a digit follows it.
        if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
            float = true;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
        }
        if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
            let mut j = i + 1;
            if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                float = true;
                i = j;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        self.advance_by(if float { K::Float } else { K::Int }, i)
    }

    fn advance_by(&mut self, kind: K, len: usize) -> CantToken {
        self.advance_with(kind, len, Spelling::Ascii)
    }

    fn advance_with(&mut self, kind: K, len: usize, spelling: Spelling) -> CantToken {
        let start = self.pos;
        // `len` is computed from `char_indices` / `find` on this same slice, so
        // it is always a boundary; clamp anyway rather than risk a panic on a
        // future caller's arithmetic.
        let end = (start + len.max(1)).min(self.text.len());
        let end = ceil_char_boundary(self.text, end);
        self.pos = end;
        self.make(kind, start, end, spelling)
    }

    fn make(&self, kind: K, start: usize, end: usize, spelling: Spelling) -> CantToken {
        CantToken {
            kind,
            span: Span::from_range(start, end),
            file: self.file.id,
            text: self.text[start..end].to_string(),
            spelling,
        }
    }
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index.min(text.len())
}

/// The source span of a run of tokens, or a dummy span for an empty run.
pub fn span_of(tokens: &[CantToken]) -> Span {
    match (tokens.first(), tokens.last()) {
        (Some(first), Some(last)) => Span::new(first.span.start, last.span.end),
        _ => Span::DUMMY,
    }
}

/// The source span of a run of tokens, with file identity.
pub fn source_span_of(tokens: &[CantToken], file: rite_core::FileId) -> SourceSpan {
    SourceSpan::new(file, span_of(tokens))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_core::FileId;

    fn lex_text(src: &str) -> (Vec<CantToken>, CantDiagnostics) {
        lex(&SourceFile::new(FileId(0), "t.cant", src))
    }

    fn kinds(src: &str) -> Vec<K> {
        lex_text(src)
            .0
            .into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != K::Eof)
            .map(|t| t.kind)
            .collect()
    }

    /// The property every later stage depends on: nothing is lost.
    #[test]
    fn tokens_reproduce_the_source_exactly() {
        for src in [
            "roots -> * -> ~{ !@fs.read -> imports -> * } :max 4096 -> []",
            "  \n\t// a comment -> not an arrow\n\"a ?{ string }\" -> f\n",
            "[1, 2, 3] -> * -> ?{ $ % 2 = 0 } -> square -> []",
            "5 -> |{ $ + 1 ; $ * 2 ; square } -> []",
            "#!/usr/bin/env cant\nx -> f\n",
            "",
            "\u{1F600} -> f",
        ] {
            let joined: String = lex_text(src).0.iter().map(|t| t.text.as_str()).collect();
            assert_eq!(joined, src, "round trip failed for {src:?}");
        }
    }

    #[test]
    fn ascii_and_glyph_spellings_give_the_same_kinds() {
        assert_eq!(kinds("a -> b"), kinds("a → b"));
        assert_eq!(kinds("?{ x }"), kinds("⊣⟦ x ⟧"));
        assert_eq!(kinds("|{ a ; b }"), kinds("⫴⟦ a ; b ⟧"));
        assert_eq!(kinds("~{ a }"), kinds("⟲⟦ a ⟧"));
        assert_eq!(kinds("a -> *"), kinds("a → ⋇"));
        assert_eq!(kinds("a -> []"), kinds("a → ⌁"));
    }

    #[test]
    fn the_spelling_used_is_recorded() {
        let ascii: Vec<_> = lex_text("a -> b")
            .0
            .into_iter()
            .map(|t| t.spelling)
            .collect();
        assert!(ascii.iter().all(|s| *s == Spelling::Ascii));
        let arrow = lex_text("a → b")
            .0
            .into_iter()
            .find(|t| t.kind == K::Flow)
            .expect("flow token");
        assert_eq!(arrow.spelling, Spelling::Glyph);
    }

    #[test]
    fn operators_inside_strings_and_comments_are_not_operators() {
        assert_eq!(kinds(r#""-> ?{ |{ ~{ [] ⋇""#), vec![K::Str]);
        assert_eq!(kinds("// -> ?{ |{ ~{ []"), Vec::<K>::new());
        assert_eq!(kinds("/* -> ?{ */"), Vec::<K>::new());
        assert_eq!(kinds(r#"r"-> raw ?{""#), vec![K::RawStr]);
    }

    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        assert_eq!(kinds(r#""a\" -> b" -> f"#), vec![K::Str, K::Flow, K::Ident]);
    }

    #[test]
    fn bang_is_an_effect_marker_but_not_the_head_of_not_equal() {
        assert_eq!(
            kinds("!@fs.read"),
            vec![K::Bang, K::At, K::Ident, K::Dot, K::Ident]
        );
        assert_eq!(kinds("$ != 2"), vec![K::Dollar, K::Op, K::Int]);
    }

    #[test]
    fn colon_is_one_token_whether_it_is_a_modifier_or_an_atom() {
        assert_eq!(kinds(":max 4096"), vec![K::Colon, K::Ident, K::Int]);
        assert_eq!(kinds("$.level = :error"), {
            vec![K::Dollar, K::Dot, K::Ident, K::Op, K::Colon, K::Ident]
        });
    }

    #[test]
    fn a_brace_from_a_rite_closure_is_ordinary_depth() {
        assert_eq!(
            kinds("keep { |n| n > 0 }"),
            vec![
                K::Ident,
                K::LBrace,
                K::Op,
                K::Ident,
                K::Op,
                K::Ident,
                K::Op,
                K::Int,
                K::BlockClose
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(kinds("1"), vec![K::Int]);
        assert_eq!(kinds("1_000"), vec![K::Int]);
        assert_eq!(kinds("0xff"), vec![K::Int]);
        assert_eq!(kinds("1.5"), vec![K::Float]);
        assert_eq!(kinds("1e9"), vec![K::Float]);
        // `1..5` is a range: the dots belong to the leaf, not to the number.
        assert_eq!(kinds("1..5"), vec![K::Int, K::Op, K::Int]);
    }

    #[test]
    fn unterminated_literals_are_reported_not_panicked_on() {
        let (tokens, diags) = lex_text("\"never closed");
        assert!(diags.has_errors());
        assert_eq!(tokens.first().map(|t| t.kind), Some(K::Str));
        let (_, diags) = lex_text("/* never closed");
        assert!(diags.has_errors());
    }

    #[test]
    fn an_unreadable_character_is_reported_and_lexing_continues() {
        let (tokens, diags) = lex_text("a \u{7} b");
        assert_eq!(diags.len(), 1);
        assert!(tokens.iter().any(|t| t.kind == K::Error));
        assert_eq!(
            tokens.iter().filter(|t| t.kind == K::Ident).count(),
            2,
            "the identifier after the bad character should still lex"
        );
    }

    #[test]
    fn a_shebang_is_trivia_at_byte_zero_only() {
        assert_eq!(lex_text("#!/usr/bin/env cant\nx").0[0].kind, K::Shebang);
        // A `#` anywhere else is a Rite atom glyph, and belongs to the leaf.
        assert_eq!(
            kinds("x -> #gold"),
            vec![K::Ident, K::Flow, K::Op, K::Ident]
        );
    }
}
