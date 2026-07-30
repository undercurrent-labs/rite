//! The editor grammar and the grammar/ tables must describe the language the
//! lexer actually implements.
//!
//! Nothing enforced this before, and all three drifted: `rite.tmLanguage.json`
//! never learned about `@csv` / `@db`, nor about any of the sugar pack (`∧ ∨ ¬
//! ⊻ ¶ ∘ ∀ ¿ ⊏`, `compose` / `for` / `say` / `unless` / `xor`), so a whole layer
//! of syntax rendered as plain text in VS Code. `grammar/keywords.toml` listed
//! builtins as reserved words while omitting real ones.
//!
//! The sources of truth here are the live capability registry and the lexer's
//! own `keyword_or_ident`, not another hand-written list — a check against a
//! copy would only prove the copies agree.

use rite_caps::{HostCapabilities, PermissionSet};
use rite_syntax::{keyword_or_ident, TokenKind};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmlanguage() -> serde_json::Value {
    let path = repo_root().join("editors/vscode/syntaxes/rite.tmLanguage.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("tmLanguage is valid JSON")
}

/// Every regex in the grammar, wherever it appears in the pattern tree.
fn patterns(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                match (key.as_str(), child.as_str()) {
                    ("match" | "begin" | "end", Some(regex)) => out.push(regex.to_string()),
                    _ => patterns(child, out),
                }
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| patterns(i, out)),
        _ => {}
    }
}

fn all_patterns() -> Vec<String> {
    let mut out = Vec::new();
    patterns(&tmlanguage(), &mut out);
    assert!(!out.is_empty(), "no regexes found — grammar shape changed?");
    out
}

/// `(@|host\.)(a|b|c)\b` → {a, b, c}
fn alternatives_in(pattern: &str, after: &str) -> BTreeSet<String> {
    let rest = pattern
        .split_once(after)
        .unwrap_or_else(|| panic!("{after} not found in {pattern}"))
        .1;
    let group = rest
        .trim_start_matches('(')
        .split(')')
        .next()
        .expect("closing paren");
    group.split('|').map(|s| s.to_string()).collect()
}

/// `grammar/aliases.json` claims `compose` as the ASCII spelling of `∘`, but no
/// such keyword exists: `f compose g` lexes as three identifiers and evaluates
/// to `f` alone, so it answers with a wrong number rather than an error. The
/// working ASCII form is the builtin call `compose(f, g)`, which is what the
/// book documents and what `rite fmt --ascii` emits — and making `compose` a
/// keyword would break that call. `grammar/sigils.toml` never claimed the
/// alias; aliases.json is the outlier and the entry should go.
const BROKEN_ASCII_ALIASES: &[&str] = &["compose"];

fn ascii_aliases() -> BTreeSet<String> {
    let text = std::fs::read_to_string(repo_root().join("grammar/aliases.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["aliases"]
        .as_object()
        .expect("aliases object")
        .values()
        .filter_map(|entry| entry["ascii"].as_str())
        .filter(|ascii| ascii.chars().all(|c| c.is_ascii_lowercase()))
        .filter(|ascii| !BROKEN_ASCII_ALIASES.contains(ascii))
        .map(|s| s.to_string())
        .collect()
}

fn single_char_glyphs() -> BTreeSet<char> {
    let text = std::fs::read_to_string(repo_root().join("grammar/aliases.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["aliases"]
        .as_object()
        .expect("aliases object")
        .values()
        .filter_map(|entry| entry["glyph"].as_str())
        .filter_map(|glyph| {
            let mut chars = glyph.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            }
        })
        .collect()
}

/// A capability the registry serves but the grammar never colours is a silent
/// hole in every Rite file someone opens.
#[test]
fn tmlanguage_lists_every_host_capability() {
    let host = HostCapabilities::with_defaults(PermissionSet::default_secure());
    let live: BTreeSet<String> = host
        .all_descriptors()
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect();

    let pattern = all_patterns()
        .into_iter()
        .find(|p| p.contains(r"(@|host\.)"))
        .expect("capability pattern");
    let grammar = alternatives_in(&pattern, r"(@|host\.)");

    assert_eq!(
        grammar,
        live,
        "\ntmLanguage capabilities disagree with the registry.\n  \
         only in grammar: {:?}\n  only in registry: {:?}\n",
        grammar.difference(&live).collect::<Vec<_>>(),
        live.difference(&grammar).collect::<Vec<_>>()
    );
}

#[test]
fn tmlanguage_covers_every_glyph() {
    let blob = all_patterns().join("\n");
    let missing: Vec<char> = single_char_glyphs()
        .into_iter()
        .filter(|c| !blob.contains(*c))
        .collect();
    assert!(
        missing.is_empty(),
        "glyphs in grammar/aliases.json that the editor grammar never matches: {missing:?}"
    );
}

#[test]
fn tmlanguage_covers_every_ascii_keyword() {
    let pattern = all_patterns()
        .into_iter()
        .find(|p| p.contains("def|return"))
        .expect("keyword pattern");
    let grammar = alternatives_in(&pattern, r"\b");

    let missing: Vec<String> = ascii_aliases()
        .into_iter()
        .filter(|word| !grammar.contains(word))
        .collect();
    assert!(
        missing.is_empty(),
        "ASCII spellings in grammar/aliases.json missing from the editor's keyword pattern: {missing:?}"
    );
}

/// Both directions: the table must not invent keywords, and the ASCII half of
/// the alias table must actually lex as one.
#[test]
fn grammar_tables_match_the_lexer() {
    let keywords_toml = std::fs::read_to_string(repo_root().join("grammar/keywords.toml")).unwrap();
    let listed: Vec<String> = keywords_toml
        .split_once("keywords = [")
        .expect("keywords array")
        .1
        .split(']')
        .next()
        .unwrap()
        .split('"')
        .skip(1)
        .step_by(2)
        .map(|s| s.to_string())
        .collect();
    assert!(!listed.is_empty(), "no keywords parsed from keywords.toml");

    let not_really: Vec<&String> = listed
        .iter()
        .filter(|word| keyword_or_ident(word) == TokenKind::Ident)
        .collect();
    assert!(
        not_really.is_empty(),
        "grammar/keywords.toml lists words the lexer treats as plain identifiers: {not_really:?}"
    );

    let alias_not_keyword: Vec<String> = ascii_aliases()
        .into_iter()
        .filter(|word| keyword_or_ident(word) == TokenKind::Ident)
        .collect();
    assert!(
        alias_not_keyword.is_empty(),
        "ASCII spellings in grammar/aliases.json that do not lex as keywords: {alias_not_keyword:?}"
    );
}
