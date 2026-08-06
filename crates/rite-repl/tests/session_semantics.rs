//! REPL session behaviour across inputs.
//!
//! The session replays a prelude of earlier definitions before every input, because each
//! input is compiled in isolation and the resolver has no memory of previous ones. That
//! design is why effects had to stop being replayed with it: `r ← ! @http.post(…)`
//! re-submitted on every subsequent line.

use rite_caps::PermissionSet;
use rite_repl::{is_complete, ReplSession};

fn session() -> ReplSession {
    ReplSession::new(PermissionSet::allow_all())
}

// --------------------------------------------------------------------- effect replay

#[tokio::test]
async fn an_effectful_binding_runs_its_effect_once() {
    // The bug: the prelude re-ran the binding's source before every later input, so the
    // effect happened again each time — invisibly, and for the life of the session.
    let mut s = session();
    let first = s.eval(r#"data ← ! @console.println("EFFECT")"#).await;
    assert!(first.ok, "{:?}", first.error);

    // The context is rebuilt per eval, so stdout holds only this input's output.
    for input in ["1 + 1", "2 + 2", "3 + 3"] {
        let r = s.eval(input).await;
        assert!(r.ok, "{input}: {:?}", r.error);
        assert!(
            s.ctx.stdout.is_empty(),
            "evaluating `{input}` re-ran an earlier effect: {:?}",
            s.ctx.stdout
        );
    }
}

#[tokio::test]
async fn the_value_of_an_effectful_binding_still_persists() {
    // Not replaying the effect must not cost the binding: the point is to keep the
    // value, not to forget it.
    let mut s = session();
    let bound = s.eval(r#"n ← ! @random.int(5, 5)"#).await;
    assert!(bound.ok, "{:?}", bound.error);
    let read = s.eval("n + 1").await;
    assert!(read.ok, "the binding did not survive: {:?}", read.error);
    assert_eq!(read.display.as_deref(), Some("6"));
}

#[tokio::test]
async fn a_pure_binding_is_unaffected() {
    let mut s = session();
    assert!(s.eval("x ← 41").await.ok);
    let r = s.eval("x + 1").await;
    assert_eq!(r.display.as_deref(), Some("42"));
}

// -------------------------------------------------------------- value round-tripping

/// Every value with a literal form must survive being written back into the prelude.
/// A literal that did not re-parse would poison every later input in the session.
#[tokio::test]
async fn effectful_bindings_of_every_literal_kind_round_trip() {
    // `none` has no display, so the expectation is an Option rather than a string.
    let cases: &[(&str, Option<&str>)] = &[
        ("! @random.int(7, 7)", Some("7")),
        (r#"! @json.decode("{{\"a\":1}}")?"#, Some("⟨a: 1⟩")),
        (r#"! @json.decode("[1,2,3]")?"#, Some("[1, 2, 3]")),
        (r#"! @json.decode("\"text\"")?"#, Some("text")),
        (r#"! @json.decode("true")?"#, Some("true")),
        (r#"! @json.decode("null")?"#, None),
        (r#"! @json.decode("[ [1,2],[3] ]")?"#, Some("[[1, 2], [3]]")),
        (
            r#"! @json.decode("{{\"nested\":{{\"k\":[1,\"two\"]}}}}")?"#,
            Some("⟨nested: ⟨k: [1, two]⟩⟩"),
        ),
    ];
    for (expr, expected) in cases {
        let mut s = session();
        let bound = s.eval(&format!("v ← {expr}")).await;
        assert!(bound.ok, "{expr}: {:?}", bound.error);
        assert_eq!(
            bound.display.as_deref(),
            *expected,
            "binding value for {expr}"
        );

        // The next input replays the prelude — if the stored literal did not re-parse,
        // this fails rather than the binding.
        let read = s.eval("v").await;
        assert!(read.ok, "{expr} did not round-trip: {:?}", read.error);
        assert_eq!(
            read.display.as_deref(),
            *expected,
            "value changed for {expr}"
        );
    }
}

#[tokio::test]
async fn a_string_with_quotes_braces_and_escapes_round_trips() {
    // `{` opens an interpolation hole, so a stored string containing one must escape it
    // or the replay would substitute a binding that does not exist.
    let mut s = session();
    let bound = s
        .eval(r#"v ← ! @json.decode("\"he said {{name}} and \\\\ backslash\"")?"#)
        .await;
    assert!(bound.ok, "{:?}", bound.error);
    let original = bound.display.clone().expect("display");
    assert!(original.contains("{name}"), "got {original}");

    let read = s.eval("v").await;
    assert!(read.ok, "string did not round-trip: {:?}", read.error);
    assert_eq!(read.display, Some(original), "the string changed on replay");
}

#[tokio::test]
async fn a_value_with_no_literal_form_keeps_working() {
    // A closure cannot be written back as a literal, so that binding keeps its original
    // source. It must still be usable rather than breaking the session.
    let mut s = session();
    assert!(s.eval("f ← { |x| x * 2 }").await.ok);
    let r = s.eval("f(21)").await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.display.as_deref(), Some("42"));
}

// ------------------------------------------------------------------- session basics

#[tokio::test]
async fn functions_persist_across_inputs() {
    let mut s = session();
    assert!(s.eval("◆ double(n) ⟦ ^ n * 2 ⟧").await.ok);
    let r = s.eval("double(21)").await;
    assert_eq!(r.display.as_deref(), Some("42"));
}

#[tokio::test]
async fn a_later_binding_shadows_an_earlier_one() {
    let mut s = session();
    assert!(s.eval("x ← 1").await.ok);
    assert!(s.eval("x ← 2").await.ok);
    assert_eq!(s.eval("x").await.display.as_deref(), Some("2"));
}

#[tokio::test]
async fn a_function_can_be_redefined() {
    // This failed silently: the second definition errored and the *first* body stayed
    // live, so the REPL kept running code the user had just replaced.
    let mut s = session();
    assert!(s.eval("◆ f() ⟦ ^ 1 ⟧").await.ok);
    let second = s.eval("◆ f() ⟦ ^ 2 ⟧").await;
    assert!(
        second.ok,
        "redefining a function failed: {:?}",
        second.error
    );
    assert_eq!(
        s.eval("f()").await.display.as_deref(),
        Some("2"),
        "the new body must be the one that runs"
    );
}

#[tokio::test]
async fn a_redefinition_is_visible_to_earlier_definitions() {
    // The prelude replays in order, so a function defined before a rebinding still sees
    // the current value rather than the one that existed when it was written.
    let mut s = session();
    assert!(s.eval("x ← 1").await.ok);
    assert!(s.eval("◆ get() ⟦ ^ x ⟧").await.ok);
    assert!(s.eval("x ← 99").await.ok);
    assert_eq!(s.eval("get()").await.display.as_deref(), Some("99"));
}

#[tokio::test]
async fn a_failed_input_does_not_enter_the_prelude() {
    // A stored input that does not compile would break every later evaluation.
    let mut s = session();
    let bad = s.eval("y ← undefined_thing").await;
    assert!(!bad.ok, "expected a failure");
    let good = s.eval("1 + 1").await;
    assert!(
        good.ok,
        "a failed input poisoned the session: {:?}",
        good.error
    );
    assert_eq!(good.display.as_deref(), Some("2"));
}

#[tokio::test]
async fn reset_clears_definitions_and_effects() {
    let mut s = session();
    assert!(s.eval("x ← 1").await.ok);
    s.reset();
    let r = s.eval("x").await;
    assert!(!r.ok, "reset did not clear the binding");
}

#[tokio::test]
async fn an_expression_is_not_remembered() {
    // Only definitions belong in the prelude; replaying every expression would make the
    // session quadratic and re-run work the user already saw.
    let mut s = session();
    assert!(s.eval("1 + 1").await.ok);
    assert!(s.eval("2 + 2").await.ok);
    let r = s.eval("3 + 3").await;
    assert_eq!(r.display.as_deref(), Some("6"));
}

#[tokio::test]
async fn a_syntax_error_is_reported_not_fatal() {
    let mut s = session();
    let bad = s.eval("◆ f( ⟦⟧⟧").await;
    assert!(!bad.ok);
    assert!(bad.error.is_some());
    assert!(s.eval("1 + 1").await.ok, "the session survived");
}

// ------------------------------------------------------------- multi-line completeness

#[test]
fn incomplete_input_waits_for_its_closing_delimiter() {
    assert!(!is_complete("◆ f() ⟦"), "an open block is incomplete");
    assert!(is_complete("◆ f() ⟦ ^ 1 ⟧"), "a closed block is complete");
    assert!(!is_complete("def f() [["), "ASCII too");
    assert!(is_complete("def f() [[ return 1 ]]"));
    assert!(is_complete("1 + 1"), "a plain expression is complete");
}

// ------------------------------------------------------------------- handle bindings

/// A binding that holds a host handle is carried across inputs by value.
///
/// It has no literal form, so the prelude used to replay its source: `h ← ! @fs.open(f,
/// #read)?` reopened the file before every later input, and three `@fs.read_line(h)` in
/// a row each answered the *first* line. `@mcp.connect` had the same shape and a worse
/// cost — a second server subprocess per line of the session.
#[tokio::test]
async fn a_handle_is_not_reacquired_on_every_input() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("repl-handles");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("lines.txt");
    std::fs::write(&path, "alpha\nbeta\ngamma\n").expect("write fixture");

    let mut s = session();
    let opened = s
        .eval(&format!(
            r#"h ← ! @fs.open("{}", #read)?"#,
            path.to_str().unwrap()
        ))
        .await;
    assert!(opened.ok, "{:?}", opened.error);

    for expected in ["alpha", "beta", "gamma"] {
        let r = s.eval("! @fs.read_line(h)?").await;
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(
            r.display.as_deref(),
            Some(expected),
            "the handle was reopened rather than carried"
        );
    }
}

/// The handle table is the one part of the context that survives an input, so a handle
/// opened on one line is still open on the next. `:reset` builds a new one, which is
/// what closes everything the session held.
#[tokio::test]
async fn reset_releases_what_the_session_held() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("repl-handles");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("reset.txt");
    std::fs::write(&path, "alpha\n").expect("write fixture");

    let mut s = session();
    assert!(
        s.eval(&format!(
            r#"h ← ! @fs.open("{}", #read)?"#,
            path.to_str().unwrap()
        ))
        .await
        .ok
    );
    s.reset();
    let after = s.eval("h").await;
    assert!(!after.ok, "the handle outlived :reset");
}

/// Rebinding the name runs the new expression: the seeded value stands in for a
/// replayed definition, it does not outrank one the user typed.
#[tokio::test]
async fn redefining_a_handle_binding_opens_the_new_one() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("repl-handles");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let first = dir.join("first.txt");
    let second = dir.join("second.txt");
    std::fs::write(&first, "one\n").expect("write fixture");
    std::fs::write(&second, "two\n").expect("write fixture");

    let mut s = session();
    let open =
        |p: &std::path::Path| format!(r#"h ← ! @fs.open("{}", #read)?"#, p.to_str().unwrap());
    assert!(s.eval(&open(&first)).await.ok);
    assert_eq!(
        s.eval("! @fs.read_line(h)?").await.display.as_deref(),
        Some("one")
    );
    assert!(s.eval(&open(&second)).await.ok);
    assert_eq!(
        s.eval("! @fs.read_line(h)?").await.display.as_deref(),
        Some("two")
    );
}
