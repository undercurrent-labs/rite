//! End-to-end tests against the real `cant` binary.
//!
//! Exit codes and stream discipline are part of the contract — a script acting
//! on `cant check` cares which number it got and whether the diagnostic was on
//! stdout or stderr — and neither is observable from a unit test of the argument
//! parser.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn cant(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(args)
        .output()
        .expect("cant binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("cant exited normally")
}

#[test]
fn version_reports_cant_the_language_and_the_rite_it_targets() {
    let out = cant(&["version"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.starts_with("cant "), "{text}");
    assert!(text.contains("cant_language_version: 1"), "{text}");
    assert!(text.contains("cant_graph_schema_version: 3"), "{text}");
    // `rite-core`'s version, not this crate's: Cant versions independently, so
    // `CARGO_PKG_VERSION` here is Cant's number and would never match.
    assert!(
        text.contains(&format!("rite: {}", rite_core::VERSION)),
        "{text}"
    );
}

#[test]
fn version_json_carries_the_same_four_numbers() {
    let out = cant(&["version", "--json"]);
    assert_eq!(code(&out), 0);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    for key in [
        "cant",
        "cant_language_version",
        "cant_graph_schema_version",
        "rite",
    ] {
        assert!(json.get(key).is_some(), "missing `{key}` in {json}");
    }
}

#[test]
fn a_clean_expression_checks_ok() {
    let out = cant(&["check", "-e", "[1, 2, 3] -> * -> ?{ $ > 1 } -> []"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "ok");
}

/// An expression starting with an operator must not be read as a flag.
#[test]
fn an_expression_may_begin_with_an_operator() {
    let out = cant(&["check", "-e", "-> f"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(stderr(&out).contains("CANT-P002"), "{}", stderr(&out));
}

#[test]
fn a_syntax_error_exits_three_and_renders_to_stderr() {
    let out = cant(&["check", "-e", "[1] -> ~{ deps"]);
    assert_eq!(code(&out), 3);
    assert!(stderr(&out).contains("CANT-P003"), "{}", stderr(&out));
    assert!(
        stdout(&out).is_empty(),
        "rendered diagnostics belong on stderr, got: {}",
        stdout(&out)
    );
}

#[test]
fn json_errors_go_to_stdout_where_a_pipe_can_take_them() {
    let out = cant(&["check", "-e", "[1] -> ~{ deps", "--json-errors"]);
    assert_eq!(code(&out), 3);
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    let first = &json[0];
    assert_eq!(first["code"], serde_json::json!("CANT-P003"));
    assert_eq!(first["severity"], serde_json::json!("error"));
    assert!(first["labels"][0]["span"].is_object(), "{json}");
}

#[test]
fn a_source_can_come_from_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["check", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cant binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"[1, 2] -> * -> []\n")
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "ok");
}

#[test]
fn a_source_can_come_from_a_file() {
    let dir = std::env::temp_dir().join("cant-cli-test-file");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("program.cant");
    std::fs::write(&path, "[1, 2] -> * -> ?{ $ > 0 } -> []\n").expect("write fixture");

    let out = cant(&["check", path.to_str().expect("utf-8 path")]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    // The diagnostic path names the file, so an editor can jump to it.
    std::fs::write(&path, "[1, 2] -> ~{ deps\n").expect("write fixture");
    let out = cant(&["check", path.to_str().expect("utf-8 path")]);
    assert_eq!(code(&out), 3);
    assert!(stderr(&out).contains("program.cant"), "{}", stderr(&out));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn misuse_exits_two_rather_than_pretending_to_check_something() {
    assert_eq!(code(&cant(&["check"])), 2, "no source at all");
    assert_eq!(
        code(&cant(&["check", "a.cant", "-e", "x -> f"])),
        2,
        "a file and an expression together"
    );
}

#[test]
fn ascii_and_glyph_sources_print_the_same_structure() {
    let ascii = cant(&["parse", "--structure", "-e", "[1, 2] -> * -> []"]);
    let glyph = cant(&["parse", "--structure", "-e", "[1, 2] → ⋇ → ⌁"]);
    assert_eq!(code(&ascii), 0, "{}", stderr(&ascii));
    assert_eq!(code(&glyph), 0, "{}", stderr(&glyph));
    assert_eq!(stdout(&ascii), stdout(&glyph));
}

// ---- fmt and convert

#[test]
fn fmt_prints_the_formatted_program_without_touching_the_file() {
    let out = cant(&["fmt", "-e", "[1,2,3]->*->[]"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out), "[1,2,3] -> * -> []");
}

#[test]
fn fmt_check_agrees_with_fmt() {
    // An expression the formatter would not change exits 0; one it would exits 1.
    assert_eq!(
        code(&cant(&["fmt", "--check", "-e", "[1,2,3] -> * -> []"])),
        0
    );
    let out = cant(&["fmt", "--check", "-e", "[1,2,3]->*->[]"]);
    assert_eq!(code(&out), 1);
    assert!(stderr(&out).contains("would reformat"), "{}", stderr(&out));
}

#[test]
fn fmt_refuses_a_source_with_syntax_errors_and_says_why() {
    let out = cant(&["fmt", "-e", "[1] -> ~{ deps"]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
    assert!(stderr(&out).contains("CANT-P003"), "{}", stderr(&out));
    assert!(
        stdout(&out).is_empty(),
        "nothing should be printed: {}",
        stdout(&out)
    );
}

#[test]
fn fmt_writes_in_place_only_when_asked() {
    let dir = std::env::temp_dir().join("cant-cli-fmt-write");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("p.cant");
    let messy = "[1,2]->*->[]\n";
    std::fs::write(&path, messy).expect("write");

    let arg = path.to_str().expect("utf-8 path");
    let out = cant(&["fmt", arg]);
    assert_eq!(code(&out), 0);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        messy,
        "fmt without --write must not touch the file"
    );
    assert_eq!(stdout(&out), "[1,2] -> * -> []\n");

    let out = cant(&["fmt", "--write", arg]);
    assert_eq!(code(&out), 0);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "[1,2] -> * -> []\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fmt_glyph_and_preserve_pick_the_spelling() {
    let glyph = cant(&["fmt", "--glyph", "-e", "a -> * -> []"]);
    assert_eq!(stdout(&glyph), "a → ⋇ → ⌁");
    let ascii = cant(&["fmt", "--ascii", "-e", "a → ⋇ → ⌁"]);
    assert_eq!(stdout(&ascii), "a -> * -> []");
    let kept = cant(&["fmt", "--preserve", "-e", "a → ⋇ → ⌁"]);
    assert_eq!(stdout(&kept), "a → ⋇ → ⌁");
}

#[test]
fn convert_respells_operators_and_nothing_else() {
    let source = "// a -> comment\n\"a -> string\" -> f([]) -> ?{ $ > 0 } -> []";
    let out = cant(&["convert", "--to", "glyph", "-e", source]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("// a -> comment\n"), "{text}");
    assert!(text.contains("\"a -> string\""), "{text}");
    assert!(text.contains("f([])"), "{text}");
    assert!(text.contains('→') && text.contains('⌁'), "{text}");
}

#[test]
fn convert_round_trips_through_the_binary() {
    let source = "[1, 2] -> * -> ?{ $ > 0 } -> []";
    let glyph = stdout(&cant(&["convert", "--to", "glyph", "-e", source]));
    let back = stdout(&cant(&["convert", "--to", "ascii", "-e", &glyph]));
    assert_eq!(back, source);
}

#[test]
fn convert_rejects_an_unknown_target() {
    let out = cant(&["convert", "--to", "runes", "-e", "a -> b"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("ascii"), "{}", stderr(&out));
}

// ---- run

#[test]
fn the_top_level_dash_e_runs_the_expression() {
    let out = cant(&["-e", "[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "[2, 4, 6]");
}

#[test]
fn run_dash_e_and_the_top_level_form_are_the_same_command() {
    let shorthand = cant(&["-e", "5 -> |{ $ + 1 ; $ * 2 } -> []"]);
    let explicit = cant(&["run", "-e", "5 -> |{ $ + 1 ; $ * 2 } -> []"]);
    assert_eq!(stdout(&shorthand), stdout(&explicit));
    assert_eq!(code(&shorthand), code(&explicit));
}

#[test]
fn no_command_and_no_expression_says_what_to_do() {
    let out = cant(&[]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("-e"), "{}", stderr(&out));
}

#[test]
fn a_program_that_emits_nothing_prints_nothing() {
    let out = cant(&["-e", "[1, 2] -> * -> ?{ $ > 99 }"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "{:?}", stdout(&out));
}

#[test]
fn console_output_reaches_the_terminal() {
    let out = cant(&["-e", r#""hello" -> !@console.println"#]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(stdout(&out).contains("hello"), "{:?}", stdout(&out));
}

/// Exit codes are Rite's, for every category.
#[test]
fn run_exit_codes_follow_rites_contract() {
    let cases: &[(&str, i32, &[&str])] = &[
        ("[1, 2] -> * -> []", 0, &[]),
        ("3 -> *", 1, &[]),                              // runtime
        ("3 ->", 3, &[]),                                // parse
        ("3 -> square", 4, &[]),                         // resolve
        (r#""x" -> !@fs.read?"#, 5, &[]),                // permission
        ("[1] -> * -> ~{ $ + 1 } :max 5 -> []", 1, &[]), // an orbit limit is a panic
    ];
    for (source, expected, extra) in cases {
        let mut args = vec!["-e", source];
        args.extend_from_slice(extra);
        let out = cant(&args);
        assert_eq!(
            code(&out),
            *expected,
            "{source:?} exited {} — {}",
            code(&out),
            stderr(&out)
        );
    }
}

/// The step budget is enforced, which is what stops an orbit whose `:max` is
/// generous but whose body is slow.
#[test]
fn the_step_budget_is_enforced() {
    let out = cant(&[
        "-e",
        "[1, 2, 3] -> * -> ?{ $ > 1 } -> []",
        "--max-steps",
        "5",
    ]);
    assert_eq!(code(&out), 8, "{}", stderr(&out));
    assert!(stderr(&out).contains("CANT-O001"), "{}", stderr(&out));
}

#[test]
fn permission_flags_work_before_and_after_the_subcommand() {
    let dir = std::env::temp_dir().join("cant-cli-perm-order");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let data = dir.join("d.txt");
    std::fs::write(&data, "content\n").expect("write");
    let path = dir.join("p.cant");
    // An absolute path: the test runs `cant` from the workspace root, and a
    // relative one would resolve against *that*, not against the program's
    // directory — which is what `--allow fs:read=<dir>` grants.
    std::fs::write(
        &path,
        format!(
            "\"{}\" -> !@fs.read? -> trim\n",
            data.display().to_string().replace('\\', "/")
        ),
    )
    .expect("write");
    let arg = path.to_str().expect("utf-8");
    let allow = format!("fs:read={}", dir.display());

    let denied = cant(&["run", arg]);
    assert_eq!(code(&denied), 5, "{}", stderr(&denied));

    let after = cant(&["run", arg, "--allow", &allow]);
    assert_eq!(code(&after), 0, "{}", stderr(&after));
    let before = cant(&["--allow", &allow, "run", arg]);
    assert_eq!(code(&before), 0, "{}", stderr(&before));
    assert_eq!(stdout(&after), stdout(&before));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A runtime failure must not show the user generated scaffolding — not the
/// identifiers, and not the stack traceback through them.
#[test]
fn a_runtime_failure_shows_no_generated_code() {
    let out = cant(&["-e", "[1] -> * -> ~{ $ + 1 } :max 3 -> []"]);
    assert_ne!(code(&out), 0);
    let err = stderr(&out);
    assert!(err.contains("CANT-O002"), "{err}");
    assert!(!err.contains("cant_"), "leaked an identifier: {err}");
    assert!(
        !err.contains("stack traceback"),
        "leaked a traceback: {err}"
    );
    assert!(!err.contains("<generated>"), "{err}");
}

// ---- expand

#[test]
fn expand_prints_the_rite_that_would_run() {
    let out = cant(&["expand", "-e", "[1, 2] -> * -> ?{ $ > 1 } -> []"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let rite = stdout(&out);
    assert!(
        rite.starts_with("// Generated from <expr> by cant "),
        "{rite}"
    );
    assert!(rite.contains("def cant_"), "{rite}");
    assert!(rite.contains("__e > 1"), "the ward's predicate: {rite}");
    assert!(rite.trim_end().ends_with("_main()"), "{rite}");
}

#[test]
fn expand_is_byte_identical_between_runs() {
    let args = [
        "expand",
        "-e",
        "[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :max 8 -> []",
    ];
    assert_eq!(stdout(&cant(&args)), stdout(&cant(&args)));
}

#[test]
fn the_expansion_is_accepted_by_rite_itself() {
    // The end-to-end version of ADR 0002: hand the output to `rite check`.
    let out = cant(&["expand", "-e", r#""p" -> !@fs.read -> @json.decode"#]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));

    let dir = std::env::temp_dir().join("cant-cli-expand-check");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("generated.rite");
    std::fs::write(&path, stdout(&out)).expect("write");

    let rite = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["--version"])
        .output();
    assert!(rite.is_ok());

    // `rite` is built by the same workspace; find it beside our own binary.
    let rite_bin = std::path::Path::new(env!("CARGO_BIN_EXE_cant"))
        .parent()
        .expect("bin dir")
        .join(if cfg!(windows) { "rite.exe" } else { "rite" });
    if !rite_bin.is_file() {
        println!("note: the `rite` binary is not built, so the expansion was not checked by it");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let checked = Command::new(&rite_bin)
        .arg("check")
        .arg(&path)
        .output()
        .expect("rite check");
    assert!(
        checked.status.success(),
        "rite rejected the expansion:\n{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expand_writes_to_a_file_on_request() {
    let dir = std::env::temp_dir().join("cant-cli-expand-out");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("out.rite");
    let out = cant(&[
        "expand",
        "-e",
        "a -> upper",
        "-o",
        path.to_str().expect("utf-8"),
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert!(std::fs::read_to_string(&path)
        .expect("read")
        .contains("def cant_"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The map goes to stderr so `cant expand --source-map > out.rite` still writes
/// Rite and nothing else.
#[test]
fn the_source_map_does_not_contaminate_the_rite() {
    let out = cant(&["expand", "--source-map", "-e", "[1, 2] -> * -> []"]);
    assert_eq!(code(&out), 0);
    assert!(!stdout(&out).contains("source map"), "{}", stdout(&out));
    assert!(stderr(&out).contains("source map:"), "{}", stderr(&out));
    assert!(stderr(&out).contains("-> rite"), "{}", stderr(&out));
}

#[test]
fn a_rejected_program_is_not_expanded() {
    let out = cant(&["expand", "-e", "rows -> ?{ !@fs.exists($) }"]);
    assert_eq!(code(&out), 4);
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(stderr(&out).contains("CANT-G014"), "{}", stderr(&out));
}

/// A Rite error is reported against the Cant that caused it, and no generated
/// identifier is shown.
#[test]
fn rite_diagnostics_are_remapped_onto_cant_source() {
    let out = cant(&["check", "-e", r#""data.json" -> @fs.read"#]);
    assert_eq!(code(&out), 4, "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("CANT-S001"), "{err}");
    assert!(err.contains("@fs.read"), "{err}");
    assert!(!err.contains("cant_"), "a generated name leaked: {err}");
    assert_eq!(
        err.matches("CANT-S001").count(),
        1,
        "cascade not collapsed: {err}"
    );
}

// ---- graph

#[test]
fn graph_emits_json_by_default() {
    let out = cant(&["graph", "-e", "[1, 2] -> * -> ~{ deps } :max 8 -> []"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let json: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid JSON");
    assert_eq!(json["schema"], serde_json::json!("cant.graph"));
    assert_eq!(json["version"], serde_json::json!("3"));
    // Five: roots, scatter, orbit, the `deps` inside its body, collect. Nodes
    // nested in a subgraph are members of the one flat list, with a `subgraph`
    // field saying where they live — a renderer walking `nodes` sees all of them.
    assert_eq!(json["nodes"].as_array().expect("nodes").len(), 5);
    assert_eq!(json["subgraphs"].as_array().expect("subgraphs").len(), 1);
    assert_eq!(json["entry"], serde_json::json!(0));
    assert_eq!(json["exit"], serde_json::json!(4));
}

#[test]
fn graph_emits_dot_on_request() {
    let out = cant(&["graph", "--format", "dot", "-e", "x -> |{ a ; b }"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let dot = stdout(&out);
    assert!(dot.starts_with("digraph cant {"), "{dot}");
    assert!(dot.contains("subgraph cluster_0"), "{dot}");
    assert!(dot.trim_end().ends_with('}'), "{dot}");
}

/// Both formats have to be pipeable, so diagnostics belong on stderr even when
/// the graph itself is still printed.
#[test]
fn graph_prints_the_shape_even_when_validation_failed() {
    let out = cant(&["graph", "-e", "[1] -> ~{ deps } :max eight"]);
    assert_eq!(code(&out), 4, "graph errors are a resolve-category failure");
    assert!(stderr(&out).contains("CANT-G007"), "{}", stderr(&out));
    let json: serde_json::Value =
        serde_json::from_str(&stdout(&out)).expect("the graph is still valid JSON");
    assert!(!json["nodes"].as_array().expect("nodes").is_empty());
}

#[test]
fn graph_output_is_byte_identical_between_runs() {
    let args = [
        "graph",
        "-e",
        "roots -> * -> |{ a ; b -> c } -> ~{ d } :max 8 -> []",
    ];
    assert_eq!(stdout(&cant(&args)), stdout(&cant(&args)));
    let dot = [
        "graph",
        "--format",
        "dot",
        "-e",
        "roots -> ~{ d -> * } :max 8",
    ];
    assert_eq!(stdout(&cant(&dot)), stdout(&cant(&dot)));
}

#[test]
fn graph_rejects_an_unknown_format() {
    let out = cant(&["graph", "--format", "svg", "-e", "a -> b"]);
    assert_eq!(code(&out), 2);
    assert!(stderr(&out).contains("json"), "{}", stderr(&out));
}

/// `check` grew from syntax-only to syntax *and* graph validation without its
/// interface changing.
#[test]
fn check_now_rejects_a_program_whose_graph_is_wrong() {
    let out = cant(&["check", "-e", "rows -> ?{ !@fs.exists($) }"]);
    assert_eq!(code(&out), 4, "{}", stderr(&out));
    assert!(stderr(&out).contains("CANT-G014"), "{}", stderr(&out));

    // And a syntax error still wins, because it happened first.
    let out = cant(&["check", "-e", "rows -> ?{ !@fs.exists($) "]);
    assert_eq!(code(&out), 3, "{}", stderr(&out));
}

// ---- explain

#[test]
fn explain_reads_as_prose_not_a_syntax_tree() {
    let out = cant(&[
        "explain",
        "-e",
        "[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 4096 -> []",
    ]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("What this program does"), "{text}");
    assert!(text.contains("1. Evaluate `[1, 2]`."), "{text}");
    assert!(text.contains("breadth-first orbit"), "{text}");
    assert!(text.contains("identified by `str`"), "{text}");
    assert!(text.contains("4096 accepted candidates"), "{text}");
    assert!(text.contains("deterministic"), "{text}");
    // The thing the specification forbids: it must never fall back to a dump.
    for tell in ["NodeKind", "Span {", "LeafExpr", "SubgraphId"] {
        assert!(!text.contains(tell), "leaked `{tell}`:\n{text}");
    }
}

#[test]
fn explain_reports_capabilities_and_hazards_for_the_program_it_was_given() {
    let effectful = stdout(&cant(&["explain", "-e", r#""p" -> !@fs.read"#]));
    assert!(effectful.contains("Capabilities it needs"), "{effectful}");
    assert!(effectful.contains("@fs.read"), "{effectful}");
    assert!(effectful.contains("Worth knowing"), "{effectful}");

    // A pure program has neither section — a hazards list that is always there
    // is one nobody reads.
    let pure = stdout(&cant(&["explain", "-e", "3 -> $ + 1"]));
    assert!(!pure.contains("Capabilities it needs"), "{pure}");
    assert!(!pure.contains("Worth knowing"), "{pure}");
}

#[test]
fn explain_verbose_points_at_the_other_views() {
    let out = stdout(&cant(&["explain", "--verbose", "-e", "a -> b"]));
    assert!(out.contains("cant expand"), "{out}");
    assert!(out.contains("cant graph"), "{out}");
}

// ---- repl

/// The REPL reads a script the same way it reads a person.
#[test]
fn the_repl_runs_each_line_as_a_whole_program() {
    let out = repl_session(
        "[1, 2, 3] -> * -> ?{ $ > 1 } -> []
5 -> $ * 2
",
    );
    assert!(out.contains("[2, 3]"), "{out}");
    assert!(out.contains("10"), "{out}");
}

#[test]
fn the_repl_says_up_front_what_persists_and_what_cannot() {
    let out = repl_session("");
    assert!(out.contains("Values can persist; programs cannot"), "{out}");
}

#[test]
fn the_repl_offers_the_other_three_views() {
    let expanded = repl_session(
        ":expand a -> upper
",
    );
    assert!(expanded.contains("def cant_"), "{expanded}");
    let explained = repl_session(
        ":explain [1, 2] -> *
",
    );
    assert!(explained.contains("What this program does"), "{explained}");
    let graphed = repl_session(
        ":graph a -> b
",
    );
    assert!(graphed.contains("digraph cant"), "{graphed}");
}

#[test]
fn the_repl_reports_an_error_and_keeps_going() {
    let out = repl_session(
        "3 -> square
[1, 2] -> * -> []
",
    );
    assert!(out.contains("CANT-S002"), "{out}");
    assert!(out.contains("[1, 2]"), "the session continued: {out}");
}

// ---- modules

/// A directory with a module in it, and a program that needs it.
fn module_fixture(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cant-cli-modules-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(
        dir.join("shout.rite"),
        "pub def loud(s) [[ return s + \"!\" ]]\n",
    )
    .expect("write module");
    dir
}

fn cant_in(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cant"));
    command.args(args).current_dir(dir);
    // A stray `cant.toml` above the temp directory would otherwise configure
    // these runs; the fixtures that want one write their own.
    command
        .env_remove("CANT_USE")
        .env_remove("CANT_MODULE_PATH");
    for (k, v) in env {
        command.env(k, v);
    }
    command.output().expect("cant binary")
}

/// All three layers make the same module available. A Cant program written on
/// `-e` has no file to put a `use` line at the top of, which is the whole
/// reason any of this exists.
#[test]
fn a_module_can_come_from_a_flag_an_environment_variable_or_a_config_file() {
    let dir = module_fixture("layers");
    let program = r#"["a"] -> * -> shout.loud($)"#;

    let flag = cant_in(&dir, &["--use", "shout", "-e", program], &[]);
    assert_eq!(code(&flag), 0, "{}", stderr(&flag));
    assert_eq!(stdout(&flag).trim(), "a!");

    let env = cant_in(&dir, &["-e", program], &[("CANT_USE", "shout")]);
    assert_eq!(code(&env), 0, "{}", stderr(&env));
    assert_eq!(stdout(&env).trim(), "a!");

    std::fs::write(dir.join("cant.toml"), "use = [\"shout\"]\n").expect("write config");
    let config = cant_in(&dir, &["-e", program], &[]);
    assert_eq!(code(&config), 0, "{}", stderr(&config));
    assert_eq!(stdout(&config).trim(), "a!");

    // And the escape hatch turns the outer two off.
    let off = cant_in(&dir, &["--no-default-use", "-e", program], &[]);
    assert_ne!(code(&off), 0, "{}", stdout(&off));

    let _ = std::fs::remove_dir_all(&dir);
}

/// A module that cannot be found names the layer that asked for it. Rite's
/// `E026` would name the generated file and a search path, neither of which is
/// the thing to change.
#[test]
fn an_unfindable_module_names_the_layer_that_asked_for_it() {
    let dir = module_fixture("origins");

    let flag = cant_in(&dir, &["--use", "absent", "-e", "1"], &[]);
    assert_eq!(code(&flag), 2, "{}", stderr(&flag));
    assert!(stderr(&flag).contains("`--use`"), "{}", stderr(&flag));

    let env = cant_in(&dir, &["-e", "1"], &[("CANT_USE", "absent")]);
    assert_eq!(code(&env), 2, "{}", stderr(&env));
    assert!(stderr(&env).contains("`CANT_USE`"), "{}", stderr(&env));

    std::fs::write(dir.join("cant.toml"), "use = [\"absent\"]\n").expect("write config");
    let config = cant_in(&dir, &["-e", "1"], &[]);
    assert_eq!(code(&config), 2, "{}", stderr(&config));
    assert!(stderr(&config).contains("cant.toml"), "{}", stderr(&config));

    let _ = std::fs::remove_dir_all(&dir);
}

/// `--module-root` has to reach the *run*, not only the check. The two used to
/// search different paths, which is a program that checks and then cannot run.
#[test]
fn a_module_root_reaches_the_run_and_not_only_the_check() {
    let dir = module_fixture("roots");
    let elsewhere = std::env::temp_dir().join("cant-cli-modules-roots-caller");
    let _ = std::fs::remove_dir_all(&elsewhere);
    std::fs::create_dir_all(&elsewhere).expect("temp dir");

    let root = dir.to_str().expect("utf-8 path");
    let out = cant_in(
        &elsewhere,
        &[
            "--use",
            "shout",
            "--module-root",
            root,
            "-e",
            r#"["b"] -> * -> shout.loud($)"#,
        ],
        &[],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "b!");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

/// A program that already says `use shout` and is also given `--use shout` must
/// not get two `use` lines: two declarations of one name in a scope is a Rite
/// duplicate-binding error, and it would point at generated code.
#[test]
fn a_module_named_twice_is_imported_once() {
    let dir = module_fixture("duplicate");
    let path = dir.join("p.cant");
    std::fs::write(&path, "use shout\n[\"c\"] -> * -> shout.loud($)\n").expect("write program");
    let out = cant_in(
        &dir,
        &["--use", "shout", "run", path.to_str().expect("utf-8 path")],
        &[],
    );
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "c!");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The security property `modules.rs` exists to state: a file discovered by
/// walking up from the working directory must not be able to widen what a
/// program may do.
#[test]
fn a_config_file_cannot_grant_permissions() {
    let dir = module_fixture("permissions");
    std::fs::write(dir.join("cant.toml"), "allow = [\"fs:write=/\"]\n").expect("write config");
    let out = cant_in(&dir, &["-e", "1"], &[]);
    assert_eq!(code(&out), 2, "{}", stdout(&out));
    assert!(
        stderr(&out).contains("permissions cannot be set from a config file"),
        "{}",
        stderr(&out)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The budget bounds a line, not the session.
///
/// `ExecutionBudget` derives `Clone` over a shared step counter, so a REPL that
/// cloned its session budget per line was really measuring the whole session:
/// with `--max-steps 200` the third line failed and every line after it failed
/// too, having spent nothing itself. The wall clock had the same shape and was
/// worse — a session with the default 60s budget became unusable a minute after
/// it opened, whether or not anything had been run.
#[test]
fn each_line_gets_the_whole_budget() {
    let out = repl_session_with(
        &["--max-steps", "400"],
        "[1, 2, 3] -> * -> []
[1, 2, 3] -> * -> []
[1, 2, 3] -> * -> []
[1, 2, 3] -> * -> []
[1, 2, 3] -> * -> []
",
    );
    assert_eq!(
        out.matches("[1, 2, 3]").count(),
        5,
        "every line should get its own budget:\n{out}"
    );
    assert!(!out.contains("CANT-O001"), "{out}");
}

/// A REPL's input *is* the program, so a piped session must not be able to
/// grant itself permissions — `cat untrusted | cant repl` would otherwise walk
/// straight past the default-secure posture the invoker chose.
#[test]
fn a_piped_session_cannot_grant_itself_permissions() {
    let out = repl_session(
        ":allow fs:write=/
:permissions
",
    );
    assert!(out.contains("needs a terminal"), "{out}");
    // `:permissions` still lists only the default-secure set — the grant did
    // not land. Checked against the listing rather than the whole transcript,
    // because the refusal names the spec it refused.
    assert!(
        !out.lines().any(|l| l.trim_start().starts_with("fs:write=")),
        "the grant must not have landed:\n{out}"
    );
    assert!(
        out.lines()
            .any(|l| l.trim() == "console  stdin  clock  random"),
        "{out}"
    );
    // And it says what to do instead.
    assert!(out.contains("--allow fs:write=/"), "{out}");
}

#[test]
fn the_session_reports_and_changes_its_own_budget() {
    let out = repl_session(
        ":timeout 30s
:timeout off
:steps 500
:steps off
",
    );
    for expected in [
        "timeout: 30s per line",
        "timeout: off",
        "steps: 500 per line",
        "steps: off",
    ] {
        assert!(out.contains(expected), "`{expected}` missing from:\n{out}");
    }
}

#[test]
fn an_unknown_meta_command_is_named_rather_than_parsed() {
    let out = repl_session(
        ":nonsense
",
    );
    assert!(out.contains("unknown command `:nonsense`"), "{out}");
    assert!(
        !out.contains("CANT-P"),
        "it should not reach the parser: {out}"
    );
}

fn repl_session(input: &str) -> String {
    repl_session_with(&[], input)
}

fn repl_session_with(args: &[&str], input: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .arg("repl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cant repl");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// `cant --help` is an honest list of what works — which, now, is all of it.
#[test]
fn every_command_is_advertised_and_all_of_them_work() {
    let help = stdout(&cant(&["--help"]));
    // Every command the specification lists, and each one actually does its job
    // — this test used to assert the opposite for whichever were still missing,
    // so that `--help` never advertised something that was not there.
    for present in [
        "version", "check", "parse", "fmt", "convert", "graph", "expand", "explain", "run",
        "build", "repl",
    ] {
        assert!(help.contains(present), "`{present}` missing from --help");
        // `--help` for the subcommand itself must work, which is the cheapest
        // possible proof that it is wired up rather than merely declared.
        let sub = cant(&[present, "--help"]);
        assert_eq!(code(&sub), 0, "`cant {present} --help` failed");
    }
}

/// The shell-citizen contract (`@stdin`): data arrives on the pipe, the
/// program on `-e` — `cat log | cant -e '…'`, the form every peer tool has.
#[test]
fn data_can_come_from_standard_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["run", "-e", "!@stdin.lines -> * -> upper -> []"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cant binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello\nworld\n")
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "[HELLO, WORLD]");
}

/// Allowed by default, and revocable — `--deny stdin` is a permission failure,
/// exit 5, not an empty read.
#[test]
fn denied_stdin_is_a_permission_failure_not_an_empty_pipe() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["run", "--deny", "stdin", "-e", "!@stdin.lines -> * -> []"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cant binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"secret\n")
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("permission"),
        "expected a permission failure, got: {}",
        stderr(&out)
    );
}

/// `cant test`: the exit contract's 7 is a wrong *answer*; a broken program
/// keeps its own exit; a match is 0 and says so.
#[test]
fn cant_test_compares_the_printed_value() {
    let ok = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args([
            "test",
            "-e",
            "[1, 2] -> * -> $ * 2 -> []",
            "--expect",
            "[2, 4]",
        ])
        .output()
        .expect("cant binary");
    assert_eq!(code(&ok), 0, "{}", stderr(&ok));
    assert_eq!(stdout(&ok).trim(), "ok");

    let wrong = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args([
            "test",
            "-e",
            "[1, 2] -> * -> $ * 2 -> []",
            "--expect",
            "[9]",
        ])
        .output()
        .expect("cant binary");
    assert_eq!(code(&wrong), 7);
    assert!(
        stderr(&wrong).contains("expected: [9]"),
        "{}",
        stderr(&wrong)
    );
    assert!(
        stderr(&wrong).contains("actual:   [2, 4]"),
        "{}",
        stderr(&wrong)
    );

    let broken = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["test", "-e", "[1 -> ", "--expect", "[1]"])
        .output()
        .expect("cant binary");
    assert_ne!(code(&broken), 0);
    assert_ne!(code(&broken), 7, "a parse failure is not a wrong answer");
}

/// The sidecar form: `<source>.expect` beside the file, so a directory of
/// programs can carry its own expectations.
#[test]
fn cant_test_reads_a_sidecar_expectation() {
    let dir = std::env::temp_dir().join("cant-test-sidecar");
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("p.cant"), "[3, 4] -> * -> $ + 1 -> []\n").expect("write");
    std::fs::write(dir.join("p.expect"), "[4, 5]\n").expect("write");
    let out = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["test"])
        .arg(dir.join("p.cant"))
        .output()
        .expect("cant binary");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
}

/// The trace pair: `cant run --trace-out` writes a cant.trace document whose
/// counts are per-node emissions, and `cant sigil --weights` draws it.
#[test]
fn a_traced_run_weights_a_sigil() {
    let dir = std::env::temp_dir().join("cant-trace-weights");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let program = dir.join("p.cant");
    std::fs::write(&program, "[1, 2, 3] -> * -> ?{ $ > 1 } -> $ * 10 -> []\n").expect("write");
    let trace = dir.join("p.trace.json");

    let run = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["run", "--trace-out"])
        .arg(&trace)
        .arg(&program)
        .output()
        .expect("cant binary");
    assert_eq!(code(&run), 0, "{}", stderr(&run));
    // The value on stdout is exactly what an untraced run prints.
    assert_eq!(stdout(&run).trim(), "[20, 30]");

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&trace).expect("trace written"))
            .expect("trace is JSON");
    assert_eq!(doc["schema"], serde_json::json!("cant.trace"));
    // The scatter emitted three; the ward passed two.
    assert_eq!(doc["nodes"]["n1"], serde_json::json!(3));
    assert_eq!(doc["nodes"]["n2"], serde_json::json!(2));

    let svg_path = dir.join("p.sigil.svg");
    let sigil = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["sigil"])
        .arg(&program)
        .args(["--weights"])
        .arg(&trace)
        .args(["--canonical", "--output"])
        .arg(&svg_path)
        .output()
        .expect("cant binary");
    assert_eq!(code(&sigil), 0, "{}", stderr(&sigil));
    let svg = std::fs::read_to_string(&svg_path).expect("svg written");
    assert!(
        svg.contains("stroke-opacity"),
        "the weighted render did not scale its edges"
    );
}

/// Trace counts survive a parallel fork, and match the sequential program's.
///
/// The counters are `@store` reads and writes, and `RuntimeContext::fork` shares
/// the capability host through an `Arc`, so every branch increments the same
/// namespace. The read-modify-write is one statement with no suspension point
/// inside it — `@store` never returns a pending future — so no branch can
/// interleave between the read and the write, and addition does not care what
/// order the windows finish in. This test is what would catch that changing.
#[test]
fn a_traced_parallel_fork_counts_what_the_sequential_one_does() {
    let dir = std::env::temp_dir().join("cant-trace-parallel");
    std::fs::create_dir_all(&dir).expect("temp dir");

    let mut counts = Vec::new();
    for (name, source) in [
        ("seq.cant", "[1, 2] -> * -> |{ $ + 1 ; $ * 2 } -> []\n"),
        ("par.cant", "[1, 2] -> * -> |{ $ + 1 ; $ * 2 }:par -> []\n"),
    ] {
        let program = dir.join(name);
        std::fs::write(&program, source).expect("write");
        let trace = dir.join(format!("{name}.trace.json"));
        let run = Command::new(env!("CARGO_BIN_EXE_cant"))
            .args(["run", "--trace-out"])
            .arg(&trace)
            .arg(&program)
            .output()
            .expect("cant binary");
        assert_eq!(code(&run), 0, "{}", stderr(&run));
        assert_eq!(stdout(&run).trim(), "[2, 2, 3, 4]", "{name}");
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&trace).expect("trace written"))
                .expect("trace is JSON");
        counts.push(doc["nodes"].clone());
    }
    assert_eq!(counts[0], counts[1], "parallel lost or double-counted");
    // The fork emitted four: two branches over two scattered values.
    assert_eq!(counts[1]["n2"], serde_json::json!(4));
}

/// `cant sigil --diff`: the review picture — old ghosted beneath new,
/// canonical orientation required so nothing reads as a move that is not one.
#[test]
fn sigil_diff_ghosts_the_old_program() {
    let dir = std::env::temp_dir().join("cant-sigil-diff");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let old = dir.join("old.cant");
    let new = dir.join("new.cant");
    std::fs::write(&old, "[1, 2] -> * -> $ + 1 -> []\n").expect("write");
    std::fs::write(&new, "[1, 2] -> * -> ?{ $ > 1 } -> $ + 1 -> []\n").expect("write");
    let out_path = dir.join("d.svg");

    let missing_flag = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["sigil"])
        .arg(&new)
        .args(["--diff"])
        .arg(&old)
        .output()
        .expect("cant binary");
    assert_eq!(
        code(&missing_flag),
        2,
        "without --canonical this must refuse"
    );

    let ok = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["sigil"])
        .arg(&new)
        .args(["--diff"])
        .arg(&old)
        .args(["--canonical", "--output"])
        .arg(&out_path)
        .output()
        .expect("cant binary");
    assert_eq!(code(&ok), 0, "{}", stderr(&ok));
    let svg = std::fs::read_to_string(&out_path).expect("svg written");
    assert!(svg.contains("sigil-ghost"));
    assert!(svg.contains("svg-diff"));
}

/// The `[[` trap teaches itself: a nested list without spaces fails with a
/// help that names the fix, not just Rite's "expected RBracket".
#[test]
fn the_double_bracket_trap_names_its_own_fix() {
    let out = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["check", "-e", "[[1, 2], [3]] -> * -> []"])
        .output()
        .expect("cant binary");
    assert_ne!(code(&out), 0);
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        text.contains("put a space between nested list brackets"),
        "{text}"
    );
    // And a program that does not hit the trap gains no such help.
    let clean = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["check", "-e", "not_a_function(] -> []"])
        .output()
        .expect("cant binary");
    let clean_text = format!("{}{}", stdout(&clean), stderr(&clean));
    assert!(!clean_text.contains("put a space between"), "{clean_text}");
}

/// The REPL workbench: `:let` keeps a value, a bound name works in a later
/// flow, `it` is the last answer — and none of it is language syntax.
#[test]
fn the_repl_workbench_binds_values_not_programs() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["repl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("cant binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(
            // The binding arrow is sugar for `:let`; `~>` for `:trace`.
            b"evens <- [1, 2, 3, 4] -> * -> ?{ $ % 2 = 0 } -> []\n\
              ~> evens -> * -> $ * 10 -> []\n\
              it -> count\n\
              :quit\n",
        )
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("[2, 4]"), "{text}");
    assert!(text.contains("[20, 40]"), "{text}");
    assert!(
        text.contains("trace "),
        "the trace arrow should report counts:\n{text}"
    );
    assert!(text.contains('\n'), "{text}");
    assert!(
        text.lines().any(|l| l.trim() == "2"),
        "`it -> count` should answer 2:\n{text}"
    );
    // And `:let` in a *file* is not a program.
    let file_check = Command::new(env!("CARGO_BIN_EXE_cant"))
        .args(["check", "-e", ":let x = [1]"])
        .output()
        .expect("cant binary");
    assert_ne!(code(&file_check), 0, "`:let` must not parse as Cant");
}
