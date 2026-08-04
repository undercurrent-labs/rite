//! What a standard SVG export may not contain, asserted over hostile input.
//!
//! §22.2 lists the prohibitions. Every one of them is a property of *output*
//! rather than of the code that produced it, so every one is checked by looking
//! at the bytes — the only place a mistake would actually show.
//!
//! The fixtures are deliberately nasty. Every string here is one that has broken
//! a real renderer somewhere: an attribute-closing quote, a comment-terminating
//! sequence, a CDATA escape, a null byte, a right-to-left override, a `javascript:`
//! URL. If any of them survives into markup, the artifact is one someone could be
//! harmed by opening.

use rite_sigil::{
    build_scene, normalize, render_svg, Background, DisclosureMode, EdgeId, EdgeKind,
    LayoutOptions, MetadataMode, NodeId, NormalizeOptions, PortRef, SigilEdge, SigilGraph,
    SigilNode, SigilNodeKind, SourceLanguage, SvgOptions, ThemeId,
};

/// Strings that have broken a renderer somewhere.
const HOSTILE: &[&str] = &[
    r#""><script>alert(1)</script>"#,
    r#"' onload='alert(1)"#,
    r#"</title><script>alert(1)</script><title>"#,
    r#"<![CDATA[]]><script>alert(1)</script>"#,
    r#"--><script>alert(1)</script><!--"#,
    r#"javascript:alert(1)"#,
    r#"</style><script>alert(1)</script><style>"#,
    "\u{202e}drowssap",
    "line\u{0}break",
    r#"&lt;&amp;&#x3c;script&#x3e;"#,
    r#"</svg><svg onload="alert(1)">"#,
    r#"url(#x) onmouseover=alert(1)"#,
];

/// A small graph carrying `text` everywhere text can go.
/// A small graph carrying `text` in every *label* position, with benign node
/// identifiers.
///
/// Identifiers and labels are different things and `safe` metadata treats them
/// differently on purpose — §13.5 keeps "semantic kinds and IDs" and drops
/// source snippets. Using one string for both would make "did the label leak?"
/// unanswerable. [`poisoned_identifiers`] covers the identifier side separately.
fn poisoned_graph(text: &str) -> SigilGraph {
    let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
    let mut source = SigilNode::new("n0", SigilNodeKind::Source);
    source.label = Some(text.to_string());
    source.short_label = Some(text.to_string());
    source.source = Some(rite_sigil::SourceRef {
        span: rite_core::Span::from_range(0, 4),
        snippet: Some(text.to_string()),
    });
    source
        .attributes
        .insert("note".into(), serde_json::json!(text));

    let mut effect = SigilNode::new("n1", SigilNodeKind::Effect);
    effect.label = Some(text.to_string());
    effect.effect = Some(rite_sigil::EffectMetadata {
        performs: true,
        capabilities: vec![rite_sigil::Capability {
            name: Some(text.to_string()),
            family: rite_sigil::CapabilityFamily::Other(text.to_string()),
        }],
    });

    let mut output = SigilNode::new("n2", SigilNodeKind::Output);
    output.label = Some(text.to_string());

    g.nodes.push(source);
    g.nodes.push(effect);
    g.nodes.push(output);
    g.exits.push("n2".into());
    g.metadata.source_name = Some(text.to_string());
    g.metadata.producer = Some(text.to_string());

    for (i, (from, to)) in [("n0", "n1"), ("n1", "n2")].into_iter().enumerate() {
        g.edges.push(SigilEdge {
            id: EdgeId::new(format!("e{i}")),
            from: PortRef::new(NodeId::new(from), 0),
            to: PortRef::new(NodeId::new(to), 0),
            ordinal: 0,
            kind: EdgeKind::Flow,
            region: None,
        });
    }
    g
}

/// The other half: hostile text in the *identifier* positions, which do reach
/// element IDs and therefore have to survive sanitization rather than escaping.
fn poisoned_identifiers(text: &str) -> SigilGraph {
    let mut g = SigilGraph::new(SourceLanguage::Cant, NodeId::new(text));
    g.nodes
        .push(SigilNode::new(NodeId::new(text), SigilNodeKind::Source));
    g.nodes.push(SigilNode::new("n1", SigilNodeKind::Output));
    g.exits.push("n1".into());
    g.edges.push(SigilEdge {
        id: EdgeId::new(text),
        from: PortRef::new(NodeId::new(text), 0),
        to: PortRef::new(NodeId::new("n1"), 0),
        ordinal: 0,
        kind: EdgeKind::Flow,
        region: None,
    });
    g
}

fn render_graph(graph: SigilGraph, options: &SvgOptions) -> String {
    let normalized = normalize(graph, &NormalizeOptions::default().with_snippets())
        .unwrap_or_else(|d| panic!("hostile graph did not normalize:\n{d}"));
    let scene = build_scene(&normalized, &LayoutOptions::canonical());
    render_svg(&scene, options).svg
}

fn render(text: &str, options: &SvgOptions) -> String {
    let graph = poisoned_graph(text);
    let normalized = normalize(graph, &NormalizeOptions::default().with_snippets())
        .unwrap_or_else(|d| panic!("hostile graph did not normalize:\n{d}"));
    let scene = build_scene(&normalized, &LayoutOptions::canonical());
    render_svg(&scene, options).svg
}

/// Every disclosure × metadata combination, because a leak in one of sixteen is
/// still a leak and the interesting ones are rarely the default.
fn every_option_set() -> Vec<SvgOptions> {
    let mut out = Vec::new();
    for disclosure in [
        DisclosureMode::Veiled,
        DisclosureMode::Inscribed,
        DisclosureMode::Revealed,
    ] {
        for metadata in [
            MetadataMode::Full,
            MetadataMode::Safe,
            MetadataMode::Minimal,
            MetadataMode::None,
        ] {
            for theme in ThemeId::ALL {
                out.push(SvgOptions {
                    theme: *theme,
                    disclosure,
                    metadata,
                    ..Default::default()
                });
            }
        }
    }
    out
}

#[test]
fn no_standard_svg_contains_a_script_or_an_event_handler() {
    for text in HOSTILE {
        for options in every_option_set() {
            let svg = render(text, &options);
            let lower = svg.to_lowercase();
            // `<script` over the whole document: escaping turns a hostile
            // `<script>` into `&lt;script&gt;`, so a literal one would be real.
            assert!(!lower.contains("<script"), "script in output for {text:?}");
            // A URL scheme is only dangerous in an attribute. In element text it
            // is a string someone asked to have drawn, and Revealed mode draws
            // exactly what it was given.
            assert!(
                !tags(&lower).iter().any(|t| t.contains("javascript:")),
                "javascript: URL in an attribute for {text:?}"
            );
            // Attribute syntax, not the substring. A sanitized element ID may
            // legitimately contain the letters `onload` — `id="s_-onload-1-"`
            // is a name, not a handler — and banning the substring would be a
            // check that fires on the sanitizer working correctly.
            assert!(
                !has_event_handler(&lower),
                "event handler attribute in output for {text:?}"
            );
            assert!(
                !lower.contains("<foreignobject"),
                "foreignObject for {text:?}"
            );
        }
    }
}

#[test]
fn no_standard_svg_references_anything_external() {
    for text in HOSTILE {
        for options in every_option_set() {
            let svg = render(text, &options);
            let lower = svg.to_lowercase();
            // The SVG namespace declaration is a URI and is required. It is
            // exempted by exact match rather than by allowing `http://`
            // anywhere, so a second remote URL still fails.
            let without_ns = lower.replace("xmlns=\"http://www.w3.org/2000/svg\"", "");
            for banned in [
                "http://",
                "https://",
                "<image",
                "xlink:href",
                "@import",
                "src=",
            ] {
                assert!(
                    !without_ns.contains(banned),
                    "external reference `{banned}` for {text:?}"
                );
            }
            // One `url(...)` is legitimate: the glow filter, which is local.
            for (i, _) in lower.match_indices("url(") {
                assert!(
                    lower[i..].starts_with("url(#"),
                    "non-local url() for {text:?}"
                );
            }
        }
    }
}

/// The document parses. An injection that produced ill-formed XML would be as
/// bad as one that produced a script — a viewer that repairs it may repair it
/// into something else.
#[test]
fn every_rendered_svg_is_well_formed_xml() {
    for text in HOSTILE {
        for options in every_option_set() {
            let svg = render(text, &options);
            check_well_formed(&svg)
                .unwrap_or_else(|e| panic!("malformed XML for {text:?}: {e}\n{svg}"));
        }
    }
}

/// The Veiled guarantee, over hostile input and every metadata mode: no source
/// text is *visible*. Visible means inside element content, not inside an
/// attribute the renderer controls.
#[test]
fn veiled_output_never_draws_a_label() {
    for text in HOSTILE {
        for metadata in [
            MetadataMode::Full,
            MetadataMode::Safe,
            MetadataMode::Minimal,
            MetadataMode::None,
        ] {
            let svg = render(
                text,
                &SvgOptions {
                    disclosure: DisclosureMode::Veiled,
                    metadata,
                    ..Default::default()
                },
            );
            assert!(
                !svg.contains("<text"),
                "a veiled render drew text ({metadata:?}) for {text:?}"
            );
        }
    }
}

/// `--metadata none` embeds nothing (§13.5).
///
/// **Embedding is not drawing.** The two axes are orthogonal on purpose, so
/// `--mode revealed --metadata none` means "draw the labels, embed nothing" and
/// the drawn labels are there because the disclosure mode asked for them. What
/// `none` guarantees is that nothing is carried *invisibly*: no title, no desc,
/// no graph-derived identifier, no snippet.
///
/// The combination is contradictory as an intent, and `cant sigil` warns about
/// it rather than silently resolving it one way — see
/// `metadata_none_with_a_revealing_mode_warns` in `cant-cli`.
#[test]
fn metadata_none_embeds_nothing_invisible() {
    const SECRET: &str = "ZZQQ_SECRET_LABEL_ZZQQ";
    for disclosure in [
        DisclosureMode::Veiled,
        DisclosureMode::Inscribed,
        DisclosureMode::Revealed,
    ] {
        let svg = render(
            SECRET,
            &SvgOptions {
                disclosure,
                metadata: MetadataMode::None,
                ..Default::default()
            },
        );
        if disclosure == DisclosureMode::Veiled {
            assert!(
                !svg.contains(SECRET),
                "metadata=none + veiled leaked the label"
            );
        }
        assert!(!svg.contains("<title"), "metadata=none emitted a title");
        assert!(!svg.contains("<desc"), "metadata=none emitted a desc");
        // Graph-derived identifiers specifically. The glow filter carries an
        // `id` because a filter has to be referenced by one; it is a renderer
        // constant and reveals nothing about the program.
        for prefix in ["id=\"node-", "id=\"edge-", "id=\"region-", "id=\"boundary-"] {
            assert!(
                !svg.contains(prefix),
                "metadata=none emitted a graph-derived identifier ({prefix})"
            );
        }
    }
}

/// `safe` — the default — keeps accessible titles but never a source snippet.
#[test]
fn safe_metadata_keeps_titles_and_drops_snippets() {
    const SECRET: &str = "ZZQQ_SECRET_LABEL_ZZQQ";
    let svg = render(SECRET, &SvgOptions::default());
    assert!(svg.contains("<title"), "safe metadata dropped every title");
    assert!(
        !svg.contains(SECRET),
        "safe metadata leaked a source label into the artifact"
    );
}

/// Determinism, over hostile input: the same scene and options are the same
/// bytes. A golden that was not byte-stable would be a golden of nothing.
#[test]
fn rendering_is_byte_identical_across_runs() {
    for text in HOSTILE {
        let options = SvgOptions::default();
        assert_eq!(render(text, &options), render(text, &options), "{text:?}");
    }
}

/// A hostile *identifier* reaches an element ID and must survive sanitization.
#[test]
fn hostile_identifiers_survive_sanitization_into_valid_ids() {
    for text in HOSTILE {
        let svg = render_graph(poisoned_identifiers(text), &SvgOptions::default());
        check_well_formed(&svg)
            .unwrap_or_else(|e| panic!("malformed XML from identifier {text:?}: {e}"));
        assert!(!has_event_handler(&svg.to_lowercase()), "{text:?}");
        assert!(!svg.to_lowercase().contains("<script"), "{text:?}");
    }
}

/// `on<letters>=` preceded by whitespace, **inside a tag**.
///
/// An event handler is an attribute, and an attribute only exists between `<`
/// and `>`. Scanning the whole document also matched element *text*: an
/// inscription drawn from the label `' onload='alert(1)` serializes as
/// `&apos; onload=&apos;alert(1)` inside a `<text>` element, which is inert —
/// the quotes are escaped, so nothing can close an attribute — but contains the
/// literal ` onload=`. A checker that cannot tell the two apart fails on the
/// escaper working correctly, which is the failure mode that gets a security
/// test deleted.
fn has_event_handler(lower: &str) -> bool {
    for tag in tags(lower) {
        let bytes = tag.as_bytes();
        for (i, _) in tag.match_indices("on") {
            if i == 0 || !bytes[i - 1].is_ascii_whitespace() {
                continue;
            }
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            if j > i + 2 && bytes.get(j) == Some(&b'=') {
                return true;
            }
        }
    }
    false
}

/// Every `<...>` region, exclusive of the angle brackets.
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

#[test]
fn the_event_handler_check_looks_only_inside_tags() {
    assert!(has_event_handler("<rect onload=\"x\"/>"));
    assert!(has_event_handler("<a b=\"1\" onmouseover=\"y\">"));
    // Escaped text content is not an attribute.
    assert!(!has_event_handler(
        "<text>&apos; onload=&apos;alert(1)</text>"
    ));
    assert!(!has_event_handler("<path id=\"s-onload-1\"/>"));
    assert!(!has_event_handler("plain onload= text"));
}

/// A validated background cannot escape its attribute, and an invalid one is
/// refused before it can try.
#[test]
fn a_hostile_background_is_refused_rather_than_escaped() {
    assert!(Background::hex("#fff\" onload=\"alert(1)").is_err());
    let svg = render(
        "plain",
        &SvgOptions {
            background: Background::hex("#102030").expect("valid"),
            ..Default::default()
        },
    );
    assert!(svg.contains("fill=\"#102030\""));
    assert!(!svg.to_lowercase().contains("onload"));
}

/// Transparent means no background rect at all, not a rect with zero alpha —
/// the second still paints over whatever it is composited onto.
#[test]
fn a_transparent_background_emits_no_rectangle() {
    let svg = render(
        "plain",
        &SvgOptions {
            background: Background::Transparent,
            ..Default::default()
        },
    );
    assert!(!svg.contains("<rect"), "transparent still painted a rect");
}

/// A minimal, dependency-free well-formedness check.
///
/// Not a full XML parser — a real one would be a dependency this crate does not
/// otherwise need, and what these tests have to catch is unbalanced or
/// unescaped markup, which tag matching finds. Attribute quoting is checked
/// alongside it, since an unterminated attribute is the other half of the same
/// attack.
fn check_well_formed(svg: &str) -> Result<(), String> {
    let bytes = svg.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            // Bare `&` outside a tag must begin an entity.
            if bytes[i] == b'&' {
                let rest = &svg[i..];
                let ok = ["&lt;", "&gt;", "&amp;", "&quot;", "&apos;"]
                    .iter()
                    .any(|e| rest.starts_with(e));
                if !ok {
                    return Err(format!("bare `&` at byte {i}"));
                }
            }
            i += 1;
            continue;
        }

        let close = svg[i..]
            .find('>')
            .map(|o| i + o)
            .ok_or_else(|| format!("unterminated tag at byte {i}"))?;
        let inner = &svg[i + 1..close];

        // Quotes inside a tag must balance, or an attribute has been closed
        // early and another opened.
        if !inner.matches('"').count().is_multiple_of(2) {
            return Err(format!("unbalanced quotes in `<{inner}>`"));
        }

        if let Some(name) = inner.strip_prefix('/') {
            let name = name.trim().to_string();
            match stack.pop() {
                Some(open) if open == name => {}
                Some(open) => return Err(format!("`</{name}>` closes `<{open}>`")),
                None => return Err(format!("`</{name}>` with nothing open")),
            }
        } else if !inner.ends_with('/') && !inner.starts_with('!') && !inner.starts_with('?') {
            let name = inner
                .split([' ', '\n', '\t'])
                .next()
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                return Err(format!("empty tag name at byte {i}"));
            }
            stack.push(name);
        }
        i = close + 1;
    }

    if let Some(open) = stack.last() {
        return Err(format!("`<{open}>` was never closed"));
    }
    Ok(())
}

#[test]
fn the_well_formedness_check_rejects_what_it_should() {
    assert!(check_well_formed("<a><b/></a>").is_ok());
    assert!(check_well_formed("<a>text &amp; more</a>").is_ok());
    assert!(check_well_formed("<a><b></a>").is_err());
    assert!(check_well_formed("<a>").is_err());
    assert!(check_well_formed("<a x=\"1></a>").is_err());
    assert!(check_well_formed("<a>bare & amp</a>").is_err());
    assert!(check_well_formed("</a>").is_err());
}
