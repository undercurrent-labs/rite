//! Spans, source maps and diagnostic rendering.
//!
//! Every position the toolchain reports goes through this crate, and it had two tests.
//! The area is also quietly subtle: Rite source is glyph-heavy, so anywhere a byte offset
//! is confused with a character or UTF-16 column is wrong only on non-ASCII input — which
//! is most real Rite. That exact confusion produced a real bug in the LSP's inlay hints.

use rite_core::{
    simple_error, BytePos, Diagnostic, Diagnostics, FileId, Severity, SourceFile, SourceMap, Span,
    E020_UNDEFINED_NAME, E021_EFFECT_REQUIRED,
};

fn file(text: &str) -> SourceFile {
    SourceFile::new(FileId(0), "t.rite", text)
}

// ---- spans ---------------------------------------------------------------

#[test]
fn span_merge_covers_the_outer_bounds() {
    let merged = Span::from_range(2, 5).merge(Span::from_range(9, 12));
    assert_eq!(merged.start.as_usize(), 2);
    assert_eq!(merged.end.as_usize(), 12);
    assert_eq!(
        Span::from_range(9, 12).merge(Span::from_range(2, 5)),
        merged,
        "merge should not depend on argument order"
    );
}

#[test]
fn span_len_and_emptiness() {
    assert_eq!(Span::from_range(3, 7).len(), 4);
    assert!(Span::from_range(4, 4).is_empty());
    assert!(!Span::from_range(4, 5).is_empty());
    assert!(Span::DUMMY.is_dummy());
    assert!(!Span::from_range(0, 1).is_dummy());
}

// ---- line/column mapping ------------------------------------------------

#[test]
fn line_col_is_one_based_on_both_axes() {
    let f = file("ab\ncd\n");
    let at = |p| {
        let lc = f.line_col(BytePos::new(p));
        (lc.line, lc.column)
    };
    assert_eq!(at(0), (1, 1));
    assert_eq!(at(1), (1, 2));
    assert_eq!(at(3), (2, 1));
}

/// Columns count characters, not bytes — otherwise every caret in a glyph file is wrong.
#[test]
fn line_col_counts_characters_not_bytes() {
    let text = "◆ f() ⟦ ^ 1 ⟧\nx ← 1\n";
    let f = file(text);
    let byte = text.find('f').expect("f");
    let lc = f.line_col(BytePos::new(byte));
    assert_eq!(lc.line, 1);
    assert_eq!(
        lc.column, 3,
        "`f` is the 3rd character (◆, space, f) but reported column {} — byte offsets leaking",
        lc.column
    );
}

#[test]
fn line_text_and_line_span_agree_with_the_source() {
    let f = file("first\nsecond\nthird\n");
    assert_eq!(f.line_text(2), Some("second"));
    // `line_span` covers the line *including* its terminator; `line_text` strips it.
    assert_eq!(f.slice(f.line_span(2).expect("span")), "second\n");
    assert_eq!(f.line_text(99), None, "a missing line is None, not a panic");
    // Counts line *starts*, so a trailing newline opens an empty final line: this
    // three-line file reports 4. Surprising, but consistent with `line_span`, which can
    // address that empty line.
    assert_eq!(f.line_count(), 4);
    assert_eq!(
        f.line_text(4),
        Some(""),
        "the final empty line is addressable"
    );
}

#[test]
fn slice_returns_exactly_the_span() {
    let f = file("hello world");
    assert_eq!(f.slice(Span::from_range(6, 11)), "world");
    assert_eq!(f.slice(Span::from_range(0, 0)), "");
}

#[test]
fn positions_at_and_past_the_end_do_not_panic() {
    let f = file("ab\n");
    let _ = f.line_col(BytePos::new(f.len()));
    let _ = f.line_col(BytePos::new(f.len() + 50));
    let _ = f.slice(Span::from_range(0, f.len()));
}

#[test]
fn an_empty_file_is_handled() {
    let f = file("");
    assert!(f.is_empty());
    let lc = f.line_col(BytePos::new(0));
    assert_eq!((lc.line, lc.column), (1, 1));
}

// ---- source maps ---------------------------------------------------------

#[test]
fn a_source_map_keeps_files_addressable_by_id() {
    let mut map = SourceMap::new();
    let a = map.add_file("a.rite", "1\n");
    let b = map.add_file("b.rite", "2\n");
    assert_ne!(a, b, "each file gets its own id");
    assert_eq!(map.get(a).expect("a").name, "a.rite");
    assert_eq!(map.get(b).expect("b").as_str(), "2\n");
    assert!(map.get(FileId(999)).is_none(), "an unknown id is None");
    assert_eq!(map.files().len(), 2);
}

#[test]
fn add_path_reads_from_disk_and_reports_a_missing_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path().join("s.rite");
    std::fs::write(&p, "42\n").expect("write");
    let mut map = SourceMap::new();
    let id = map.add_path(&p).expect("add_path");
    assert_eq!(map.get(id).expect("file").as_str(), "42\n");
    assert!(map.add_path(dir.path().join("missing.rite")).is_err());
}

// ---- diagnostics ---------------------------------------------------------

#[test]
fn error_codes_render_in_the_stable_e_form() {
    // docs/diagnostics/ pages are named after this exact string.
    assert_eq!(E021_EFFECT_REQUIRED.as_str(), "E021");
    assert_eq!(E020_UNDEFINED_NAME.as_str(), "E020");
    assert_eq!(format!("{}", E021_EFFECT_REQUIRED), "E021");
}

#[test]
fn severity_decides_whether_a_collection_has_errors() {
    let mut d = Diagnostics::new();
    assert!(!d.has_errors() && d.is_empty());
    d.push(Diagnostic::warning(E020_UNDEFINED_NAME, "just a warning"));
    assert!(!d.has_errors(), "a warning alone is not an error");
    assert_eq!(d.len(), 1);
    d.push(Diagnostic::error(E020_UNDEFINED_NAME, "a real error"));
    assert!(d.has_errors());
    assert_eq!(d.errors().count(), 1, "errors() filters by severity");
}

/// Used by the parser's speculative pattern parse: rewinding the token position without
/// rewinding diagnostics left phantom errors from an attempt that was abandoned.
#[test]
fn rewind_discards_only_what_came_after() {
    let mut d = Diagnostics::new();
    d.push(Diagnostic::error(E020_UNDEFINED_NAME, "keep me"));
    let mark = d.len();
    d.push(Diagnostic::error(E021_EFFECT_REQUIRED, "speculative"));
    d.push(Diagnostic::error(E021_EFFECT_REQUIRED, "also speculative"));
    d.rewind(mark);
    assert_eq!(d.len(), 1);
    assert_eq!(d.iter().next().expect("kept").title, "keep me");
}

#[test]
fn rendering_shows_the_code_the_location_and_a_caret() {
    let mut map = SourceMap::new();
    let id = map.add_file("t.rite", "x ← 1\ny ← undefined_name\n");
    let text = map.get(id).expect("file").as_str().to_string();
    let at = text.find("undefined_name").expect("offset");
    let mut d = Diagnostics::new();
    d.push(simple_error(
        E020_UNDEFINED_NAME,
        "undefined name `undefined_name`",
        id,
        Span::from_range(at, at + "undefined_name".len()),
        "not found in scope",
    ));
    let out = d.render_all(&map);
    assert!(out.contains("E020"), "no code in output:\n{out}");
    assert!(out.contains("undefined_name"), "no message:\n{out}");
    assert!(out.contains("t.rite:2:"), "no file:line:col:\n{out}");
    assert!(out.contains('^'), "no caret:\n{out}");
}

/// The caret must sit *under* the span on a glyph line, not merely exist.
///
/// This is what a byte column got wrong. `◆ f() ⟦ ^ undefined_name ⟧` reported column 15
/// for a name starting at character 11, so every caret on idiomatic Rite pointed four
/// columns right of the problem — and the reported `line:col` was unusable for jumping.
#[test]
fn the_caret_lines_up_under_the_span_on_a_glyph_line() {
    let mut map = SourceMap::new();
    let text = "◆ f() ⟦ ^ undefined_name ⟧\n";
    let id = map.add_file("g.rite", text);
    let at = text.find("undefined_name").expect("offset");
    let mut d = Diagnostics::new();
    d.push(simple_error(
        E020_UNDEFINED_NAME,
        "undefined name",
        id,
        Span::from_range(at, at + "undefined_name".len()),
        "here",
    ));
    let out = d.render_all(&map);

    // Reported column is in characters: ◆,space,f,(,),space,⟦,space,^,space → 11th char.
    assert!(
        out.contains("g.rite:1:11"),
        "expected character column 11:\n{out}"
    );

    // The source line and the caret line must agree once the gutter is removed.
    // Select by position, not by content: Rite's return sigil is `^`, so searching for a
    // line containing '^' finds the *source* line and compares it against itself.
    let lines: Vec<&str> = out.lines().collect();
    let src_idx = lines
        .iter()
        .position(|l| l.contains("undefined_name") && l.contains('|'))
        .expect("source line");
    let src_line = lines[src_idx];
    let caret_line = lines[src_idx + 1];
    let after_gutter = |l: &str| l.split_once('|').expect("gutter").1.to_string();
    let src_after = after_gutter(src_line);
    let caret_after = after_gutter(caret_line);
    assert!(
        caret_after.trim_start().starts_with('^'),
        "line after the source should be the caret line, got: {caret_line}"
    );
    // Compare *display* columns. Byte offsets differ purely because the source line has
    // multi-byte glyphs and the caret line does not — measuring those would fail on
    // correct output, which is exactly the confusion this whole area is about.
    let caret_col = caret_after.find('^').expect("caret");
    let name_byte = src_after.find("undefined_name").expect("name");
    let name_col = unicode_width::UnicodeWidthStr::width(&src_after[..name_byte]);
    assert_eq!(
        caret_col, name_col,
        "caret at display column {caret_col}, name at {name_col}:\n{out}"
    );
    assert_eq!(
        caret_after.trim().len(),
        "undefined_name".len(),
        "caret should be exactly as long as the span:\n{out}"
    );
}

/// A double-width character occupies two terminal cells, which only display width knows.
#[test]
fn the_caret_accounts_for_double_width_characters() {
    let mut map = SourceMap::new();
    let text = "s ← \"日本語\" + missing\n";
    let id = map.add_file("w.rite", text);
    let at = text.find("missing").expect("offset");
    let mut d = Diagnostics::new();
    d.push(simple_error(
        E020_UNDEFINED_NAME,
        "undefined name",
        id,
        Span::from_range(at, at + "missing".len()),
        "here",
    ));
    let out = d.render_all(&map);
    let src_line = out
        .lines()
        .find(|l| l.contains("missing") && l.contains('|'))
        .expect("source line");
    let caret_line = out.lines().find(|l| l.contains('^')).expect("caret line");
    let after_gutter = |l: &str| l.split_once('|').expect("gutter").1.to_string();
    let prefix_to_name = {
        let s = after_gutter(src_line);
        let i = s.find("missing").expect("name");
        s[..i].to_string()
    };
    let want = unicode_width::UnicodeWidthStr::width(prefix_to_name.as_str());
    let got = after_gutter(caret_line).find('^').expect("caret");
    assert_eq!(
        got, want,
        "caret at {got}, expected display column {want}:\n{out}"
    );
}

#[test]
fn rendering_on_a_glyph_line_does_not_panic() {
    let mut map = SourceMap::new();
    let text = "◆ f() ⟦ ^ undefined_name ⟧\n";
    let id = map.add_file("g.rite", text);
    let at = text.find("undefined_name").expect("offset");
    let mut d = Diagnostics::new();
    d.push(simple_error(
        E020_UNDEFINED_NAME,
        "undefined name",
        id,
        Span::from_range(at, at + "undefined_name".len()),
        "here",
    ));
    let out = d.render_all(&map);
    assert!(out.contains("E020") && out.contains('^'), "{out}");
}

#[test]
fn help_and_notes_survive_into_json() {
    let d = Diagnostic::error(
        E021_EFFECT_REQUIRED,
        "effectful capability call requires `!`",
    )
    .with_help("mark the operation as an explicit effect: ! @fs.write")
    .with_note("reads count too");
    let json = d.to_json();
    let text = json.to_string();
    assert!(text.contains("21"), "code lost: {text}");
    assert!(text.contains("explicit effect"), "help lost: {text}");
    assert!(text.contains("reads count too"), "note lost: {text}");
}

/// `--json-errors` serialises a whole collection; an editor consumes this.
#[test]
fn a_collection_serialises_to_json() {
    let mut d = Diagnostics::new();
    d.push(Diagnostic::error(E020_UNDEFINED_NAME, "first"));
    d.push(Diagnostic::warning(E021_EFFECT_REQUIRED, "second"));
    let json = d.to_json();
    let text = json.to_string();
    assert!(text.contains("first") && text.contains("second"), "{text}");
}

#[test]
fn diagnostics_extend_and_convert_to_a_vec() {
    let mut a = Diagnostics::new();
    a.push(Diagnostic::error(E020_UNDEFINED_NAME, "one"));
    let mut b = Diagnostics::new();
    b.push(Diagnostic::warning(E021_EFFECT_REQUIRED, "two"));
    a.extend(b.into_vec());
    assert_eq!(a.len(), 2);
    let all = a.into_vec();
    assert_eq!(all[0].severity, Severity::Error);
    assert_eq!(all[1].severity, Severity::Warning);
}

// ------------------------------------------------------- the three column conventions

/// Bytes, characters and UTF-16 units are three different numbers, and they agree only
/// on ASCII — which is exactly why a suite written in ASCII cannot tell them apart.
#[test]
fn the_column_conventions_agree_on_ascii_and_diverge_elsewhere() {
    let f = SourceFile::new(FileId(0), "t.rite", "abc|def\n");
    let at = BytePos::new(4); // the `d`
    assert_eq!(f.line_col(at).column, 5, "1-based characters");
    assert_eq!(f.line_utf16_col(at), (0, 4), "0-based UTF-16 units");
}

#[test]
fn a_bmp_glyph_is_one_character_and_one_utf16_unit() {
    // `◆` is three bytes but a single unit in both other conventions: this is the case
    // idiomatic Rite hits on nearly every line.
    let text = "◆ f\n";
    let f = SourceFile::new(FileId(0), "t.rite", text);
    let at = BytePos::new(text.find('f').expect("f"));
    assert_eq!(
        at.as_usize(),
        4,
        "the byte offset really is past a 3-byte glyph"
    );
    assert_eq!(f.line_col(at).column, 3, "characters: glyph, space, then f");
    assert_eq!(
        f.line_utf16_col(at),
        (0, 2),
        "UTF-16 agrees with characters here"
    );
}

#[test]
fn an_astral_character_is_one_character_but_two_utf16_units() {
    // The one case where characters and UTF-16 part company, and the reason an editor
    // cannot be handed a character column.
    let text = "\u{1F600} f\n";
    let f = SourceFile::new(FileId(0), "t.rite", text);
    let at = BytePos::new(text.find('f').expect("f"));
    assert_eq!(at.as_usize(), 5, "four bytes of emoji plus a space");
    assert_eq!(f.line_col(at).column, 3, "one character for the emoji");
    assert_eq!(
        f.line_utf16_col(at),
        (0, 3),
        "two UTF-16 units for the emoji, so the editor column is one further right"
    );
}

#[test]
fn utf16_columns_are_relative_to_their_own_line() {
    let text = "◆ first\n\u{1F600} second\nplain\n";
    let f = SourceFile::new(FileId(0), "t.rite", text);
    let second = BytePos::new(text.find("second").expect("second"));
    assert_eq!(
        f.line_utf16_col(second),
        (1, 3),
        "line 1 (0-based), after an emoji and a space"
    );
    let plain = BytePos::new(text.find("plain").expect("plain"));
    assert_eq!(f.line_utf16_col(plain), (2, 0), "start of the third line");
}

#[test]
fn a_position_past_the_end_is_clamped_rather_than_panicking() {
    let f = SourceFile::new(FileId(0), "t.rite", "◆ f\n");
    let (line, col) = f.line_utf16_col(BytePos::new(9_999));
    assert!(line <= 1, "clamped to a real line, got {line}");
    let _ = col;
}
