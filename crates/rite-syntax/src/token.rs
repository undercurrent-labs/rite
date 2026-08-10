use rite_core::{SourceSpan, Span};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical token kinds after glyph/ASCII normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenKind {
    // Glyphs (normalized)
    Def,
    Bind,
    BindMut,
    Arrow,
    Return,
    If,
    Match,
    Effect,
    Host,
    AtomPrefix,
    BlockOpen,
    BlockClose,
    RecordOpen,
    RecordClose,
    In,
    NotIn,
    Coalesce,
    Assign,
    Rest,       // ..
    Spread,     // ... list/record spread
    RangeIncl,  // ..= or ‥
    Power,      // **
    Idiv,       // //
    PlusAssign, // +=
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    // Sugar keywords / glyphs
    Else,
    For,
    Unless,
    While,
    Loop,
    Break,
    Continue,
    Say,
    Xor,
    Compose,   // ∘
    ForAll,    // ∀
    OkMark,    // ✓
    ErrMark,   // ✗
    Paragraph, // ¶

    // Punctuation
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Colon,
    Semicolon,
    Pipe,
    Underscore,
    Dollar,
    At, // raw @ when not part of host alias handled specially

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Not,
    And,
    Or,

    // Keywords
    Use,
    As,
    Pub,
    True,
    False,
    None,
    Item,
    Room,
    World,
    Test,
    Ok,
    Err,
    Some,
    // HTTP methods
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,

    // Literals / idents
    Ident,
    Int,
    Float,
    String,
    MultilineString,
    RawString,
    Atom, // full atom including parts as text after #

    // Trivia retained for formatter (optional stream)
    Comment,
    DocComment,
    ModuleDocComment,
    Whitespace,
    Newline,

    // Special
    Shebang,
    Eof,
    Error,
}

impl TokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Comment
                | TokenKind::DocComment
                | TokenKind::ModuleDocComment
                | TokenKind::Whitespace
                | TokenKind::Newline
                | TokenKind::Shebang
        )
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TokenKind::Def => "◆/def",
            TokenKind::Bind => "←/<-",
            TokenKind::BindMut => "↢/<~",
            TokenKind::Arrow => "→/->",
            TokenKind::Return => "^/return",
            TokenKind::If => "?/if",
            TokenKind::Match => "~/match",
            TokenKind::Effect => "!/do",
            TokenKind::Host => "@/host.",
            TokenKind::BlockOpen => "⟦/[[",
            TokenKind::BlockClose => "⟧/]]",
            TokenKind::RecordOpen => "⟨/<<",
            TokenKind::RecordClose => "⟩/>>",
            TokenKind::In => "∈/in",
            TokenKind::NotIn => "∉/not in",
            TokenKind::Coalesce => "??",
            TokenKind::Assign => ":=",
            TokenKind::Ident => "identifier",
            TokenKind::Int => "integer",
            TokenKind::Float => "float",
            TokenKind::String => "string",
            TokenKind::Eof => "end of file",
            other => return write!(f, "{:?}", other),
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub file: rite_core::FileId,
    /// Lexeme text (for idents, literals, atoms, comments).
    pub text: String,
    /// Is this the first token on its source line?
    ///
    /// Rite has no statement terminator, so a line break is the only thing separating
    /// `xs ← f` from a following `[0]`. Without this the parser read the `[` as a
    /// postfix index into the previous statement and silently changed the program's
    /// meaning. Set by the lexer, which is the only place that still sees whitespace.
    pub starts_line: bool,
}

impl Token {
    pub fn source_span(&self) -> SourceSpan {
        SourceSpan::new(self.file, self.span)
    }

    pub fn is(&self, kind: TokenKind) -> bool {
        self.kind == kind
    }
}

/// Keyword / multi-char ASCII lookup for identifiers that may be keywords.
pub fn keyword_or_ident(text: &str) -> TokenKind {
    match text {
        "def" => TokenKind::Def,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "match" => TokenKind::Match,
        "do" => TokenKind::Effect,
        "in" => TokenKind::In,
        "not" => TokenKind::Not,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "none" => TokenKind::None,
        "use" => TokenKind::Use,
        "as" => TokenKind::As,
        "pub" => TokenKind::Pub,
        "item" => TokenKind::Item,
        "room" => TokenKind::Room,
        "world" => TokenKind::World,
        "test" => TokenKind::Test,
        "ok" => TokenKind::Ok,
        "err" => TokenKind::Err,
        "some" => TokenKind::Some,
        "else" => TokenKind::Else,
        "for" => TokenKind::For,
        "unless" => TokenKind::Unless,
        "while" => TokenKind::While,
        "loop" => TokenKind::Loop,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "say" => TokenKind::Say,
        "xor" => TokenKind::Xor,
        "GET" => TokenKind::Get,
        "POST" => TokenKind::Post,
        "PUT" => TokenKind::Put,
        "PATCH" => TokenKind::Patch,
        "DELETE" => TokenKind::Delete,
        "HEAD" => TokenKind::Head,
        "OPTIONS" => TokenKind::Options,
        _ => TokenKind::Ident,
    }
}
