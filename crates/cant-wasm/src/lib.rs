//! Browser-facing Cant API.
//!
//! Everything Studio needs, in one place, returning JSON: check, expand, graph,
//! explain, format and run.
//!
//! # Cant does not evaluate anything here either
//!
//! [`run`] expands to canonical Rite and hands the text to `rite_wasm`, which is
//! the same boundary the command line uses — ADR 0002, holding in the browser.
//! There is no second evaluator to keep in step, and a program that behaves
//! differently in Studio than in a terminal would have to be Rite behaving
//! differently, not Cant.
//!
//! # What the browser cannot do
//!
//! The generated Rite is real Rite, so a program that reads a file asks the host
//! for a file and the browser has none. That is reported rather than hidden: see
//! [`run`], which refuses a program whose capabilities cannot be served instead
//! of failing somewhere inside generated code.
//!
//! # Features
//!
//! `native` (default) builds the same functions against Rite's native runtime so
//! they can be tested with `cargo test`. `wasm` adds the bindings. The bodies are
//! shared, so a test here exercises what the browser calls.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(feature = "wasm")]
mod wasm_api;

/// The name a browser program is given.
///
/// Diagnostics point at it, and it appears in the generated Rite's header, so it
/// is stable rather than derived from anything about the page.
pub const SOURCE_NAME: &str = "studio.cant";

/// Diagnostics, plus whether they stop the program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResultDto {
    pub ok: bool,
    pub diagnostics: Value,
    /// The exit code the CLI would give this program. `0` when it is clean.
    pub exit_code: u8,
    /// Rendered exactly as a terminal would render it, carets and all.
    pub rendered: String,
}

/// Generated Rite, and the span map that ties it back to the Cant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpandResultDto {
    pub ok: bool,
    pub rite: Option<String>,
    /// The hygienic prefix every generated name carries.
    pub prefix: Option<String>,
    pub diagnostics: Value,
    pub rendered: String,
}

/// What running a program produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResultDto {
    pub ok: bool,
    pub value: Value,
    pub stdout: String,
    /// Present when the program was rejected, or when it failed while running.
    pub error: Option<String>,
    /// The generated Rite that ran. Present even on failure — it is the thing
    /// that failed, and hiding it would make a runtime error unreadable.
    pub rite: Option<String>,
    pub diagnostics: Value,
    pub rendered: String,
}

/// Parse, build the graph, expand, and check the result as Rite.
pub fn check(source: &str) -> CheckResultDto {
    let result = cant::check(SOURCE_NAME, source);
    CheckResultDto {
        ok: !result.has_errors(),
        diagnostics: result.diagnostics.to_json(),
        exit_code: result.exit_code(),
        rendered: result.render(),
    }
}

/// The canonical ASCII Rite a program becomes.
pub fn expand(source: &str) -> ExpandResultDto {
    // `cant::check` rather than `cant::expand`: expansion alone stops at Cant's
    // own diagnostics, and a leaf that is not valid Rite would come back as
    // successfully expanded text that Rite refuses. Studio shows what runs.
    let result = cant::check(SOURCE_NAME, source);
    let ok = !result.has_errors();
    ExpandResultDto {
        ok,
        rite: if ok {
            result.expansion.as_ref().map(|e| e.rite.clone())
        } else {
            None
        },
        prefix: result.expansion.as_ref().map(|e| e.prefix.clone()),
        diagnostics: result.diagnostics.to_json(),
        rendered: result.render(),
    }
}

/// The flow graph, in the schema `docs/cant/graph-schema.md` describes.
///
/// Present even for a program with errors: a diagnostic points *at* the graph,
/// and seeing the shape is usually how someone works out what went wrong.
pub fn graph(source: &str) -> Value {
    let analysis = cant::analyze(SOURCE_NAME, source);
    json!({
        "ok": !analysis.has_errors(),
        "graph": analysis.graph.as_ref().map(|g| g.to_json()),
        "diagnostics": analysis.diagnostics.to_json(),
        "rendered": analysis.render(),
    })
}

/// Graphviz DOT, for anyone who wants to render it elsewhere.
pub fn dot(source: &str) -> String {
    let analysis = cant::analyze(SOURCE_NAME, source);
    analysis
        .graph
        .as_ref()
        .map(cant_sem::to_dot)
        .unwrap_or_default()
}

/// The program in prose, with the capabilities it needs.
pub fn explain(source: &str) -> Value {
    let analysis = cant::analyze(SOURCE_NAME, source);
    let Some(graph) = analysis.graph.as_ref() else {
        return json!({ "ok": false, "text": "", "capabilities": [], "diagnostics": analysis.diagnostics.to_json() });
    };
    let explanation = cant_sem::explain(graph);
    json!({
        "ok": !analysis.has_errors(),
        "text": cant_sem::explain::render(&explanation, false),
        "capabilities": explanation.capabilities,
        "effects": explanation.effects,
        "max_orbit_items": explanation.max_orbit_items,
        "hazards": explanation.hazards,
        "diagnostics": analysis.diagnostics.to_json(),
    })
}

fn dialect_of(name: &str) -> cant_syntax::Dialect {
    match name.to_ascii_lowercase().as_str() {
        "glyph" => cant_syntax::Dialect::Glyph,
        _ => cant_syntax::Dialect::Ascii,
    }
}

/// Lay a program out, in the given spelling.
///
/// Refused — `ok: false` — for a source with syntax errors, because the AST is a
/// recovery and reprinting a guess as though it were the program is how a
/// formatter loses someone's code. Use [`convert`] to change spelling without
/// that risk.
pub fn format(source: &str, dialect: &str) -> Value {
    let options = cant_syntax::FormatOptions {
        dialect: dialect_of(dialect),
        ..Default::default()
    };
    match cant_syntax::format(source, options) {
        Ok(result) => json!({ "ok": true, "text": result.text }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

/// Swap the spelling of the structural operators, and nothing else.
///
/// Works on a source the parser could not finish, which is what makes it safe to
/// bind to a toggle: someone mid-edit still gets their glyphs.
pub fn convert(source: &str, dialect: &str) -> String {
    cant_syntax::convert(source, dialect_of(dialect))
}

/// Capabilities the browser cannot serve, and what to say about each.
///
/// Checked against the *Cant* source rather than the expansion, so the message
/// names something the user wrote. The list is short on purpose: `@console` and
/// the pure namespaces work, and anything not named here is left to Rite's own
/// browser runtime to accept or refuse.
const NO_BROWSER_ANSWER: &[(&str, &str)] = &[
    ("@fs", "there is no file system here"),
    ("@process", "there is no subprocess here"),
    ("@db", "there is no database here"),
    ("@net", "the browser's network is not the host's"),
    ("@tcp", "the browser has no socket layer"),
    ("@udp", "the browser has no socket layer"),
];

/// Run a program, by expanding it and handing the Rite to Rite.
pub fn run(source: &str) -> RunResultDto {
    let checked = cant::check(SOURCE_NAME, source);
    let rendered = checked.render();
    let diagnostics = checked.diagnostics.to_json();

    if checked.has_errors() {
        return RunResultDto {
            ok: false,
            value: Value::Null,
            stdout: String::new(),
            error: Some("the program was rejected before it ran".into()),
            rite: None,
            diagnostics,
            rendered,
        };
    }

    let Some(expansion) = checked.expansion else {
        return RunResultDto {
            ok: false,
            value: Value::Null,
            stdout: String::new(),
            error: Some("there was nothing to run".into()),
            rite: None,
            diagnostics,
            rendered,
        };
    };

    // Refused up front, naming the capability. The alternative is a failure
    // inside generated code, which points at a line the user never wrote.
    if let Some((cap, why)) = NO_BROWSER_ANSWER
        .iter()
        .find(|(cap, _)| source.contains(*cap))
    {
        return RunResultDto {
            ok: false,
            value: Value::Null,
            stdout: String::new(),
            error: Some(format!(
                "{cap} needs a host and this is a browser — {why}. \
                 The expansion is below; run it with `cant run` to use this capability."
            )),
            rite: Some(expansion.rite.clone()),
            diagnostics,
            rendered,
        };
    }

    let outcome = rite_wasm::run_blocking(
        &expansion.rite,
        rite_wasm::RunOptions {
            allow_all: true,
            browser_safe: true,
            ..Default::default()
        },
    );

    RunResultDto {
        ok: outcome.ok,
        value: outcome.value,
        stdout: outcome.stdout,
        // Rite's runtime errors name generated identifiers. Studio shows the
        // expansion beside them, which is what makes such a message readable —
        // and is the same trade `cant run` makes when it prints one.
        error: outcome.error,
        rite: Some(expansion.rite),
        diagnostics,
        rendered,
    }
}

/// The versions a browser build speaks, matching `cant version`.
pub fn version() -> Value {
    let info = cant::version_info();
    json!({
        "cant": info.tool,
        "cant_language_version": info.language,
        "cant_graph_schema_version": info.graph_schema,
        "rite": info.rite,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_program_checks() {
        let result = check("[1, 2, 3] -> * -> ?{ $ > 1 } -> []");
        assert!(result.ok, "{}", result.rendered);
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn a_broken_program_reports_the_cant_code_and_the_cli_exit_code() {
        let result = check("[1, 2] -> ~{ deps");
        assert!(!result.ok);
        assert_eq!(result.exit_code, 3, "a parse failure");
        assert!(result.rendered.contains("CANT-P003"), "{}", result.rendered);
    }

    /// A leaf that parses as Cant but is not valid Rite is caught here, not at
    /// run time — the whole reason `expand` goes through `check`.
    #[test]
    fn expansion_is_withheld_when_rite_refuses_the_program() {
        let result = expand("[[1, 2], [3]] -> * -> sum -> []");
        assert!(!result.ok);
        assert!(result.rite.is_none());
        assert!(result.rendered.contains("CANT-S004"), "{}", result.rendered);
    }

    #[test]
    fn expansion_is_the_rite_that_runs() {
        let result = expand("[1, 2] -> * -> $ + 1 -> []");
        let rite = result.rite.expect("expands");
        assert!(rite.contains("Generated from studio.cant"), "{rite}");
        assert!(rite.contains(&result.prefix.expect("a prefix")));
    }

    #[test]
    fn running_returns_the_value_and_the_rite_that_produced_it() {
        let result = run("[1, 2, 3, 4, 5, 6] -> * -> ?{ $ % 2 = 0 } -> []");
        assert!(result.ok, "{:?} {}", result.error, result.rendered);
        assert_eq!(result.value, json!([2, 4, 6]));
        assert!(result.rite.is_some(), "the expansion comes back too");
    }

    #[test]
    fn console_output_comes_back_as_stdout() {
        let result = run(r#""hi" -> !@console.println"#);
        assert!(result.ok, "{:?} {}", result.error, result.rendered);
        assert!(result.stdout.contains("hi"), "{:?}", result.stdout);
    }

    /// The refusal names the capability and still shows the expansion, so the
    /// user can see what they would run elsewhere.
    #[test]
    fn a_capability_the_browser_cannot_serve_is_refused_by_name() {
        let result = run(r#""data.txt" -> !@fs.read"#);
        assert!(!result.ok);
        let error = result.error.expect("an error");
        assert!(error.contains("@fs"), "{error}");
        assert!(error.contains("cant run"), "{error}");
        assert!(result.rite.is_some(), "the expansion is still shown");
    }

    #[test]
    fn a_rejected_program_does_not_run_and_says_so() {
        let result = run("rows -> ?{ !@fs.exists($) }");
        assert!(!result.ok);
        assert!(result.rite.is_none());
        assert!(!result.rendered.is_empty(), "the diagnostics come back");
    }

    #[test]
    fn the_graph_is_the_documented_schema() {
        let value = graph("[1, 2] -> * -> []");
        let graph = &value["graph"];
        assert_eq!(graph["version"], json!(cant_sem::GRAPH_SCHEMA_VERSION));
        assert!(graph["nodes"].as_array().expect("nodes").len() >= 3);
        assert!(graph["edges"].is_array());
    }

    /// A broken program still has a shape, and that is what someone looks at to
    /// understand the error.
    #[test]
    fn the_graph_survives_a_program_with_errors() {
        let value = graph("[1, 2] -> * -> ?{ }");
        assert_eq!(value["ok"], json!(false));
        assert!(value["graph"].is_object(), "{value}");
    }

    #[test]
    fn dot_output_is_graphviz() {
        let dot = dot("a -> b");
        assert!(dot.starts_with("digraph cant {"), "{dot}");
    }

    #[test]
    fn explaining_names_the_capabilities_a_program_needs() {
        let value = explain(r#""p" -> !@fs.read -> lines"#);
        // The whole call, not the namespace: `@fs.read` is what the program
        // asked for, and a permission is granted against the namespace anyway.
        let caps = value["capabilities"].as_array().expect("capabilities");
        assert!(
            caps.iter().any(|c| c.as_str() == Some("@fs.read")),
            "{:?}",
            value["capabilities"]
        );
        assert!(!value["text"].as_str().expect("text").is_empty());
    }

    #[test]
    fn converting_between_spellings_round_trips() {
        let ascii = "[1, 2] -> * -> []";
        let glyph = convert(ascii, "glyph");
        assert!(glyph.contains('\u{2192}'), "{glyph}");
        assert_eq!(convert(&glyph, "ascii"), ascii);
    }

    /// The toggle has to keep working while the program is being typed.
    #[test]
    fn converting_a_half_written_program_still_changes_the_spelling() {
        let glyph = convert("[1, 2] -> * -> ~{ deps", "glyph");
        assert!(glyph.contains('\u{2192}'), "{glyph}");
    }

    #[test]
    fn formatting_refuses_a_program_with_syntax_errors() {
        let value = format("[1, 2] -> ~{ deps", "ascii");
        assert_eq!(value["ok"], json!(false), "{value}");
    }

    #[test]
    fn the_version_matches_the_crate() {
        let value = version();
        assert_eq!(value["cant"], json!(cant::TOOL_VERSION));
        assert_eq!(
            value["cant_graph_schema_version"],
            json!(cant_sem::GRAPH_SCHEMA_VERSION)
        );
    }

    /// Nothing here may panic on nonsense: a panic in a wasm build takes the
    /// page's runtime with it, not just the call.
    #[test]
    fn no_input_panics() {
        for source in [
            "",
            "   ",
            "->",
            "~{",
            "}}}",
            "?{ $ > 1 }",
            "\u{feff}* -> []",
            "[1, 2] -> ~{ $ } :max 0",
            &"-> a".repeat(200),
        ] {
            let _ = check(source);
            let _ = expand(source);
            let _ = graph(source);
            let _ = dot(source);
            let _ = explain(source);
            let _ = format(source, "glyph");
            let _ = convert(source, "glyph");
            let _ = run(source);
        }
    }
}
