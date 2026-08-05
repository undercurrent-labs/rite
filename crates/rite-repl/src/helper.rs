//! Colour and completion for the prompt.
//!
//! The classification is `rite_render::runs` — the language's own lexer — and
//! the colours are `grammar/palette.json`, so the prompt, the site and a
//! generated image cannot disagree about what a string looks like.
//!
//! Continuation is deliberately *not* done here. [`Validator`] always accepts,
//! because the read loop in `lib.rs` already buffers incomplete input with
//! [`crate::is_complete`], and two mechanisms deciding when an input ends is one
//! more than can be right.

use std::borrow::Cow;

use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use rite_render::term::{self, RESET};
use rite_render::Kind;

/// Every meta-command the REPL answers to. Kept beside the dispatch in `lib.rs`.
pub const META_COMMANDS: &[&str] = &[
    ":allow",
    ":bindings",
    ":capabilities",
    ":deny",
    ":format",
    ":help",
    ":load",
    ":modules",
    ":prelude",
    ":q",
    ":quit",
    ":reload",
    ":reset",
    ":timeout",
];

pub struct RiteHelper {
    color: bool,
    names: Vec<String>,
}

impl RiteHelper {
    pub fn new(color: bool) -> Self {
        Self {
            color,
            names: Vec::new(),
        }
    }

    /// The session's bound and defined names, refreshed after every input.
    pub fn set_names(&mut self, names: Vec<String>) {
        self.names = names;
    }
}

impl Highlighter for RiteHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.color || line.is_empty() {
            return Cow::Borrowed(line);
        }
        if line.trim_start().starts_with(':') {
            // A meta-command is not Rite. Colouring `:load ./x.rite` through
            // the lexer would make the path an argument list.
            return Cow::Owned(term::paint_as(line, Kind::Keyword, true));
        }
        Cow::Owned(term::paint(&rite_render::runs(line), true))
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if self.color {
            Cow::Owned(term::paint_as(prompt, Kind::Capability, true))
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        // Repaint per keystroke, or the line is always one character stale —
        // and that character is the one being typed.
        self.color
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if self.color {
            Cow::Owned(format!("{}{hint}{RESET}", term::start(Kind::Comment)))
        } else {
            Cow::Borrowed(hint)
        }
    }
}

impl Completer for RiteHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        let (start, word) = word_before(line, pos);
        if word.is_empty() {
            return Ok((start, Vec::new()));
        }
        let mut out: Vec<String> = Vec::new();
        if word.starts_with(':') {
            if start == 0 {
                out.extend(
                    META_COMMANDS
                        .iter()
                        .filter(|c| c.starts_with(word))
                        .map(|c| c.to_string()),
                );
            }
        } else if let Some(prefix) = word.strip_prefix('@') {
            out.extend(
                rite_render::capability_names()
                    .into_iter()
                    .filter(|c| c.starts_with(prefix))
                    .map(|c| format!("@{c}")),
            );
        } else {
            out.extend(self.names.iter().filter(|n| n.starts_with(word)).cloned());
        }
        out.sort();
        out.dedup();
        Ok((start, out))
    }
}

/// The word the cursor sits at the end of, and where it began.
fn word_before(line: &str, pos: usize) -> (usize, &str) {
    let upto = &line[..pos.min(line.len())];
    let start = upto
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == ':' || *c == '@' || *c == '.'))
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    (start, &upto[start..])
}

impl Hinter for RiteHelper {
    type Hint = String;
}

impl Validator for RiteHelper {}

impl Helper for RiteHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_off_is_the_line_unchanged() {
        let helper = RiteHelper::new(false);
        let line = "◆ f(n) ⟦ ^ n * 2 ⟧";
        assert_eq!(helper.highlight(line, 0), line);
        assert_eq!(helper.highlight_prompt("rite〉", false), "rite〉");
    }

    #[test]
    fn colouring_keeps_every_character() {
        let helper = RiteHelper::new(true);
        let line = "x ← \"a ⟧ brace\" # not a comment";
        let painted = helper.highlight(line, 0).into_owned();
        assert!(painted.contains('\x1b'));
        let mut stripped = String::new();
        let mut chars = painted.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                stripped.push(c);
            }
        }
        assert_eq!(stripped, line);
    }

    #[test]
    fn capabilities_complete_from_the_generated_manifest() {
        let helper = RiteHelper::new(false);
        let history = rustyline::history::MemHistory::new();
        let ctx = Context::new(&history);
        let (_, caps) = helper.complete("@f", 2, &ctx).expect("complete");
        assert!(caps.contains(&"@fs".to_string()), "{caps:?}");
    }

    #[test]
    fn meta_commands_complete_only_at_the_start_of_a_line() {
        let helper = RiteHelper::new(false);
        let history = rustyline::history::MemHistory::new();
        let ctx = Context::new(&history);
        let (_, at_start) = helper.complete(":relo", 5, &ctx).expect("complete");
        assert_eq!(at_start, vec![":reload".to_string()]);
        let (_, mid) = helper.complete("x #a", 4, &ctx).expect("complete");
        assert!(mid.is_empty(), "{mid:?}");
    }

    #[test]
    fn bound_names_complete() {
        let mut helper = RiteHelper::new(false);
        helper.set_names(vec!["total".into(), "toggle".into(), "other".into()]);
        let history = rustyline::history::MemHistory::new();
        let ctx = Context::new(&history);
        let (start, names) = helper.complete("^ tot", 5, &ctx).expect("complete");
        assert_eq!(start, 2);
        assert_eq!(names, vec!["total".to_string()]);
    }
}
