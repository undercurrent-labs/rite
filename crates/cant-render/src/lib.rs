//! Classify Cant source for colouring, through the language's own lexer.
//!
//! `apps/cant-web/src/lib/highlight.ts` is the other implementation today. This
//! is not a third one and not a set of regular expressions: it runs
//! [`cant_syntax::lex`], for the reason that file's own header gives — a string
//! or a comment containing `->` or `?{` has to come out as a string or a
//! comment, and only the lexer knows which it is.
//!
//! The output is [`rite_render::Run`], the same shape [`rite_render::runs`]
//! produces for Rite, so everything downstream — the palette, the terminal
//! escapes, an image — is shared and cannot drift per language.
//!
//! # Capabilities are recognised by shape, not by a table
//!
//! `rite-render` checks `@fs.read` against the generated capability manifest.
//! This does not, because Cant does not know what a capability is: `@name.method`
//! is leaf text passed to Rite verbatim (`crates/cant-sem/src/graph.rs`), and a
//! capability Rite has and Cant has never heard of must colour the same as one
//! it has. The rule is `@` `ident` (`.` `ident`)*.

use cant_syntax::{lex, CantTokenKind as K, Spelling};
use rite_core::{FileId, SourceFile};
use rite_render::{Kind, Run};

/// A file id of our own, so nothing here can be mistaken for a real source.
const HIGHLIGHT_FILE: FileId = FileId(u32::MAX - 2);

/// Split Cant source into coloured runs.
///
/// Every byte of the input appears in exactly one run, in order: the lexer is
/// lossless and keeps trivia, so concatenating the runs reproduces the source.
/// That is the property a terminal highlighter needs — anything dropped is a
/// character that vanishes from the line as you type it.
pub fn runs(source: &str) -> Vec<Run> {
    let file = SourceFile::new(HIGHLIGHT_FILE, "<highlight>", source);
    let (tokens, _) = lex(&file);

    let mut out: Vec<Run> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if token.kind == K::Eof {
            break;
        }
        // `@fs.read` is four tokens and one idea. Taken together so the dots
        // inside it are not punctuation and the method is not a bare name.
        if token.kind == K::At {
            if let Some(consumed) = capability(&tokens[i..], &mut out) {
                i += consumed;
                continue;
            }
        }
        push(&mut out, kind_of(token.kind, token.spelling), &token.text);
        i += 1;
    }
    out
}

/// `@name(.name)*` starting at `tokens[0]`, or `None` when what follows the `@`
/// is not that. Returns how many tokens were consumed.
fn capability(tokens: &[cant_syntax::CantToken], out: &mut Vec<Run>) -> Option<usize> {
    if tokens.get(1)?.kind != K::Ident {
        return None;
    }
    push(out, Kind::Capability, &tokens[0].text);
    push(out, Kind::Capability, &tokens[1].text);
    let mut i = 2;
    while tokens.get(i).is_some_and(|t| t.kind == K::Dot)
        && tokens.get(i + 1).is_some_and(|t| t.kind == K::Ident)
    {
        push(out, Kind::Punctuation, &tokens[i].text);
        push(out, Kind::CapabilityFn, &tokens[i + 1].text);
        i += 2;
    }
    Some(i)
}

/// Append, merging into the previous run when the kind is the same — so a line
/// of plain text is one run rather than one per token.
fn push(out: &mut Vec<Run>, kind: Kind, text: &str) {
    if text.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => out.push(Run {
            kind,
            text: text.to_string(),
        }),
    }
}

fn kind_of(token: K, spelling: Spelling) -> Kind {
    // A glyph spelling is its own colour whatever it spells: it is the thing
    // the eye finds first on a line, and the palette gives it a glow for that
    // reason. Checked before the kind, because every structural operator has
    // both spellings.
    if spelling == Spelling::Glyph {
        return Kind::Glyph;
    }
    match token {
        K::Comment | K::Shebang => Kind::Comment,
        K::Str | K::RawStr => Kind::String,
        K::Int | K::Float => Kind::Number,

        // The flow itself, and the stages built out of operators.
        K::Flow
        | K::Star
        | K::Collect
        | K::WardOpen
        | K::ForkOpen
        | K::OrbitOpen
        | K::BlockClose
        | K::Dollar
        | K::Bang
        | K::Op => Kind::Operator,

        // `:` is a modifier prefix after a block and Rite's atom prefix
        // everywhere else — a distinction the parser makes from position and
        // the lexer deliberately does not. Colouring it as an atom prefix is
        // right in both readings: `:max` on an orbit reads as the same kind of
        // named thing `:ok` does.
        K::Colon => Kind::Atom,

        K::LParen
        | K::RParen
        | K::LBracket
        | K::RBracket
        | K::LBrace
        | K::Comma
        | K::Dot
        | K::Semi
        | K::At => Kind::Punctuation,

        K::Ident | K::Whitespace | K::Newline | K::Eof | K::Error => Kind::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn kinds_of(source: &str, needle: &str) -> Vec<Kind> {
        runs(source)
            .into_iter()
            .filter(|r| r.text.contains(needle))
            .map(|r| r.kind)
            .collect()
    }

    /// The property a terminal highlighter lives or dies by: nothing is lost.
    /// A dropped byte is a character that disappears from the line as it is
    /// typed.
    #[test]
    fn every_byte_of_the_source_survives() {
        for source in [
            "[1, 2, 3] -> * -> ?{ $ > 1 } -> []",
            "  // a comment\n\"a string\" -> upper",
            "/* unterminated",
            "[1] → ⋇ → ⊣⟦ $ > 0 ⟧ → ⌁",
            "use helpers [\"a\"] -> * -> helpers.emphasize($) -> []",
            "",
            "   ",
            "1 -> ~{ $ + 1 } :by str :max 10",
        ] {
            assert_eq!(text(&runs(source)), source, "lost bytes in {source:?}");
        }
    }

    /// The reason this runs the lexer instead of matching patterns: an arrow
    /// inside a string is not an arrow.
    #[test]
    fn an_operator_inside_a_string_stays_a_string() {
        let runs = runs(r#""a -> b" -> upper"#);
        let string: Vec<&Run> = runs.iter().filter(|r| r.kind == Kind::String).collect();
        assert_eq!(string.len(), 1);
        assert_eq!(string[0].text, r#""a -> b""#);
        // And exactly one real flow arrow was found.
        assert_eq!(
            runs.iter()
                .filter(|r| r.kind == Kind::Operator && r.text.contains("->"))
                .count(),
            1
        );
    }

    #[test]
    fn a_comment_containing_a_stage_stays_a_comment() {
        let line = runs("// -> * -> []\n1");
        assert_eq!(line[0].kind, Kind::Comment);
        assert_eq!(line[0].text, "// -> * -> []");
        let block = runs("/* -> * -> [] */ 1");
        assert_eq!(block[0].kind, Kind::Comment);
        assert_eq!(block[0].text, "/* -> * -> [] */");
    }

    /// `!@fs.read` is a bang, a capability and its function — not one blob and
    /// not four unrelated tokens.
    #[test]
    fn a_capability_and_its_function_are_told_apart() {
        let runs = runs("!@fs.read");
        let pairs: Vec<(Kind, &str)> = runs.iter().map(|r| (r.kind, r.text.as_str())).collect();
        assert_eq!(
            pairs,
            vec![
                (Kind::Operator, "!"),
                (Kind::Capability, "@fs"),
                (Kind::Punctuation, "."),
                (Kind::CapabilityFn, "read"),
            ]
        );
    }

    /// A capability Rite has and this crate has never heard of colours the same
    /// as one it has: Cant passes the text through, so it must not pretend to
    /// know the roster.
    #[test]
    fn an_unknown_capability_colours_like_a_known_one() {
        assert_eq!(kinds_of("@fs", "@fs"), kinds_of("@zz", "@zz"));
    }

    #[test]
    fn a_glyph_spelling_gets_the_glyph_colour() {
        assert_eq!(kinds_of("a → b", "→"), vec![Kind::Glyph]);
        assert_eq!(kinds_of("a -> b", "->"), vec![Kind::Operator]);
    }

    #[test]
    fn numbers_and_atoms_are_their_own() {
        assert_eq!(kinds_of("1 -> $ + 2.5", "2.5"), vec![Kind::Number]);
        assert_eq!(kinds_of("1 -> ~{ $ } :max 4", ":"), vec![Kind::Atom]);
    }

    /// Malformed input still classifies. The lexer emits an `Error` token and
    /// keeps going, and a highlighter that gave up on the first bad character
    /// would blank the line someone is in the middle of typing.
    #[test]
    fn an_unfinished_line_still_classifies() {
        for source in ["\"unterminated", "[1] -> ~{", "1 -> \u{0}"] {
            assert_eq!(text(&runs(source)), source, "{source:?}");
        }
    }
}
