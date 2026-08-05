//! The capabilities a browser tab can actually serve.
//!
//! `@json`, `@csv`, `@crypto`, `@regex` and `@store` compute over their
//! arguments and touch nothing outside the process, so they work wherever the
//! evaluator does. They did not, because the wasm build installed no capability
//! host at all and `@json.encode(⟨…⟩)` came back as "capability `@json` not
//! registered" — a packaging answer to a question about the language.
//!
//! Every test here runs in both builds. Under `native` they exercise the same
//! `rite-caps` host `rite run` uses; under `--no-default-features` they exercise
//! the browser half of it, which is the configuration `wasm32` gets. The
//! refusals that only *differ* between the two are at the bottom, gated.

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

#[tokio::test]
async fn json_encodes_and_decodes() {
    // Keys come back sorted, not in literal order: records are ordered by key.
    let r = run(r#"@json.encode(⟨name: "rite", n: 2⟩)"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!(r#"{"n":2,"name":"rite"}"#));

    let r = run(r#"@json.decode(@json.encode(⟨n: 41⟩))?.n + 1"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!(42));
}

#[tokio::test]
async fn csv_decodes_to_records() {
    let r = run(r#"@csv.decode("name,size\nrite,2")?"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!([{"name": "rite", "size": "2"}]),);
}

/// The digest is the published SHA-256 of "abc": a wrong host would have to be
/// wrong in exactly the right way to pass this.
#[tokio::test]
async fn crypto_hashes_and_encodes() {
    let r = run(r#"@crypto.sha256("abc")"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(
        r.value,
        serde_json::json!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );

    let r = run(r#"@crypto.base64_encode("rite")"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!("cml0ZQ=="));
}

#[tokio::test]
async fn regex_matches_and_captures() {
    let r = run(
        r#"@regex.is_match("ERROR 42: disk full", "[0-9]+")?"#,
        opts(),
    )
    .await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!(true));

    let r = run(r#"@regex.find("ERROR 42: disk full", "[0-9]+")?"#, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!("42"));
}

/// `@store` is the one in-process state the browser gets: writes are effectful,
/// reads are not, and both must survive within a single run.
#[tokio::test]
async fn store_round_trips_within_a_run() {
    let src = r#"
! @store.set("cache", "hits", 7)
@store.get("cache", "hits")?
"#;
    let r = run(src, opts()).await;
    assert!(r.ok, "{:?}", r.error);
    assert_eq!(r.value, serde_json::json!(7));
}

/// Entropy is the one thing here the browser reaches for outside the program,
/// and `getrandom`'s `js` backend supplies it. A seeded run must still be
/// reproducible, so this asserts the shape rather than the value.
#[tokio::test]
async fn crypto_random_bytes_answers_hex() {
    let r = run("! @crypto.random_bytes(8)", opts()).await;
    assert!(r.ok, "{:?}", r.error);
    let hex = r.value.as_str().unwrap_or_default().to_string();
    assert_eq!(hex.len(), 16, "8 bytes as hex, got {hex:?}");
    assert!(
        hex.chars().all(|c| c.is_ascii_hexdigit()),
        "not hex: {hex:?}"
    );
}

/// Without the `native` feature these are the paths a browser tab takes, and the
/// error has to name the capability and say why. Reported as "not registered"
/// before, which described the host's packaging rather than the program's
/// problem. `cargo run -p xtask -- wasm-check` runs this configuration.
#[cfg(not(feature = "native"))]
mod browser_only {
    use super::*;

    #[tokio::test]
    async fn fs_read_is_refused_by_name() {
        let r = run(r#"! @fs.read("Cargo.toml")"#, opts()).await;
        assert!(!r.ok, "@fs must not read in a browser: {:?}", r.value);
        let e = r.error.unwrap_or_default();
        assert!(
            e.contains("@fs") && e.contains("native-only"),
            "error should name the capability and why: {e}"
        );
    }

    #[tokio::test]
    async fn the_other_host_capabilities_are_refused_the_same_way() {
        for (src, cap) in [
            (r#"! @env.get("HOME")"#, "@env"),
            ("! @clock.now()", "@clock"),
            (r#"! @db.query("select 1")"#, "@db"),
            (r#"! @http.get("https://example.com")"#, "@http"),
        ] {
            let r = run(src, opts()).await;
            assert!(!r.ok, "`{src}` must not run in the browser");
            let e = r.error.unwrap_or_default();
            assert!(
                e.contains(cap) && e.contains("native-only"),
                "`{src}` should be refused by name: {e}"
            );
        }
    }

    /// A name no build has fails the compile, not the host: an unknown
    /// `@namespace` is E042 at resolve, so the browser never reaches a
    /// missing-host error for it.
    #[tokio::test]
    async fn an_unknown_capability_is_rejected_at_compile_time() {
        let r = run("! @teapot.brew()", opts()).await;
        assert!(!r.ok);
        let e = r.error.unwrap_or_default();
        assert!(e.contains("compile error"), "{e}");
    }
}
