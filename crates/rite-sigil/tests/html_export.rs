//! The interactive HTML export, over hostile input.
//!
//! It is the one output format with a script in it, so the assertions are about
//! what *else* is in it: no remote reference, no inline handler, no user text in
//! executable position, and the same disclosure guarantees the SVG has.

use rite_sigil::{
    build_scene, normalize, render_html, Capability, CapabilityFamily, DisclosureMode, EdgeId,
    EdgeKind, EffectMetadata, HtmlOptions, LayoutOptions, MetadataMode, NormalizeOptions, PortRef,
    SigilEdge, SigilGraph, SigilNode, SigilNodeKind, SourceLanguage, SourceRef, SvgOptions,
};

const HOSTILE: &[&str] = &[
    r#"</script><script>alert(1)</script>"#,
    r#"" onload="alert(1)"#,
    r#"</style><script>alert(1)</script>"#,
    r#"javascript:alert(1)"#,
    r#"</div></aside><img src=x onerror=alert(1)>"#,
    r#"<script>"#,
    "line\u{0}break",
];

fn graph_with(label: &str) -> SigilGraph {
    let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
    let mut source = SigilNode::new("n0", SigilNodeKind::Source);
    source.label = Some(label.to_string());
    source.source = Some(SourceRef {
        span: rite_core::Span::from_range(0, 8),
        snippet: Some(label.to_string()),
    });

    let mut effect = SigilNode::new("n1", SigilNodeKind::Effect);
    effect.label = Some(label.to_string());
    effect.effect = Some(EffectMetadata {
        performs: true,
        capabilities: vec![Capability {
            name: Some(label.to_string()),
            family: CapabilityFamily::Other(label.to_string()),
        }],
    });

    let mut output = SigilNode::new("n2", SigilNodeKind::Output);
    output.label = Some(label.to_string());

    g.nodes.push(source);
    g.nodes.push(effect);
    g.nodes.push(output);
    g.exits.push("n2".into());
    for (i, (from, to)) in [("n0", "n1"), ("n1", "n2")].into_iter().enumerate() {
        g.edges.push(SigilEdge {
            id: EdgeId::new(format!("e{i}")),
            from: PortRef::new(from, 0),
            to: PortRef::new(to, 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
    }
    g
}

fn html(label: &str, options: &HtmlOptions) -> String {
    let normalized = normalize(
        graph_with(label),
        &NormalizeOptions::default().with_snippets(),
    )
    .expect("valid");
    let scene = build_scene(&normalized, &LayoutOptions::canonical());
    render_html(&scene, options)
}

fn revealed(metadata: MetadataMode) -> HtmlOptions {
    HtmlOptions {
        svg: SvgOptions {
            disclosure: DisclosureMode::Revealed,
            metadata,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Offline means offline: nothing to fetch, from anywhere.
#[test]
fn the_page_references_nothing_remote() {
    for label in HOSTILE {
        for options in [HtmlOptions::default(), revealed(MetadataMode::Full)] {
            let page = html(label, &options).to_lowercase();
            let without_ns = page.replace("xmlns=\"http://www.w3.org/2000/svg\"", "");
            for banned in [
                "http://",
                "https://",
                "//cdn",
                "<link",
                "@import",
                "url(http",
                "fetch(",
                "xmlhttprequest",
                "importscripts",
            ] {
                assert!(
                    !without_ns.contains(banned),
                    "remote reference `{banned}` for {label:?}"
                );
            }
        }
    }
}

/// One script block, ours, with no inline handlers anywhere.
#[test]
fn behaviour_is_one_managed_script_with_no_inline_handlers() {
    for label in HOSTILE {
        let page = html(label, &revealed(MetadataMode::Full));
        let lower = page.to_lowercase();

        // Exactly two `<script` openings: the inert JSON block is not emitted by
        // default, so this is the behaviour block alone.
        assert_eq!(
            lower.matches("<script").count(),
            1,
            "expected one script block for {label:?}"
        );
        // Inside tags only. An escaped label drawn as element text can contain
        // the literal ` onload=` — `&quot; onload=&quot;alert(1)` is inert, and
        // a whole-document search would fire on the escaper working.
        for tag in tags(&lower) {
            for banned in [
                " onload=",
                " onerror=",
                " onclick=",
                " onmouseover=",
                "javascript:",
            ] {
                assert!(
                    !tag.contains(banned),
                    "inline handler `{banned}` in `<{tag}>` for {label:?}"
                );
            }
        }
        // The script must not evaluate anything.
        for banned in ["eval(", "new function", "settimeout(\"", "innerhtml ="] {
            assert!(!lower.contains(banned), "`{banned}` for {label:?}");
        }
    }
}

/// No user text reaches executable position. The script is a constant; the label
/// lives in markup that was escaped.
#[test]
fn no_user_text_appears_inside_the_script_block() {
    const MARKER: &str = "ZZQQMARKERZZQQ";
    let page = html(MARKER, &revealed(MetadataMode::Full));
    let start = page.find("<script").expect("a script block");
    let end = page[start..].find("</script>").expect("a close") + start;
    assert!(
        !page[start..end].contains(MARKER),
        "user text reached the script block"
    );
    // And it is present in the document, so the test is not passing vacuously.
    assert!(page.contains(MARKER), "the label was not rendered at all");
}

/// The Codex is in the document and collapsed, which is §13.4's web default.
#[test]
fn the_codex_is_present_and_collapsed_by_default() {
    let page = html("plain", &HtmlOptions::default());
    assert!(page.contains("id=\"sigil-codex\""));
    assert!(
        page.contains("<aside class=\"codex\" id=\"sigil-codex\" hidden>"),
        "the codex is not collapsed"
    );
    assert!(page.contains("aria-expanded=\"false\""));

    let without = html(
        "plain",
        &HtmlOptions {
            codex: false,
            ..Default::default()
        },
    );
    assert!(!without.contains("id=\"sigil-codex\""));
}

/// The same disclosure guarantee the SVG has: Veiled draws nothing readable in
/// the artifact itself.
#[test]
fn a_veiled_page_draws_no_label_in_the_canvas() {
    const MARKER: &str = "ZZQQMARKERZZQQ";
    let page = html(MARKER, &HtmlOptions::default());
    let canvas_start = page.find("<div class=\"canvas\">").expect("canvas");
    let canvas_end = page.find("</div>").expect("canvas end");
    assert!(
        !page[canvas_start..canvas_end].contains(MARKER),
        "a veiled canvas drew the label"
    );
}

/// `metadata none` keeps labels out of the Codex — the panel is markup rather
/// than artifact, and it would otherwise be the way round the guarantee.
///
/// The *canvas* is a different question: `--mode revealed` asks for labels to be
/// drawn, and they are, because disclosure governs what is drawn and metadata
/// governs what is embedded. `cant sigil` warns about the pairing rather than
/// silently resolving it.
#[test]
fn metadata_none_keeps_labels_out_of_the_codex() {
    const MARKER: &str = "ZZQQMARKERZZQQ";
    let page = html(MARKER, &revealed(MetadataMode::None));

    if let Some(start) = page.find("<aside class=\"codex\"") {
        let end = page[start..].find("</aside>").expect("codex close") + start;
        assert!(
            !page[start..end].contains(MARKER),
            "metadata=none leaked a label into the Codex"
        );
    }
    // And nothing invisible carries it either.
    assert!(!page.contains("<desc"), "metadata=none emitted a desc");

    // Veiled + none is the combination that guarantees nothing at all.
    let sealed = html(
        MARKER,
        &HtmlOptions {
            svg: SvgOptions {
                metadata: MetadataMode::None,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert!(
        !sealed.contains(MARKER),
        "veiled + metadata=none leaked the label"
    );
}

/// Every `<...>` region, exclusive of the brackets.
fn tags(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else { break };
        out.push(&after[..end]);
        rest = &after[end + 1..];
    }
    out
}

/// The scene is embedded only when asked for *and* metadata allows it, and it is
/// inert when it is.
#[test]
fn the_scene_is_embedded_only_on_request_and_never_executable() {
    let default = html("plain", &HtmlOptions::default());
    assert!(!default.contains("id=\"sigil-scene\""));

    // Asked for, but `safe` metadata: refused, because a scene carries labels.
    let refused = html(
        "plain",
        &HtmlOptions {
            embed_scene: true,
            ..Default::default()
        },
    );
    assert!(!refused.contains("id=\"sigil-scene\""));

    let embedded = html(
        "plain",
        &HtmlOptions {
            embed_scene: true,
            ..revealed(MetadataMode::Full)
        },
    );
    assert!(embedded.contains("id=\"sigil-scene\""));
    assert!(
        embedded.contains("type=\"application/json\""),
        "the embedded scene is not marked inert"
    );
}

/// A `</script` inside the embedded JSON would close the element whatever its
/// type says.
#[test]
fn an_embedded_scene_cannot_close_its_own_element() {
    for label in HOSTILE {
        let page = html(
            label,
            &HtmlOptions {
                embed_scene: true,
                ..revealed(MetadataMode::Full)
            },
        );
        let start = page.find("id=\"sigil-scene\"").expect("the scene block");
        let block = &page[start..];
        let end = block.find("</script>").expect("a close");
        assert!(
            !block[..end].contains('<'),
            "an unescaped `<` inside the embedded scene for {label:?}"
        );
    }
}

/// Accessibility basics §23 asks for: a language, a live region for the tooltip,
/// and a reduced-motion rule.
#[test]
fn the_page_carries_the_accessibility_basics() {
    let page = html("plain", &HtmlOptions::default());
    assert!(page.contains("<html lang=\"en\">"));
    assert!(page.contains("aria-live=\"polite\""));
    assert!(page.contains("prefers-reduced-motion"));
    assert!(page.contains("aria-controls=\"sigil-codex\""));
    // The structured summary, which is what a screen-reader user gets instead of
    // the picture.
    assert!(page.contains("This sigil contains"));
}

/// Determinism, like every other format.
#[test]
fn the_page_is_byte_identical_across_runs() {
    for label in HOSTILE {
        let options = revealed(MetadataMode::Full);
        assert_eq!(html(label, &options), html(label, &options), "{label:?}");
    }
}
