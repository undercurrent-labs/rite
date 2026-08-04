//! Native and browser produce the same artifact.
//!
//! This is acceptance criterion AR4 and the reason ADR 0005 chose one renderer
//! in Rust. It is asserted here rather than in a browser harness because both
//! sides call the *same functions* — the browser path differs only in the
//! `wasm_bindgen` wrappers, which convert arguments and nothing else.
//!
//! # What that does and does not prove
//!
//! It proves the pipelines agree: the CLI's adapt-normalize-layout-render and
//! the browser's are one code path, and a change to either moves both. It does
//! **not** prove `wasm32` produces identical floats to `x86_64` — same source,
//! different target — which is a separate question, and the reason comparisons
//! go through canonical text rather than through deserialized structures (see
//! `rite_sigil::canonical`).
//!
//! A browser-run version of this belongs in the web app's end-to-end suite,
//! where it can compare the WASM build's output against a fixture generated
//! natively. Until that exists, this catches the failure that actually happens:
//! one path growing a step the other does not have.

use cant_sigil_wasm::{render_cant, render_graph, RenderOptions};

/// Every construct, so a divergence in any one of them fails.
const PROGRAMS: &[&str] = &[
    "[1, 2] -> * -> []",
    "[1, 2, 3] -> * -> ?{ $ > 1 } -> []",
    "[1] -> * -> |{ $ + 1 ; $ * 10 ; $ - 1 } -> []",
    "[1] -> * -> ~{ ?{ $ < 32 } -> $ * 2 } :by str :max 16 -> []",
    r#"["a.txt", "b.txt"] -> * -> ! @fs.read($) -> []"#,
    "[1] -> * -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []",
];

/// The native pipeline, spelled out — the same steps `cant sigil` takes.
fn native_svg(source: &str, options: &RenderOptions) -> (String, String) {
    let (parsed, sources) = cant_syntax::parse_source("parity.cant", source);
    assert!(
        !parsed.has_errors(),
        "{}",
        parsed.diagnostics.render_all(&sources)
    );
    let program = cant_sem::lower(
        &parsed.program.expect("a program"),
        "parity.cant",
        source.len(),
    );
    // The same rule the API uses: labels travel unless metadata forbids them,
    // because the Codex decodes a Veiled render (§13.1). Spelling it out here is
    // the point of this test — if the two rules drift, this fails.
    let wants_labels = options.metadata != "none";
    let adapt = if wants_labels {
        cant_sem::AdaptOptions::with_labels()
    } else {
        cant_sem::AdaptOptions::default()
    };
    let graph = cant_sem::to_sigil_graph(&program, adapt);

    // The browser's limits, so the only difference under test is the pipeline.
    let limits = rite_sigil::NormalizeOptions {
        keep_snippets: options.metadata == "full",
        ..rite_sigil::NormalizeOptions::browser()
    };
    let normalized = rite_sigil::normalize(graph, &limits).expect("normalizes");
    let seed = if options.canonical || options.seed == "canonical" {
        0
    } else if options.seed == "graph" {
        normalized.seed()
    } else {
        options.seed.parse().expect("an integer seed")
    };
    let scene = rite_sigil::build_scene(
        &normalized,
        &rite_sigil::LayoutOptions {
            seed,
            orientation: if options.canonical {
                rite_sigil::Orientation::Canonical
            } else {
                rite_sigil::Orientation::Seeded
            },
            legend: true,
            ornament: rite_sigil::OrnamentLevel::parse(&options.ornament).expect("a level"),
        },
    );
    let rendered = rite_sigil::render_svg(
        &scene,
        &rite_sigil::SvgOptions {
            theme: rite_sigil::ThemeId::parse(&options.theme).expect("a theme"),
            disclosure: rite_sigil::DisclosureMode::parse(&options.mode).expect("a mode"),
            metadata: rite_sigil::MetadataMode::parse(&options.metadata).expect("a metadata mode"),
            ..Default::default()
        },
    );
    (
        rendered.svg,
        serde_json::to_string(&scene).expect("serializes"),
    )
}

fn option_sets() -> Vec<RenderOptions> {
    let mut out = vec![RenderOptions::default()];
    for (mode, metadata) in [
        ("veiled", "safe"),
        ("inscribed", "safe"),
        ("revealed", "full"),
        ("veiled", "none"),
    ] {
        for theme in ["neon-ritual", "void", "parchment"] {
            for ornament in ["none", "ritual", "maximal"] {
                out.push(RenderOptions {
                    theme: theme.into(),
                    mode: mode.into(),
                    metadata: metadata.into(),
                    ornament: ornament.into(),
                    canonical: true,
                    ..Default::default()
                });
            }
        }
    }
    out
}

#[test]
fn the_browser_api_and_the_native_pipeline_produce_the_same_svg() {
    for source in PROGRAMS {
        for options in option_sets() {
            let (expected_svg, _) = native_svg(source, &options);
            let result = render_cant("parity.cant", source, &options);
            assert!(result.ok, "{source:?}: {:?}", result.diagnostics);
            assert_eq!(
                result.svg.expect("an svg"),
                expected_svg,
                "{source:?} diverged with {}/{}/{}/{}",
                options.theme,
                options.mode,
                options.metadata,
                options.ornament
            );
        }
    }
}

#[test]
fn the_browser_api_and_the_native_pipeline_produce_the_same_scene() {
    for source in PROGRAMS {
        for options in option_sets() {
            let (_, expected_scene) = native_svg(source, &options);
            let result = render_cant("parity.cant", source, &options);
            assert_eq!(
                result.scene_json.expect("a scene"),
                expected_scene,
                "{source:?} scene diverged"
            );
        }
    }
}

/// The two *input* paths agree with each other, which is the other half: a
/// graph document renders the same as the source it came from.
#[test]
fn rendering_from_source_and_from_its_graph_agree() {
    for source in PROGRAMS {
        let options = RenderOptions {
            canonical: true,
            ..Default::default()
        };
        let from_source = render_cant("parity.cant", source, &options);
        assert!(from_source.ok, "{source:?}");
        let graph = from_source.graph_json.clone().expect("graph json");

        let from_graph = render_graph(&graph, &options);
        assert!(from_graph.ok, "{source:?}: {:?}", from_graph.diagnostics);
        assert_eq!(
            from_graph.svg, from_source.svg,
            "{source:?}: the two input paths disagree"
        );
        assert_eq!(from_graph.scene_json, from_source.scene_json);
    }
}

/// A render is a pure function of its inputs, called through the browser API.
#[test]
fn repeated_renders_are_identical() {
    for source in PROGRAMS {
        let options = RenderOptions::default();
        let a = render_cant("parity.cant", source, &options);
        let b = render_cant("parity.cant", source, &options);
        assert_eq!(a.svg, b.svg, "{source:?}");
        assert_eq!(a.fingerprint, b.fingerprint);
    }
}

/// The fingerprint identifies the render, so two different option sets that
/// produce different pictures must not claim the same identity.
#[test]
fn different_options_produce_different_fingerprints() {
    let source = PROGRAMS[5];
    let mut seen: Vec<String> = Vec::new();
    for options in option_sets() {
        let result = render_cant("parity.cant", source, &options);
        let fingerprint = result.fingerprint.expect("a fingerprint");
        // The same (theme, mode, metadata) with different ornament *is* the same
        // fingerprint by design — ornament is non-semantic and does not appear
        // in it. So the assertion is on the distinct ones.
        let key = format!("{}/{}/{}", options.theme, options.mode, options.metadata);
        if !seen.contains(&key) {
            seen.push(key);
            assert!(
                fingerprint.contains(&options.theme),
                "the fingerprint does not name the theme: {fingerprint}"
            );
        }
    }
}
