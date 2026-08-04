//! The browser-facing Sigil API.
//!
//! A binding layer and nothing else. Every decision about what a picture looks
//! like is in `rite-sigil`, which is compiled here rather than reimplemented —
//! that is ADR 0005, and it is what makes "the browser renders the same scene as
//! the CLI" a testable claim instead of a coincidence maintained by hand.
//!
//! # What is not in this build
//!
//! No Rite runtime, no capabilities, no compiler, no filesystem, no process, no
//! async runtime, no rasteriser. `rite-sigil` is taken without its `png`
//! feature, so `resvg` and a font stack stay out; the browser rasterises through
//! canvas, which it can already do.
//!
//! Cant's parser *is* here, and has to be: rendering pasted source means parsing
//! pasted source. What is not here is anything that could run it.
//!
//! # The `native` feature
//!
//! Every function is written twice-over: once as a plain Rust function, and once
//! as a `wasm_bindgen` wrapper around it. `cargo test` exercises the first, which
//! is why parity can be asserted in a normal test run rather than only in a
//! browser harness.

use serde::{Deserialize, Serialize};

pub use rite_sigil::{
    graph::GRAPH_SCHEMA_VERSION as SIGIL_GRAPH_SCHEMA_VERSION,
    scene::SCENE_SCHEMA_VERSION as SIGIL_SCENE_SCHEMA_VERSION, RENDERER_VERSION,
};

/// Everything the browser can ask for. Mirrors the CLI's flags, by name, so a
/// user who learns one knows the other.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenderOptions {
    pub theme: String,
    pub mode: String,
    pub metadata: String,
    pub ornament: String,
    /// `"graph"`, `"canonical"`, or a decimal integer. A string rather than a
    /// number because JavaScript cannot hold a `u64` exactly, and a seed that
    /// silently lost its low bits would produce a different picture than the
    /// CLI given the same input.
    pub seed: String,
    pub background: String,
    /// How traces are drawn: `flowing`, `concentric`, or `circuit`.
    pub tracery: String,
    pub canonical: bool,
    pub simplify: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            theme: "neon-ritual".into(),
            mode: "veiled".into(),
            metadata: "safe".into(),
            ornament: "ritual".into(),
            seed: "graph".into(),
            background: "theme".into(),
            tracery: "flowing".into(),
            canonical: false,
            simplify: false,
        }
    }
}

/// One diagnostic, flattened for the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_end: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// What a render produces.
///
/// Always a value, never a thrown exception: a UI that has to `try`/`catch` to
/// find out whether a program has a syntax error will get it wrong somewhere.
/// `ok` says whether there is an artifact; `diagnostics` is populated either way.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_json: Option<String>,
    /// The self-contained interactive page, present only when asked for
    /// through [`render_cant_html`] / [`render_graph_html`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    /// Milliseconds, filled in by the caller — this crate has no clock, and
    /// asking for one would put a capability in a renderer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<f64>,
}

/// Versions, so a page can report what it is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub renderer: String,
    pub graph_schema: u32,
    pub scene_schema: u32,
    pub cant_graph_schema: String,
    pub theme_version: u32,
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        renderer: RENDERER_VERSION.to_string(),
        graph_schema: SIGIL_GRAPH_SCHEMA_VERSION,
        scene_schema: SIGIL_SCENE_SCHEMA_VERSION,
        cant_graph_schema: cant_sem::GRAPH_SCHEMA_VERSION.to_string(),
        theme_version: rite_sigil::THEME_VERSION,
    }
}

/// Which graph schemas this build reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSupport {
    pub cant_graph: Vec<String>,
    pub sigil_graph: Vec<u32>,
    pub sigil_scene: Vec<u32>,
}

pub fn supported_schemas() -> SchemaSupport {
    SchemaSupport {
        cant_graph: vec![cant_sem::GRAPH_SCHEMA_VERSION.to_string()],
        sigil_graph: vec![SIGIL_GRAPH_SCHEMA_VERSION],
        sigil_scene: vec![SIGIL_SCENE_SCHEMA_VERSION],
    }
}

fn diagnostic_of(d: &rite_sigil::SigilDiagnostic) -> Diagnostic {
    use rite_sigil::GraphRef;
    Diagnostic {
        code: d.code.to_string(),
        severity: format!("{}", d.severity),
        message: d.message.clone(),
        graph_id: match &d.graph_ref {
            GraphRef::Node(id) | GraphRef::Edge(id) | GraphRef::Region(id) => Some(id.clone()),
            GraphRef::Graph => None,
        },
        span_start: d.span.map(|s| s.start.0),
        span_end: d.span.map(|s| s.end.0),
        notes: d.notes.clone(),
    }
}

fn failure(code: &str, message: impl Into<String>) -> RenderResult {
    RenderResult {
        ok: false,
        diagnostics: vec![Diagnostic {
            code: code.to_string(),
            severity: "error".into(),
            message: message.into(),
            graph_id: None,
            span_start: None,
            span_end: None,
            notes: Vec::new(),
        }],
        ..Default::default()
    }
}

/// Resolve options into the renderer's own types.
///
/// The error is a whole `RenderResult` — boxed, because it is much larger than
/// the success value and clippy is right that returning it by value makes every
/// call site pay for the failure path. It carries a full result rather than a
/// message so a bad option reaches the UI through the same shape as everything
/// else: a value with diagnostics, never an exception.
fn resolve(
    options: &RenderOptions,
) -> Result<
    (
        rite_sigil::SvgOptions,
        rite_sigil::OrnamentLevel,
        rite_sigil::Tracery,
    ),
    Box<RenderResult>,
> {
    let theme = rite_sigil::ThemeId::parse(&options.theme).ok_or_else(|| {
        Box::new(failure(
            "SIGIL-T001",
            format!("unknown theme `{}`", options.theme),
        ))
    })?;
    let disclosure = rite_sigil::DisclosureMode::parse(&options.mode).ok_or_else(|| {
        Box::new(failure(
            "SIGIL-C001",
            format!("unknown mode `{}`", options.mode),
        ))
    })?;
    let metadata = rite_sigil::MetadataMode::parse(&options.metadata).ok_or_else(|| {
        Box::new(failure(
            "SIGIL-C001",
            format!("unknown metadata mode `{}`", options.metadata),
        ))
    })?;
    let ornament = rite_sigil::OrnamentLevel::parse(&options.ornament).ok_or_else(|| {
        Box::new(failure(
            "SIGIL-C001",
            format!("unknown ornament level `{}`", options.ornament),
        ))
    })?;
    let tracery = rite_sigil::Tracery::parse(&options.tracery).ok_or_else(|| {
        Box::new(failure(
            "SIGIL-C001",
            format!("unknown tracery `{}`", options.tracery),
        ))
    })?;
    let background = match options.background.as_str() {
        "theme" => rite_sigil::Background::Theme,
        "transparent" => rite_sigil::Background::Transparent,
        hex => rite_sigil::Background::hex(hex)
            .map_err(|e| Box::new(failure("SIGIL-C001", format!("background: {e}"))))?,
    };

    Ok((
        rite_sigil::SvgOptions {
            theme,
            disclosure,
            metadata,
            background,
            mark_detail: if options.simplify {
                rite_sigil::MarkDetail::Minimal
            } else {
                rite_sigil::MarkDetail::Full
            },
            width: None,
            height: None,
        },
        ornament,
        tracery,
    ))
}

/// The shared tail: a normalized graph plus options into an artifact.
fn render_normalized(
    normalized: rite_sigil::NormalizedGraph,
    options: &RenderOptions,
    graph_json: Option<String>,
    with_html: bool,
) -> RenderResult {
    let (svg_options, ornament, tracery) = match resolve(options) {
        Ok(triple) => triple,
        Err(result) => return *result,
    };

    let seed = if options.canonical || options.seed == "canonical" {
        0
    } else if options.seed == "graph" {
        normalized.seed()
    } else {
        match options.seed.parse::<u64>() {
            Ok(seed) => seed,
            Err(_) => {
                return failure(
                    "SIGIL-C001",
                    format!(
                        "unknown seed `{}` — expected graph, canonical, or an integer",
                        options.seed
                    ),
                )
            }
        }
    };

    let layout = rite_sigil::LayoutOptions {
        seed,
        orientation: if options.canonical {
            rite_sigil::Orientation::Canonical
        } else {
            rite_sigil::Orientation::Seeded
        },
        legend: true,
        ornament,
        tracery,
    };

    let mut diagnostics: Vec<Diagnostic> =
        normalized.diagnostics.iter().map(diagnostic_of).collect();
    let scene = rite_sigil::build_scene(&normalized, &layout);
    for warning in &scene.warnings {
        diagnostics.push(Diagnostic {
            code: "SIGIL-L001".into(),
            severity: "warning".into(),
            message: warning.clone(),
            graph_id: None,
            span_start: None,
            span_end: None,
            notes: Vec::new(),
        });
    }

    let rendered = rite_sigil::render_svg(&scene, &svg_options);
    // Only on request: the page embeds the SVG and a stylesheet over again,
    // and exports are rare while renders are constant. The scene is never
    // embedded from the browser — §16's `--embed-scene` stays a CLI decision.
    let html = with_html.then(|| {
        rite_sigil::render_html(
            &scene,
            &rite_sigil::HtmlOptions {
                svg: svg_options.clone(),
                codex: true,
                embed_scene: false,
            },
        )
    });
    RenderResult {
        ok: true,
        svg: Some(rendered.svg),
        scene_json: serde_json::to_string(&scene).ok(),
        graph_json,
        html,
        fingerprint: Some(rendered.fingerprint.to_line()),
        summary: Some(scene.summary()),
        diagnostics,
        elapsed_ms: None,
    }
}

/// Render Cant source.
pub fn render_cant(source_name: &str, source: &str, options: &RenderOptions) -> RenderResult {
    render_cant_impl(source_name, source, options, false)
}

/// [`render_cant`], with the self-contained interactive HTML page included.
///
/// The page's Codex carries labels under the same policy every render uses:
/// they travel unless `metadata` is `none`. Disclosure still governs the
/// canvas, so a Veiled export is a veiled picture with a decodable Codex
/// beside it — §13.4's web default, in a file.
pub fn render_cant_html(source_name: &str, source: &str, options: &RenderOptions) -> RenderResult {
    render_cant_impl(source_name, source, options, true)
}

fn render_cant_impl(
    source_name: &str,
    source: &str,
    options: &RenderOptions,
    with_html: bool,
) -> RenderResult {
    // Browser limits, not native ones: a tab should refuse rather than hang.
    let limits = rite_sigil::NormalizeOptions {
        keep_snippets: options.metadata == "full",
        ..rite_sigil::NormalizeOptions::browser()
    };
    if source.len() > limits.limits.max_input_bytes {
        return failure(
            "SIGIL-S005",
            format!(
                "{} bytes of source, cap is {}",
                source.len(),
                limits.limits.max_input_bytes
            ),
        );
    }

    let (parsed, sources) = cant_syntax::parse_source(source_name, source);
    if parsed.has_errors() {
        let rendered = parsed.diagnostics.render_all(&sources);
        return RenderResult {
            ok: false,
            diagnostics: parsed
                .diagnostics
                .iter()
                .map(|d| {
                    // The primary label's span, when there is one. Cant's
                    // diagnostics carry labels rather than a single span,
                    // because a message can point at two places at once — the
                    // browser's editor highlights the primary.
                    let primary = d
                        .labels
                        .iter()
                        .find(|l| l.primary)
                        .or_else(|| d.labels.first());
                    Diagnostic {
                        code: d.code.to_string(),
                        severity: format!("{}", d.severity),
                        message: d.title.clone(),
                        graph_id: None,
                        span_start: primary.map(|l| l.span.span.start.0),
                        span_end: primary.map(|l| l.span.span.end.0),
                        // The rendered excerpt, carried as a note so the app can
                        // show a caret-underlined snippet without re-deriving one.
                        notes: std::iter::once(rendered.clone())
                            .chain(d.notes.iter().cloned())
                            .collect(),
                    }
                })
                .collect(),
            ..Default::default()
        };
    }
    let Some(program) = parsed.program else {
        return failure("SIGIL-G009", "the source contains no program");
    };

    let cant_graph = cant_sem::lower(&program, source_name, source.len());
    // Labels travel unless metadata forbids them — *not* only when the artifact
    // will draw them.
    //
    // Veiled governs the picture, and the Codex is a separate surface: §13.1
    // says a Veiled render "may reveal details on hover/focus or through the
    // Codex", with Deep Veil to suppress that. Tying the two together made
    // "veiled sigil, full Codex" — the intended default — impossible to ask for.
    //
    // Carrying them does not leak into the artifact: the serializer refuses to
    // draw a text element in Veiled mode and a `<title>` carries a semantic kind
    // rather than a label, so the two guards that keep the picture clean are
    // still the ones doing it.
    let wants_labels = options.metadata != "none";
    let adapt = if wants_labels {
        cant_sem::AdaptOptions::with_labels()
    } else {
        cant_sem::AdaptOptions::default()
    };
    let graph_json = serde_json::to_string(&cant_graph.to_json()).ok();
    let sigil_graph = cant_sem::to_sigil_graph(&cant_graph, adapt);

    match rite_sigil::normalize(sigil_graph, &limits) {
        Ok(normalized) => render_normalized(normalized, options, graph_json, with_html),
        Err(diagnostics) => RenderResult {
            ok: false,
            diagnostics: diagnostics.iter().map(diagnostic_of).collect(),
            ..Default::default()
        },
    }
}

/// Render a `cant.graph` JSON document, without parsing any source.
pub fn render_graph(graph_json: &str, options: &RenderOptions) -> RenderResult {
    render_graph_impl(graph_json, options, false)
}

/// [`render_graph`], with the self-contained interactive HTML page included.
pub fn render_graph_html(graph_json: &str, options: &RenderOptions) -> RenderResult {
    render_graph_impl(graph_json, options, true)
}

fn render_graph_impl(graph_json: &str, options: &RenderOptions, with_html: bool) -> RenderResult {
    let limits = rite_sigil::NormalizeOptions {
        keep_snippets: options.metadata == "full",
        ..rite_sigil::NormalizeOptions::browser()
    };
    if graph_json.len() > limits.limits.max_input_bytes {
        return failure("SIGIL-S005", "the graph document is too large");
    }
    let analysis = match cant_sem::validate_deserialized(graph_json, rite_core::FileId(0)) {
        Ok(analysis) => analysis,
        Err(e) => return failure("SIGIL-V001", e),
    };
    if analysis.diagnostics.has_errors() {
        return failure("SIGIL-G002", "the graph does not validate");
    }
    // Labels travel unless metadata forbids them — *not* only when the artifact
    // will draw them.
    //
    // Veiled governs the picture, and the Codex is a separate surface: §13.1
    // says a Veiled render "may reveal details on hover/focus or through the
    // Codex", with Deep Veil to suppress that. Tying the two together made
    // "veiled sigil, full Codex" — the intended default — impossible to ask for.
    //
    // Carrying them does not leak into the artifact: the serializer refuses to
    // draw a text element in Veiled mode and a `<title>` carries a semantic kind
    // rather than a label, so the two guards that keep the picture clean are
    // still the ones doing it.
    let wants_labels = options.metadata != "none";
    let adapt = if wants_labels {
        cant_sem::AdaptOptions::with_labels()
    } else {
        cant_sem::AdaptOptions::default()
    };
    let sigil_graph = cant_sem::to_sigil_graph(&analysis.graph, adapt);
    match rite_sigil::normalize(sigil_graph, &limits) {
        Ok(normalized) => {
            render_normalized(normalized, options, Some(graph_json.to_string()), with_html)
        }
        Err(diagnostics) => RenderResult {
            ok: false,
            diagnostics: diagnostics.iter().map(diagnostic_of).collect(),
            ..Default::default()
        },
    }
}

/// Validate a graph document without rendering it.
pub fn validate_graph(graph_json: &str) -> RenderResult {
    match cant_sem::validate_deserialized(graph_json, rite_core::FileId(0)) {
        Ok(analysis) if !analysis.diagnostics.has_errors() => RenderResult {
            ok: true,
            summary: Some(format!(
                "{} nodes, {} edges, {} subgraphs",
                analysis.graph.nodes.len(),
                analysis.graph.edges.len(),
                analysis.graph.subgraphs.len()
            )),
            ..Default::default()
        },
        Ok(_) => failure("SIGIL-G002", "the graph does not validate"),
        Err(e) => failure("SIGIL-V001", e),
    }
}

// ---------------------------------------------------------------------------
// The browser bindings. Thin wrappers, so the functions above stay testable.
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm")]
mod bindings {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start() {
        // A panic in a renderer should say what happened, not stop at
        // "unreachable executed".
        console_error_panic_hook::set_once();
    }

    fn options_of(json: Option<String>) -> RenderOptions {
        json.and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }

    fn to_json<T: Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap_or_else(|e| {
            format!(r#"{{"ok":false,"diagnostics":[{{"code":"SIGIL-W001","severity":"error","message":"could not serialize the result: {e}"}}]}}"#)
        })
    }

    /// JSON in, JSON out.
    ///
    /// Strings rather than structured values across the boundary: it keeps the
    /// TypeScript side one `JSON.parse` with a declared type, and it avoids
    /// `serde-wasm-bindgen`'s number handling on a `u64` seed, which JavaScript
    /// cannot represent exactly.
    #[wasm_bindgen(js_name = renderCant)]
    pub fn render_cant_js(source_name: &str, source: &str, options: Option<String>) -> String {
        to_json(&super::render_cant(
            source_name,
            source,
            &options_of(options),
        ))
    }

    #[wasm_bindgen(js_name = renderGraph)]
    pub fn render_graph_js(graph_json: &str, options: Option<String>) -> String {
        to_json(&super::render_graph(graph_json, &options_of(options)))
    }

    #[wasm_bindgen(js_name = renderCantHtml)]
    pub fn render_cant_html_js(source_name: &str, source: &str, options: Option<String>) -> String {
        to_json(&super::render_cant_html(
            source_name,
            source,
            &options_of(options),
        ))
    }

    #[wasm_bindgen(js_name = renderGraphHtml)]
    pub fn render_graph_html_js(graph_json: &str, options: Option<String>) -> String {
        to_json(&super::render_graph_html(graph_json, &options_of(options)))
    }

    #[wasm_bindgen(js_name = validateGraph)]
    pub fn validate_graph_js(graph_json: &str) -> String {
        to_json(&super::validate_graph(graph_json))
    }

    #[wasm_bindgen(js_name = version)]
    pub fn version_js() -> String {
        to_json(&super::version_info())
    }

    #[wasm_bindgen(js_name = supportedSchemas)]
    pub fn supported_schemas_js() -> String {
        to_json(&super::supported_schemas())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = "[1, 2, 3] -> * -> ?{ $ > 1 } -> $ * 10 -> []";

    #[test]
    fn cant_source_renders() {
        let result = render_cant("t.cant", PROGRAM, &RenderOptions::default());
        assert!(result.ok, "{:?}", result.diagnostics);
        let svg = result.svg.expect("an svg");
        assert!(svg.starts_with("<svg"));
        assert!(result.scene_json.is_some());
        assert!(result.graph_json.is_some());
        assert!(result
            .summary
            .expect("a summary")
            .starts_with("This sigil contains"));
        // HTML is absent unless asked for — an export, not a payload tax.
        assert!(result.html.is_none());
    }

    /// The HTML export (§16, W8): present when asked, a whole document, and as
    /// self-contained as the CLI's — no script other than its own inline one,
    /// no external reference.
    #[test]
    fn cant_source_renders_to_a_self_contained_page() {
        let result = render_cant_html("t.cant", PROGRAM, &RenderOptions::default());
        assert!(result.ok, "{:?}", result.diagnostics);
        let html = result.html.expect("an html page");
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<svg"));
        // Self-contained: nothing fetched from anywhere.
        let without_ns = html.replace("xmlns=\"http://www.w3.org/2000/svg\"", "");
        assert!(!without_ns.contains("http://"));
        assert!(!without_ns.contains("https://"));
        // The Codex decodes it — the point of the format.
        assert!(html.to_lowercase().contains("codex"));
    }

    /// A syntax error is a result, not an exception. A UI that had to catch to
    /// find out would get it wrong somewhere.
    #[test]
    fn a_syntax_error_is_a_value_with_diagnostics() {
        let result = render_cant("t.cant", "[1] -> |{", &RenderOptions::default());
        assert!(!result.ok);
        assert!(result.svg.is_none());
        assert!(!result.diagnostics.is_empty());
        assert!(result.diagnostics[0].code.starts_with("CANT-"));
    }

    #[test]
    fn a_bad_option_is_a_diagnostic_not_a_panic() {
        let options = RenderOptions {
            theme: "chartreuse".into(),
            ..Default::default()
        };
        let result = render_cant("t.cant", PROGRAM, &options);
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, "SIGIL-T001");
    }

    /// The pipe the CLI has, in the browser: graph JSON renders with no source.
    #[test]
    fn graph_json_renders_and_validates() {
        let rendered = render_cant("t.cant", PROGRAM, &RenderOptions::default());
        let graph = rendered.graph_json.expect("graph json");

        let validated = validate_graph(&graph);
        assert!(validated.ok, "{:?}", validated.diagnostics);

        let from_graph = render_graph(&graph, &RenderOptions::default());
        assert!(from_graph.ok, "{:?}", from_graph.diagnostics);
        assert_eq!(from_graph.svg, rendered.svg, "the two paths disagree");
    }

    #[test]
    fn a_foreign_graph_is_refused() {
        let result = validate_graph(r#"{"schema":"something.else","version":"1"}"#);
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, "SIGIL-V001");
        assert!(!render_graph("{}", &RenderOptions::default()).ok);
    }

    /// A `u64` seed round-trips through the string it is carried as. JavaScript
    /// cannot hold one exactly, and a seed that lost its low bits would give a
    /// different picture than the CLI for the same input.
    #[test]
    fn a_large_seed_survives_the_boundary() {
        let seed = u64::MAX - 12345;
        let options = RenderOptions {
            seed: seed.to_string(),
            ..Default::default()
        };
        let result = render_cant("t.cant", PROGRAM, &options);
        assert!(result.ok);
        assert!(
            result
                .fingerprint
                .expect("a fingerprint")
                .contains(&seed.to_string()),
            "the seed did not survive"
        );
    }

    #[test]
    fn the_browser_limits_are_the_conservative_ones() {
        let huge = "[1] -> []\n".repeat(400_000);
        assert!(huge.len() > 2 * 1024 * 1024);
        let result = render_cant("t.cant", &huge, &RenderOptions::default());
        assert!(!result.ok);
        assert_eq!(result.diagnostics[0].code, "SIGIL-S005");
    }

    /// The Veiled guarantee is about the **artifact**, not about every value
    /// crossing the boundary.
    ///
    /// The scene deliberately carries labels: it is what the Codex is built
    /// from, and §13.1 says a Veiled render may still be decoded through the
    /// Codex. What Veiled promises is that the *picture* shows nothing — which
    /// the serializer enforces by never generating a text element in that mode.
    /// `metadata none` is the setting that removes it from everywhere.
    #[test]
    fn a_veiled_render_draws_no_source_text_but_can_still_be_decoded() {
        let source = r#"["ZZSECRETZZ"] -> * -> []"#;
        let result = render_cant("t.cant", source, &RenderOptions::default());
        assert!(result.ok);
        assert!(
            !result.svg.expect("svg").contains("ZZSECRETZZ"),
            "the veiled artifact drew the label"
        );
        assert!(
            result.scene_json.expect("scene").contains("ZZSECRETZZ"),
            "the scene lost its labels, so the Codex has nothing to decode"
        );

        // And the setting that means "nothing, anywhere".
        let sealed = render_cant(
            "t.cant",
            source,
            &RenderOptions {
                metadata: "none".into(),
                ..Default::default()
            },
        );
        assert!(!sealed.svg.expect("svg").contains("ZZSECRETZZ"));
        assert!(!sealed.scene_json.expect("scene").contains("ZZSECRETZZ"));
    }

    #[test]
    fn versions_and_schemas_report_something_usable() {
        let v = version_info();
        assert_eq!(v.renderer, rite_sigil::RENDERER_VERSION);
        assert_eq!(v.graph_schema, 1);
        assert_eq!(v.scene_schema, 1);
        let s = supported_schemas();
        assert!(!s.cant_graph.is_empty());
        assert!(s.sigil_graph.contains(&1));
    }

    #[test]
    fn options_default_to_the_documented_values() {
        let o = RenderOptions::default();
        assert_eq!(o.theme, "neon-ritual");
        assert_eq!(o.mode, "veiled");
        assert_eq!(o.metadata, "safe");
        assert_eq!(o.ornament, "ritual");
        assert_eq!(o.seed, "graph");
    }
}
