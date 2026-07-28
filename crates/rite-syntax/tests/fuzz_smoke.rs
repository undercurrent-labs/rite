//! Fuzz smoke tests — no input may panic the host process.

use rite_core::{FileId, SourceFile};
use rite_syntax::lex;

#[test]
fn lexer_random_bytes_no_panic() {
    let samples: &[&[u8]] = &[
        b"",
        b"\xff\xfe",
        b"<<<>>>",
        b"def x <- ",
        "◆←→?~!@#⟦⟧⟨⟩∈∉".as_bytes(),
        b"\"unterminated",
        b"/* unclosed",
        b"0x",
        b"0b",
        b"host.",
        b"((((((",
    ];
    for s in samples {
        // Invalid UTF-8: only valid UTF-8 sources are accepted by SourceFile
        if let Ok(text) = std::str::from_utf8(s) {
            let f = SourceFile::new(FileId(0), "fuzz.rite", text);
            let _ = lex(&f);
        }
    }
}

#[test]
fn parser_garbage_no_panic() {
    let samples = [
        "",
        "????",
        "⟦⟦⟦",
        "def",
        "→→→",
        "match",
        "@",
        "1 2 3 4 5",
        "{ |x|",
        "⟨a:",
    ];
    for s in samples {
        let _ = rite_syntax::parse_source("fuzz.rite", s);
    }
}
