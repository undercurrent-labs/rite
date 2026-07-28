use crate::token::{keyword_or_ident, Token, TokenKind};
use rite_core::{
    simple_error, Diagnostics, FileId, SourceFile, Span, E001_INVALID_UTF8, E002_UNEXPECTED_CHAR,
    E003_UNTERMINATED_STRING, E004_UNTERMINATED_COMMENT, E005_INVALID_NUMBER, E006_INVALID_ESCAPE,
};

pub struct Lexer<'a> {
    file: FileId,
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    diagnostics: Diagnostics,
}

pub fn lex(file: &SourceFile) -> (Vec<Token>, Diagnostics) {
    let mut lexer = Lexer::new(file);
    let tokens = lexer.tokenize_all();
    (tokens, lexer.diagnostics)
}

impl<'a> Lexer<'a> {
    pub fn new(file: &'a SourceFile) -> Self {
        Self {
            file: file.id,
            src: file.as_str(),
            bytes: file.as_str().as_bytes(),
            pos: 0,
            diagnostics: Diagnostics::new(),
        }
    }

    pub fn tokenize_all(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        // Shebang on first line
        if self.src.starts_with("#!") {
            let start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                self.pos += 1;
            }
            tokens.push(self.make(TokenKind::Shebang, start, self.pos));
        }

        loop {
            let tok = self.next_token();
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        tokens
    }

    fn next_token(&mut self) -> Token {
        self.skip_whitespace_collect();

        if self.pos >= self.bytes.len() {
            return self.make(TokenKind::Eof, self.pos, self.pos);
        }

        let start = self.pos;
        let c = self.peek_char();

        // Multi-byte Unicode sigils
        if let Some(tok) = self.try_sigil(start) {
            return tok;
        }

        match c {
            // ASCII multi-char operators
            b'<' if self.starts_with("<-") => {
                self.pos += 2;
                self.make(TokenKind::Bind, start, self.pos)
            }
            b'<' if self.starts_with("<~") => {
                self.pos += 2;
                self.make(TokenKind::BindMut, start, self.pos)
            }
            b'<' if self.starts_with("<<") => {
                self.pos += 2;
                self.make(TokenKind::RecordOpen, start, self.pos)
            }
            b'<' if self.starts_with("<=") => {
                self.pos += 2;
                self.make(TokenKind::LtEq, start, self.pos)
            }
            b'<' => {
                self.pos += 1;
                self.make(TokenKind::Lt, start, self.pos)
            }
            b'>' if self.starts_with(">>") => {
                self.pos += 2;
                self.make(TokenKind::RecordClose, start, self.pos)
            }
            b'>' if self.starts_with(">=") => {
                self.pos += 2;
                self.make(TokenKind::GtEq, start, self.pos)
            }
            b'>' => {
                self.pos += 1;
                self.make(TokenKind::Gt, start, self.pos)
            }
            b'-' if self.starts_with("->") => {
                self.pos += 2;
                self.make(TokenKind::Arrow, start, self.pos)
            }
            b'[' if self.starts_with("[[") => {
                self.pos += 2;
                self.make(TokenKind::BlockOpen, start, self.pos)
            }
            b']' if self.starts_with("]]") => {
                self.pos += 2;
                self.make(TokenKind::BlockClose, start, self.pos)
            }
            b'?' if self.starts_with("??") => {
                self.pos += 2;
                self.make(TokenKind::Coalesce, start, self.pos)
            }
            b':' if self.starts_with(":=") => {
                self.pos += 2;
                self.make(TokenKind::Assign, start, self.pos)
            }
            b'.' if self.starts_with("..") => {
                self.pos += 2;
                self.make(TokenKind::Rest, start, self.pos)
            }
            b'!' if self.starts_with("!=") => {
                self.pos += 2;
                self.make(TokenKind::NotEq, start, self.pos)
            }
            b'/' if self.starts_with("//") => self.line_comment(start),
            b'/' if self.starts_with("/*") => self.block_comment(start),
            b'h' if self.starts_with("host.") => {
                self.pos += 5;
                self.make(TokenKind::Host, start, self.pos)
            }
            b'n' if self.starts_with("not in")
                && self
                    .bytes
                    .get(start + 6)
                    .map(|b| !is_ident_continue_byte(*b))
                    .unwrap_or(true) =>
            {
                self.pos += 6;
                self.make(TokenKind::NotIn, start, self.pos)
            }

            // Single char
            b'(' => {
                self.pos += 1;
                self.make(TokenKind::LParen, start, self.pos)
            }
            b')' => {
                self.pos += 1;
                self.make(TokenKind::RParen, start, self.pos)
            }
            b'[' => {
                self.pos += 1;
                self.make(TokenKind::LBracket, start, self.pos)
            }
            b']' => {
                self.pos += 1;
                self.make(TokenKind::RBracket, start, self.pos)
            }
            b'{' => {
                self.pos += 1;
                self.make(TokenKind::LBrace, start, self.pos)
            }
            b'}' => {
                self.pos += 1;
                self.make(TokenKind::RBrace, start, self.pos)
            }
            b',' => {
                self.pos += 1;
                self.make(TokenKind::Comma, start, self.pos)
            }
            b'.' => {
                self.pos += 1;
                self.make(TokenKind::Dot, start, self.pos)
            }
            b';' => {
                self.pos += 1;
                self.make(TokenKind::Semicolon, start, self.pos)
            }
            b'|' => {
                self.pos += 1;
                self.make(TokenKind::Pipe, start, self.pos)
            }
            b'+' => {
                self.pos += 1;
                self.make(TokenKind::Plus, start, self.pos)
            }
            b'-' => {
                self.pos += 1;
                self.make(TokenKind::Minus, start, self.pos)
            }
            b'*' => {
                self.pos += 1;
                self.make(TokenKind::Star, start, self.pos)
            }
            b'/' => {
                self.pos += 1;
                self.make(TokenKind::Slash, start, self.pos)
            }
            b'%' => {
                self.pos += 1;
                self.make(TokenKind::Percent, start, self.pos)
            }
            b'=' => {
                self.pos += 1;
                self.make(TokenKind::Eq, start, self.pos)
            }
            b'?' => {
                self.pos += 1;
                self.make(TokenKind::If, start, self.pos)
            }
            b'!' => {
                self.pos += 1;
                self.make(TokenKind::Effect, start, self.pos)
            }
            b'^' => {
                self.pos += 1;
                self.make(TokenKind::Return, start, self.pos)
            }
            b'~' => {
                self.pos += 1;
                self.make(TokenKind::Match, start, self.pos)
            }
            b'@' => {
                self.pos += 1;
                self.make(TokenKind::Host, start, self.pos)
            }
            b'$' => {
                self.pos += 1;
                self.make(TokenKind::Dollar, start, self.pos)
            }
            b'_' if !self
                .bytes
                .get(self.pos + 1)
                .map(|b| is_ident_continue_byte(*b))
                .unwrap_or(false) =>
            {
                self.pos += 1;
                self.make(TokenKind::Underscore, start, self.pos)
            }
            b':' => {
                // Atom ASCII alias :name
                self.pos += 1;
                if self.pos < self.bytes.len() && is_ident_start_byte(self.bytes[self.pos]) {
                    let atom_start = self.pos;
                    self.consume_ident_bytes();
                    // allow dotted atoms :door.open
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] == b'.'
                        && self
                            .bytes
                            .get(self.pos + 1)
                            .map(|b| is_ident_start_byte(*b))
                            .unwrap_or(false)
                    {
                        self.pos += 1;
                        self.consume_ident_bytes();
                    }
                    let text = self.src[atom_start..self.pos].to_string();
                    let mut tok = self.make(TokenKind::Atom, start, self.pos);
                    tok.text = text;
                    return tok;
                }
                self.make(TokenKind::Colon, start, self.pos)
            }
            b'#' => {
                self.pos += 1;
                if self.pos < self.bytes.len() && is_ident_start_byte(self.bytes[self.pos]) {
                    let atom_start = self.pos;
                    self.consume_ident_bytes();
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] == b'.'
                        && self
                            .bytes
                            .get(self.pos + 1)
                            .map(|b| is_ident_start_byte(*b))
                            .unwrap_or(false)
                    {
                        self.pos += 1;
                        self.consume_ident_bytes();
                    }
                    let text = self.src[atom_start..self.pos].to_string();
                    let mut tok = self.make(TokenKind::Atom, start, self.pos);
                    tok.text = text;
                    return tok;
                }
                self.make(TokenKind::AtomPrefix, start, self.pos)
            }
            b'"' if self.starts_with("\"\"\"") => self.multiline_string(start),
            b'"' => self.string_literal(start),
            b'r' if self.bytes.get(self.pos + 1) == Some(&b'"') => self.raw_string(start),
            b'`' => self.escaped_ident(start),
            b'0'..=b'9' => self.number(start),
            c if is_ident_start_byte(c) || (c & 0x80) != 0 => self.ident_or_keyword(start),
            _ => {
                let ch = self.peek_char_full();
                let len = ch.len_utf8();
                self.pos += len;
                self.diagnostics.push(simple_error(
                    E002_UNEXPECTED_CHAR,
                    format!("unexpected character {:?}", ch),
                    self.file,
                    Span::from_range(start, self.pos),
                    "not valid here",
                ));
                self.make(TokenKind::Error, start, self.pos)
            }
        }
    }

    fn try_sigil(&mut self, start: usize) -> Option<Token> {
        let rest = &self.src[self.pos..];
        let pairs: &[(&str, TokenKind)] = &[
            ("◆", TokenKind::Def),
            ("←", TokenKind::Bind),
            ("↢", TokenKind::BindMut),
            ("→", TokenKind::Arrow),
            ("⟦", TokenKind::BlockOpen),
            ("⟧", TokenKind::BlockClose),
            ("⟨", TokenKind::RecordOpen),
            ("⟩", TokenKind::RecordClose),
            ("∈", TokenKind::In),
            ("∉", TokenKind::NotIn),
        ];
        for (sigil, kind) in pairs {
            if rest.starts_with(sigil) {
                self.pos += sigil.len();
                return Some(self.make(*kind, start, self.pos));
            }
        }
        None
    }

    fn line_comment(&mut self, start: usize) -> Token {
        // // or /// or //!
        let kind = if self.starts_with("///") {
            TokenKind::DocComment
        } else if self.starts_with("//!") {
            TokenKind::ModuleDocComment
        } else {
            TokenKind::Comment
        };
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        self.make(kind, start, self.pos)
    }

    fn block_comment(&mut self, start: usize) -> Token {
        self.pos += 2; // /*
        let mut depth = 1;
        while self.pos < self.bytes.len() && depth > 0 {
            if self.starts_with("/*") {
                self.pos += 2;
                depth += 1;
            } else if self.starts_with("*/") {
                self.pos += 2;
                depth -= 1;
            } else {
                self.pos += 1;
            }
        }
        if depth != 0 {
            self.diagnostics.push(simple_error(
                E004_UNTERMINATED_COMMENT,
                "unterminated block comment",
                self.file,
                Span::from_range(start, self.pos),
                "reached end of file",
            ));
        }
        self.make(TokenKind::Comment, start, self.pos)
    }

    fn string_literal(&mut self, start: usize) -> Token {
        self.pos += 1; // opening "
        let mut text = String::new();
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            if c == b'"' {
                self.pos += 1;
                let mut tok = self.make(TokenKind::String, start, self.pos);
                tok.text = text;
                return tok;
            }
            if c == b'\\' {
                self.pos += 1;
                if self.pos >= self.bytes.len() {
                    break;
                }
                match self.bytes[self.pos] {
                    b'n' => text.push('\n'),
                    b't' => text.push('\t'),
                    b'r' => text.push('\r'),
                    b'\\' => text.push('\\'),
                    b'"' => text.push('"'),
                    b'0' => text.push('\0'),
                    b'{' => text.push('{'),
                    b'}' => text.push('}'),
                    b'u' => {
                        // \u{...}
                        self.pos += 1;
                        if self.bytes.get(self.pos) == Some(&b'{') {
                            self.pos += 1;
                            let hex_start = self.pos;
                            while self.pos < self.bytes.len()
                                && self.bytes[self.pos].is_ascii_hexdigit()
                            {
                                self.pos += 1;
                            }
                            let hex = &self.src[hex_start..self.pos];
                            if self.bytes.get(self.pos) == Some(&b'}') {
                                if let Ok(cp) = u32::from_str_radix(hex, 16) {
                                    if let Some(ch) = char::from_u32(cp) {
                                        text.push(ch);
                                    }
                                }
                                self.pos += 1;
                                continue;
                            }
                        }
                        self.diagnostics.push(simple_error(
                            E006_INVALID_ESCAPE,
                            "invalid unicode escape",
                            self.file,
                            Span::from_range(start, self.pos),
                            "expected \\u{hex}",
                        ));
                        continue;
                    }
                    other => {
                        self.diagnostics.push(simple_error(
                            E006_INVALID_ESCAPE,
                            format!("invalid escape \\{}", other as char),
                            self.file,
                            Span::from_range(self.pos - 1, self.pos + 1),
                            "unknown escape sequence",
                        ));
                        text.push(other as char);
                    }
                }
                self.pos += 1;
            } else if c == b'{' {
                // Interpolation marker: keep as text with special handling at parse time
                // For lexer we store raw including braces; parser/eval handles interpolation
                text.push('{');
                self.pos += 1;
            } else {
                let ch = self.peek_char_full();
                text.push(ch);
                self.pos += ch.len_utf8();
            }
        }
        self.diagnostics.push(simple_error(
            E003_UNTERMINATED_STRING,
            "unterminated string literal",
            self.file,
            Span::from_range(start, self.pos),
            "expected closing \"",
        ));
        let mut tok = self.make(TokenKind::String, start, self.pos);
        tok.text = text;
        tok
    }

    fn multiline_string(&mut self, start: usize) -> Token {
        self.pos += 3; // """
        let content_start = self.pos;
        while self.pos < self.bytes.len() {
            if self.starts_with("\"\"\"") {
                let raw = &self.src[content_start..self.pos];
                self.pos += 3;
                let text = normalize_multiline(raw);
                let mut tok = self.make(TokenKind::MultilineString, start, self.pos);
                tok.text = text;
                return tok;
            }
            self.pos += 1;
        }
        self.diagnostics.push(simple_error(
            E003_UNTERMINATED_STRING,
            "unterminated multiline string",
            self.file,
            Span::from_range(start, self.pos),
            "expected closing \"\"\"",
        ));
        self.make(TokenKind::MultilineString, start, self.pos)
    }

    fn raw_string(&mut self, start: usize) -> Token {
        self.pos += 2; // r"
        let content_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'"' {
            self.pos += 1;
        }
        let text = self.src[content_start..self.pos].to_string();
        if self.pos < self.bytes.len() {
            self.pos += 1;
        } else {
            self.diagnostics.push(simple_error(
                E003_UNTERMINATED_STRING,
                "unterminated raw string",
                self.file,
                Span::from_range(start, self.pos),
                "expected closing \"",
            ));
        }
        let mut tok = self.make(TokenKind::RawString, start, self.pos);
        tok.text = text;
        tok
    }

    fn escaped_ident(&mut self, start: usize) -> Token {
        self.pos += 1; // `
        let content_start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'`' {
            self.pos += 1;
        }
        let text = self.src[content_start..self.pos].to_string();
        if self.pos < self.bytes.len() {
            self.pos += 1;
        }
        let mut tok = self.make(TokenKind::Ident, start, self.pos);
        tok.text = text;
        tok
    }

    fn number(&mut self, start: usize) -> Token {
        // 0x, 0b, decimal, float
        if self.starts_with("0x") || self.starts_with("0X") {
            self.pos += 2;
            let digits_start = self.pos;
            while self.pos < self.bytes.len()
                && (self.bytes[self.pos].is_ascii_hexdigit() || self.bytes[self.pos] == b'_')
            {
                self.pos += 1;
            }
            if self.pos == digits_start {
                self.diagnostics.push(simple_error(
                    E005_INVALID_NUMBER,
                    "invalid hexadecimal number",
                    self.file,
                    Span::from_range(start, self.pos),
                    "expected hex digits",
                ));
            }
            return self.make(TokenKind::Int, start, self.pos);
        }
        if self.starts_with("0b") || self.starts_with("0B") {
            self.pos += 2;
            let digits_start = self.pos;
            while self.pos < self.bytes.len()
                && (self.bytes[self.pos] == b'0'
                    || self.bytes[self.pos] == b'1'
                    || self.bytes[self.pos] == b'_')
            {
                self.pos += 1;
            }
            if self.pos == digits_start {
                self.diagnostics.push(simple_error(
                    E005_INVALID_NUMBER,
                    "invalid binary number",
                    self.file,
                    Span::from_range(start, self.pos),
                    "expected binary digits",
                ));
            }
            return self.make(TokenKind::Int, start, self.pos);
        }

        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'_')
        {
            self.pos += 1;
        }

        let mut is_float = false;
        if self.pos < self.bytes.len()
            && self.bytes[self.pos] == b'.'
            && self
                .bytes
                .get(self.pos + 1)
                .map(|b| b.is_ascii_digit())
                .unwrap_or(false)
        {
            is_float = true;
            self.pos += 1;
            while self.pos < self.bytes.len()
                && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'_')
            {
                self.pos += 1;
            }
        }

        if self.pos < self.bytes.len() && (self.bytes[self.pos] == b'e' || self.bytes[self.pos] == b'E')
        {
            is_float = true;
            self.pos += 1;
            if self.pos < self.bytes.len()
                && (self.bytes[self.pos] == b'+' || self.bytes[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.bytes.len()
                && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'_')
            {
                self.pos += 1;
            }
        }

        let kind = if is_float {
            TokenKind::Float
        } else {
            TokenKind::Int
        };
        self.make(kind, start, self.pos)
    }

    fn ident_or_keyword(&mut self, start: usize) -> Token {
        // Consume Unicode ident
        while self.pos < self.bytes.len() {
            let ch = self.peek_char_full();
            if self.pos == start {
                if !is_ident_start(ch) {
                    break;
                }
            } else if !is_ident_continue(ch) {
                break;
            }
            self.pos += ch.len_utf8();
        }
        let text = &self.src[start..self.pos];
        let kind = keyword_or_ident(text);
        let mut tok = self.make(kind, start, self.pos);
        if kind == TokenKind::Ident {
            tok.text = text.to_string();
        } else {
            tok.text = text.to_string();
        }
        tok
    }

    fn consume_ident_bytes(&mut self) {
        while self.pos < self.bytes.len() {
            let ch = self.peek_char_full();
            if !is_ident_continue(ch) {
                break;
            }
            self.pos += ch.len_utf8();
        }
    }

    fn skip_whitespace_collect(&mut self) {
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> u8 {
        self.bytes.get(self.pos).copied().unwrap_or(0)
    }

    fn peek_char_full(&self) -> char {
        self.src[self.pos..].chars().next().unwrap_or('\0')
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s)
    }

    fn make(&self, kind: TokenKind, start: usize, end: usize) -> Token {
        Token {
            kind,
            span: Span::from_range(start, end),
            file: self.file,
            text: self.src[start..end.min(self.src.len())].to_string(),
        }
    }
}

fn is_ident_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_ident_continue_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn normalize_multiline(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.split('\n').collect();
    // Drop leading empty line after opening """
    if let Some(first) = lines.first() {
        if first.trim().is_empty() {
            lines.remove(0);
        }
    }
    // Drop trailing empty line before closing """
    if let Some(last) = lines.last() {
        if last.trim().is_empty() {
            lines.pop();
        }
    }
    // Common indent
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').count())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                *l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Validate UTF-8 at load time — SourceFile already requires valid String.
// Provide helper for byte diagnostics if invalid bytes are forced through.
pub fn validate_utf8(bytes: &[u8], file: FileId) -> Result<String, Diagnostics> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        Err(e) => {
            let mut d = Diagnostics::new();
            d.push(simple_error(
                E001_INVALID_UTF8,
                "source is not valid UTF-8",
                file,
                Span::from_range(e.valid_up_to(), e.valid_up_to() + 1),
                "invalid UTF-8 sequence",
            ));
            Err(d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_core::SourceFile;

    fn lex_kinds(src: &str) -> Vec<TokenKind> {
        let f = SourceFile::new(FileId(0), "t.rite", src);
        let (toks, _) = lex(&f);
        toks.into_iter()
            .filter(|t| !t.kind.is_trivia() && t.kind != TokenKind::Eof)
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn lex_glyph_sigils() {
        let k = lex_kinds("◆ x ← 1 → y");
        assert_eq!(
            k,
            vec![
                TokenKind::Def,
                TokenKind::Ident,
                TokenKind::Bind,
                TokenKind::Int,
                TokenKind::Arrow,
                TokenKind::Ident
            ]
        );
    }

    #[test]
    fn lex_ascii_aliases() {
        let k = lex_kinds("def x <- 1 -> y");
        assert_eq!(
            k,
            vec![
                TokenKind::Def,
                TokenKind::Ident,
                TokenKind::Bind,
                TokenKind::Int,
                TokenKind::Arrow,
                TokenKind::Ident
            ]
        );
    }

    #[test]
    fn lex_atoms() {
        let f = SourceFile::new(FileId(0), "t.rite", "#ok :error #door.open");
        let (toks, d) = lex(&f);
        assert!(!d.has_errors());
        let atoms: Vec<_> = toks
            .iter()
            .filter(|t| t.kind == TokenKind::Atom)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(atoms, vec!["ok", "error", "door.open"]);
    }

    #[test]
    fn lex_numbers() {
        let k = lex_kinds("42 0xff 0b1010 3.14 1_000");
        assert_eq!(
            k,
            vec![
                TokenKind::Int,
                TokenKind::Int,
                TokenKind::Int,
                TokenKind::Float,
                TokenKind::Int
            ]
        );
    }

    #[test]
    fn lex_nested_comment() {
        let k = lex_kinds("1 /* a /* b */ c */ 2");
        assert_eq!(k, vec![TokenKind::Int, TokenKind::Int]);
    }

    #[test]
    fn lex_host() {
        let k = lex_kinds("@fs.read host.json.decode");
        assert!(k.contains(&TokenKind::Host));
    }
}
