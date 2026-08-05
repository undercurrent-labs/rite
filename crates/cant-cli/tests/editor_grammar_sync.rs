//! The VS Code grammar describes the Cant the lexer implements.
//!
//! `grammar/cant/operators.toml` is the one table of spellings, and
//! `manifest_sync.rs` already holds it against `CantTokenKind`. This holds the
//! editor grammar against the same table, so a spelling added to the language
//! cannot render as plain text in an editor — which is how Rite's grammar fell
//! a whole sugar pack behind before `editor_grammar_sync.rs` was written for it.
//!
//! Lives in `cant-cli` rather than beside Rite's version because `rite-*` crates
//! may not name `cant-*` ones (ADR 0001), and this needs `cant_syntax::manifest`.

use cant_syntax::manifest;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn grammar() -> serde_json::Value {
    let path = repo_root().join("editors/vscode/syntaxes/cant.tmLanguage.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("cant.tmLanguage.json is valid JSON")
}

/// Every regex in the grammar, wherever it sits in the pattern tree.
fn regexes(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                match (key.as_str(), child.as_str()) {
                    ("match" | "begin" | "end", Some(r)) => out.push(r.to_string()),
                    _ => regexes(child, out),
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                regexes(item, out);
            }
        }
        _ => {}
    }
}

fn all_regexes() -> String {
    let mut out = Vec::new();
    regexes(&grammar(), &mut out);
    assert!(
        out.len() >= 10,
        "only {} regexes found — the walker is probably broken",
        out.len()
    );
    out.join("\n")
}

/// Every spelling in the manifest appears somewhere in the grammar.
///
/// Checked as a substring of the regex source rather than by matching, because a
/// TextMate rule escapes its metacharacters (`\\?\\{`) and the point is that the
/// spelling is *accounted for*, not how.
#[test]
fn every_operator_spelling_is_in_the_grammar() {
    let joined = all_regexes();
    // The regex source with backslashes removed, so `\\?\\{` contains `?{`.
    let unescaped: String = joined.replace('\\', "");

    let mut missing = Vec::new();
    for op in &manifest().operators {
        if !unescaped.contains(&op.ascii) {
            missing.push(format!("{} (ascii `{}`)", op.concept, op.ascii));
        }
        if let Some(glyph) = &op.glyph {
            if !joined.contains(glyph) {
                missing.push(format!("{} (glyph `{}`)", op.concept, glyph));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} operator spelling(s) the editor grammar does not colour:\n  {}\n\
         Add a rule to editors/vscode/syntaxes/cant.tmLanguage.json.",
        missing.len(),
        missing.join("\n  ")
    );
}

/// Comments and strings come first, so an arrow inside one is not an arrow.
///
/// This is the property the whole grammar rests on, and it is an ordering
/// property — the rules can all be right and the file still wrong.
#[test]
fn comments_and_strings_are_matched_before_operators() {
    let g = grammar();
    let order: Vec<&str> = g["patterns"]
        .as_array()
        .expect("top-level patterns")
        .iter()
        .filter_map(|p| p["include"].as_str())
        .collect();

    let pos = |name: &str| {
        order
            .iter()
            .position(|p| *p == name)
            .unwrap_or_else(|| panic!("no `{name}` in the top-level patterns: {order:?}"))
    };
    assert!(
        pos("#comments") < pos("#operators"),
        "comments must be matched before operators, or `// -> x` colours an arrow"
    );
    assert!(
        pos("#strings") < pos("#operators"),
        "strings must be matched before operators, or `\"a -> b\"` colours an arrow"
    );
    assert!(
        pos("#capabilities") < pos("#operators"),
        "`!@fs.read` must be matched before the bare `!`, or the marker splits off"
    );
}

/// The manifest is the only place spellings live, so the grammar must not
/// invent one. Checks the structural glyphs specifically: an editor showing a
/// glyph Cant does not lex is worse than showing none.
#[test]
fn the_grammar_invents_no_glyphs() {
    let joined = all_regexes();
    let known: Vec<&str> = manifest()
        .operators
        .iter()
        .filter_map(|op| op.glyph.as_deref())
        .collect();

    let mut stray = Vec::new();
    for ch in joined.chars() {
        // Anything outside ASCII in a Cant grammar rule is a glyph claim.
        if (ch as u32) > 127 && !known.iter().any(|g| g.contains(ch)) {
            let s = ch.to_string();
            if !stray.contains(&s) {
                stray.push(s);
            }
        }
    }
    assert!(
        stray.is_empty(),
        "the grammar colours glyph(s) the manifest does not define: {}",
        stray.join(" ")
    );
}

/// The extension declares Cant, and points at the grammar that is here.
///
/// The Rite-side gate in `palette_sync.rs` checks that every declared language
/// has a grammar file, without naming one — it may not mention Cant. This is
/// the other half, and it lives where naming Cant is allowed.
#[test]
fn the_extension_declares_the_cant_language() {
    let root = repo_root();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("editors/vscode/package.json")).expect("package.json"),
    )
    .expect("package.json is valid JSON");

    let languages = manifest["contributes"]["languages"]
        .as_array()
        .expect("contributes.languages");
    let cant = languages
        .iter()
        .find(|l| l["id"].as_str() == Some("cant"))
        .expect("the extension declares the `cant` language");
    let extensions: Vec<&str> = cant["extensions"]
        .as_array()
        .expect("cant declares file extensions")
        .iter()
        .filter_map(|e| e.as_str())
        .collect();
    assert!(extensions.contains(&".cant"), "got {extensions:?}");

    let grammar = manifest["contributes"]["grammars"]
        .as_array()
        .expect("contributes.grammars")
        .iter()
        .find(|g| g["language"].as_str() == Some("cant"))
        .expect("a grammar for `cant`");
    assert_eq!(grammar["scopeName"].as_str(), Some("source.cant"));
}
