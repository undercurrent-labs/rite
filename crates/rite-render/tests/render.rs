//! What `rite render` draws, and the one property everything else rests on.

use rite_render::{render, runs, Format, Frame, Kind, RenderOptions};

const SAMPLE: &str = r#"// A worked line of Rite, for the renderer to draw.
◆! main() ⟦
  raw ← ! @fs.read("orders.json")?
  orders ← @json.decode(raw)?
  total ← orders → map({ |o| o.qty * 2 }) → sum
  ! @console.println("total " + str(total))
  ^ ⟨status: #ok, total: total, rate: 0.15⟩
⟧
"#;

/// Every byte of the source appears in the output, once, in order.
///
/// This is the property the whole renderer rests on: colouring is allowed to
/// disagree with taste, but a picture that quietly drops a character — or
/// duplicates one — is worse than no picture. Spans make it easy to get wrong,
/// since the lexer's own `text` is the *lexeme*: a string's without its quotes,
/// an atom's without its `#`.
#[test]
fn the_runs_reconstruct_the_source_exactly() {
    for source in [
        SAMPLE,
        "",
        "\n\n\n",
        "   leading and trailing   \n",
        "x ← \"quotes \\\" inside\"\n",
        "def f(n) [[ return n ]]\n", // ASCII dialect
        "◆ f() ⟦ ^ #atom ⟧",         // no trailing newline
        "// comment only",
        "🎉 ← \"emoji binding\"\n",
    ] {
        let rebuilt: String = runs(source).iter().map(|r| r.text.as_str()).collect();
        assert_eq!(rebuilt, source, "runs did not reconstruct: {source:?}");
    }
}

/// Source that does not parse still renders. A highlighter that needs a valid
/// program cannot draw the broken example in a diagnostics page.
#[test]
fn source_that_does_not_parse_still_renders() {
    for broken in ["◆ ⟧⟧ ←", "\"unterminated", "@@@ ???", "⟦ ⟦ ⟦"] {
        let rebuilt: String = runs(broken).iter().map(|r| r.text.as_str()).collect();
        assert_eq!(rebuilt, broken);
        let svg = render(broken, &RenderOptions::default()).expect("render");
        assert!(svg.starts_with("<svg"), "no svg for {broken:?}");
    }
}

#[test]
fn the_pieces_are_classified_as_the_site_classifies_them() {
    let found = runs(SAMPLE);
    let kind_of = |needle: &str| {
        found
            .iter()
            .find(|r| r.text.contains(needle))
            .unwrap_or_else(|| panic!("no run containing {needle:?}"))
            .kind
    };

    assert_eq!(kind_of("// A worked line"), Kind::Comment);
    assert_eq!(kind_of("orders.json"), Kind::String);
    assert_eq!(kind_of("0.15"), Kind::Number);
    assert_eq!(kind_of("#ok"), Kind::Atom);
    // The capability keeps its `@`, and the function after the dot is its own
    // colour — that is what makes a host call recognisable at a glance.
    assert_eq!(kind_of("@fs"), Kind::Capability);
    assert_eq!(kind_of("read"), Kind::CapabilityFn);
    assert_eq!(kind_of("◆"), Kind::Glyph);
    assert_eq!(kind_of("*"), Kind::Operator);
}

/// An unknown capability is not coloured as one — `@nope` is a mistake, and
/// drawing it like `@fs` would hide that.
#[test]
fn an_unknown_capability_is_not_coloured_as_one() {
    let found = runs("! @nope.read(1)\n");
    assert!(
        !found.iter().any(|r| r.kind == Kind::Capability),
        "coloured an unknown capability: {found:?}"
    );
}

/// Both dialects are one language, so they colour alike.
#[test]
fn the_two_dialects_colour_the_same() {
    let glyph: Vec<Kind> = runs("◆ f(n) ⟦ ^ n ⟧\n")
        .iter()
        .filter(|r| !r.text.trim().is_empty())
        .map(|r| r.kind)
        .collect();
    let ascii: Vec<Kind> = runs("def f(n) [[ return n ]]\n")
        .iter()
        .filter(|r| !r.text.trim().is_empty())
        .map(|r| r.kind)
        .collect();
    assert_eq!(glyph, ascii, "the dialects were coloured differently");
}

/// The picture is stable. Regenerate with `RITE_UPDATE_GOLDEN=1` and *look at the
/// diff* — this file exists so a layout change is a decision someone made rather
/// than something that happened.
#[test]
fn the_svg_matches_the_golden_file() {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sample.svg");
    let svg = render(
        SAMPLE,
        &RenderOptions {
            format: Format::Svg,
            frame: Frame::Window,
            font_size: 15.0,
        },
    )
    .expect("render");

    if std::env::var("RITE_UPDATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &svg).unwrap();
        return;
    }

    let want = std::fs::read_to_string(&golden).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} — regenerate with RITE_UPDATE_GOLDEN=1",
            golden.display()
        )
    });
    assert_eq!(
        svg, want,
        "the rendered SVG changed. If that was deliberate: RITE_UPDATE_GOLDEN=1 \
         cargo test -p rite-render, and check the diff"
    );
}

/// Each frame draws its own chrome, and none of them changes the code.
#[test]
fn the_frames_differ_only_in_their_chrome() {
    let text = render(
        SAMPLE,
        &RenderOptions {
            frame: Frame::Text,
            ..Default::default()
        },
    )
    .unwrap();
    let boxed = render(
        SAMPLE,
        &RenderOptions {
            frame: Frame::Box,
            ..Default::default()
        },
    )
    .unwrap();
    let window = render(
        SAMPLE,
        &RenderOptions {
            frame: Frame::Window,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(!text.contains("<circle"), "plain text grew window dots");
    assert!(boxed.contains("stroke="), "the box frame has no border");
    assert!(window.contains("<circle"), "the window frame has no dots");

    // Same number of drawn runs in all three: the chrome is around the code, not
    // instead of some of it.
    let texts = |s: &str| s.matches("<text ").count();
    assert_eq!(texts(&text), texts(&boxed));
    assert_eq!(texts(&text), texts(&window));
}

/// Markup in a string must not become markup in the picture.
#[test]
fn source_that_looks_like_markup_is_escaped() {
    let svg = render(
        "x ← \"<script>alert('x') & </script>\"\n",
        &RenderOptions::default(),
    )
    .unwrap();
    assert!(
        !svg.contains("<script>"),
        "unescaped markup reached the SVG"
    );
    assert!(svg.contains("&lt;script&gt;"), "expected escaped markup");
    assert!(svg.contains("&amp;"), "expected an escaped ampersand");
}

/// No `<text>` element contains a space.
///
/// Whitespace is counted into the column of the next visible segment, never
/// drawn. Drawing it and trusting `xml:space="preserve"` looked correct in the
/// golden file and wrong in a browser — Chrome collapses runs of spaces inside
/// `<text>` whatever that attribute says, so `^ n * n` rendered as `^n  *n`, with
/// glyphs in the wrong columns. The golden file could not catch that, having been
/// generated from the same mistake; only looking at the picture did.
#[test]
fn no_drawn_text_carries_whitespace() {
    let svg = render(SAMPLE, &RenderOptions::default()).expect("render");
    for chunk in svg.split("<text ").skip(1) {
        let body = chunk
            .split_once('>')
            .and_then(|(_, rest)| rest.split_once("</text>"))
            .map(|(body, _)| body)
            .expect("a text element");
        assert!(
            !body.contains(' '),
            "a drawn run carries a space, so the browser may collapse it: {body:?}"
        );
    }
}

/// Indentation still lands where it should: the second line of the sample starts
/// two columns in, and the x of its first run says so.
#[test]
fn indentation_becomes_position_rather_than_spaces() {
    let svg = render("◆ f() ⟦\n  ^ 1\n⟧\n", &RenderOptions::default()).expect("render");
    let xs: Vec<f32> = svg
        .split("<text x=\"")
        .skip(1)
        .filter_map(|c| c.split('"').next()?.parse().ok())
        .collect();
    let first = xs.first().copied().expect("a first run");
    // Something on the indented line sits two columns right of the margin.
    let advance = 15.0 * 0.602;
    assert!(
        xs.iter().any(|x| (x - (first + advance * 2.0)).abs() < 0.5),
        "no run at the two-column indent: {xs:?}"
    );
}
