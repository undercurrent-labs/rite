//! Classify Rite source for colouring, through the language's own lexer.
//!
//! The site has a hand-written tokeniser in TypeScript. This is not a second one:
//! it runs `rite_syntax::lex`, so what counts as a keyword, an atom or a sigil is
//! whatever the language says it is, and adding a keyword cannot leave the
//! renderer behind.
//!
//! **Text comes from spans, never from `Token::text`.** The lexer stores the
//! *lexeme* — a string token's `text` is its contents without the quotes, an
//! atom's is the name without the `#`. Rendering those would silently drop
//! characters. Spans also give back the whitespace between tokens, which the
//! lexer skips and an image plainly needs.

use crate::palette::Kind;
use rite_core::{FileId, SourceFile};
use rite_syntax::{lex, TokenKind};
use std::collections::HashSet;
use std::sync::OnceLock;

/// A run of source and what to colour it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub kind: Kind,
    pub text: String,
}

/// Capability names (`fs`, `http`) and their function names (`read`, `listen`),
/// from the manifest the CLI generates — the same file the site's build reads.
fn capability_tables() -> &'static (HashSet<String>, HashSet<String>) {
    static TABLES: OnceLock<(HashSet<String>, HashSet<String>)> = OnceLock::new();
    TABLES.get_or_init(|| {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../skills/rite/machine/capabilities.json"
        ))
        .expect("capability manifest is valid JSON");
        let mut caps = HashSet::new();
        let mut fns = HashSet::new();
        if let Some(map) = manifest.as_object() {
            for (cap, entries) in map {
                caps.insert(cap.clone());
                for entry in entries.as_array().into_iter().flatten() {
                    if let Some(name) = entry.get("name").and_then(|n| n.as_str()) {
                        fns.insert(name.to_string());
                    }
                }
            }
        }
        (caps, fns)
    })
}

fn kind_of(token: TokenKind) -> Kind {
    use TokenKind as T;
    match token {
        T::Comment | T::DocComment | T::ModuleDocComment | T::Shebang => Kind::Comment,
        T::String | T::MultilineString | T::RawString => Kind::String,
        T::Int | T::Float => Kind::Number,
        T::Atom | T::AtomPrefix => Kind::Atom,
        T::Get | T::Post | T::Put | T::Patch | T::Delete | T::Head | T::Options => Kind::Http,

        // Words that are part of the language rather than the program.
        T::Use
        | T::As
        | T::Pub
        | T::True
        | T::False
        | T::None
        | T::Item
        | T::Room
        | T::World
        | T::Test
        | T::Ok
        | T::Err
        | T::Some
        | T::Else
        | T::For
        | T::Unless
        | T::While
        | T::Loop
        | T::Say
        | T::Xor
        | T::In
        | T::NotIn => Kind::Keyword,

        // Rite's signature marks. Their ASCII twins lex to the same tokens, so
        // `def` and `◆` colour alike — which is the point of the two dialects
        // being one language.
        T::Def
        | T::Bind
        | T::BindMut
        | T::Arrow
        | T::Return
        | T::If
        | T::Match
        | T::Effect
        | T::Host
        | T::BlockOpen
        | T::BlockClose
        | T::RecordOpen
        | T::RecordClose
        | T::Compose
        | T::ForAll
        | T::OkMark
        | T::ErrMark
        | T::Paragraph => Kind::Sigil,

        T::Plus
        | T::Minus
        | T::Star
        | T::Slash
        | T::Percent
        | T::Eq
        | T::NotEq
        | T::Lt
        | T::LtEq
        | T::Gt
        | T::GtEq
        | T::Not
        | T::And
        | T::Or
        | T::Coalesce
        | T::Assign
        | T::Rest
        | T::Spread
        | T::RangeIncl
        | T::Power
        | T::Idiv
        | T::PlusAssign
        | T::MinusAssign
        | T::StarAssign
        | T::SlashAssign
        | T::PercentAssign => Kind::Operator,

        T::LParen
        | T::RParen
        | T::LBracket
        | T::RBracket
        | T::LBrace
        | T::RBrace
        | T::Comma
        | T::Dot
        | T::Colon
        | T::Semicolon
        | T::Pipe
        | T::Underscore
        | T::Dollar
        | T::At => Kind::Punctuation,

        _ => Kind::Plain,
    }
}

/// Split source into coloured runs, covering every byte exactly once.
pub fn runs(source: &str) -> Vec<Run> {
    let file = SourceFile::new(FileId(0), "render.rite", source);
    let (tokens, _diags) = lex(&file);
    let (caps, cap_fns) = capability_tables();

    let mut out: Vec<Run> = Vec::new();
    let mut cursor = 0usize;
    let push = |out: &mut Vec<Run>, kind: Kind, text: &str| {
        if text.is_empty() {
            return;
        }
        // Merge touching runs of one kind so the SVG has one `<text>` per run
        // rather than one per token.
        match out.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(text),
            _ => out.push(Run {
                kind,
                text: text.to_string(),
            }),
        }
    };

    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.kind == TokenKind::Eof {
            break;
        }
        let (start, end) = (token.span.start.as_usize(), token.span.end.as_usize());
        // Whatever the lexer skipped — indentation, blank lines — is plain text,
        // and an image needs it or the layout collapses.
        push(&mut out, Kind::Plain, &source[cursor..start]);

        // `@fs.read` arrives as four tokens: Host, Ident, Dot, Ident. Colouring
        // them separately would lose what makes a capability call recognisable at
        // a glance, so a known capability name takes the sigil with it and a known
        // function name after the dot is coloured as one.
        if token.kind == TokenKind::Host {
            let name = tokens.get(i + 1).filter(|t| t.kind == TokenKind::Ident);
            if let Some(name) = name.filter(|t| caps.contains(&t.text)) {
                let cap_end = name.span.end.as_usize();
                push(&mut out, Kind::Capability, &source[start..cap_end]);
                cursor = cap_end;
                i += 2;

                let dot = tokens.get(i).filter(|t| t.kind == TokenKind::Dot);
                let func = tokens.get(i + 1).filter(|t| t.kind == TokenKind::Ident);
                if let (Some(dot), Some(func)) = (dot, func) {
                    if cap_fns.contains(&func.text) {
                        push(
                            &mut out,
                            Kind::Punctuation,
                            &source[dot.span.start.as_usize()..dot.span.end.as_usize()],
                        );
                        let fn_end = func.span.end.as_usize();
                        push(
                            &mut out,
                            Kind::CapabilityFn,
                            &source[func.span.start.as_usize()..fn_end],
                        );
                        cursor = fn_end;
                        i += 2;
                    }
                }
                continue;
            }
        }

        push(&mut out, kind_of(token.kind), &source[start..end]);
        cursor = end;
        i += 1;
    }
    // Trailing whitespace after the last token, including the final newline.
    push(&mut out, Kind::Plain, &source[cursor..]);
    out
}
