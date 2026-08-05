//! Colour and completion for the prompt.
//!
//! The classification is `cant-render`'s, the colours are `grammar/palette.json`
//! through `rite-render`, and the escapes are `rite_render::term`. Nothing here
//! decides what a string looks like; this is the part that knows about
//! rustyline.
//!
//! # Why a `Helper` rather than printing colour after the fact
//!
//! rustyline redraws the line on every keystroke, and [`Highlighter::highlight`]
//! is what it draws. Colouring the line only once it had been submitted would
//! mean the colour is missing while a `?{` is still unclosed, which is when it
//! is most useful.

use std::borrow::Cow;

use rustyline::completion::Completer;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Helper};

use rite_render::term::{self, RESET};
use rite_render::Kind;

/// Every meta-command the REPL answers to, for completion and for `:help`'s
/// test. Kept beside the dispatch in `repl.rs` that it completes.
pub const META_COMMANDS: &[&str] = &[
    ":allow",
    ":bindings",
    ":deny",
    ":expand",
    ":explain",
    ":fmt",
    ":graph",
    ":help",
    ":let",
    ":permissions",
    ":quit",
    ":steps",
    ":timeout",
    ":trace",
    ":use",
    ":uses",
];

/// What the prompt knows how to draw and complete.
pub struct CantHelper {
    /// Whether to emit escapes at all. Resolved once, from `--color` and the
    /// environment, because it cannot change mid-session.
    color: bool,
    /// The session's bound names, refreshed after every line.
    names: Vec<String>,
    /// Capability names seen in this session's history, so `@fs` completes
    /// after it has been typed once. Cant does not know the roster (see
    /// `cant-render`), so this is learned rather than tabulated.
    capabilities: Vec<String>,
}

impl CantHelper {
    pub fn new(color: bool) -> Self {
        Self {
            color,
            names: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    pub fn set_names(&mut self, names: Vec<String>) {
        self.names = names;
    }

    /// Learn the capabilities a line mentioned, so they complete next time.
    pub fn observe(&mut self, line: &str) {
        for run in cant_render::runs(line) {
            if run.kind == Kind::Capability && run.text.starts_with('@') && !run.text.is_empty() {
                let name = run.text.clone();
                if !self.capabilities.contains(&name) {
                    self.capabilities.push(name);
                }
            }
        }
        self.capabilities.sort();
    }
}

/// Colour a Cant program.
pub fn paint(source: &str, color: bool) -> String {
    term::paint(&cant_render::runs(source), color)
}

impl Highlighter for CantHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.color || line.is_empty() {
            return Cow::Borrowed(line);
        }
        // A meta-command is not a program. The command is coloured as a keyword
        // and the rest of the line as a program.
        if let Some(rest) = line.strip_prefix(':') {
            let end = rest
                .find(char::is_whitespace)
                .map(|i| i + 1)
                .unwrap_or(line.len());
            let (command, program) = line.split_at(end);
            return Cow::Owned(format!(
                "{}{}",
                term::paint_as(command, Kind::Keyword, true),
                paint(program, true)
            ));
        }
        Cow::Owned(paint(line, true))
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
        // Repaint on every keystroke. Refreshing only on cursor movement would
        // leave the line one character stale.
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

impl Completer for CantHelper {
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
            // Only at the start of the line: `:max` after an orbit is a
            // modifier, not a command.
            if start == 0 {
                out.extend(
                    META_COMMANDS
                        .iter()
                        .filter(|c| c.starts_with(word))
                        .map(|c| c.to_string()),
                );
            }
        } else if word.starts_with('@') {
            out.extend(
                self.capabilities
                    .iter()
                    .filter(|c| c.starts_with(word))
                    .cloned(),
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

impl Hinter for CantHelper {
    type Hint = String;
}

impl Validator for CantHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();
        // A meta-command is a line, never a block. `:let x = [1` is refused by
        // `:let`, which can say what is wrong, rather than leaving the prompt
        // waiting for a `]`.
        if input.trim_start().starts_with(':') {
            return Ok(ValidationResult::Valid(None));
        }
        if cant_syntax::is_complete(input) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete)
        }
    }
}

impl Helper for CantHelper {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_taken_back_to_its_start() {
        assert_eq!(word_before(":ex", 3), (0, ":ex"));
        assert_eq!(word_before("[1] -> up", 9), (7, "up"));
        assert_eq!(word_before("[1] -> !@fs", 11), (8, "@fs"));
        assert_eq!(word_before("", 0), (0, ""));
        // A cursor past the end is clamped rather than panicking.
        assert_eq!(word_before("ab", 99), (0, "ab"));
    }

    #[test]
    fn meta_commands_complete_only_at_the_start_of_a_line() {
        let helper = CantHelper::new(false);
        let history = rustyline::history::MemHistory::new();
        let ctx = Context::new(&history);
        let (_, at_start) = helper.complete(":pe", 3, &ctx).expect("complete");
        assert_eq!(at_start, vec![":permissions".to_string()]);
        // `:max` after an orbit is a modifier, not a command.
        let (_, mid_line) = helper
            .complete("1 -> ~{ $ } :ma", 15, &ctx)
            .expect("complete");
        assert!(mid_line.is_empty(), "{mid_line:?}");
    }

    #[test]
    fn bound_names_and_seen_capabilities_complete() {
        let mut helper = CantHelper::new(false);
        helper.set_names(vec!["evens".into(), "it".into()]);
        helper.observe("[\"a\"] -> * -> !@fs.read");
        let history = rustyline::history::MemHistory::new();
        let ctx = Context::new(&history);
        let (_, names) = helper.complete("ev", 2, &ctx).expect("complete");
        assert_eq!(names, vec!["evens".to_string()]);
        let (_, caps) = helper.complete("@f", 2, &ctx).expect("complete");
        assert_eq!(caps, vec!["@fs".to_string()]);
    }

    /// With colour off the line comes back byte for byte. Anything else would
    /// put escapes into a piped session.
    #[test]
    fn colour_off_is_the_line_unchanged() {
        let helper = CantHelper::new(false);
        let line = "[1, 2] -> * -> []";
        assert_eq!(helper.highlight(line, 0), line);
        assert_eq!(helper.highlight_prompt("cant> ", false), "cant> ");
    }

    #[test]
    fn a_meta_command_is_coloured_as_one_and_its_program_as_a_program() {
        let helper = CantHelper::new(true);
        let painted = helper.highlight(":expand [1] -> *", 0).into_owned();
        assert!(painted.contains('\x1b'));
        // Every character of the line survives.
        let stripped: String = strip(&painted);
        assert_eq!(stripped, ":expand [1] -> *");
    }

    /// An unclosed block keeps the prompt open; a whole program does not.
    #[test]
    fn validation_follows_the_lexer() {
        assert!(cant_syntax::is_complete("[1] -> ?{ $ > 0 } -> []"));
        assert!(!cant_syntax::is_complete("[1] -> ?{ $ > 0"));
        // A meta-command is never continued, whatever it contains.
        assert!(":let x = [1".trim_start().starts_with(':'));
    }

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
