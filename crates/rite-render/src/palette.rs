//! The colour table, read from `grammar/palette.json` at compile time.
//!
//! Same table the site's stylesheet uses, and
//! `crates/rite-cli/tests/palette_sync.rs` fails if the two drift apart. A
//! renderer with its own copy of the colours would look right the day it was
//! written and wrong the first time anyone changed a shade.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Style {
    pub color: String,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub weight: Option<u16>,
    /// The glyph glow. Carried so a renderer *could* draw it; the SVG output
    /// deliberately does not, because a blur filter costs more than it adds at
    /// the sizes an image of code is read at, and rasterises differently in
    /// every viewer.
    #[serde(default)]
    pub glow: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Palette {
    pub background: String,
    pub kinds: HashMap<String, Style>,
}

impl Palette {
    /// The palette every consumer shares.
    pub fn shared() -> &'static Palette {
        use std::sync::OnceLock;
        static PALETTE: OnceLock<Palette> = OnceLock::new();
        PALETTE.get_or_init(|| {
            serde_json::from_str(include_str!("../../../grammar/palette.json"))
                .expect("grammar/palette.json is valid")
        })
    }

    pub fn style(&self, kind: Kind) -> &Style {
        self.kinds
            .get(kind.as_str())
            .unwrap_or_else(|| panic!("grammar/palette.json has no `{}`", kind.as_str()))
    }
}

/// What a run of source is, for colouring.
///
/// These are the `TokenKind` values the site's highlighter emits, by the same
/// names, so one palette entry serves both and the CSS class for a kind is
/// always `.tok-<kind>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Comment,
    String,
    Number,
    Atom,
    Capability,
    CapabilityFn,
    Keyword,
    Operator,
    Http,
    Punctuation,
    Glyph,
    Plain,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Comment => "comment",
            Kind::String => "string",
            Kind::Number => "number",
            Kind::Atom => "atom",
            Kind::Capability => "capability",
            Kind::CapabilityFn => "capability-fn",
            Kind::Keyword => "keyword",
            Kind::Operator => "operator",
            Kind::Http => "http",
            Kind::Punctuation => "punctuation",
            Kind::Glyph => "glyph",
            Kind::Plain => "plain",
        }
    }

    /// Every kind, so a test can assert the table covers them.
    pub const ALL: [Kind; 12] = [
        Kind::Comment,
        Kind::String,
        Kind::Number,
        Kind::Atom,
        Kind::Capability,
        Kind::CapabilityFn,
        Kind::Keyword,
        Kind::Operator,
        Kind::Http,
        Kind::Punctuation,
        Kind::Glyph,
        Kind::Plain,
    ];
}
