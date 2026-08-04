//! Golden fixtures: Cant source → normalized graph → scene.
//!
//! The scene snapshot is the primary semantic-and-layout regression layer, as
//! §26.2 asks. It exists so that a layout change and an SVG-serialization change
//! are different test failures — a distinction that only works if the snapshot
//! is written *before* there is an SVG writer to confuse it with.
//!
//! # Snapshots are not the assertion
//!
//! A snapshot file that is written and never structurally checked proves only
//! that something was written. Every fixture here is asserted on as well: node
//! kinds present, band membership, branch order, determinism, and — for the
//! malicious set — that nothing escaped. The snapshot's job is to make a
//! *diff* readable when one of those assertions starts failing.
//!
//! Regenerate with `SIGIL_BLESS=1 cargo test -p cant-sem --test sigil_fixtures`.
//! Review the diff before committing it; a blessed regression is still a
//! regression.

use std::path::{Path, PathBuf};

use cant_sem::{to_sigil_graph, AdaptOptions};
use cant_syntax::parse_source;
use rite_sigil::{
    build_scene, normalize, render_svg, Geometry, LayoutOptions, NormalizeOptions, SigilNodeKind,
    SvgOptions, ThemeId,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-sem has two ancestors")
        .to_path_buf()
}

/// Every example, by the name its fixtures are stored under.
const EXAMPLES: &[&str] = &[
    "basic-flow",
    "ward",
    "fork",
    "orbit",
    "effects",
    "complex",
    "ceremony",
];

fn source_of(name: &str) -> String {
    let path = repo_root()
        .join("examples/sigil")
        .join(format!("{name}.cant"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn scene_of(name: &str) -> rite_sigil::SigilScene {
    let source = source_of(name);
    let (parsed, sources) = parse_source(&format!("{name}.cant"), &source);
    assert!(
        !parsed.has_errors(),
        "{name}: {}",
        parsed.diagnostics.render_all(&sources)
    );
    let program = cant_sem::lower(
        &parsed.program.expect("program"),
        &format!("{name}.cant"),
        source.len(),
    );
    let graph = to_sigil_graph(&program, AdaptOptions::default());
    let normalized = normalize(graph, &NormalizeOptions::default())
        .unwrap_or_else(|d| panic!("{name} did not normalize:\n{d}"));
    // Canonical orientation: a golden written against a seeded rotation would
    // be a golden of the seed, not of the layout.
    build_scene(&normalized, &LayoutOptions::canonical())
}

/// Compare against the stored snapshot, or write it when blessing.
fn check_snapshot(name: &str, actual: &str) {
    let path = repo_root()
        .join("fixtures/sigil/scenes")
        .join(format!("{name}.scene.json"));

    if std::env::var_os("SIGIL_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("has a parent")).expect("mkdir");
        std::fs::write(&path, actual).expect("write snapshot");
        return;
    }

    // A missing fixture is a failure, not a skip — the same position the Cant
    // conformance runner takes. A fixture that silently does not exist is
    // coverage nobody notices losing.
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing scene fixture {}: {e}\n\
             regenerate with SIGIL_BLESS=1 cargo test -p cant-sem --test sigil_fixtures",
            path.display()
        )
    });

    if expected.trim() != actual.trim() {
        // The first differing line, so the message points at the change rather
        // than printing two thousand lines of JSON.
        let diff = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b)
            .map(|(n, (a, b))| format!("line {}:\n  stored: {a}\n  actual: {b}", n + 1))
            .unwrap_or_else(|| {
                format!(
                    "length differs: stored {} lines, actual {} lines",
                    expected.lines().count(),
                    actual.lines().count()
                )
            });
        panic!("scene fixture {name} changed.\n{diff}\n\nIf intended, regenerate with SIGIL_BLESS=1 and review the diff.");
    }
}

#[test]
fn every_example_matches_its_scene_fixture() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        let json = serde_json::to_string_pretty(&scene).expect("serializes");
        check_snapshot(name, &json);
    }
}

/// The claim the golden rests on: the fixture is a function of the program, not
/// of when it was generated.
#[test]
fn scenes_are_deterministic_across_runs() {
    for name in EXAMPLES {
        let a = serde_json::to_string(&scene_of(name)).expect("serializes");
        let b = serde_json::to_string(&scene_of(name)).expect("serializes");
        assert_eq!(a, b, "{name} is not deterministic");
    }
}

/// Every node in the graph reaches the scene. §26.5's property, stated over
/// real programs: a node that vanished would be a silently missing part of the
/// picture.
#[test]
fn every_graph_node_has_a_scene_element() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        let drawn = scene
            .elements
            .iter()
            .filter(|e| matches!(e.graph_ref, Some(rite_sigil::SceneRef::Node(_))))
            .count();
        assert_eq!(
            drawn, scene.metadata.node_count,
            "{name}: {drawn} node elements for {} nodes",
            scene.metadata.node_count
        );
        assert_eq!(scene.hit_regions.len(), scene.metadata.node_count);
        assert_eq!(scene.legend.len(), scene.metadata.node_count);
    }
}

/// Every edge too — nothing is bundled away in Phase 2.
#[test]
fn every_graph_edge_has_a_scene_element() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        let drawn = scene
            .elements
            .iter()
            .filter(|e| matches!(e.graph_ref, Some(rite_sigil::SceneRef::Edge(_))))
            .count();
        assert_eq!(drawn, scene.metadata.edge_count, "{name}");
    }
}

/// The visual grammar's acceptance criterion, over real programs rather than
/// hand-built graphs: each construct produces its own kind of element.
#[test]
fn each_example_contains_the_construct_it_demonstrates() {
    let expected: &[(&str, SigilNodeKind)] = &[
        ("basic-flow", SigilNodeKind::Source),
        ("ward", SigilNodeKind::Ward),
        ("fork", SigilNodeKind::Fork),
        ("orbit", SigilNodeKind::Orbit),
        ("effects", SigilNodeKind::Effect),
        ("complex", SigilNodeKind::Fork),
        // The densest example: the one place an effect fires from *inside* a
        // fork branch rather than from the spine.
        ("ceremony", SigilNodeKind::Effect),
    ];
    for (name, kind) in expected {
        let scene = scene_of(name);
        assert!(
            scene
                .elements
                .iter()
                .any(|e| e.semantic == rite_sigil::SemanticKind::Node(kind.clone())),
            "{name} contains no {} element",
            kind.name()
        );
    }
}

/// §9.10: an invocation is on the outer boundary, and a spoke reaches it.
#[test]
fn effects_reach_the_invocation_boundary() {
    let scene = scene_of("effects");
    let boundary = rite_sigil::layout::SAFE_RADIUS * 0.85;
    let center = scene.center;

    let invocations: Vec<&rite_sigil::SceneElement> = scene
        .elements
        .iter()
        .filter(|e| e.semantic == rite_sigil::SemanticKind::Node(SigilNodeKind::Effect))
        .collect();
    assert!(!invocations.is_empty(), "the effects example has no effect");

    for element in invocations {
        let Geometry::Mark { center: p, .. } = &element.geometry else {
            panic!("an invocation must be a mark");
        };
        let r = (p.x - center.x).hypot(p.y - center.y);
        assert!(
            r >= boundary,
            "{} is at radius {r}, inside the boundary at {boundary}",
            element.id
        );
    }
}

/// §26.5: every coordinate finite, everything within the expanded viewBox.
#[test]
fn every_scene_coordinate_is_finite_and_bounded() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        // Generous: an edge's bounding box is deliberately larger than the arc
        // it contains, and a stroke may overhang the safe radius.
        let allowed = scene.view_box.expanded(rite_sigil::layout::VIEW_SIZE);
        for element in &scene.elements {
            let b = element.bounds;
            assert!(
                [b.x, b.y, b.width, b.height].iter().all(|v| v.is_finite()),
                "{name}: {} has non-finite bounds",
                element.id
            );
            assert!(
                allowed.contains(rite_sigil::Point::new(
                    b.x + b.width / 2.0,
                    b.y + b.height / 2.0
                )),
                "{name}: {} is centred far outside the canvas",
                element.id
            );
        }
    }
}

/// The default adapter carries no labels, so no scene built from it can contain
/// the program's text — anywhere, including titles and legend entries. This is
/// the Veiled guarantee at the layer it actually has to hold (ADR 0007).
#[test]
fn no_scene_built_from_the_default_adapter_contains_source_text() {
    // Words that appear only in the example sources, never in the vocabulary of
    // the visual grammar.
    let needles: &[(&str, &[&str])] = &[
        ("effects", &["notes.txt", "log.txt", "@fs.read"]),
        ("complex", &["seed.json", "@fs.read"]),
        ("ward", &["$ > 2"]),
    ];
    for (name, words) in needles {
        let json = serde_json::to_string(&scene_of(name)).expect("serializes");
        for word in *words {
            assert!(
                !json.contains(word),
                "{name}: the scene leaked source text `{word}`"
            );
        }
    }
}

/// The accessible summary §23 requires, over real programs.
#[test]
fn every_example_produces_an_accessible_summary() {
    for name in EXAMPLES {
        let summary = scene_of(name).summary();
        assert!(
            summary.starts_with("This sigil contains"),
            "{name}: {summary}"
        );
        assert!(summary.ends_with('.'), "{name}: {summary}");
        assert!(summary.len() > 25, "{name}: uselessly short: {summary}");
    }
}

/// Every example renders to SVG, and the artifact is the same bytes every time.
///
/// The SVG goldens live beside the scene goldens because they fail differently:
/// a scene diff means the layout moved, an SVG diff with an unchanged scene
/// means the serializer or a theme did.
#[test]
fn every_example_matches_its_svg_fixture() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        let rendered = render_svg(&scene, &SvgOptions::default());
        let path = repo_root()
            .join("fixtures/sigil/svg")
            .join(format!("{name}.veiled.svg"));

        if std::env::var_os("SIGIL_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, &rendered.svg).expect("write");
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing SVG fixture {}: {e}\nregenerate with SIGIL_BLESS=1",
                path.display()
            )
        });
        assert_eq!(expected, rendered.svg, "{name}'s SVG changed");

        // And the same options twice are the same bytes.
        assert_eq!(rendered.svg, render_svg(&scene, &SvgOptions::default()).svg);
    }
}

/// Every theme renders every example without a panic, and the three produce
/// visibly different artifacts — a theme that changed nothing would be a theme
/// nobody could tell they had selected.
#[test]
fn every_theme_renders_every_example_distinctly() {
    for name in EXAMPLES {
        let scene = scene_of(name);
        let mut seen = Vec::new();
        for theme in ThemeId::ALL {
            let svg = render_svg(
                &scene,
                &SvgOptions {
                    theme: *theme,
                    ..Default::default()
                },
            )
            .svg;
            assert!(svg.starts_with("<svg"), "{name}/{}", theme.name());
            assert!(
                !seen.contains(&svg),
                "{name}: two themes render identically"
            );
            seen.push(svg);
        }
    }
}

/// The render fingerprint reports everything §12.3 requires, and changing any
/// geometry-affecting option changes it.
#[test]
fn the_render_fingerprint_reports_what_produced_it() {
    let scene = scene_of("complex");
    let base = render_svg(&scene, &SvgOptions::default()).fingerprint;
    assert_eq!(base.graph, scene.metadata.graph_fingerprint);
    assert_eq!(base.theme, "neon-ritual");
    assert_eq!(base.disclosure, "veiled");
    assert_eq!(base.metadata, "safe");
    assert_eq!(base.format, "svg");
    assert!(base.to_line().contains("sigil/"));

    let other = render_svg(
        &scene,
        &SvgOptions {
            theme: ThemeId::Void,
            ..Default::default()
        },
    )
    .fingerprint;
    assert_ne!(base, other, "a theme change left the fingerprint unmoved");
}

/// A hostile label must not reach an element ID, and must survive round-tripping
/// as data. §22.1's rule at the layer where it is cheapest to hold.
#[test]
fn malicious_labels_do_not_escape_into_element_ids() {
    let hostile = r#"</script><img src=x onerror=alert(1)>"#;
    let source = format!(r#"["{}"] -> * -> []"#, hostile.replace('"', "'"));
    let (parsed, sources) = parse_source("hostile.cant", &source);
    assert!(
        !parsed.has_errors(),
        "{}",
        parsed.diagnostics.render_all(&sources)
    );
    let program = cant_sem::lower(
        &parsed.program.expect("program"),
        "hostile.cant",
        source.len(),
    );
    // With labels on — the worst case, where the text is deliberately carried.
    let graph = to_sigil_graph(&program, AdaptOptions::with_labels());
    let normalized = normalize(graph, &NormalizeOptions::default()).expect("valid");
    let scene = build_scene(&normalized, &LayoutOptions::canonical());

    for element in &scene.elements {
        for banned in ['<', '>', '"', '\'', '&'] {
            assert!(
                !element.id.contains(banned),
                "element ID carries `{banned}`: {}",
                element.id
            );
        }
    }
    // And the scene is still valid JSON that parses — an injection that broke
    // serialization would be just as bad as one that escaped.
    //
    // Structure equality after a round trip is deliberately *not* asserted:
    // `serde_json`'s float parser loses a unit in the last place on some values,
    // which is why parity comparisons go through canonical text rather than
    // through deserialized structures. See `rite_sigil::canonical`.
    let json = serde_json::to_string(&scene).expect("serializes");
    let back: rite_sigil::SigilScene = serde_json::from_str(&json).expect("parses back");
    assert_eq!(back.metadata, scene.metadata);
    assert_eq!(back.elements.len(), scene.elements.len());
    for (a, b) in back.elements.iter().zip(scene.elements.iter()) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.semantic, b.semantic);
        assert_eq!(a.graph_ref, b.graph_ref);
    }
}

/// The tracery axis, over the densest example.
///
/// Three claims. The traceries are *distinct* — each draws traces the others
/// do not. They are *node-invariant* — a tracery changes every edge's shape
/// and no mark's position, which is what makes it an axis like theme rather
/// than a different layout. And the two non-default ones have goldens of
/// their own, so a change to either is a reviewed diff rather than a drift.
/// (`flowing` is the default and is already pinned by every other golden.)
#[test]
fn traceries_are_distinct_and_move_no_mark() {
    use rite_sigil::Tracery;

    let scene_with = |tracery: Tracery| {
        let source = source_of("ceremony");
        let (parsed, _) = parse_source("ceremony.cant", &source);
        let program = cant_sem::lower(
            &parsed.program.expect("program"),
            "ceremony.cant",
            source.len(),
        );
        let graph = to_sigil_graph(&program, AdaptOptions::default());
        let normalized = normalize(graph, &NormalizeOptions::default()).expect("normalizes");
        build_scene(
            &normalized,
            &rite_sigil::LayoutOptions {
                tracery,
                ..rite_sigil::LayoutOptions::canonical()
            },
        )
    };

    let scenes: Vec<_> = Tracery::ALL.iter().map(|t| (*t, scene_with(*t))).collect();

    // Marks stay put across every tracery.
    let node_geometry = |scene: &rite_sigil::SigilScene| {
        scene
            .elements
            .iter()
            .filter(|e| matches!(e.graph_ref, Some(rite_sigil::SceneRef::Node(_))))
            .map(|e| (e.id.clone(), format!("{:?}", e.geometry)))
            .collect::<Vec<_>>()
    };
    let baseline = node_geometry(&scenes[0].1);
    for (tracery, scene) in &scenes[1..] {
        assert_eq!(
            node_geometry(scene),
            baseline,
            "{} moved a mark",
            tracery.name()
        );
    }

    // And the pictures differ.
    let svgs: Vec<String> = scenes
        .iter()
        .map(|(_, scene)| render_svg(scene, &SvgOptions::default()).svg)
        .collect();
    for i in 0..svgs.len() {
        for j in (i + 1)..svgs.len() {
            assert_ne!(
                svgs[i],
                svgs[j],
                "{} and {} rendered identically",
                scenes[i].0.name(),
                scenes[j].0.name()
            );
        }
    }

    // Goldens for the two non-default traceries.
    for (tracery, scene) in &scenes {
        if *tracery == Tracery::Flowing {
            continue;
        }
        let rendered = render_svg(scene, &SvgOptions::default());
        let path = repo_root()
            .join("fixtures/sigil/svg")
            .join(format!("ceremony.{}.veiled.svg", tracery.name()));
        if std::env::var_os("SIGIL_BLESS").is_some() {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, &rendered.svg).expect("write");
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "missing SVG fixture {}: {e}\nregenerate with SIGIL_BLESS=1",
                path.display()
            )
        });
        assert_eq!(
            expected,
            rendered.svg,
            "ceremony's {} SVG changed",
            tracery.name()
        );
    }
}
