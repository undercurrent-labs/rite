//! Render highlighted Rite source as an image.
//!
//! One highlighter, in Rust, reading the language's own lexer and the one colour
//! table in `grammar/palette.json`. The site's TypeScript tokeniser is the other
//! implementation today; it goes when Studio takes its highlighting from here
//! through `rite-wasm`, which is what keeps a picture of Rite and the Rite on the
//! page from disagreeing.
//!
//! ```
//! use rite_render::{render, Format, Frame, RenderOptions};
//!
//! let svg = render("◆ f(n) ⟦ ^ n * 2 ⟧\n", &RenderOptions::default()).unwrap();
//! assert!(svg.starts_with("<svg"));
//! ```

mod palette;
mod svg;
mod tokens;

pub use palette::{Kind, Palette, Style};
pub use tokens::{runs, Run};

/// What to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Small, and relies on the viewer having a monospace font. Layout still
    /// lines up, because positions are computed per column rather than measured.
    #[default]
    Svg,
    /// Self-contained: the face travels with the picture, so it renders the same
    /// everywhere. Larger by the size of the font.
    SvgFont,
}

/// The chrome around the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Frame {
    /// Background and nothing else.
    #[default]
    Text,
    /// A rounded border.
    Box,
    /// A title bar with three dots — for a screenshot that wants to look like one.
    Window,
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub format: Format,
    pub frame: Frame,
    pub font_size: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            format: Format::default(),
            frame: Frame::default(),
            // Large enough to read in a README at full width without scaling.
            font_size: 15.0,
        }
    }
}

#[derive(Debug)]
pub enum RenderError {
    /// `svg-font` was asked for and no face is available to embed.
    NoFont(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoFont(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render source to SVG.
pub fn render(source: &str, opts: &RenderOptions) -> Result<String, RenderError> {
    let runs = tokens::runs(source);
    let face = match opts.format {
        Format::Svg => None,
        Format::SvgFont => Some(embedded_font()?),
    };
    Ok(svg::render(&runs, opts, face.as_deref()))
}

/// The face `svg-font` embeds, base64-encoded.
///
/// DejaVu Sans Mono, chosen by checking Rite's glyphs one at a time: it has 18 of
/// the 19. Noto Sans Mono has 14 and is missing `⊏`, which Rite uses for `use`
/// and middleware. `⊻` (xor) is in almost no monospace face at all, and has an
/// ASCII spelling, so that one is a documented gap rather than a blocker.
///
/// Read from the system rather than committed: the whole face is around 450 KB,
/// which is a large thing to put in a git history for a feature not everyone
/// uses. A pre-subsetted face committed here would be single-digit KB and is the
/// obvious improvement — it needs `fontTools`, which was not available when this
/// was written.
fn embedded_font() -> Result<String, RenderError> {
    const CANDIDATES: [&str; 6] = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/local/share/fonts/DejaVuSansMono.ttf",
        "/Library/Fonts/DejaVuSansMono.ttf",
        "/System/Library/Fonts/Menlo.ttc",
    ];
    let path = std::env::var("RITE_RENDER_FONT")
        .ok()
        .filter(|p| std::path::Path::new(p).is_file())
        .or_else(|| {
            CANDIDATES
                .iter()
                .find(|p| std::path::Path::new(p).is_file())
                .map(|p| p.to_string())
        })
        .ok_or_else(|| {
            RenderError::NoFont(
                "no DejaVu Sans Mono found to embed. Install it, or point \
                 RITE_RENDER_FONT at a .ttf — or use --format svg, which relies on \
                 the viewer's own monospace font"
                    .into(),
            )
        })?;
    let bytes = std::fs::read(&path)
        .map_err(|e| RenderError::NoFont(format!("reading font {path}: {e}")))?;
    Ok(base64(&bytes))
}

/// Base64, written out rather than pulled in: one dependency for forty lines is
/// not a trade this crate needs to make.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        // RFC 4648's own examples, including both padding lengths.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_palette_covers_every_kind() {
        let p = Palette::shared();
        for kind in Kind::ALL {
            // Panics if the table is missing one, which is the point.
            let style = p.style(kind);
            assert!(
                style.color.starts_with('#'),
                "{} has no colour",
                kind.as_str()
            );
        }
    }
}
