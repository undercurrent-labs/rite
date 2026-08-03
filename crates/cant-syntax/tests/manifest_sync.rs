//! `grammar/cant/operators.toml` and `CantTokenKind` must describe the same set
//! of operators, in both directions.
//!
//! The manifest is the source of truth, and it only *is* the source of truth if
//! nothing can drift from it: a token kind added to the lexer without a manifest
//! entry has an undocumented spelling, and a manifest entry naming a kind that
//! does not exist is a promise the parser never keeps.

use cant_syntax::{lex, manifest, CantTokenKind, Spelling};
use rite_core::{FileId, SourceFile};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-syntax has two ancestors")
        .to_path_buf()
}

#[test]
fn every_manifest_entry_names_a_real_token_kind() {
    for op in &manifest().operators {
        let known = CantTokenKind::MANIFEST_KINDS
            .iter()
            .any(|k| k.manifest_name() == Some(op.token.as_str()));
        assert!(
            known,
            "operators.toml names token `{}` (concept `{}`), which is not a CantTokenKind",
            op.token, op.concept
        );
    }
}

#[test]
fn every_structural_token_kind_has_exactly_one_manifest_entry() {
    for kind in CantTokenKind::MANIFEST_KINDS {
        let name = kind.manifest_name().expect("a manifest kind has a name");
        let matches = manifest()
            .operators
            .iter()
            .filter(|op| op.token == name)
            .count();
        assert_eq!(
            matches, 1,
            "token `{name}` appears {matches} times in operators.toml, expected exactly 1"
        );
    }
}

/// The embedded copy the binary carries and the file on disk are the same file.
#[test]
fn the_embedded_manifest_matches_the_one_on_disk() {
    let on_disk = std::fs::read_to_string(repo_root().join("grammar/cant/operators.toml"))
        .expect("grammar/cant/operators.toml");
    let parsed = cant_syntax::manifest::parse_manifest(&on_disk).expect("on-disk manifest parses");
    assert_eq!(
        &parsed,
        manifest(),
        "embedded manifest differs from the file"
    );
}

/// The point of the manifest: both spellings really do produce the kind it says.
#[test]
fn both_spellings_lex_to_the_kind_the_manifest_names() {
    for op in &manifest().operators {
        // `[]` and `}` need no context, but a bare `;`, `:` or `$` is still one
        // token on its own, so every operator can be lexed alone.
        let expected = CantTokenKind::MANIFEST_KINDS
            .iter()
            .copied()
            .find(|k| k.manifest_name() == Some(op.token.as_str()))
            .expect("checked by every_manifest_entry_names_a_real_token_kind");

        assert_eq!(
            first_kind(&op.ascii),
            Some((expected, Spelling::Ascii)),
            "ASCII `{}` for concept `{}`",
            op.ascii,
            op.concept
        );
        if let Some(glyph) = &op.glyph {
            // A glyph that is spelled the same as the ASCII (`$`, `!`, `@`, `;`,
            // `:`) is not a *glyph* form — the manifest records it as identical
            // and the lexer reports it as ASCII, which is correct.
            let expected_spelling = if glyph == &op.ascii {
                Spelling::Ascii
            } else {
                Spelling::Glyph
            };
            assert_eq!(
                first_kind(glyph),
                Some((expected, expected_spelling)),
                "glyph `{glyph}` for concept `{}`",
                op.concept
            );
        }
    }
}

fn first_kind(text: &str) -> Option<(CantTokenKind, Spelling)> {
    let file = SourceFile::new(FileId(0), "op.cant", text);
    let (tokens, _) = lex(&file);
    tokens
        .into_iter()
        .find(|t| !t.kind.is_trivia() && t.kind != CantTokenKind::Eof)
        .map(|t| (t.kind, t.spelling))
}

/// Cant must be typeable. A glyph that is *required* would defeat the language.
#[test]
fn no_operator_is_glyph_only() {
    for op in &manifest().operators {
        assert!(
            op.ascii.is_ascii() && !op.ascii.is_empty(),
            "concept `{}` has no ASCII spelling",
            op.concept
        );
    }
}

/// Cant's manifest and Rite's alias table are separate files that never
/// reference each other — the boundary ADR 0001 draws, checked rather than
/// asserted in prose.
#[test]
fn the_cant_manifest_does_not_leak_into_rites_alias_table() {
    let aliases = std::fs::read_to_string(repo_root().join("grammar/aliases.json"))
        .expect("grammar/aliases.json");
    for glyph in ["⋇", "⌁", "⊣⟦", "⫴⟦", "⟲⟦"] {
        assert!(
            !aliases.contains(glyph),
            "Cant glyph `{glyph}` appears in grammar/aliases.json; Cant is not a Rite dialect"
        );
    }
    let cant_manifest = std::fs::read_to_string(repo_root().join("grammar/cant/operators.toml"))
        .expect("grammar/cant/operators.toml");
    assert!(
        !cant_manifest.contains("aliases.json")
            || cant_manifest.contains("NOT `grammar/aliases.json`"),
        "the Cant manifest should only mention Rite's alias table to say it is not that file"
    );
}
