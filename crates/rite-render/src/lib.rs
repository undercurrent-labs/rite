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
    /// Rasterised, for somewhere that will not take an SVG at all. CLI only —
    /// see [`render_png`].
    Png,
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
    /// PNG rasterisation failed.
    Raster(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoFont(m) | RenderError::Raster(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Render source to SVG.
pub fn render(source: &str, opts: &RenderOptions) -> Result<String, RenderError> {
    let runs = tokens::runs(source);
    let face = match opts.format {
        Format::Svg => None,
        // A caller asking for PNG through the SVG entry point gets the
        // self-contained SVG that PNG is rasterised from, rather than a refusal.
        Format::SvgFont | Format::Png => Some(embedded_font()?),
    };
    Ok(svg::render(&runs, opts, face.as_deref()))
}

/// Render source to PNG bytes.
///
/// Behind the `png` feature so the browser build and anything embedding the
/// highlighter do not pull in a rasteriser and a font stack they cannot use.
/// Studio produces its PNGs from `svg-font` through a canvas instead, which needs
/// no rasteriser in the browser and is WYSIWYG by construction.
///
/// Rasterising needs real glyph outlines — the computed layout says *where* each
/// run goes, not what it looks like — so this reads the system's fonts. A missing
/// face is a picture with holes in it, which is worth failing over rather than
/// shipping.
#[cfg(feature = "png")]
pub fn render_png(source: &str, opts: &RenderOptions, scale: f32) -> Result<Vec<u8>, RenderError> {
    rasterise(source, opts, scale)?
        .encode_png()
        .map_err(|e| RenderError::Raster(format!("encoding PNG: {e}")))
}

/// The pixels behind [`render_png`], so a test can look at them.
///
/// It has to: the first PNG this produced had the frame, the background and the
/// window dots, and no text — and every SVG assertion passed while it did,
/// because the markup was correct and only the rasteriser disagreed. A test that
/// reads markup cannot see an empty picture.
#[cfg(feature = "png")]
/// Rasterise arbitrary SVG markup to a PNG.
///
/// Split out of [`render_png`], which rasterises *highlighted Rite source*. This
/// takes any SVG, which is what a caller with a hand-authored one needs — a
/// social card, a brand mark, a diagram — and is the only part of the pipeline
/// that is not about Rite at all.
///
/// Fonts resolve through `usvg`'s own database, seeded with the system fonts and
/// the face this crate embeds, so text in the markup draws rather than silently
/// vanishing. That failure mode is why this is worth being a real API rather
/// than something each caller reimplements: the first PNG this crate ever
/// produced had every shape and no text, and every SVG assertion passed while it
/// did.
#[cfg(feature = "png")]
pub fn svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, RenderError> {
    rasterise_svg(svg, scale)?
        .encode_png()
        .map_err(|e| RenderError::Raster(format!("encoding PNG: {e}")))
}

#[cfg(feature = "png")]
fn rasterise_svg(svg: &str, scale: f32) -> Result<resvg::tiny_skia::Pixmap, RenderError> {
    use resvg::tiny_skia;
    use resvg::usvg;

    let mut options = usvg::Options::default();
    let db = options.fontdb_mut();
    db.load_system_fonts();
    if let Some(path) = font_path() {
        let _ = db.load_font_file(path);
    }
    options.font_family = "DejaVu Sans Mono".to_string();
    let tree = usvg::Tree::from_str(svg, &options)
        .map_err(|e| RenderError::Raster(format!("parsing the SVG: {e}")))?;

    let size = tree.size();
    let (w, h) = (
        (size.width() * scale).ceil() as u32,
        (size.height() * scale).ceil() as u32,
    );
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1))
        .ok_or_else(|| RenderError::Raster(format!("cannot allocate a {w}×{h} image")))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

fn rasterise(
    source: &str,
    opts: &RenderOptions,
    scale: f32,
) -> Result<resvg::tiny_skia::Pixmap, RenderError> {
    // *Not* the self-contained SVG. `usvg` resolves fonts through its own
    // database and ignores an `@font-face` with a data URL, so rasterising the
    // embedded form produced a picture with the frame, the background and the
    // window dots — and no text at all. Every SVG test passed while it did,
    // because the markup was right; only looking at the PNG showed it.
    //
    // So: plain SVG, and the face goes into the database instead.
    let svg = render(
        source,
        &RenderOptions {
            format: Format::Svg,
            ..opts.clone()
        },
    )?;
    // The font handling — system database, the embedded face by path, and a real
    // monospace fallback because `ui-monospace` is a CSS keyword nothing has
    // installed — now lives in `rasterise_svg`, which is the only part of this
    // that was never about Rite.
    rasterise_svg(&svg, scale)
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
    let path = font_path().ok_or_else(|| {
        RenderError::NoFont(
            "no DejaVu Sans Mono found to embed. Install it, or point \
             RITE_RENDER_FONT at a .ttf — or use --format svg, which relies on \
             the viewer's own monospace font"
                .into(),
        )
    })?;
    let bytes = std::fs::read(&path)
        .map_err(|e| RenderError::NoFont(format!("reading font {}: {e}", path.display())))?;
    Ok(base64(&bytes))
}

/// Where the face lives on this machine, if it does.
fn font_path() -> Option<std::path::PathBuf> {
    const CANDIDATES: [&str; 6] = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
        "/usr/local/share/fonts/DejaVuSansMono.ttf",
        "/Library/Fonts/DejaVuSansMono.ttf",
        "/System/Library/Fonts/Menlo.ttc",
    ];
    std::env::var("RITE_RENDER_FONT")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            CANDIDATES
                .iter()
                .map(std::path::PathBuf::from)
                .find(|p| p.is_file())
        })
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

    /// The rasterised picture actually has code in it.
    ///
    /// The first version of `render_png` produced a frame with nothing inside:
    /// `usvg` resolves fonts through its own database and ignores an
    /// `@font-face` data URL, so every glyph silently drew as nothing. The SVG
    /// tests all passed. This one counts pixels, which is the only way to notice.
    #[cfg(feature = "png")]
    #[test]
    fn the_rasterised_image_contains_glyphs() {
        let opts = RenderOptions {
            frame: Frame::Text,
            ..Default::default()
        };
        let code = rasterise("◆! main() ⟦\n  ^ 42\n⟧\n", &opts, 2.0).expect("rasterise");
        let blank = rasterise("\n\n\n", &opts, 2.0).expect("rasterise");

        // The background is one flat colour, so anything else is drawn ink.
        let ink = |p: &resvg::tiny_skia::Pixmap| {
            let bg = p.pixel(1, 1).expect("a pixel");
            p.pixels().iter().filter(|px| **px != bg).count()
        };
        let drawn = ink(&code);
        assert!(
            drawn > 500,
            "the picture is nearly empty ({drawn} non-background pixels) — the \
             rasteriser probably found no font for the text"
        );
        assert!(
            drawn > ink(&blank) * 4,
            "code drew about as little as blank lines did"
        );
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
