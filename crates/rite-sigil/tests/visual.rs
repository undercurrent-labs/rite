//! Visual regression, by perceptual hash rather than by bytes.
//!
//! §26.4 asks for rasterised comparison. It also warns against relying on pixel
//! snapshots alone, and the reason is in the PNG story: `resvg`'s output is
//! deterministic on one machine but not promised stable across its own releases,
//! and it loads system fonts, so a byte comparison would fail on a machine with
//! a different font set for reasons that have nothing to do with the renderer.
//!
//! So the assertion is structural: a coarse perceptual hash — average luminance
//! per cell of an 8×8 grid, thresholded against the mean — which is stable under
//! antialiasing differences and a subpixel of layout drift, and changes when the
//! composition does.
//!
//! What this catches, and what the SVG goldens cannot: a theme whose strokes
//! vanish against its own background, an ornament level that swallows the
//! semantics, a glow filter that blows out the picture. All of those are
//! well-formed SVG with correct coordinates.

#![cfg(feature = "png")]

use rite_sigil::{
    build_scene, normalize, render_png, EdgeId, EdgeKind, LayoutOptions, NormalizeOptions,
    OrnamentLevel, PortRef, SigilEdge, SigilGraph, SigilNode, SigilNodeKind, SourceLanguage,
    SvgOptions, ThemeId,
};

fn sample_graph() -> SigilGraph {
    let kinds = [
        ("n0", SigilNodeKind::Source),
        ("n1", SigilNodeKind::Scatter),
        ("n2", SigilNodeKind::Ward),
        ("n3", SigilNodeKind::Orbit),
        ("n4", SigilNodeKind::Collect),
        ("n5", SigilNodeKind::Output),
    ];
    let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
    for (id, kind) in &kinds {
        g.nodes.push(SigilNode::new(*id, kind.clone()));
    }
    for (i, pair) in kinds.windows(2).enumerate() {
        g.edges.push(SigilEdge {
            id: EdgeId::new(format!("e{i}")),
            from: PortRef::new(pair[0].0, 0),
            to: PortRef::new(pair[1].0, 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
    }
    g.exits.push("n5".into());
    g
}

fn png(theme: ThemeId, ornament: OrnamentLevel) -> Vec<u8> {
    let normalized = normalize(sample_graph(), &NormalizeOptions::default()).expect("valid");
    let scene = build_scene(
        &normalized,
        &LayoutOptions {
            ornament,
            ..LayoutOptions::canonical()
        },
    );
    render_png(
        &scene,
        &SvgOptions {
            theme,
            ..Default::default()
        },
        // Small on purpose: the hash is 8×8, and rendering large to downsample
        // would only cost time.
        0.25,
    )
    .expect("rasterises")
}

/// Decode a PNG into 8-bit grayscale and its dimensions.
///
/// A minimal decoder rather than the `png` crate, because the only consumer is
/// this file and `resvg` already produced the bytes — pulling a decoder in as a
/// dev-dependency to read what we just wrote is a dependency for nothing.
fn luminance_grid(bytes: &[u8]) -> [f64; 64] {
    // `resvg`'s `encode_png` writes non-interlaced 8-bit RGBA, which is the one
    // shape this needs to handle. Anything else is a change worth failing on.
    let pixmap = decode_rgba(bytes);
    let (width, height, pixels) = pixmap;
    let mut grid = [0.0f64; 64];
    let mut counts = [0.0f64; 64];
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let (r, g, b, a) = (
                pixels[i] as f64,
                pixels[i + 1] as f64,
                pixels[i + 2] as f64,
                pixels[i + 3] as f64 / 255.0,
            );
            // Composited against black, so a transparent background and a black
            // one hash alike — which is what we want: the question is where the
            // ink is.
            let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b) * a;
            let cell = (y * 8 / height) * 8 + (x * 8 / width);
            grid[cell] += luma;
            counts[cell] += 1.0;
        }
    }
    for i in 0..64 {
        if counts[i] > 0.0 {
            grid[i] /= counts[i];
        }
    }
    grid
}

/// The perceptual hash: each cell brighter than the mean is a set bit.
fn phash(bytes: &[u8]) -> u64 {
    let grid = luminance_grid(bytes);
    let mean: f64 = grid.iter().sum::<f64>() / 64.0;
    let mut hash = 0u64;
    for (i, cell) in grid.iter().enumerate() {
        if *cell > mean {
            hash |= 1 << i;
        }
    }
    hash
}

fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// A tiny PNG reader: IHDR for the size, IDAT inflated, filters undone.
fn decode_rgba(bytes: &[u8]) -> (usize, usize, Vec<u8>) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let mut i = 8;
    let mut width = 0usize;
    let mut height = 0usize;
    let mut idat: Vec<u8> = Vec::new();
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().expect("4 bytes")) as usize;
        let kind = &bytes[i + 4..i + 8];
        let data = &bytes[i + 8..i + 8 + len];
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(data[0..4].try_into().expect("4")) as usize;
                height = u32::from_be_bytes(data[4..8].try_into().expect("4")) as usize;
                assert_eq!(data[8], 8, "expected 8-bit channels");
                assert_eq!(data[9], 6, "expected RGBA");
                assert_eq!(data[12], 0, "expected non-interlaced");
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    let raw = inflate(&idat);
    let stride = width * 4;
    let mut out = vec![0u8; stride * height];
    let mut pos = 0;
    for y in 0..height {
        let filter = raw[pos];
        pos += 1;
        for x in 0..stride {
            let value = raw[pos + x];
            let a = if x >= 4 { out[y * stride + x - 4] } else { 0 };
            let b = if y > 0 { out[(y - 1) * stride + x] } else { 0 };
            let c = if x >= 4 && y > 0 {
                out[(y - 1) * stride + x - 4]
            } else {
                0
            };
            out[y * stride + x] = match filter {
                0 => value,
                1 => value.wrapping_add(a),
                2 => value.wrapping_add(b),
                3 => value.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => value.wrapping_add(paeth(a, b, c)),
                f => panic!("unknown PNG filter {f}"),
            };
        }
        pos += stride;
    }
    (width, height, out)
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let (pa, pb, pc) = (
        (p - a as i32).abs(),
        (p - b as i32).abs(),
        (p - c as i32).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Just enough of DEFLATE for what `resvg` emits.
///
/// Stored and fixed-Huffman blocks are handled directly; dynamic blocks — which
/// is what a real encoder produces — go through the full table build. It is
/// about a hundred lines and it removes a dependency whose only job would be to
/// read bytes this process just wrote.
fn inflate(data: &[u8]) -> Vec<u8> {
    // zlib header: two bytes, then the DEFLATE stream.
    let mut bits = BitReader::new(&data[2..]);
    let mut out = Vec::new();
    loop {
        let final_block = bits.bits(1) == 1;
        match bits.bits(2) {
            0 => {
                bits.align();
                let len = bits.bits(16) as usize;
                let _nlen = bits.bits(16);
                for _ in 0..len {
                    out.push(bits.bits(8) as u8);
                }
            }
            1 => {
                let (lit, dist) = fixed_tables();
                inflate_block(&mut bits, &lit, &dist, &mut out);
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits);
                inflate_block(&mut bits, &lit, &dist, &mut out);
            }
            _ => panic!("reserved DEFLATE block type"),
        }
        if final_block {
            break;
        }
    }
    out
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            bit: 0,
        }
    }
    fn bits(&mut self, count: u32) -> u32 {
        let mut value = 0;
        for i in 0..count {
            let byte = self.data[self.pos];
            let b = (byte >> self.bit) & 1;
            value |= (b as u32) << i;
            self.bit += 1;
            if self.bit == 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        value
    }
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// Canonical Huffman: (code lengths) -> a decode table of (length, symbol).
struct Huffman {
    counts: Vec<u16>,
    symbols: Vec<u16>,
}

fn build_huffman(lengths: &[u8]) -> Huffman {
    let max = 16;
    let mut counts = vec![0u16; max];
    for &l in lengths {
        counts[l as usize] += 1;
    }
    counts[0] = 0;
    let mut offsets = vec![0u16; max];
    for i in 1..max - 1 {
        offsets[i + 1] = offsets[i] + counts[i];
    }
    let mut symbols = vec![0u16; lengths.len()];
    for (symbol, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbols[offsets[l as usize] as usize] = symbol as u16;
            offsets[l as usize] += 1;
        }
    }
    Huffman { counts, symbols }
}

fn decode(bits: &mut BitReader, table: &Huffman) -> u16 {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for length in 1..16 {
        code |= bits.bits(1) as i32;
        let count = table.counts[length] as i32;
        if code - first < count {
            return table.symbols[(index + (code - first)) as usize];
        }
        index += count;
        first = (first + count) << 1;
        code <<= 1;
    }
    panic!("invalid Huffman code");
}

fn fixed_tables() -> (Huffman, Huffman) {
    let mut lengths = vec![8u8; 288];
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    (build_huffman(&lengths), build_huffman(&[5u8; 30]))
}

fn dynamic_tables(bits: &mut BitReader) -> (Huffman, Huffman) {
    let hlit = bits.bits(5) as usize + 257;
    let hdist = bits.bits(5) as usize + 1;
    let hclen = bits.bits(4) as usize + 4;
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut code_lengths = vec![0u8; 19];
    for &i in ORDER.iter().take(hclen) {
        code_lengths[i] = bits.bits(3) as u8;
    }
    let code_table = build_huffman(&code_lengths);

    let mut lengths = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        let symbol = decode(bits, &code_table);
        match symbol {
            0..=15 => lengths.push(symbol as u8),
            16 => {
                let prev = *lengths.last().expect("a previous length");
                let repeat = 3 + bits.bits(2);
                lengths.resize(lengths.len() + repeat as usize, prev);
            }
            17 => {
                let repeat = 3 + bits.bits(3);
                lengths.resize(lengths.len() + repeat as usize, 0);
            }
            _ => {
                let repeat = 11 + bits.bits(7);
                lengths.resize(lengths.len() + repeat as usize, 0);
            }
        }
    }
    (
        build_huffman(&lengths[..hlit]),
        build_huffman(&lengths[hlit..]),
    )
}

fn inflate_block(bits: &mut BitReader, lit: &Huffman, dist: &Huffman, out: &mut Vec<u8>) {
    const LEN_BASE: [u16; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const LEN_EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    const DIST_BASE: [u16; 30] = [
        1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
        2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    const DIST_EXTRA: [u32; 30] = [
        0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12,
        13, 13,
    ];

    loop {
        let symbol = decode(bits, lit);
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return,
            _ => {
                let i = symbol as usize - 257;
                let length = LEN_BASE[i] as usize + bits.bits(LEN_EXTRA[i]) as usize;
                let d = decode(bits, dist) as usize;
                let distance = DIST_BASE[d] as usize + bits.bits(DIST_EXTRA[d]) as usize;
                let start = out.len() - distance;
                for k in 0..length {
                    out.push(out[start + k]);
                }
            }
        }
    }
}

#[test]
fn a_render_rasterises_to_a_real_image() {
    let bytes = png(ThemeId::NeonRitual, OrnamentLevel::Ritual);
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h, pixels) = decode_rgba(&bytes);
    assert_eq!(w, 400, "1600 at 0.25 scale");
    assert_eq!(h, 400);
    assert_eq!(pixels.len(), w * h * 4);
}

/// The same scene rasterises to the same picture.
#[test]
fn rasterisation_is_stable_across_runs() {
    let a = phash(&png(ThemeId::NeonRitual, OrnamentLevel::Ritual));
    let b = phash(&png(ThemeId::NeonRitual, OrnamentLevel::Ritual));
    assert_eq!(a, b);
}

/// The check the SVG goldens cannot make: there is ink, and it is not
/// everywhere. A theme whose strokes vanished against its background and one
/// that blew out to solid white are both well-formed SVG.
#[test]
fn every_theme_produces_a_picture_with_contrast_in_it() {
    for theme in ThemeId::ALL {
        let bytes = png(*theme, OrnamentLevel::Ritual);
        let grid = luminance_grid(&bytes);
        let min = grid.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = grid.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            max - min > 4.0,
            "{}: the rendered image is nearly uniform ({min:.1}..{max:.1}) — \
             either nothing drew or everything did",
            theme.name()
        );
    }
}

/// A perceptual hash is deliberately blind to colour — it thresholds each cell
/// against the image's own mean, so recolouring a picture leaves the hash alone.
/// That is exactly what makes it a good *composition* check and a useless theme
/// check: `neon-ritual` and `void` draw the same shapes in different colours and
/// hash identically, which is the hash working.
///
/// So the raster asserts the thing it can see and the SVG goldens cannot: the
/// ground. `parchment` is ink on a light field and the other two are light on a
/// dark one, and a theme that lost its background — or drew it in the wrong
/// polarity — would still be well-formed SVG with correct coordinates.
#[test]
fn a_themes_ground_is_the_polarity_it_claims() {
    let mean = |theme: ThemeId| {
        let grid = luminance_grid(&png(theme, OrnamentLevel::Ritual));
        grid.iter().sum::<f64>() / 64.0
    };
    let dark = [ThemeId::NeonRitual, ThemeId::Void];
    for theme in dark {
        assert!(
            mean(theme) < 64.0,
            "{} rasterised bright; it is meant to be a dark ground",
            theme.name()
        );
    }
    assert!(
        mean(ThemeId::Parchment) > 128.0,
        "parchment rasterised dark; it is meant to be ink on a light field"
    );
}

/// The composition check the hash is actually for: two *different graphs* must
/// not rasterise alike.
#[test]
fn a_different_graph_is_a_different_picture() {
    let one = phash(&png(ThemeId::NeonRitual, OrnamentLevel::Ritual));

    let mut bigger = sample_graph();
    for i in 6..14 {
        let id = format!("x{i}");
        bigger
            .nodes
            .push(SigilNode::new(id.clone(), SigilNodeKind::Stage));
        bigger.edges.push(SigilEdge {
            id: EdgeId::new(format!("x{i}e")),
            from: PortRef::new(
                if i == 6 {
                    "n5".to_string()
                } else {
                    format!("x{}", i - 1)
                },
                0,
            ),
            to: PortRef::new(id, 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
    }
    let normalized = normalize(bigger, &NormalizeOptions::default()).expect("valid");
    let scene = build_scene(
        &normalized,
        &LayoutOptions {
            ornament: OrnamentLevel::Ritual,
            ..LayoutOptions::canonical()
        },
    );
    let other = phash(&render_png(&scene, &SvgOptions::default(), 0.25).expect("rasterises"));
    assert!(
        distance(one, other) > 2,
        "a graph twice the size rasterised to nearly the same picture ({} cells differ)",
        distance(one, other)
    );
}

/// Ornament changes the picture — and `maximal` does not swallow it. If
/// `maximal` were so dense that its hash no longer resembled `none`'s at all,
/// the semantics would have been buried, which §15.1 forbids.
#[test]
fn ornament_changes_the_picture_without_burying_it() {
    let bare = phash(&png(ThemeId::NeonRitual, OrnamentLevel::None));
    let maximal = phash(&png(ThemeId::NeonRitual, OrnamentLevel::Maximal));
    assert_ne!(bare, maximal, "`maximal` looks identical to `none`");
    assert!(
        distance(bare, maximal) < 48,
        "`maximal` buried the semantics: {} of 64 cells changed",
        distance(bare, maximal)
    );
}

/// The scale guard: a huge canvas is a denial of service, not a picture.
#[test]
fn an_absurd_scale_is_refused() {
    let normalized = normalize(sample_graph(), &NormalizeOptions::default()).expect("valid");
    let scene = build_scene(&normalized, &LayoutOptions::canonical());
    assert!(render_png(&scene, &SvgOptions::default(), 10_000.0).is_err());
    assert!(render_png(&scene, &SvgOptions::default(), 0.0).is_err());
    assert!(render_png(&scene, &SvgOptions::default(), 1.0).is_ok());
}
