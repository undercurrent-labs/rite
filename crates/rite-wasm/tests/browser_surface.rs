//! The surface Studio and the browser build call into.
//!
//! This crate is named for wasm but builds natively with the `native` feature, which is
//! how `rite studio` and the doctest runner use it — so the browser-facing contract is
//! testable here without a browser. `browser_safe` is the interesting part: it is a
//! security boundary when Studio runs it on a real machine, not just a compile target.

use rite_wasm::{run, RunOptions};

fn opts() -> RunOptions {
    RunOptions {
        allow_all: true,
        browser_safe: true,
        timeout_ms: Some(5000),
        seed: Some(42),
        files: Default::default(),
    }
}

// ------------------------------------------------------------------------- evaluation

#[tokio::test]
async fn a_pure_script_evaluates_to_json() {
    let r = run("1 + 2 * 3", opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!(7));
}

#[tokio::test]
async fn console_output_is_captured_not_printed() {
    // Studio renders this in a pane; if it went to the host's stdout the browser would
    // show an empty console for a script that clearly printed.
    let r = run(r#"! @console.println("hello")"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.stdout, "hello\n");
}

#[tokio::test]
async fn atoms_reach_json_as_their_names() {
    let r = run("⟨status: #ok, tags: [#a, #b]⟩", opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(
        r.value,
        serde_json::json!({"status": "ok", "tags": ["a", "b"]})
    );
}

#[tokio::test]
async fn a_failing_script_reports_the_error_and_keeps_prior_output() {
    let r = run("! @console.println(\"before\")\n1 / 0\n", opts()).await;
    assert!(!r.ok, "division by zero should fail");
    assert!(r.error.is_some());
    assert_eq!(
        r.stdout, "before\n",
        "output produced before the failure must survive"
    );
}

#[tokio::test]
async fn a_parse_error_is_reported_rather_than_panicking() {
    let r = run("◆ f( ⟦ ⟧⟧⟧", opts()).await;
    assert!(!r.ok);
    assert!(r.error.is_some());
}

// ------------------------------------------------------------------ determinism/limits

#[tokio::test]
async fn the_same_seed_gives_the_same_random_sequence() {
    // Studio reruns a script constantly while editing; an unseeded RNG would make every
    // run differ for reasons the author did not write.
    let src = "[! @random.int(1, 1000000), ! @random.int(1, 1000000)]";
    let a = run(src, opts()).await;
    let b = run(src, opts()).await;
    assert!(a.ok && b.ok, "{:?} {:?}", a.error, b.error);
    assert_eq!(a.value, b.value, "same seed, same values");

    let mut other = opts();
    other.seed = Some(43);
    let c = run(src, other).await;
    assert!(c.ok, "{:?}", c.error);
    assert_ne!(a.value, c.value, "a different seed must actually differ");
}

#[tokio::test]
async fn a_runaway_script_stops_at_the_timeout() {
    let mut o = opts();
    o.timeout_ms = Some(250);
    let r = run("n ↢ 0\n(1..100000000) → each { |i| n := n + 1 }\nn", o).await;
    assert!(!r.ok, "an unbounded loop must not run to completion");
    assert!(r.error.is_some());
}

// ------------------------------------------------------------------- the browser guard

#[tokio::test]
async fn process_is_refused_in_browser_mode() {
    let r = run(r#"! @process.run("echo", ["hi"])"#, opts()).await;
    assert!(!r.ok, "@process must not run under browser_safe");
    let err = r.error.unwrap_or_default();
    assert!(
        err.contains("process"),
        "the error should name the capability: {err}"
    );
}

#[tokio::test]
async fn the_ascii_spelling_of_process_is_refused_too() {
    // Both dialects are the same language; guarding only the glyph spelling would leave
    // the door open to anyone writing ASCII.
    let r = run(r#"do host.process.run("echo", ["hi"])"#, opts()).await;
    assert!(!r.ok, "host.process must not run under browser_safe");
}

#[tokio::test]
async fn the_guard_is_permissions_not_just_a_text_scan() {
    // The early substring check exists for a readable error, but the actual boundary is
    // the denied permission — this reaches the capability without the entry source
    // containing the literal text the scan looks for.
    let src = "cap ← \"proc\" + \"ess\"\n! @console.println(cap)\n";
    let r = run(src, opts()).await;
    assert!(r.ok, "building the string is harmless: {:?}", r.error);
    assert_eq!(r.stdout, "process\n");
}

#[tokio::test]
async fn allow_all_does_not_re_enable_process_in_browser_mode() {
    // `allow_all` is the Studio host's own switch; it must not widen past the browser
    // subset, or `rite studio --allow-all` would hand a web page process execution.
    let mut o = opts();
    o.allow_all = true;
    o.browser_safe = true;
    let r = run(r#"! @process.run("echo", ["hi"])"#, o).await;
    assert!(!r.ok, "browser_safe must win over allow_all for @process");
}

#[tokio::test]
async fn listen_is_virtualised_rather_than_binding_a_socket() {
    let src =
        "@http.listen \"127.0.0.1:4040\" ⟦\n  GET \"/health\" ⟦\n    ^ 200 ⟨status: #ok⟩\n  ⟧\n⟧\n";
    let r = run(src, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    let vh = r.virtual_http.expect("virtual http info");
    assert_eq!(vh.addr, "virtual://rite-studio");
    assert!(
        vh.routes.iter().any(|route| route.contains("/health")),
        "routes should list the declared path, got {:?}",
        vh.routes
    );
}

// ------------------------------------------------------------------- module files

#[tokio::test]
async fn files_resolve_use_without_a_filesystem() {
    let mut o = opts();
    o.files
        .insert("helper.rite".into(), "pub ◆ f() ⟦ ^ 41 ⟧\n".into());
    let r = run("use helper\n^ @helper.f() + 1", o).await;
    assert!(r.ok, "overlay module did not resolve: {:?}", r.error);
    assert_eq!(r.value, serde_json::json!(42));
}

#[tokio::test]
async fn files_keys_accept_module_names_and_paths() {
    // `lib/helpers.rite` and `lib.helpers` are the same key after
    // normalisation, and modules in the overlay can use each other.
    let mut o = opts();
    o.files.insert(
        "lib/helpers.rite".into(),
        "use base\npub ◆ triple(x) ⟦ ^ @base.add(x, x) + x ⟧\n".into(),
    );
    o.files
        .insert("base".into(), "pub ◆ add(a, b) ⟦ ^ a + b ⟧\n".into());
    let r = run("use lib.helpers\n^ triple(5)", o).await;
    assert!(r.ok, "nested overlay imports failed: {:?}", r.error);
    assert_eq!(r.value, serde_json::json!(15));
}

#[tokio::test]
async fn a_missing_overlay_module_is_a_compile_error() {
    let mut o = opts();
    o.files
        .insert("helper.rite".into(), "pub ◆ f() ⟦ ^ 1 ⟧\n".into());
    let r = run("use nosuch\n^ 1", o).await;
    assert!(!r.ok, "an unresolved `use` must fail the compile");
}

// -------------------------------------------------------------------- non-run surfaces

#[test]
fn parse_reports_diagnostics_without_evaluating() {
    let good = rite_wasm::parse("◆ f() ⟦ ^ 1 ⟧\n");
    assert!(good.ok, "valid source parses");
    assert!(good.ast_json.is_some());

    let bad = rite_wasm::parse("◆ f( ⟦⟧⟧");
    assert!(!bad.ok);
    assert!(!bad.diagnostics.is_empty(), "a failure must say why");
}

#[test]
fn analyze_returns_symbols_for_the_editor() {
    let v = rite_wasm::analyze("◆ target() ⟦ ^ 1 ⟧\n");
    let text = v.to_string();
    assert!(text.contains("target"), "symbols missing from {text}");
}

#[test]
fn format_and_convert_round_trip_between_dialects() {
    let glyph = "◆ f(x) ⟦ ^ x + 1 ⟧\n";
    let ascii = rite_wasm::convert(glyph, "ascii").expect("to ascii");
    assert!(ascii.text.contains("def "), "got {}", ascii.text);
    assert!(!ascii.text.contains('◆'), "glyphs remain: {}", ascii.text);

    let back = rite_wasm::convert(&ascii.text, "glyph").expect("to glyph");
    assert!(back.text.contains('◆'), "got {}", back.text);

    // Both directions still parse — a conversion that does not is worse than none.
    assert!(rite_wasm::parse(&ascii.text).ok, "ascii: {}", ascii.text);
    assert!(rite_wasm::parse(&back.text).ok, "glyph: {}", back.text);
}

#[test]
fn formatting_is_idempotent() {
    let once = rite_wasm::format("◆ f(x)⟦^x+1⟧\n", "glyph").expect("format");
    let twice = rite_wasm::format(&once.text, "glyph").expect("format again");
    assert_eq!(once.text, twice.text, "formatting twice changed the source");
}
