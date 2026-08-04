//! The SVG serializer.
//!
//! # Everything here is untrusted
//!
//! Labels, identifiers, capability names, file names — all of it is text someone
//! else wrote, and all of it is about to be put inside markup. There is exactly
//! one way text reaches the output ([`escape`]) and exactly one way an identifier
//! becomes an element ID ([`sanitize_id`]), because a second path is a path
//! nobody audits.
//!
//! Standard SVG output contains **no** `<script>`, no `on*` attribute, no
//! external reference, no `foreignObject`, and no user-supplied markup. That is
//! not a review note — `tests/svg_security.rs` parses the output and asserts it.
//!
//! # Determinism
//!
//! Attributes are written in a fixed order, numbers through one formatter, and
//! elements in scene order. Two renders of the same scene are byte-identical,
//! which is what makes a golden meaningful.
//!
//! Numbers are rounded to three decimals on the way out. At a 1600-unit canvas
//! that is well under a thousandth of a pixel, and it keeps a coordinate from
//! serializing as seventeen significant figures that differ in the last one
//! between platforms — the same class of problem the module documentation in
//! `canonical` describes from the parsing side.

use std::fmt::Write as _;

use crate::canonical::Prng;
use crate::graph::SigilNodeKind;
use crate::marks::{self, MarkDetail};
use crate::scene::*;
use crate::theme::{Theme, ThemeId, THEME_VERSION};

/// What is allowed to be *visible* in the artifact.
///
/// Orthogonal to [`MetadataMode`], and the separation is the whole point:
/// "no visible labels but an accessible title" and "no visible labels and
/// nothing embedded" are both useful and a single axis cannot express both.
/// See `docs/adr/0007-veil-and-source-privacy.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisclosureMode {
    /// No source labels, no names, no legend. Marks and geometry only.
    #[default]
    Veiled,
    /// Short symbolic annotations; no full source expressions.
    Inscribed,
    /// Readable labels where they fit.
    Revealed,
}

impl DisclosureMode {
    pub fn name(self) -> &'static str {
        match self {
            DisclosureMode::Veiled => "veiled",
            DisclosureMode::Inscribed => "inscribed",
            DisclosureMode::Revealed => "revealed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "veiled" => Some(DisclosureMode::Veiled),
            "inscribed" => Some(DisclosureMode::Inscribed),
            "revealed" => Some(DisclosureMode::Revealed),
            _ => None,
        }
    }

    /// Whether any visible text is drawn at all.
    fn draws_text(self) -> bool {
        self != DisclosureMode::Veiled
    }
}

/// What is allowed to be *embedded*, visible or not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum MetadataMode {
    /// Source snippets, labels, spans, IDs, renderer metadata.
    ///
    /// **Snippets are not embedded yet.** `full` currently differs from `safe`
    /// only in what it permits, not in what is written, because nothing emits a
    /// metadata block — that arrives with the Codex in Phase 4. The mode exists
    /// now so the CLI surface and the gating are settled before there is data
    /// flowing through them; `metadata_none_contains_no_label_snippet_or_identifier`
    /// will keep meaning what it means when they do.
    Full,
    /// Semantic kinds and stable IDs. No source snippets. The default, because
    /// accessibility needs `<title>` and a semantic kind is not the user's source.
    #[default]
    Safe,
    /// The render fingerprint and schema only.
    Minimal,
    /// Nothing beyond valid SVG structure.
    None,
}

impl MetadataMode {
    pub fn name(self) -> &'static str {
        match self {
            MetadataMode::Full => "full",
            MetadataMode::Safe => "safe",
            MetadataMode::Minimal => "minimal",
            MetadataMode::None => "none",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "full" => Some(MetadataMode::Full),
            "safe" => Some(MetadataMode::Safe),
            "minimal" => Some(MetadataMode::Minimal),
            "none" => Some(MetadataMode::None),
            _ => None,
        }
    }

    /// Whether `<title>` elements are emitted. They carry a semantic kind, never
    /// a label, so `safe` keeps them — an artifact with no accessible names is
    /// worse for a screen-reader user than one that says "orbit".
    fn allows_titles(self) -> bool {
        matches!(self, MetadataMode::Full | MetadataMode::Safe)
    }

    /// Whether graph identifiers may appear as element IDs.
    fn allows_ids(self) -> bool {
        matches!(self, MetadataMode::Full | MetadataMode::Safe)
    }

    fn allows_fingerprint(self) -> bool {
        self != MetadataMode::None
    }
}

/// The background of the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Background {
    #[default]
    Theme,
    Transparent,
    /// A validated `#rrggbb`. Constructed only through [`Background::hex`], so
    /// an unvalidated string cannot reach a `fill` attribute.
    Hex(String),
}

impl Background {
    /// Validated at construction rather than at use, so there is one place a bad
    /// colour is rejected and no way to build one that skips it.
    pub fn hex(value: &str) -> Result<Self, String> {
        let trimmed = value.trim_start_matches('#');
        if trimmed.len() != 6 || !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("`{value}` is not a #rrggbb colour"));
        }
        Ok(Background::Hex(format!("#{}", trimmed.to_lowercase())))
    }
}

/// Everything that decides what an SVG looks like.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SvgOptions {
    pub theme: ThemeId,
    pub disclosure: DisclosureMode,
    pub metadata: MetadataMode,
    pub background: Background,
    pub mark_detail: MarkDetail,
    /// Explicit pixel width, or `None` to leave the artifact resolution-free.
    pub width: Option<f64>,
    pub height: Option<f64>,
}

/// A rendered artifact and the identity of the render that produced it.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedSvg {
    pub svg: String,
    pub fingerprint: RenderFingerprint,
}

/// Everything that determined this render (§12.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderFingerprint {
    pub graph: String,
    pub renderer_version: String,
    pub theme: String,
    pub theme_version: u32,
    pub tracery: String,
    pub seed: u64,
    pub disclosure: String,
    pub metadata: String,
    pub format: String,
}

impl RenderFingerprint {
    /// A single line, for `--check`, for a filename, and for the metadata block.
    pub fn to_line(&self) -> String {
        format!(
            "sigil/{} graph={} theme={}@{} tracery={} seed={} mode={} metadata={} format={}",
            self.renderer_version,
            self.graph,
            self.theme,
            self.theme_version,
            self.tracery,
            self.seed,
            self.disclosure,
            self.metadata,
            self.format
        )
    }
}

/// Render a scene to SVG.
pub fn render_svg(scene: &SigilScene, options: &SvgOptions) -> RenderedSvg {
    let theme = options.theme.resolve();
    let fingerprint = RenderFingerprint {
        graph: scene.metadata.graph_fingerprint.clone(),
        renderer_version: scene.metadata.renderer_version.clone(),
        theme: options.theme.name().to_string(),
        theme_version: THEME_VERSION,
        tracery: scene.metadata.tracery.clone(),
        seed: scene.metadata.seed,
        disclosure: options.disclosure.name().to_string(),
        metadata: options.metadata.name().to_string(),
        format: "svg".to_string(),
    };

    let mut out = String::with_capacity(4096);
    let vb = scene.view_box;

    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    if let (Some(w), Some(h)) = (options.width, options.height) {
        let _ = write!(out, " width=\"{}\" height=\"{}\"", num(w), num(h));
    } else if let Some(w) = options.width {
        // Square canvas, so one dimension determines the other. Writing only
        // `width` would leave a viewer to guess the aspect ratio.
        let _ = write!(out, " width=\"{}\" height=\"{}\"", num(w), num(w));
    }
    let _ = writeln!(
        out,
        " viewBox=\"{} {} {} {}\" role=\"img\">",
        num(vb.x),
        num(vb.y),
        num(vb.width),
        num(vb.height)
    );

    // An accessible name for the whole artifact. The summary is generated from
    // the census — it names kinds and counts, never a label — so it is safe in
    // every disclosure mode and is exactly what a screen-reader user needs.
    if options.metadata.allows_titles() {
        let _ = writeln!(out, "<title>{}</title>", escape(&scene.summary()));
    }

    write_defs(&mut out, &theme);
    write_style(&mut out, &theme);
    write_background(&mut out, scene, options, &theme);

    // Layers in paint order, each its own group, so ornament is one subtree to
    // drop rather than a class to filter.
    for layer in SceneLayerKind::ALL {
        let elements: Vec<&SceneElement> = scene
            .elements
            .iter()
            .filter(|e| e.layer == *layer)
            .collect();
        if elements.is_empty() {
            continue;
        }
        let _ = writeln!(
            out,
            "<g class=\"sigil-layer sigil-layer-{}\">",
            layer.name()
        );
        for element in elements {
            write_element(&mut out, element, scene, options, &theme);
        }
        out.push_str("</g>\n");
    }

    if options.metadata.allows_fingerprint() {
        let _ = writeln!(out, "<desc>{}</desc>", escape(&fingerprint.to_line()));
    }

    out.push_str("</svg>\n");

    RenderedSvg {
        svg: out,
        fingerprint,
    }
}

fn write_defs(out: &mut String, theme: &Theme) {
    if theme.glow <= 0.0 {
        return;
    }
    // One filter, referenced by class. A per-element filter would multiply the
    // rasterizer's work by the node count for no visual difference.
    let _ = writeln!(
        out,
        "<defs><filter id=\"sigil-glow\" x=\"-30%\" y=\"-30%\" width=\"160%\" height=\"160%\">\
         <feGaussianBlur stdDeviation=\"{}\" result=\"b\"/>\
         <feMerge><feMergeNode in=\"b\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>\
         </filter></defs>",
        num(theme.glow)
    );
}

/// The stylesheet.
///
/// Presentation lives here rather than in per-element attributes so that the
/// interactive HTML export and the web app can restyle without the geometry
/// changing, and so a 500-node artifact does not repeat the same six attributes
/// three thousand times.
///
/// Nothing in here interpolates user text. The only values are theme constants
/// and numbers.
fn write_style(out: &mut String, theme: &Theme) {
    let _ = writeln!(
        out,
        "<style>\
         .sigil-node{{fill:none;stroke-width:{nw};stroke-linecap:round;stroke-linejoin:round}}\
         .sigil-edge{{fill:none;stroke-width:{ew};stroke-linecap:round}}\
         .sigil-region{{fill:none;stroke-width:{rw}}}\
         .sigil-boundary{{fill:none;stroke:{seal};stroke-width:{rw};stroke-opacity:0.55}}\
         .sigil-ornament{{fill:none;stroke:{orn};stroke-opacity:{oo};stroke-width:1}}\
         .sigil-text{{fill:{text};font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;\
         font-size:15px;text-anchor:middle;dominant-baseline:middle}}\
         </style>",
        nw = num(theme.node_stroke_width),
        ew = num(theme.edge_stroke_width),
        rw = num(theme.region_stroke_width),
        seal = theme.seal,
        orn = theme.ornament,
        oo = num(theme.ornament_opacity),
        text = theme.text,
    );
}

fn write_background(out: &mut String, scene: &SigilScene, options: &SvgOptions, theme: &Theme) {
    let fill = match &options.background {
        Background::Transparent => return,
        Background::Theme => theme.background,
        // Validated by `Background::hex`; there is no other constructor.
        Background::Hex(value) => value.as_str(),
    };
    let vb = scene.view_box;
    let _ = writeln!(
        out,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        num(vb.x),
        num(vb.y),
        num(vb.width),
        num(vb.height),
        fill
    );
}

fn write_element(
    out: &mut String,
    element: &SceneElement,
    scene: &SigilScene,
    options: &SvgOptions,
    theme: &Theme,
) {
    let (class, stroke) = match &element.semantic {
        SemanticKind::Node(kind) => ("sigil-node", theme.node_color(kind)),
        SemanticKind::Edge(kind) => ("sigil-edge", theme.edge_color(*kind)),
        SemanticKind::Region(_) => ("sigil-region", theme.region),
        SemanticKind::InvocationBoundary => ("sigil-boundary", theme.seal),
        SemanticKind::Ornament => ("sigil-ornament", theme.ornament),
    };

    let mut attrs = String::new();
    if options.metadata.allows_ids() {
        let _ = write!(attrs, " id=\"{}\"", sanitize_id(&element.id));
    }
    // Ornament's layer class and its semantic class are both `sigil-ornament`,
    // so writing both produced `class="sigil-ornament sigil-ornament"` — valid,
    // and visibly sloppy in an artifact someone is meant to look at.
    let semantic_class = format!("sigil-{}", sanitize_id(&element.semantic.class()));
    if semantic_class == class {
        let _ = write!(attrs, " class=\"{class}\"");
    } else {
        let _ = write!(attrs, " class=\"{class} {semantic_class}\"");
    }
    if !matches!(element.semantic, SemanticKind::InvocationBoundary) {
        let _ = write!(attrs, " stroke=\"{stroke}\"");
    }
    if let SemanticKind::Edge(kind) = &element.semantic {
        if let Some(dash) = theme.edge_dash(*kind) {
            let _ = write!(attrs, " stroke-dasharray=\"{dash}\"");
        }
    }
    if theme.glow > 0.0 && matches!(element.semantic, SemanticKind::Node(_)) {
        attrs.push_str(" filter=\"url(#sigil-glow)\"");
    }

    // A title is the element's *kind*, never its label — a label here would be
    // in a Veiled render's accessibility tree.
    let title = element
        .title
        .as_ref()
        .filter(|_| options.metadata.allows_titles())
        .map(|t| format!("<title>{}</title>", escape(t)));

    match &element.geometry {
        Geometry::Circle { center, radius } => {
            emit(
                out,
                "circle",
                &format!(
                    " cx=\"{}\" cy=\"{}\" r=\"{}\"{attrs}",
                    num(center.x),
                    num(center.y),
                    num(*radius)
                ),
                title.as_deref(),
            );
        }
        Geometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let d = arc_path(*center, *radius, *start_angle, *end_angle);
            emit(out, "path", &format!(" d=\"{d}\"{attrs}"), title.as_deref());
        }
        Geometry::Polygon { points } => {
            let mut d = String::new();
            for (i, p) in points.iter().enumerate() {
                let _ = write!(
                    d,
                    "{}{} {}",
                    if i == 0 { "M" } else { "L" },
                    num(p.x),
                    num(p.y)
                );
            }
            d.push('Z');
            emit(out, "path", &format!(" d=\"{d}\"{attrs}"), title.as_deref());
        }
        Geometry::Path { commands } => {
            let d = path_data(commands);
            emit(out, "path", &format!(" d=\"{d}\"{attrs}"), title.as_deref());
        }
        Geometry::Mark {
            center,
            size,
            rotation,
            path,
        } => {
            // Generated here when the scene left it empty, so a scene produced
            // before the mark grammar existed still renders — and so scene JSON
            // does not have to carry every path twice.
            let commands = if path.is_empty() {
                let kind = match &element.semantic {
                    SemanticKind::Node(kind) => kind.clone(),
                    _ => SigilNodeKind::Stage,
                };
                let family = family_of(element, scene);
                let mark = marks::generate(
                    &kind,
                    family.as_ref(),
                    &Prng::new(scene.metadata.seed),
                    &element.id,
                    options.mark_detail,
                );
                scale_and_place(&mark.commands, *center, *size)
            } else {
                scale_and_place(path, *center, *size)
            };
            let d = path_data(&commands);
            // The rotation is applied about the mark's own centre, so a mark
            // aligns with local flow without its position depending on the angle.
            let transform = format!(
                " transform=\"rotate({} {} {})\"",
                num(rotation.to_degrees()),
                num(center.x),
                num(center.y)
            );
            emit(
                out,
                "path",
                &format!(" d=\"{d}\"{attrs}{transform}"),
                title.as_deref(),
            );
        }
        Geometry::Text {
            anchor,
            content,
            size,
            rotation,
        } => {
            // Text is drawn only when the disclosure mode allows it. In Veiled
            // mode a text element is not styled away — it is never generated.
            if !options.disclosure.draws_text() {
                return;
            }
            let shown = if options.disclosure == DisclosureMode::Inscribed {
                abbreviate(content)
            } else {
                content.clone()
            };
            let transform = if *rotation == 0.0 {
                String::new()
            } else {
                format!(
                    " transform=\"rotate({} {} {})\"",
                    num(rotation.to_degrees()),
                    num(anchor.x),
                    num(anchor.y)
                )
            };
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{}\" class=\"sigil-text\" font-size=\"{}\"{transform}>{}</text>",
                num(anchor.x),
                num(anchor.y),
                num(*size),
                escape(&shown)
            );
        }
    }
}

/// The capability family an element's node belongs to, for the invocation mark.
fn family_of(element: &SceneElement, scene: &SigilScene) -> Option<crate::graph::CapabilityFamily> {
    let SceneRef::Node(id) = element.graph_ref.as_ref()? else {
        return None;
    };
    // Read off the legend, which is where the scene keeps per-node semantics.
    // The renderer does not get to reach back into the graph — a scene has to be
    // renderable on its own, which is what makes scene JSON a real format.
    let entry = scene
        .legend
        .iter()
        .find(|e| matches!(&e.graph_ref, SceneRef::Node(n) if n == id))?;
    entry.capabilities.first().map(|name| {
        crate::graph::CapabilityFamily::from_namespace(name.split('.').next().unwrap_or(name))
    })
}

fn emit(out: &mut String, tag: &str, attrs: &str, title: Option<&str>) {
    match title {
        Some(title) => {
            let _ = writeln!(out, "<{tag}{attrs}>{title}</{tag}>");
        }
        None => {
            let _ = writeln!(out, "<{tag}{attrs}/>");
        }
    }
}

/// Move a normalized mark into place.
fn scale_and_place(commands: &[PathCommand], center: Point, size: f64) -> Vec<PathCommand> {
    let at = |p: &Point| Point::new(center.x + p.x * size, center.y + p.y * size);
    commands
        .iter()
        .map(|c| match c {
            PathCommand::MoveTo(p) => PathCommand::MoveTo(at(p)),
            PathCommand::LineTo(p) => PathCommand::LineTo(at(p)),
            PathCommand::CubicTo { c1, c2, to } => PathCommand::CubicTo {
                c1: at(c1),
                c2: at(c2),
                to: at(to),
            },
            PathCommand::ArcTo {
                radius,
                large,
                sweep,
                to,
            } => PathCommand::ArcTo {
                radius: radius * size,
                large: *large,
                sweep: *sweep,
                to: at(to),
            },
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

fn path_data(commands: &[PathCommand]) -> String {
    let mut d = String::new();
    for command in commands {
        match command {
            PathCommand::MoveTo(p) => {
                let _ = write!(d, "M{} {}", num(p.x), num(p.y));
            }
            PathCommand::LineTo(p) => {
                let _ = write!(d, "L{} {}", num(p.x), num(p.y));
            }
            PathCommand::CubicTo { c1, c2, to } => {
                let _ = write!(
                    d,
                    "C{} {} {} {} {} {}",
                    num(c1.x),
                    num(c1.y),
                    num(c2.x),
                    num(c2.y),
                    num(to.x),
                    num(to.y)
                );
            }
            PathCommand::ArcTo {
                radius,
                large,
                sweep,
                to,
            } => {
                let _ = write!(
                    d,
                    "A{} {} 0 {} {} {} {}",
                    num(*radius),
                    num(*radius),
                    u8::from(*large),
                    u8::from(*sweep),
                    num(to.x),
                    num(to.y)
                );
            }
            PathCommand::Close => d.push('Z'),
        }
    }
    d
}

fn arc_path(center: Point, radius: f64, start: f64, end: f64) -> String {
    let a = Point::new(
        center.x + radius * start.cos(),
        center.y + radius * start.sin(),
    );
    let b = Point::new(center.x + radius * end.cos(), center.y + radius * end.sin());
    let large = (end - start).abs() > std::f64::consts::PI;
    let sweep = end > start;
    format!(
        "M{} {}A{} {} 0 {} {} {} {}",
        num(a.x),
        num(a.y),
        num(radius),
        num(radius),
        u8::from(large),
        u8::from(sweep),
        num(b.x),
        num(b.y)
    )
}

/// Rasterise a rendered SVG.
///
/// Delegates to `rite_render::svg_to_png`, which is arbitrary-SVG-to-PNG and was
/// already audited for the Cant social card. Sigil does not own a rasteriser and
/// should not: the alternative was a second `resvg` integration with its own
/// font handling, differing in ways nobody would notice until an artifact came
/// out wrong.
///
/// Determinism is "within practical rasterisation limits" (§16.2): the same SVG
/// and scale give the same bytes on one machine, and `resvg`'s antialiasing is
/// deterministic, but a different `resvg` release may differ by a subpixel. That
/// is why the visual-regression tests compare a perceptual hash rather than
/// bytes, and why SVG rather than PNG is the canonical format.
#[cfg(feature = "png")]
pub fn render_png(scene: &SigilScene, options: &SvgOptions, scale: f32) -> Result<Vec<u8>, String> {
    if !(0.05..=32.0).contains(&scale) {
        return Err(format!(
            "scale {scale} is outside the supported range 0.05..=32 — a huge \
             canvas is a denial-of-service, not a picture"
        ));
    }
    let rendered = render_svg(scene, options);
    rite_render::svg_to_png(&rendered.svg, scale).map_err(|e| e.to_string())
}

/// A number, rounded and without a trailing `.0`.
///
/// Non-finite values become `0` rather than `NaN`: `NaN` in an SVG attribute is
/// an invalid document that renders as nothing, and a mark at the origin is a
/// visible bug. Layout's bounds pass reports the underlying problem.
fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        format!("{}", rounded as i64)
    } else {
        let mut s = format!("{rounded:.3}");
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
        s
    }
}

/// The only way text reaches the output.
///
/// All five XML predefined entities, including both quote forms — the writer
/// uses double quotes for attributes, and an escaper that handled only `<`, `>`
/// and `&` would let a label close an attribute and open another.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not legal XML at all. Dropped rather than
            // escaped: `&#1;` is still invalid.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// The only way an identifier becomes an element ID.
///
/// XML IDs may not start with a digit and may not contain most punctuation.
/// Anything outside a conservative set becomes `_`, and a leading digit gets a
/// prefix — so a hostile identifier cannot break out of the attribute *or*
/// produce a document a strict parser rejects.
pub fn sanitize_id(id: &str) -> String {
    let mut out: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // `/` and `:` are legal in an ID but awkward in a CSS selector; the element
    // IDs this crate generates use `/` as a separator, so they are normalized
    // rather than rejected.
    out = out.replace(['/', ':'], "-");
    if out
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.')
    {
        out.insert(0, 's');
    }
    if out.is_empty() {
        out.push('s');
    }
    out
}

/// A short form for Inscribed mode.
fn abbreviate(text: &str) -> String {
    const MAX: usize = 14;
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(MAX).collect();
    if chars.next().is_none() {
        head
    } else {
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_covers_every_xml_entity_including_both_quotes() {
        assert_eq!(escape(r#"<script>&"'"#), "&lt;script&gt;&amp;&quot;&apos;");
        // A label that closed an attribute and opened another is the attack the
        // quote escapes exist for.
        let hostile = r#"" onload="alert(1)"#;
        let escaped = escape(hostile);
        assert!(!escaped.contains('"'), "{escaped}");
    }

    #[test]
    fn control_characters_are_dropped_not_escaped() {
        // `&#1;` is still not legal XML, so escaping would produce an invalid
        // document rather than a safe one.
        let escaped = escape("a\u{1}b\u{7}c");
        assert_eq!(escaped, "abc");
        assert!(escaped.chars().all(|c| (c as u32) >= 0x20));
    }

    #[test]
    fn identifiers_are_sanitized_into_valid_xml_ids() {
        assert_eq!(sanitize_id("node/n1"), "node-n1");
        assert_eq!(sanitize_id("9lives"), "s9lives");
        assert_eq!(sanitize_id(""), "s");
        let hostile = sanitize_id(r#""><script>alert(1)</script>"#);
        for banned in ['<', '>', '"', '\'', '&', '(', ')'] {
            assert!(!hostile.contains(banned), "{hostile}");
        }
        assert!(!hostile.starts_with(|c: char| c.is_ascii_digit()));
    }

    #[test]
    fn numbers_round_and_lose_their_trailing_zeros() {
        assert_eq!(num(800.0), "800");
        assert_eq!(num(800.5), "800.5");
        assert_eq!(num(1.0 / 3.0), "0.333");
        assert_eq!(num(-0.0001), "0");
    }

    /// A `NaN` in an attribute is an invalid document that renders as nothing.
    #[test]
    fn non_finite_numbers_become_zero_rather_than_nan() {
        assert_eq!(num(f64::NAN), "0");
        assert_eq!(num(f64::INFINITY), "0");
        assert_eq!(num(f64::NEG_INFINITY), "0");
    }

    #[test]
    fn a_background_hex_is_validated_at_construction() {
        assert_eq!(
            Background::hex("#AABBCC"),
            Ok(Background::Hex("#aabbcc".into()))
        );
        assert_eq!(
            Background::hex("aabbcc"),
            Ok(Background::Hex("#aabbcc".into()))
        );
        assert!(Background::hex("red").is_err());
        assert!(Background::hex("#ff").is_err());
        assert!(Background::hex("#gggggg").is_err());
        // The attack this closes: a colour that ends the attribute.
        assert!(Background::hex("#fff\" onload=\"x").is_err());
    }

    #[test]
    fn disclosure_and_metadata_modes_round_trip() {
        for mode in [
            DisclosureMode::Veiled,
            DisclosureMode::Inscribed,
            DisclosureMode::Revealed,
        ] {
            assert_eq!(DisclosureMode::parse(mode.name()), Some(mode));
        }
        for mode in [
            MetadataMode::Full,
            MetadataMode::Safe,
            MetadataMode::Minimal,
            MetadataMode::None,
        ] {
            assert_eq!(MetadataMode::parse(mode.name()), Some(mode));
        }
        assert_eq!(DisclosureMode::parse("naked"), None);
        assert_eq!(MetadataMode::parse("everything"), None);
    }

    /// The defaults §17.3 specifies.
    #[test]
    fn the_defaults_are_the_documented_ones() {
        let options = SvgOptions::default();
        assert_eq!(options.theme, ThemeId::NeonRitual);
        assert_eq!(options.disclosure, DisclosureMode::Veiled);
        assert_eq!(options.metadata, MetadataMode::Safe);
        assert_eq!(options.background, Background::Theme);
    }

    #[test]
    fn metadata_modes_gate_what_they_say_they_gate() {
        assert!(MetadataMode::Safe.allows_titles());
        assert!(!MetadataMode::Minimal.allows_titles());
        assert!(MetadataMode::Minimal.allows_fingerprint());
        assert!(!MetadataMode::None.allows_fingerprint());
        assert!(!MetadataMode::None.allows_ids());
    }
}
