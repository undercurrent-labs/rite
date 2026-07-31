//! Lay coloured runs out on a monospace grid and write SVG.
//!
//! Positions are computed rather than measured: every glyph in a monospace face
//! is one advance wide, so column `n` sits at `n * advance` and there is no need
//! to load the font to know where anything goes. That is what lets `svg` — the
//! format that does *not* embed a face — still line up in a viewer that has a
//! different monospace font, and what keeps this crate free of a text-shaping
//! dependency.
//!
//! The exception is a character that is not one column wide. Rite's glyphs are
//! not: `⟦` and friends are drawn as wide as they need to be. They are still
//! *advanced* one column, because that is how a terminal and an editor treat
//! them, and matching what the author saw while typing beats matching the font's
//! metrics.

use crate::palette::{Kind, Palette};
use crate::tokens::Run;
use crate::{Frame, RenderOptions};

/// Width of one column as a fraction of the font size.
///
/// DejaVu Sans Mono advances 1233/2048 em. Hardcoded rather than read from the
/// face because the `svg` format has no face to read, and the two formats must
/// lay out identically or the same source would produce two different pictures.
const ADVANCE: f32 = 0.602;

/// Baseline-to-baseline distance, as a fraction of the font size.
const LINE_HEIGHT: f32 = 1.45;

/// Room around the code inside the frame.
const PAD_X: f32 = 16.0;
const PAD_Y: f32 = 14.0;

/// Height of a window frame's title bar.
const TITLE_BAR: f32 = 28.0;

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// One line's worth of runs, with the column each starts at.
struct Placed {
    column: usize,
    kind: Kind,
    text: String,
}

/// Break runs at newlines, so each line can be positioned on its own baseline.
fn lines(runs: &[Run]) -> Vec<Vec<Placed>> {
    let mut out: Vec<Vec<Placed>> = vec![Vec::new()];
    let mut column = 0usize;
    for run in runs {
        for (i, piece) in run.text.split('\n').enumerate() {
            if i > 0 {
                out.push(Vec::new());
                column = 0;
            }
            if piece.is_empty() {
                continue;
            }
            // Columns, not bytes: a glyph is one column and several bytes.
            let width = piece.chars().count();
            // Leading whitespace only moves the cursor; drawing it would put an
            // empty `<text>` in the output for every indent.
            if !piece.trim().is_empty() {
                out.last_mut().unwrap().push(Placed {
                    column,
                    kind: run.kind,
                    text: piece.to_string(),
                });
            }
            column += width;
        }
    }
    // A trailing newline makes a last empty line; it is not a line of code.
    if out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

pub fn render(runs: &[Run], opts: &RenderOptions, font_face: Option<&str>) -> String {
    let palette = Palette::shared();
    let lines = lines(runs);
    let size = opts.font_size;
    let advance = size * ADVANCE;
    let line_height = size * LINE_HEIGHT;

    let columns = lines
        .iter()
        .map(|l| {
            l.last()
                .map(|p| p.column + p.text.chars().count())
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);
    // At least one line, so an empty file still produces a picture rather than a
    // zero-height SVG that viewers refuse to open.
    let rows = lines.len().max(1);

    let code_w = columns as f32 * advance;
    let code_h = rows as f32 * line_height;
    let top = match opts.frame {
        Frame::Window => TITLE_BAR,
        _ => 0.0,
    };
    let width = code_w + PAD_X * 2.0;
    let height = code_h + PAD_Y * 2.0 + top;

    let bg = &palette.background;
    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width:.0}\" height=\"{height:.0}\" \
         viewBox=\"0 0 {width:.0} {height:.0}\" role=\"img\">\n"
    ));

    if let Some(face) = font_face {
        // The face travels with the picture, so it renders the same everywhere —
        // including in viewers with no monospace font worth the name.
        svg.push_str(
            "<defs><style>\n@font-face{font-family:'RiteMono';src:url(data:font/ttf;base64,",
        );
        svg.push_str(face);
        svg.push_str(") format('truetype');}\n</style></defs>\n");
    }

    match opts.frame {
        Frame::Text => {
            svg.push_str(&format!(
                "<rect width=\"100%\" height=\"100%\" fill=\"{bg}\"/>\n"
            ));
        }
        Frame::Box => {
            svg.push_str(&format!(
                "<rect x=\"0.5\" y=\"0.5\" width=\"{:.0}\" height=\"{:.0}\" rx=\"10\" fill=\"{bg}\" \
                 stroke=\"{}\" stroke-opacity=\"0.35\"/>\n",
                width - 1.0,
                height - 1.0,
                palette.style(Kind::Capability).color
            ));
        }
        Frame::Window => {
            svg.push_str(&format!(
                "<rect width=\"{width:.0}\" height=\"{height:.0}\" rx=\"10\" fill=\"{bg}\"/>\n"
            ));
            svg.push_str(&format!(
                "<line x1=\"0\" y1=\"{TITLE_BAR}\" x2=\"{width:.0}\" y2=\"{TITLE_BAR}\" \
                 stroke=\"{}\" stroke-opacity=\"0.25\"/>\n",
                palette.style(Kind::Plain).color
            ));
            // Three dots, in the palette's own colours rather than a borrowed
            // traffic-light red/amber/green that belongs to somebody else's OS.
            for (i, kind) in [Kind::Atom, Kind::Http, Kind::String].iter().enumerate() {
                svg.push_str(&format!(
                    "<circle cx=\"{:.0}\" cy=\"{:.0}\" r=\"5\" fill=\"{}\" fill-opacity=\"0.85\"/>\n",
                    18 + i * 18,
                    TITLE_BAR / 2.0,
                    palette.style(*kind).color
                ));
            }
        }
    }

    let family = match font_face {
        Some(_) => "'RiteMono', ui-monospace, monospace",
        None => "ui-monospace, 'DejaVu Sans Mono', 'Menlo', 'Consolas', monospace",
    };
    svg.push_str(&format!(
        "<g font-family=\"{family}\" font-size=\"{size}\" xml:space=\"preserve\">\n"
    ));

    for (row, line) in lines.iter().enumerate() {
        // `dominant-baseline` is inconsistent between renderers, so the baseline
        // is placed outright: the first one sits a bit under the top padding.
        let y = top + PAD_Y + line_height * row as f32 + size * 0.78;
        for placed in line {
            let style = palette.style(placed.kind);
            let x = PAD_X + placed.column as f32 * advance;
            svg.push_str(&format!(
                "<text x=\"{x:.2}\" y=\"{y:.2}\" fill=\"{}\"",
                style.color
            ));
            if style.italic {
                svg.push_str(" font-style=\"italic\"");
            }
            if let Some(weight) = style.weight {
                svg.push_str(&format!(" font-weight=\"{weight}\""));
            }
            svg.push('>');
            svg.push_str(&escape(&placed.text));
            svg.push_str("</text>\n");
        }
    }

    svg.push_str("</g>\n</svg>\n");
    svg
}
