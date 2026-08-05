//! The same palette, on a terminal.
//!
//! [`crate::runs`] classifies Rite; a sibling front end classifies its own
//! source into the same [`Run`]s. Everything downstream of that — which colour a
//! [`Kind`] is, whether colour is wanted at all, and how to spell it for a
//! terminal — is here, once, so a REPL and an image cannot disagree about what
//! a string looks like.
//!
//! # The palette is dark
//!
//! `grammar/palette.json` is built against a `#121821` background and checked
//! for contrast against it. On a light terminal these colours are weak. The fix
//! is not a second table — that is exactly the drift the palette file's own
//! header warns about — it is [`ColorMode::Never`], which every caller exposes.

use crate::palette::{Kind, Palette, Style};
use crate::tokens::Run;
use std::io::IsTerminal;

/// Whether to emit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Colour when the destination is a terminal that has not asked otherwise.
    #[default]
    Auto,
    Always,
    Never,
}

impl ColorMode {
    /// Parse the `--color` spelling every tool takes.
    pub fn parse(spec: &str) -> Result<Self, String> {
        match spec {
            "auto" => Ok(ColorMode::Auto),
            "always" | "yes" | "true" => Ok(ColorMode::Always),
            "never" | "no" | "false" => Ok(ColorMode::Never),
            other => Err(format!(
                "unknown colour setting `{other}` — expected auto, always or never"
            )),
        }
    }
}

/// Should this run be coloured?
///
/// `NO_COLOR` wins over everything but an explicit `--color always`, which is
/// the [no-color.org](https://no-color.org) contract: the variable is an opt-out
/// for output the user did not ask to be coloured, not a veto over a flag they
/// typed themselves. `CLICOLOR_FORCE` is the other half of the same convention,
/// for a pipe into something that renders escapes.
pub fn enabled(mode: ColorMode) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            if env_set("NO_COLOR") {
                return false;
            }
            if env_set("CLICOLOR_FORCE") {
                return true;
            }
            std::io::stdout().is_terminal()
        }
    }
}

/// Set *and* non-empty. `NO_COLOR=` is how a shell profile un-sets a variable it
/// exported earlier, and treating that as "no colour" would make it impossible
/// to turn back on.
fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// The escape that starts a run in this style, as 24-bit colour.
///
/// Truecolour rather than the 256-colour cube: the palette is specified in hex
/// and checked for contrast in hex, and quantising it here would mean the
/// contrast figures `palette_sync` derives are not the ones anybody sees.
pub fn ansi(style: &Style) -> String {
    let mut out = String::with_capacity(24);
    if style.italic {
        out.push_str("\x1b[3m");
    }
    if style.weight.is_some_and(|w| w >= 600) {
        out.push_str("\x1b[1m");
    }
    if let Some((r, g, b)) = rgb(&style.color) {
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
    }
    out
}

/// Ends any run. A full reset rather than the individual off-codes, because a
/// run can carry colour, italics and weight at once.
pub const RESET: &str = "\x1b[0m";

/// `#rrggbb` → components. `None` for anything else, which colours nothing
/// rather than colouring wrongly.
fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// The escape sequence for a kind, ready to concatenate.
pub fn start(kind: Kind) -> String {
    ansi(Palette::shared().style(kind))
}

/// Colour a classified source. With `on` false this is exactly the input text
/// back, which is what makes it safe to call unconditionally.
pub fn paint(runs: &[Run], on: bool) -> String {
    let mut out = String::new();
    for run in runs {
        if on && run.kind != Kind::Plain {
            out.push_str(&start(run.kind));
            out.push_str(&run.text);
            out.push_str(RESET);
        } else {
            out.push_str(&run.text);
        }
    }
    out
}

/// One string in one kind's colour — for a prompt, a label, a value.
pub fn paint_as(text: &str, kind: Kind, on: bool) -> String {
    if on {
        format!("{}{text}{RESET}", start(kind))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind in the palette has to produce something a terminal will take.
    /// A kind whose colour did not parse would print as plain text with no sign
    /// that anything was wrong.
    #[test]
    fn every_kind_has_a_colour() {
        for kind in Kind::ALL {
            let escape = start(kind);
            assert!(
                escape.contains("\x1b[38;2;"),
                "{} has no 24-bit colour: {escape:?}",
                kind.as_str()
            );
        }
    }

    #[test]
    fn hex_parses_to_components() {
        assert_eq!(rgb("#c3e88d"), Some((0xc3, 0xe8, 0x8d)));
        assert_eq!(rgb("#000000"), Some((0, 0, 0)));
        assert_eq!(rgb("c3e88d"), None, "the hash is required");
        assert_eq!(rgb("#abc"), None, "short form is not the spelling used");
        assert_eq!(rgb("#gggggg"), None);
    }

    /// Italics and weight travel with the colour: `comment` is italic in the
    /// palette, and losing that on a terminal would make a comment read as code.
    #[test]
    fn italics_and_weight_come_through() {
        assert!(start(Kind::Comment).starts_with("\x1b[3m"), "comment");
        // `http` carries weight 500 — below the bold threshold, deliberately:
        // terminal bold is a different thing from a 500 weight face.
        assert!(!start(Kind::Http).contains("\x1b[1m"), "http");
    }

    #[test]
    fn painting_off_returns_the_source_unchanged() {
        let runs = crate::runs("◆ f(n) ⟦ ^ n * 2 ⟧\n");
        assert_eq!(paint(&runs, false), "◆ f(n) ⟦ ^ n * 2 ⟧\n");
        let painted = paint(&runs, true);
        assert!(painted.contains('\x1b'));
        // Every byte of the source survives colouring.
        assert_eq!(strip(&painted), "◆ f(n) ⟦ ^ n * 2 ⟧\n");
    }

    /// `--color always` beats `NO_COLOR`: the variable opts out of colour
    /// nobody asked for, and a flag on the command line is asking.
    #[test]
    fn explicit_settings_do_not_consult_the_environment() {
        assert!(enabled(ColorMode::Always));
        assert!(!enabled(ColorMode::Never));
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
