//! `cant sigil` — render a program's topology as an artifact.
//!
//! The command is thin on purpose. Everything that decides what the picture
//! looks like lives in `rite-sigil`, so the CLI and the browser produce the same
//! bytes (ADR 0005); this file parses flags, reads a file, and writes one.
//!
//! # Two input paths, one renderer
//!
//! Cant source goes through the parser and the adapter. Graph JSON skips both —
//! it is read as `cant.graph` and adapted, which is what makes `cant graph … |
//! cant sigil --graph -` work and what proves the adapter is a real boundary
//! rather than a function call in disguise.
//!
//! # Diagnostics
//!
//! `SIGIL-*` codes, with the exit status the category maps to. Parse failures
//! keep Cant's own codes and Cant's exit statuses — a syntax error is a syntax
//! error whichever command found it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rite_sigil::{
    build_scene, normalize, render_svg, Background, DisclosureMode, HtmlOptions, LayoutOptions,
    MarkDetail, MetadataMode, NormalizeOptions, Orientation, OrnamentLevel, SvgOptions, ThemeId,
    Tracery,
};

/// Everything `cant sigil` accepts. Mirrors §17.1.
#[derive(Debug, Clone)]
pub struct SigilArgs {
    pub source: Option<PathBuf>,
    pub expr: Option<String>,
    pub graph: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub format: String,
    pub theme: String,
    pub mode: String,
    pub metadata: String,
    pub seed: String,
    pub canonical: bool,
    pub background: String,
    pub ornament: String,
    pub tracery: String,
    /// A `cant.trace` document (`cant run --trace-out`), for a weighted render.
    pub weights: Option<PathBuf>,
    /// An older version of the program, ghosted beneath the render.
    pub diff: Option<PathBuf>,
    pub width: Option<f64>,
    pub scale: f64,
    pub embed_scene: bool,
    pub simplify: bool,
    pub max_nodes: Option<usize>,
    pub check: bool,
}

/// The rendered bytes and how to describe them.
struct Artifact {
    bytes: Vec<u8>,
    /// The extension the default output path gets.
    extension: &'static str,
    fingerprint: String,
}

pub fn run(args: SigilArgs) -> ExitCode {
    match render(&args) {
        Ok(artifact) => write_artifact(&args, artifact),
        Err(failure) => {
            eprintln!("{}", failure.message);
            ExitCode::from(failure.exit)
        }
    }
}

struct Failure {
    message: String,
    exit: u8,
}

impl Failure {
    fn usage(message: impl Into<String>) -> Self {
        Failure {
            message: format!("cant: {}", message.into()),
            exit: 2,
        }
    }
}

fn render(args: &SigilArgs) -> Result<Artifact, Failure> {
    // Options first, so a typo in `--theme` is reported before a file is read.
    // Nothing is more annoying than waiting for a parse to discover a flag was
    // misspelled.
    let theme = ThemeId::parse(&args.theme).ok_or_else(|| {
        Failure::usage(format!(
            "unknown --theme `{}` — expected neon-ritual, void, or parchment",
            args.theme
        ))
    })?;
    let disclosure = DisclosureMode::parse(&args.mode).ok_or_else(|| {
        Failure::usage(format!(
            "unknown --mode `{}` — expected veiled, inscribed, or revealed",
            args.mode
        ))
    })?;
    let metadata = MetadataMode::parse(&args.metadata).ok_or_else(|| {
        Failure::usage(format!(
            "unknown --metadata `{}` — expected full, safe, minimal, or none",
            args.metadata
        ))
    })?;
    let ornament = OrnamentLevel::parse(&args.ornament).ok_or_else(|| {
        Failure::usage(format!(
            "unknown --ornament `{}` — expected none, sparse, ritual, or maximal",
            args.ornament
        ))
    })?;
    let tracery = Tracery::parse(&args.tracery).ok_or_else(|| {
        Failure::usage(format!(
            "unknown --tracery `{}` — expected flowing, concentric, or circuit",
            args.tracery
        ))
    })?;
    let background = match args.background.as_str() {
        "theme" => Background::Theme,
        "transparent" => Background::Transparent,
        hex => Background::hex(hex).map_err(|e| Failure::usage(format!("--background: {e}")))?,
    };
    if !matches!(args.format.as_str(), "svg" | "png" | "html" | "scene-json") {
        return Err(Failure::usage(format!(
            "unknown --format `{}` — expected svg, png, html, or scene-json",
            args.format
        )));
    }

    // A contradictory pair, warned about rather than silently resolved. The two
    // axes are orthogonal by design (ADR 0007), so this combination is
    // *meaningful* — draw the labels, embed nothing — but it is also what
    // someone picks when they have confused "hide it" with "do not embed it",
    // and the artifact they get has their source written across it.
    if metadata == MetadataMode::None && disclosure != DisclosureMode::Veiled {
        eprintln!(
            "warning[SIGIL-C001]: --metadata none embeds nothing, but --mode {} draws \
             labels into the artifact; use --mode veiled for a source-free picture",
            disclosure.name()
        );
    }

    // HTML always wants labels: its Codex is the point of the format, and a
    // Codex with nothing in it is a panel. Disclosure still governs what the
    // *canvas* draws, so `--format html --mode veiled` is a veiled picture with
    // a decodable Codex beside it — which is §13.4's web default.
    let html_needs_labels = args.format == "html";

    // Labels only travel when they will be drawn or embedded. The privacy
    // decision is made here, at the adapter, rather than filtered out later —
    // see `docs/adr/0007-veil-and-source-privacy.md`.
    let wants_labels = disclosure != DisclosureMode::Veiled
        || metadata == MetadataMode::Full
        || (html_needs_labels && metadata != MetadataMode::None);
    let adapt = if wants_labels {
        cant_sem::AdaptOptions::with_labels()
    } else {
        cant_sem::AdaptOptions::default()
    };

    let mut limits = NormalizeOptions {
        keep_snippets: metadata == MetadataMode::Full,
        ..Default::default()
    };
    if let Some(max) = args.max_nodes {
        limits.limits.max_nodes = max;
    }

    let (mut graph, source_name) = load_graph(args, adapt)?;
    if let Some(path) = &args.weights {
        apply_weights(&mut graph, path)?;
    }

    let normalized = normalize(graph, &limits).map_err(|diagnostics| Failure {
        message: diagnostics.to_string(),
        exit: diagnostics.exit_code().max(1),
    })?;
    for warning in normalized.diagnostics.iter() {
        eprintln!("{warning}");
    }

    let layout = LayoutOptions {
        seed: resolve_seed(args, &normalized)?,
        orientation: if args.canonical {
            Orientation::Canonical
        } else {
            Orientation::Seeded
        },
        legend: true,
        ornament,
        tracery,
    };
    let scene = build_scene(&normalized, &layout);
    for warning in &scene.warnings {
        eprintln!("warning[SIGIL-L001]: {warning}");
    }

    // The diff render: the old program's scene ghosted beneath this one.
    // Canonical for both — two seeded rotations would turn everything into a
    // "move" — and SVG only, because the ghost is an overlay of vector layers.
    if let Some(old_path) = &args.diff {
        if args.format != "svg" {
            return Err(Failure::usage(
                "--diff renders SVG only — drop --format or set it to svg",
            ));
        }
        if !args.canonical {
            return Err(Failure::usage(
                "--diff needs --canonical: with seeded rotation, every element becomes a \"move\"",
            ));
        }
        let old_text = std::fs::read_to_string(old_path).map_err(|e| Failure {
            message: format!("cant: could not read {}: {e}", old_path.display()),
            exit: 2,
        })?;
        let (old_parsed, old_sources) =
            cant_syntax::parse_source(&old_path.display().to_string(), &old_text);
        if old_parsed.has_errors() {
            return Err(Failure {
                message: old_parsed.diagnostics.render_all(&old_sources),
                exit: 3,
            });
        }
        let old_program = cant_sem::lower(
            &old_parsed.program.expect("no errors, so a program"),
            &old_path.display().to_string(),
            old_text.len(),
        );
        let old_graph = cant_sem::to_sigil_graph(&old_program, cant_sem::AdaptOptions::default());
        let old_normalized = normalize(old_graph, &limits).map_err(|diagnostics| Failure {
            message: diagnostics.to_string(),
            exit: diagnostics.exit_code().max(1),
        })?;
        // No ornament on the ghost's scene: its seed differs, so its ornament
        // would be unrelated noise under the real render's.
        let old_scene = build_scene(
            &old_normalized,
            &LayoutOptions {
                ornament: rite_sigil::OrnamentLevel::None,
                tracery: layout.tracery,
                ..LayoutOptions::canonical()
            },
        );
        let svg_options = SvgOptions {
            theme,
            disclosure,
            metadata,
            background,
            mark_detail: if args.simplify {
                MarkDetail::Minimal
            } else {
                MarkDetail::Full
            },
            width: args.width,
            height: args.width,
        };
        let rendered = rite_sigil::render_diff(&old_scene, &scene, &svg_options);
        return Ok(Artifact {
            bytes: rendered.svg.into_bytes(),
            extension: "sigil.diff.svg",
            fingerprint: rendered.fingerprint.to_line(),
        });
    }

    if args.format == "scene-json" {
        let json = serde_json::to_string_pretty(&scene).map_err(|e| Failure {
            message: format!("cant: could not serialize the scene: {e}"),
            exit: 1,
        })?;
        return Ok(Artifact {
            bytes: json.into_bytes(),
            extension: "sigil.json",
            fingerprint: scene.metadata.graph_fingerprint.clone(),
        });
    }

    let svg_options = SvgOptions {
        theme,
        disclosure,
        metadata,
        background,
        mark_detail: if args.simplify {
            MarkDetail::Minimal
        } else {
            MarkDetail::Full
        },
        width: args.width,
        height: args.width,
    };
    let rendered = render_svg(&scene, &svg_options);
    let _ = source_name;

    if args.format == "html" {
        let page = rite_sigil::render_html(
            &scene,
            &HtmlOptions {
                svg: svg_options,
                codex: true,
                embed_scene: args.embed_scene,
            },
        );
        return Ok(Artifact {
            bytes: page.into_bytes(),
            extension: "sigil.html",
            fingerprint: rendered
                .fingerprint
                .to_line()
                .replace("format=svg", "format=html"),
        });
    }

    if args.format == "png" {
        // Width, when given, sets the scale: the canvas is 1600 square, so a
        // `--width 3200` is a 2× render. Expressing it as a scale rather than
        // resampling afterwards keeps the strokes crisp.
        let scale = match args.width {
            Some(width) => (width / rite_sigil::layout::VIEW_SIZE) as f32,
            None => args.scale as f32,
        };
        let bytes = rite_sigil::render_png(&scene, &svg_options, scale).map_err(|e| Failure {
            message: format!("error[SIGIL-R001]: {e}"),
            exit: 1,
        })?;
        return Ok(Artifact {
            bytes,
            extension: "sigil.png",
            fingerprint: rendered
                .fingerprint
                .to_line()
                .replace("format=svg", "format=png"),
        });
    }

    Ok(Artifact {
        bytes: rendered.svg.into_bytes(),
        extension: "sigil.svg",
        fingerprint: rendered.fingerprint.to_line(),
    })
}

/// Cant source or graph JSON into a normalized graph.
/// Read a `cant.trace` document and put its counts on the graph.
///
/// Ids the graph does not have are reported rather than dropped — a trace from
/// last week's program silently half-applying is how a picture lies. Nodes the
/// trace does not name ran zero times, which is a fact worth drawing.
fn apply_weights(graph: &mut rite_sigil::SigilGraph, path: &Path) -> Result<(), Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure {
        message: format!("cant: could not read the trace: {}: {e}", path.display()),
        exit: 2,
    })?;
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| Failure {
        message: format!("cant: {} is not JSON: {e}", path.display()),
        exit: 2,
    })?;
    if doc.get("schema").and_then(|v| v.as_str()) != Some("cant.trace") {
        return Err(Failure {
            message: format!(
                "cant: {} is not a cant.trace document — write one with `cant run --trace-out`",
                path.display()
            ),
            exit: 2,
        });
    }
    let counts = doc
        .get("nodes")
        .and_then(|v| v.as_object())
        .ok_or_else(|| Failure {
            message: format!("cant: {} has no `nodes` object", path.display()),
            exit: 2,
        })?;

    let known: std::collections::BTreeSet<&str> =
        graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for id in counts.keys() {
        if !known.contains(id.as_str()) {
            eprintln!(
                "warning[SIGIL-W002]: the trace counts `{id}`, which this program does not have — \
                 is the trace from an older version of it?"
            );
        }
    }
    for node in &mut graph.nodes {
        node.weight = Some(
            counts
                .get(node.id.as_str())
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        );
    }
    Ok(())
}

fn load_graph(
    args: &SigilArgs,
    adapt: cant_sem::AdaptOptions,
) -> Result<(rite_sigil::SigilGraph, String), Failure> {
    if let Some(path) = &args.graph {
        let text = read_input(path)?;
        let analysis =
            cant_sem::validate_deserialized(&text, rite_core::FileId(0)).map_err(|e| Failure {
                message: format!("error[SIGIL-V001]: {e}"),
                exit: 4,
            })?;
        if analysis.diagnostics.has_errors() {
            return Err(Failure {
                message: "error[SIGIL-G002]: the graph does not validate".to_string(),
                exit: 4,
            });
        }
        let name = analysis.graph.source.name.clone();
        return Ok((cant_sem::to_sigil_graph(&analysis.graph, adapt), name));
    }

    let (name, text) = match (&args.source, &args.expr) {
        (Some(_), Some(_)) => return Err(Failure::usage("give a source file or `-e`, not both")),
        (None, None) => {
            return Err(Failure::usage(
                "no source: pass a file, `-` for standard input, `-e 'expression'`, or `--graph`",
            ))
        }
        (None, Some(expr)) => ("<expr>".to_string(), expr.clone()),
        (Some(path), None) => (path.display().to_string(), read_input(path)?),
    };

    let analysis = cant::analyze(&name, &text);
    if !analysis.diagnostics.is_empty() {
        eprintln!("{}", analysis.render());
    }
    let Some(graph) = analysis.graph else {
        return Err(Failure {
            message: String::new(),
            exit: analysis.diagnostics.rejection_exit_code().max(1),
        });
    };
    if analysis.diagnostics.has_errors() {
        return Err(Failure {
            message: String::new(),
            exit: analysis.diagnostics.rejection_exit_code(),
        });
    }

    Ok((cant_sem::to_sigil_graph(&graph, adapt), name))
}

fn read_input(path: &Path) -> Result<String, Failure> {
    if path.as_os_str() == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).map_err(|e| Failure {
            message: format!("cant: cannot read standard input: {e}"),
            exit: 2,
        })?;
        return Ok(text);
    }
    std::fs::read_to_string(path).map_err(|e| Failure {
        message: format!("cant: cannot read {}: {e}", path.display()),
        exit: 2,
    })
}

/// `--seed graph|canonical|random|<integer>`.
fn resolve_seed(
    args: &SigilArgs,
    normalized: &rite_sigil::NormalizedGraph,
) -> Result<u64, Failure> {
    if args.canonical {
        return Ok(0);
    }
    match args.seed.as_str() {
        "graph" => Ok(normalized.seed()),
        // A documented fixed value, so `--seed canonical` and `--canonical`
        // agree about what canonical means.
        "canonical" => Ok(0),
        "random" => {
            // Time-derived rather than from a CSPRNG: this seeds an ornament
            // pattern, not a key, and pulling in a random-number dependency for
            // it would put one in a crate whose whole argument is a small
            // dependency graph. The value is reported, so a render is still
            // reproducible.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            eprintln!("note: --seed random chose {nanos}");
            Ok(nanos)
        }
        other => other.parse::<u64>().map_err(|_| {
            Failure::usage(format!(
                "unknown --seed `{other}` — expected graph, canonical, random, or an integer"
            ))
        }),
    }
}

/// Write the artifact, or check it without writing.
fn write_artifact(args: &SigilArgs, artifact: Artifact) -> ExitCode {
    if args.check {
        // `--check` renders and reports, so a build can assert an artifact is
        // producible without producing one.
        println!("{}", artifact.fingerprint);
        return ExitCode::SUCCESS;
    }

    let target = match &args.output {
        Some(path) if path.as_os_str() == "-" => None,
        Some(path) => Some(path.clone()),
        // §17.3: `<source-basename>.sigil.svg` for a file, stdout otherwise —
        // writing a file for `-e` would leave artifacts in whatever directory
        // someone happened to be in.
        None => args
            .source
            .as_ref()
            .filter(|p| p.as_os_str() != "-")
            .map(|p| p.with_extension(artifact.extension))
            .or_else(|| {
                args.graph
                    .as_ref()
                    .filter(|p| p.as_os_str() != "-")
                    .map(|p| p.with_extension(artifact.extension))
            }),
    };

    match target {
        Some(path) => {
            if let Err(e) = std::fs::write(&path, &artifact.bytes) {
                eprintln!("cant: cannot write {}: {e}", path.display());
                return ExitCode::from(1);
            }
            eprintln!("{} written", path.display());
            ExitCode::SUCCESS
        }
        None => {
            let mut out = std::io::stdout().lock();
            if let Err(e) = out.write_all(&artifact.bytes) {
                eprintln!("cant: cannot write to standard output: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
    }
}
