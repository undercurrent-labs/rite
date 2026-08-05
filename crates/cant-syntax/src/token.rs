//! Cant tokens.
//!
//! ASCII and glyph spellings of the same operator produce the same
//! [`CantTokenKind`]; which spelling was written is recorded in
//! [`CantToken::spelling`] so the converter can round-trip without re-reading the
//! source. The mapping lives in `grammar/cant/operators.toml` — see
//! [`crate::manifest`].

use rite_core::{FileId, SourceSpan, Span};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Which of an operator's two spellings the source used.
///
/// Every token carries one, including tokens with no glyph form (they are always
/// [`Spelling::Ascii`]), so the formatter never has to ask "does this operator
/// have a glyph?" at a call site that only has a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spelling {
    Ascii,
    Glyph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CantTokenKind {
    // ---- Structural operators. These, and only these, appear in the manifest.
    /// `->` `→`
    Flow,
    /// `*` `⋇` — scatter when it is a whole stage, multiplication inside a leaf.
    Star,
    /// `[]` `⌁` — collect in stage position, the empty list literal in source
    /// position.
    Collect,
    /// `?{` `⊣⟦`
    WardOpen,
    /// `|{` `⫴⟦`
    ForkOpen,
    /// `~{` `⟲⟦`
    OrbitOpen,
    /// `!{` `↯⟦` — one token only when the brace touches the `!`.
    RescueOpen,
    /// `:{` `≔⟦` — opens a named flow definition after an identifier in the
    /// preamble, and is leaf material anywhere else. A Rite record field holding
    /// a block (`<< f:{ |x| x } >>`) is spelled the same way, which is why this
    /// counts leaf depth but does not open a Cant block.
    DefineOpen,
    /// `}` `⟧` — and `⟩`, which closes a Rite record inside a leaf. All three
    /// close *something*; only one seen at leaf-depth zero closes a Cant block.
    BlockClose,
    /// `;`
    Semi,
    /// `$`
    Dollar,
    /// `!` — never the head of `!=`, which lexes as [`CantTokenKind::Op`].
    Bang,
    /// `@`
    At,
    /// `:` — a modifier prefix after a block, Rite's atom prefix anywhere else.
    Colon,

    // ---- Delimiters. Depth is counted over these.
    LParen,
    RParen,
    LBracket,
    RBracket,
    /// `{`, and the glyph openers `⟦` and `⟨` that belong to a Rite leaf.
    LBrace,

    // ---- Leaf material, passed to Rite unchanged.
    Ident,
    Int,
    Float,
    Str,
    RawStr,
    Comma,
    Dot,
    /// Any other Rite operator: `+ - / % = == != < <= > >= << >> ?? := ...`
    Op,

    // ---- Trivia. Retained by the lexer; dropped by the parser.
    Comment,
    Whitespace,
    Newline,
    Shebang,

    // ---- Special
    Eof,
    /// A character the lexer could not read. Always accompanied by a diagnostic.
    Error,
}

impl CantTokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            CantTokenKind::Comment
                | CantTokenKind::Whitespace
                | CantTokenKind::Newline
                | CantTokenKind::Shebang
        )
    }

    /// Does this token open a nesting level inside a leaf expression?
    pub fn opens_depth(self) -> bool {
        matches!(
            self,
            CantTokenKind::LParen
                | CantTokenKind::LBracket
                | CantTokenKind::LBrace
                | CantTokenKind::DefineOpen
        )
    }

    /// Does this token close one?
    ///
    /// [`CantTokenKind::BlockClose`] is here as well as being the Cant block
    /// terminator: which of the two it is depends entirely on the depth it is
    /// seen at, and that is the parser's call, not the lexer's.
    pub fn closes_depth(self) -> bool {
        matches!(
            self,
            CantTokenKind::RParen | CantTokenKind::RBracket | CantTokenKind::BlockClose
        )
    }

    /// Opens a Cant structural block: a stage that contains a flow.
    ///
    /// [`CantTokenKind::DefineOpen`] is deliberately absent. A block opener
    /// breaks a leaf run ("a block opener can only start a stage"), and `:{` is
    /// how a Rite record holds a block, so treating one inside a leaf as
    /// structural would truncate the leaf. A definition is recognised by the
    /// parser from position instead.
    pub fn opens_block(self) -> bool {
        matches!(
            self,
            CantTokenKind::WardOpen
                | CantTokenKind::ForkOpen
                | CantTokenKind::OrbitOpen
                | CantTokenKind::RescueOpen
        )
    }

    /// The `token = "…"` name this kind carries in the operator manifest, or
    /// `None` for kinds the manifest does not describe (delimiters, leaf
    /// material, trivia).
    ///
    /// The two directions of this mapping are what `manifest_sync` checks.
    pub fn manifest_name(self) -> Option<&'static str> {
        Some(match self {
            CantTokenKind::Flow => "Flow",
            CantTokenKind::Star => "Star",
            CantTokenKind::Collect => "Collect",
            CantTokenKind::WardOpen => "WardOpen",
            CantTokenKind::ForkOpen => "ForkOpen",
            CantTokenKind::OrbitOpen => "OrbitOpen",
            CantTokenKind::RescueOpen => "RescueOpen",
            CantTokenKind::DefineOpen => "DefineOpen",
            CantTokenKind::BlockClose => "BlockClose",
            CantTokenKind::Semi => "Semi",
            CantTokenKind::Dollar => "Dollar",
            CantTokenKind::Bang => "Bang",
            CantTokenKind::At => "At",
            CantTokenKind::Colon => "Colon",
            _ => return None,
        })
    }

    /// Every kind the manifest is expected to describe, in declaration order.
    pub const MANIFEST_KINDS: &'static [CantTokenKind] = &[
        CantTokenKind::Flow,
        CantTokenKind::Star,
        CantTokenKind::Collect,
        CantTokenKind::WardOpen,
        CantTokenKind::ForkOpen,
        CantTokenKind::OrbitOpen,
        CantTokenKind::RescueOpen,
        CantTokenKind::DefineOpen,
        CantTokenKind::BlockClose,
        CantTokenKind::Semi,
        CantTokenKind::Dollar,
        CantTokenKind::Bang,
        CantTokenKind::At,
        CantTokenKind::Colon,
    ];
}

impl fmt::Display for CantTokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CantTokenKind::Flow => "`->`",
            CantTokenKind::Star => "`*`",
            CantTokenKind::Collect => "`[]`",
            CantTokenKind::WardOpen => "`?{`",
            CantTokenKind::ForkOpen => "`|{`",
            CantTokenKind::OrbitOpen => "`~{`",
            CantTokenKind::RescueOpen => "`!{`",
            CantTokenKind::DefineOpen => "`:{`",
            CantTokenKind::BlockClose => "`}`",
            CantTokenKind::Semi => "`;`",
            CantTokenKind::Dollar => "`$`",
            CantTokenKind::Bang => "`!`",
            CantTokenKind::At => "`@`",
            CantTokenKind::Colon => "`:`",
            CantTokenKind::Ident => "an identifier",
            CantTokenKind::Int => "an integer",
            CantTokenKind::Float => "a number",
            CantTokenKind::Str | CantTokenKind::RawStr => "a string",
            CantTokenKind::Eof => "the end of the program",
            other => return write!(f, "{other:?}"),
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CantToken {
    pub kind: CantTokenKind,
    pub span: Span,
    pub file: FileId,
    /// The exact source lexeme. Concatenating every token's text in order
    /// reproduces the source byte for byte — which is what lets the formatter
    /// preserve strings and comments without re-reading them.
    pub text: String,
    pub spelling: Spelling,
}

impl CantToken {
    pub fn source_span(&self) -> SourceSpan {
        SourceSpan::new(self.file, self.span)
    }

    pub fn is(&self, kind: CantTokenKind) -> bool {
        self.kind == kind
    }
}
