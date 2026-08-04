//! The highlighting palette is one table, and everything that draws Rite reads it.
//!
//! It lived only as raw hex in `apps/rite-studio/src/highlight.css`, which was
//! fine while a stylesheet was the only thing that drew Rite source. `rite render`
//! draws it too, and a second copy of eleven colours is the drift this project
//! keeps getting bitten by — change one, and generated images disagree with the
//! site with nothing to catch it. Same reasoning as `grammar/aliases.json`, and
//! the same shape of gate as `editor_grammar_sync.rs`.
//!
//! The stylesheet stays hand-written rather than generated: it carries things a
//! colour table has no business knowing about, like the reduced-motion rule that
//! turns the glyph glow off. What is checked is that the parts which *are* the
//! palette agree.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn palette() -> serde_json::Value {
    let path = repo_root().join("grammar/palette.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("grammar/palette.json is valid JSON")
}

fn stylesheet() -> String {
    let path = repo_root().join("apps/rite-studio/src/highlight.css");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn highlighter() -> String {
    let path = repo_root().join("apps/rite-studio/src/highlight.ts");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The `color:` declared for `.tok-<kind>`, lowercased.
fn css_color(css: &str, kind: &str) -> Option<String> {
    let selector = format!(".tok-{kind} {{");
    let start = css.find(&selector)? + selector.len();
    let body = &css[start..css[start..].find('}')? + start];
    let line = body
        .lines()
        .find(|l| l.trim_start().starts_with("color:"))?;
    Some(
        line.trim()
            .trim_start_matches("color:")
            .trim()
            .trim_end_matches(';')
            .to_lowercase(),
    )
}

fn kinds(p: &serde_json::Value) -> Vec<String> {
    p["kinds"]
        .as_object()
        .expect("kinds is an object")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_stylesheet_uses_the_palette_colours() {
    let p = palette();
    let css = stylesheet();
    for kind in kinds(&p) {
        let want = p["kinds"][&kind]["color"]
            .as_str()
            .expect("every kind has a colour")
            .to_lowercase();
        let got = css_color(&css, &kind)
            .unwrap_or_else(|| panic!("highlight.css has no `color` for .tok-{kind}"));
        assert_eq!(
            got, want,
            "`.tok-{kind}` in highlight.css disagrees with grammar/palette.json"
        );
    }
}

/// A colour in the stylesheet that the table does not know about is the drift
/// this gate exists to stop — the renderer would simply not have it.
#[test]
fn the_stylesheet_introduces_no_colours_of_its_own() {
    let p = palette();
    let css = stylesheet();
    let known: BTreeSet<String> = kinds(&p)
        .iter()
        .map(|k| p["kinds"][k]["color"].as_str().unwrap().to_lowercase())
        .chain(std::iter::once(
            p["background"].as_str().unwrap().to_lowercase(),
        ))
        .collect();

    let mut stray = Vec::new();
    let bytes: Vec<char> = css.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '#' {
            let hex: String = bytes[i + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            if hex.len() == 3 || hex.len() == 6 || hex.len() == 8 {
                let colour = format!("#{}", hex.to_lowercase());
                if !known.contains(&colour) {
                    stray.push(colour);
                }
            }
            i += 1 + hex.len();
        } else {
            i += 1;
        }
    }
    assert!(
        stray.is_empty(),
        "colours in highlight.css that are not in grammar/palette.json: {stray:?} \
         — add them to the table so `rite render` draws the same thing"
    );
}

/// Every kind the highlighter can emit needs a colour, or it renders as nothing
/// in an image while looking fine on the site.
#[test]
fn every_token_kind_has_a_colour() {
    let p = palette();
    let ts = highlighter();

    // The `TokenKind` union in highlight.ts, which is what the tokeniser can emit.
    let start = ts.find("export type TokenKind =").expect("TokenKind union");
    let end = start + ts[start..].find(';').expect("union ends");
    let declared: BTreeSet<String> = ts[start..end]
        .split('|')
        .filter_map(|part| {
            let part = part.trim();
            part.strip_prefix('"')?
                .strip_suffix('"')
                .map(str::to_string)
        })
        .collect();
    assert!(
        !declared.is_empty(),
        "parsed no token kinds from highlight.ts"
    );

    let table: BTreeSet<String> = kinds(&p).into_iter().collect();
    let missing: Vec<_> = declared.difference(&table).collect();
    assert!(
        missing.is_empty(),
        "token kinds highlight.ts emits with no entry in grammar/palette.json: {missing:?}"
    );
    let unused: Vec<_> = table.difference(&declared).collect();
    assert!(
        unused.is_empty(),
        "entries in grammar/palette.json for kinds nothing emits: {unused:?}"
    );
}

/// The stylesheet says every colour clears 4.5:1 against the panel background —
/// "a neon theme that cannot be read is just a screenshot". That was a comment,
/// which is a claim nothing checked. It is true (the worst is 6.32:1 on the
/// comment grey), and it stays true or this fails.
#[test]
fn every_colour_is_readable_on_the_background() {
    fn channel(c: f64) -> f64 {
        let c = c / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    fn luminance(hex: &str) -> f64 {
        let h = hex.trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).expect("hex pair") as f64;
        0.2126 * channel(v(0)) + 0.7152 * channel(v(2)) + 0.0722 * channel(v(4))
    }

    let p = palette();
    let bg = luminance(p["background"].as_str().expect("background"));
    let floor = p["min_contrast"].as_f64().expect("min_contrast");

    for kind in kinds(&p) {
        let colour = p["kinds"][&kind]["color"].as_str().unwrap();
        let fg = luminance(colour);
        let ratio = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
        assert!(
            ratio >= floor,
            "`{kind}` ({colour}) is {ratio:.2}:1 against {} — below the {floor}:1 the \
             palette promises",
            p["background"].as_str().unwrap()
        );
    }
}
