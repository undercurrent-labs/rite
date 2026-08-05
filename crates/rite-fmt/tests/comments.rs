//! Comment preservation: the formatter must never delete source.

use rite_fmt::{comment_texts, convert_source, format_with_dialect, Dialect};

const ALL_DIALECTS: [Dialect; 4] = [
    Dialect::Glyph,
    Dialect::Ascii,
    Dialect::Mixed,
    Dialect::Preserve,
];

/// The bug report from the field: every comment vanished.
const REPRO: &str = r#"//! Module doc comment
// leading comment about the constant
limit <- 10   // trailing comment

/// Doc comment for double
def double(n) [[
  // explain the math
  ^ n * 2
]]

! @console.println(str(double(limit)))
"#;

#[test]
fn repro_keeps_every_comment() {
    let out = format_with_dialect(REPRO, Dialect::Glyph).unwrap().text;
    for needle in [
        "//! Module doc comment",
        "// leading comment about the constant",
        "// trailing comment",
        "/// Doc comment for double",
        "// explain the math",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
    }
    // Trailing comment stays on its line, doc comment stays glued to the def.
    assert!(out.contains("limit ← 10 // trailing comment"), "{out}");
    assert!(
        out.contains("/// Doc comment for double\n◆ double(n)"),
        "{out}"
    );
    // Module doc is still line 1.
    assert!(out.starts_with("//! Module doc comment\n"), "{out}");
}

#[test]
fn comments_survive_every_dialect() {
    for d in ALL_DIALECTS {
        let out = format_with_dialect(REPRO, d).unwrap().text;
        assert_eq!(
            comment_texts(REPRO),
            comment_texts(&out),
            "dialect {d:?} changed comments:\n{out}"
        );
        let converted = convert_source(REPRO, d).unwrap().text;
        assert_eq!(
            comment_texts(REPRO),
            comment_texts(&converted),
            "convert to {d:?} changed comments:\n{converted}"
        );
    }
}

#[test]
fn shebang_stays_on_line_one() {
    let src = "#!/usr/bin/env rite\n// after shebang\nx <- 1\n";
    for d in ALL_DIALECTS {
        let out = format_with_dialect(src, d).unwrap().text;
        assert!(
            out.starts_with("#!/usr/bin/env rite\n"),
            "dialect {d:?}: {out}"
        );
        assert!(out.contains("// after shebang"), "dialect {d:?}: {out}");
    }
}

#[test]
fn block_and_nested_block_comments_survive() {
    let src = "/* outer /* nested */ still outer */\nx <- 1 /* trailing block */\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(
        out.contains("/* outer /* nested */ still outer */"),
        "{out}"
    );
    assert!(out.contains("/* trailing block */"), "{out}");
    assert_eq!(comment_texts(src), comment_texts(&out));
}

/// Non-ASCII comments and strings: the lexer accepts these now, so the
/// formatter must place them by character boundary, not byte guesswork.
#[test]
fn non_ascii_comments_and_strings_survive() {
    let src = "/* résumé */\ns <- \"Café ☕\" // αβγ\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("/* résumé */"), "{out}");
    assert!(out.contains("// αβγ"), "{out}");
    assert!(out.contains("\"Café ☕\""), "{out}");
    assert_eq!(comment_texts(src), comment_texts(&out));
    let twice = format_with_dialect(&out, Dialect::Glyph).unwrap().text;
    assert_eq!(out, twice);
}

#[test]
fn multiline_block_comment_survives() {
    let src = "/*\n  line one\n  line two\n*/\nx <- 1\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("line one"), "{out}");
    assert!(out.contains("line two"), "{out}");
    assert_eq!(comment_texts(src), comment_texts(&out));
}

#[test]
fn comment_only_file_is_kept() {
    let src = "// nothing but a comment\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert_eq!(out, "// nothing but a comment\n");
}

#[test]
fn dangling_comment_at_end_of_block() {
    let src = "def f() [[\n  ^ 1\n  // why we stop here\n]]\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("  // why we stop here\n⟧"), "{out}");
}

#[test]
fn comment_in_empty_block() {
    let src = "def f() [[\n  // todo\n]]\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("// todo"), "{out}");
    assert_eq!(comment_texts(src), comment_texts(&out));
}

#[test]
fn comments_inside_match_arms() {
    let src = concat!(
        "def f(x) [[\n",
        "  ^ match x [[\n",
        "    // zero case\n",
        "    0 -> \"zero\"\n",
        "    _ -> \"other\" // fallback\n",
        "  ]]\n",
        "]]\n"
    );
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("// zero case"), "{out}");
    assert!(out.contains("// fallback"), "{out}");
    assert_eq!(comment_texts(src), comment_texts(&out));
}

#[test]
fn blank_line_grouping_is_not_collapsed() {
    let src = "// group one\nx <- 1\n\n// group two\ny <- 2\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert_eq!(out, "// group one\nx ← 1\n\n// group two\ny ← 2\n", "{out}");
}

#[test]
fn no_runaway_blank_lines() {
    let src = "x <- 1\n\n\n\n\ny <- 2\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert_eq!(out, "x ← 1\n\ny ← 2\n", "{out}");
}

/// Comments are never lost, whatever else the formatter does with position.
#[test]
fn corpus_keeps_all_comments_in_all_dialects() {
    for src in corpus() {
        for d in ALL_DIALECTS {
            let r = match format_with_dialect(src, d) {
                Ok(r) => r,
                Err(e) => {
                    assert!(is_parse_error(&e), "dialect {d:?}: {e}\nfor:\n{src}");
                    continue;
                }
            };
            assert_eq!(
                comment_texts(src),
                comment_texts(&r.text),
                "dialect {d:?} changed comments of:\n{src}\n--- output ---\n{}",
                r.text
            );
            let twice = format_with_dialect(&r.text, d).unwrap().text;
            assert_eq!(r.text, twice, "dialect {d:?} not idempotent for:\n{src}");
        }
    }
}

/// A parse error means the formatter declines to touch the file at all; any
/// other error means the fail-safe fired, i.e. a formatter bug.
fn is_parse_error(err: &str) -> bool {
    err.contains("parse errors")
}

/// Every `.rite` file shipped in the repo is a real-world fixture.
#[test]
fn repo_sources_keep_all_comments() {
    let files = repo_rite_files();
    assert!(
        files.len() > 5,
        "expected example fixtures, found {files:?}"
    );
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap();
        for d in ALL_DIALECTS {
            match format_with_dialect(&src, d) {
                Ok(r) => assert_eq!(
                    comment_texts(&src),
                    comment_texts(&r.text),
                    "{}: dialect {d:?} changed comments",
                    path.display()
                ),
                Err(e) => assert!(is_parse_error(&e), "{}: dialect {d:?}: {e}", path.display()),
            }
        }
    }
}

// Property: whatever we generate, formatting keeps every comment and is a
// fixed point. Catches comment positions the table-driven corpus misses.
proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]
    #[test]
    fn generated_sources_keep_comments(parts in proptest::collection::vec(fragment(), 1..12)) {
        let src = format!("{}\n", parts.join("\n"));
        match format_with_dialect(&src, Dialect::Glyph) {
            Ok(once) => {
                proptest::prop_assert_eq!(
                    comment_texts(&src),
                    comment_texts(&once.text),
                    "comments changed for input:\n{}\n--- output ---\n{}",
                    src,
                    once.text
                );
                let twice = format_with_dialect(&once.text, Dialect::Glyph)
                    .expect("formatter output must re-format");
                proptest::prop_assert_eq!(&once.text, &twice.text, "not idempotent");
            }
            // Input the parser rejects is out of scope; a refusal is a real bug.
            Err(e) => proptest::prop_assert!(is_parse_error(&e), "{e}\nfor input:\n{src}"),
        }
    }
}

fn fragment() -> impl proptest::strategy::Strategy<Value = &'static str> {
    proptest::prop_oneof![
        proptest::prelude::Just("x <- 1"),
        proptest::prelude::Just("y <- x + 1 // trailing"),
        proptest::prelude::Just("// own line"),
        proptest::prelude::Just("  // indented own line"),
        proptest::prelude::Just("//! module doc"),
        proptest::prelude::Just("/// doc comment"),
        proptest::prelude::Just("/* block */"),
        proptest::prelude::Just("/* multi\n   line */"),
        proptest::prelude::Just("/* outer /* nested */ back */"),
        proptest::prelude::Just(""),
        proptest::prelude::Just("z <- [1, 2] // list"),
        proptest::prelude::Just("r <- <<a: 1>> // record"),
        proptest::prelude::Just("def f(n) [[\n  // inside\n  ^ n * 2 // ret\n]]"),
        proptest::prelude::Just("def g() [[\n  // only comment\n]]"),
        proptest::prelude::Just("! @console.println(\"hi\") // effect"),
        proptest::prelude::Just("if true [[\n  // then\n  x <- 1\n]]"),
    ]
}

fn corpus() -> Vec<&'static str> {
    vec![
        "// only a comment\n",
        "//! module doc\n",
        "/// orphan doc comment\n",
        "x <- 1 // trailing\n",
        "x <- 1 /* trailing block */\n",
        "/* leading block */ x <- 1\n",
        "// a\n// b\n// c\nx <- 1\n",
        "x <- 1\n// dangling at eof\n",
        "x <- 1\n\n\n// after blank lines\ny <- 2\n",
        "def f() [[ ]] // empty body\n",
        "def f() [[\n  // only a comment\n]]\n",
        "def f(n) [[\n  // step 1\n  a <- n + 1 // inline\n\n  // step 2\n  ^ a\n]]\n",
        "x <- [\n  1, // one\n  2, // two\n]\n",
        "x <- <<a: 1, b: 2>> // record\n",
        "use math // import comment\n",
        "def test \"named\" [[\n  // in a test\n  ^ true\n]]\n",
        "if true [[\n  // then\n  x <- 1\n]] : [[\n  // else\n  x <- 2\n]]\n",
        "#!/usr/bin/env rite\n// after shebang\nx <- 1\n",
        "/* multi\n   line\n   block */\nx <- 1\n",
        "x <- 1 // one\ny <- 2 // two\nz <- 3 // three\n",
        "◆ f(n) ⟦\n  // glyph source\n  ^ n\n⟧\n",
        "// comment with \"quotes\" and ⟦glyphs⟧ and //\nx <- 1\n",
        // Non-ASCII inside comments and strings (these used to panic the lexer).
        "/* résumé */\nx <- 1 // café\n",
        "s <- \"\"\"Café\n  résumé\n\"\"\" // multiline string\n",
        "/* 日本語 /* ネスト */ */\nx <- 1\n",
        "x <- \"// not a comment\" // but this is\n",
    ]
}

fn repo_rite_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut out = Vec::new();
    for dir in ["examples", "docs"] {
        collect_rite(&root.join(dir), &mut out);
    }
    out.sort();
    out
}

fn collect_rite(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rite(&path, out);
        } else if path.extension().is_some_and(|e| e == "rite") {
            out.push(path);
        }
    }
}

/// Formatting must not turn a valid file into one that no longer parses.
/// (Regression: relative imports were printed as `⊏ ..lib.helpers`.)
#[test]
fn relative_imports_round_trip() {
    // NB: `use ../shared/util` does not parse today (the lexer sees `..` as one
    // token), so it cannot be formatted either.
    let src = "use ./lib/helpers\nuse math as m\npub use math\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert_eq!(out, "⊏ ./lib/helpers\n⊏ math → m\npub ⊏ math\n", "{out}");
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert_eq!(ascii, src, "{ascii}");
}

/// Route handler params are part of the program: dropping `|req|` produced
/// `undefined name `req`` at request time.
#[test]
fn route_params_survive_formatting() {
    let src = concat!(
        "@http.listen \"127.0.0.1:0\" [[\n",
        "  GET \"/echo/:word\" |req| [[\n",
        "    return [200, <<echo: req.path.word>>]\n",
        "  ]]\n",
        "]]\n"
    );
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("GET \"/echo/:word\" |req| ⟦"), "{out}");
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(ascii.contains("GET \"/echo/:word\" |req| [["), "{ascii}");
}

/// The formatted output of every shipped example must be as resolvable as the
/// input — not merely parseable. Where the formatter cannot manage that, it must
/// refuse to write rather than hand back a broken program.
#[test]
fn repo_sources_gain_no_diagnostics() {
    for path in repo_rite_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        for d in ALL_DIALECTS {
            match format_with_dialect(&src, d) {
                Ok(r) => assert_eq!(
                    diagnostic_titles(&src),
                    diagnostic_titles(&r.text),
                    "{}: dialect {d:?} changed the program's diagnostics",
                    path.display()
                ),
                Err(e) => assert!(
                    is_parse_error(&e) || e.contains("refusing to format"),
                    "{}: dialect {d:?}: {e}",
                    path.display()
                ),
            }
        }
    }
}

/// Same guarantee, stated as a property over the generated corpus.
#[test]
fn corpus_gains_no_diagnostics() {
    for src in corpus() {
        let Ok(r) = format_with_dialect(src, Dialect::Glyph) else {
            continue;
        };
        assert_eq!(
            diagnostic_titles(src),
            diagnostic_titles(&r.text),
            "diagnostics changed for:\n{src}\n--- output ---\n{}",
            r.text
        );
    }
}

/// Span-free diagnostic fingerprints, mirroring the formatter's own fail-safe.
fn diagnostic_titles(source: &str) -> Vec<String> {
    let mut sources = rite_core::SourceMap::new();
    let id = sources.add_file("t.rite", source);
    let file = sources.get(id).cloned().unwrap();
    let (_, diags) = rite_sem::compile_to_ir(&file);
    let mut out: Vec<String> = diags
        .into_vec()
        .iter()
        .map(|d| format!("{:?}: {}", d.code, d.title))
        .collect();
    out.sort();
    out
}

// ---- sugar fidelity ----------------------------------------------------------
//
// The formatter prints from the AST, so anything the parser desugars is at risk of
// coming back in its lowered form. These pin the cases that mattered.

#[test]
fn middleware_use_is_not_printed_as_its_internal_call() {
    // `use @http.log` parses into a call to `__middleware_use`; printing that name
    // rewrote hand-written middleware into an internal symbol.
    let src =
        "@http.listen \"127.0.0.1:0\" ⟦\n  use @http.log\n  GET \"/\" ⟦ ^ 200 ⟨ok: true⟩ ⟧\n⟧\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(
        !out.contains("__middleware_use"),
        "internal symbol leaked into output:\n{out}"
    );
    assert!(out.contains("⊏ @http.log"), "{out}");

    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(ascii.contains("use host.http.log"), "{ascii}");
}

#[test]
fn juxtaposed_return_is_not_printed_as_a_list() {
    // `^ 200 ⟨…⟩` lowers to a list; printing `^ [200, ⟨…⟩]` reworded the central
    // HTTP handler idiom.
    let src = "◆ h() ⟦\n  ^ 200 ⟨ok: true⟩\n⟧\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("^ 200 ⟨ok: true⟩"), "{out}");
    assert!(!out.contains("[200,"), "printed the lowered list:\n{out}");
}

#[test]
fn an_explicit_list_return_is_still_a_list() {
    // The flag must not make every list-valued return juxtaposed.
    let src = "◆ h() ⟦\n  ^ [1, 2]\n⟧\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("^ [1, 2]"), "{out}");
}

#[test]
fn else_follows_the_dialect() {
    let src = "? true ⟦\n  1\n⟧ else ⟦\n  2\n⟧\n";
    let glyph = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(glyph.contains("⟧ : ⟦"), "glyph else:\n{glyph}");
    let ascii = format_with_dialect(src, Dialect::Ascii).unwrap().text;
    assert!(ascii.contains("]] else [["), "ascii else:\n{ascii}");
}

#[test]
fn multi_line_literals_keep_their_layout() {
    // Collapsing these produced single lines hundreds of characters wide out of
    // deliberately tabulated data.
    let src = "cfg ← ⟨\n  a: 1,\n  b: 2\n⟩\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("⟨\n"), "record was collapsed:\n{out}");
    assert!(out.contains("  a: 1,\n"), "{out}");
    // ...and a one-liner stays a one-liner.
    let one = format_with_dialect("cfg ← ⟨a: 1, b: 2⟩\n", Dialect::Glyph)
        .unwrap()
        .text;
    assert!(one.contains("⟨a: 1, b: 2⟩"), "{one}");
}

#[test]
fn multi_line_layout_is_idempotent() {
    let src = "rows ← [\n  1,\n  2\n]\ncfg ← ⟨\n  a: 1\n⟩\n";
    let once = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    let twice = format_with_dialect(&once, Dialect::Glyph).unwrap().text;
    assert_eq!(once, twice, "layout not stable across two passes");
}

#[test]
fn multi_line_pipelines_keep_one_stage_per_line() {
    let src = "xs ← [1, 2, 3]\nout ← xs\n  → keep(⟦ |n| n > 1 ⟧)\n  → sum\n";
    let out = format_with_dialect(src, Dialect::Glyph).unwrap().text;
    assert!(out.contains("xs\n  → keep"), "pipeline collapsed:\n{out}");
    assert!(out.contains("\n  → sum"), "{out}");
    // A single-line pipeline stays on one line.
    let one = format_with_dialect("out ← [1, 2] → sum\n", Dialect::Glyph)
        .unwrap()
        .text;
    assert!(one.contains("[1, 2] → sum"), "{one}");
}

#[test]
fn inline_blocks_stay_inline() {
    // `◆ sq(n) ⟦ ^ n * n ⟧` is how the README and book write short helpers; the
    // formatter used to expand every one of them to three lines.
    let src = "◆ sq(n) ⟦ ^ n * n ⟧\n";
    assert_eq!(format_with_dialect(src, Dialect::Glyph).unwrap().text, src);

    let cond = "score ← 1\nr ← ? score > 0 ⟦ #ok ⟧ : ⟦ #nope ⟧\n";
    assert_eq!(
        format_with_dialect(cond, Dialect::Glyph).unwrap().text,
        cond
    );

    // A block written across lines is still printed across lines.
    let multi = "◆ f(n) ⟦\n  ^ n\n⟧\n";
    assert_eq!(
        format_with_dialect(multi, Dialect::Glyph).unwrap().text,
        multi
    );
}
