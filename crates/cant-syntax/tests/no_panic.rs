//! Malformed input must never panic, never hang, and always produce a
//! diagnostic that points somewhere real.
//!
//! Three layers, because each catches a different thing:
//!
//! 1. a hand-written corpus of mistakes people actually make;
//! 2. **every prefix** of every fixture and corpus entry, which is what an editor
//!    sees while someone is typing and is where unbalanced-delimiter bugs live;
//! 3. a `proptest` generator over the operator alphabet, for the shapes nobody
//!    thought to write down.
//!
//! The invariants asserted on every input are the same throughout: parsing
//! terminates, every diagnostic span is inside the source, and a program is
//! returned exactly when no error was reported... or not returned, which is also
//! fine — what is not fine is a panic.

use cant_syntax::{lex, parse_source};
use proptest::prelude::*;
use rite_core::{FileId, SourceFile};
use std::path::{Path, PathBuf};

const CORPUS: &[&str] = &[
    "",
    " ",
    "\n\n\n",
    "->",
    "-> -> ->",
    "?{",
    "|{",
    "~{",
    "}",
    "}}}}",
    "?{}",
    "|{}",
    "~{}",
    "|{;;;}",
    "?{ ?{ ?{",
    "a -> ?{ b -> |{ c",
    ":",
    "::::",
    ":max",
    ":max :by :max",
    "a :",
    "$",
    "!",
    "@",
    "*",
    "[]",
    "[",
    "]",
    "((((",
    "))))",
    "\"",
    "\"\\",
    "r\"",
    "/*",
    "//",
    "'",
    "\\",
    "\u{0}",
    "\u{7}",
    "\u{feff}a -> b",
    "😀 -> 🎉",
    "→",
    "⋇",
    "⌁",
    "⟧",
    "⊣⟦",
    "⫴⟦",
    "⟲⟦",
    "⊣⟦ ⫴⟦ ⟲⟦",
    "a → ⋇ → ⊣⟦",
    "1.",
    "1..",
    "0x",
    "1e",
    "1e+",
    "a -> ~{ f } :max 4 :by",
    "a -> b -> c -> ",
    "\r\n\r\n",
    "#!",
    "#!/usr/bin/env cant",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-syntax has two ancestors")
        .to_path_buf()
}

/// Everything asserted of every input, however malformed.
fn check_invariants(source: &str) {
    let file = SourceFile::new(FileId(0), "fuzz.cant", source);

    // Lexing is lossless even when it fails.
    let (tokens, lex_diags) = lex(&file);
    let joined: String = tokens.iter().map(|t| t.text.as_str()).collect();
    assert_eq!(
        joined, source,
        "lexer lost or invented bytes for {source:?}"
    );
    assert!(
        tokens.last().map(|t| t.kind) == Some(cant_syntax::CantTokenKind::Eof),
        "token stream must end with Eof for {source:?}"
    );
    for token in &tokens {
        assert!(
            token.span.end.as_usize() <= source.len(),
            "token span past the end of {source:?}"
        );
        assert!(
            token.span.start <= token.span.end,
            "inverted token span in {source:?}"
        );
    }

    let (result, _) = parse_source("fuzz.cant", source);
    for diagnostic in result.diagnostics.iter().chain(lex_diags.iter()) {
        for label in &diagnostic.labels {
            assert!(
                label.span.span.end.as_usize() <= source.len(),
                "diagnostic {} points past the end of {source:?}",
                diagnostic.code
            );
            assert!(
                label.span.span.start <= label.span.span.end,
                "diagnostic {} has an inverted span in {source:?}",
                diagnostic.code
            );
        }
    }
    // A source that produced no errors must have produced a program, and one
    // that produced a program must have produced at least one stage.
    if !result.has_errors() {
        let program = result
            .program
            .unwrap_or_else(|| panic!("{source:?} reported no errors but yielded no program"));
        assert!(
            !program.flow.stages.is_empty(),
            "{source:?} yielded an empty program with no error"
        );
    }
}

#[test]
fn the_corpus_of_mistakes_never_panics() {
    for source in CORPUS {
        check_invariants(source);
    }
}

/// Truncation is the interesting case: an editor parses a half-typed program on
/// every keystroke.
#[test]
fn every_prefix_of_every_corpus_entry_and_fixture_parses() {
    let mut sources: Vec<String> = CORPUS.iter().map(|s| s.to_string()).collect();
    for dir in ["conformance/cant/syntax", "conformance/cant/diagnostics"] {
        let root = repo_root().join(dir);
        for entry in std::fs::read_dir(&root).expect("fixture directory") {
            let path = entry.expect("directory entry").path().join("case.cant");
            if path.is_file() {
                sources.push(std::fs::read_to_string(&path).expect("fixture"));
            }
        }
    }
    for source in &sources {
        for end in 0..=source.len() {
            if !source.is_char_boundary(end) {
                continue;
            }
            check_invariants(&source[..end]);
        }
    }
}

proptest! {
    // The alphabet is the operator characters plus enough leaf material to build
    // something that looks like a program. Uniform random bytes would spend
    // almost all their time in the lexer's "unexpected character" arm.
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn random_operator_soup_never_panics(
        source in proptest::collection::vec(
            proptest::sample::select(vec![
                "->", "→", "*", "⋇", "[]", "⌁", "?{", "⊣⟦", "|{", "⫴⟦", "~{", "⟲⟦",
                "}", "⟧", ";", "$", "!", "@", ":", "(", ")", "[", "]", "{", ",", ".",
                "a", "1", "\"s\"", "//c\n", " ", "\n", "=", "%", "+",
            ]),
            0..40,
        ).prop_map(|parts| parts.concat())
    ) {
        check_invariants(&source);
    }

    /// Arbitrary text, for the cases the alphabet above cannot reach.
    #[test]
    fn arbitrary_text_never_panics(source in ".{0,200}") {
        check_invariants(&source);
    }
}
