//! The native half of the browser parity gate (AR4/Q2).
//!
//! `parity.rs` proves the CLI and the browser binding are one code path — by
//! calling both *natively*. What it cannot prove is that the wasm32 build,
//! actually executed, produces the same bytes. This pair does:
//!
//! - this test renders the ceremony example through the crate's own
//!   `render_cant` — the exact function the browser calls — natively, and pins
//!   the SVG and fingerprint as fixtures;
//! - `scripts/check-sigil-wasm-parity.mjs` loads the *built* wasm bundle in
//!   Node, renders the same source with the same options, and compares against
//!   the same fixtures byte for byte. It runs in `build-sigil-site.sh`, so CI
//!   and every release exercise it.
//!
//! Regenerate with `SIGIL_BLESS=1 cargo test -p cant-sigil-wasm --test
//! browser_fixture`. Comparison is over canonical *text*, never deserialized
//! structures — serde_json does not round-trip every f64.

use std::path::{Path, PathBuf};

use cant_sigil_wasm::{render_cant, RenderOptions};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-sigil-wasm has two ancestors")
        .to_path_buf()
}

fn check(path: &Path, actual: &str) {
    if std::env::var_os("SIGIL_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, actual).expect("write fixture");
        return;
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "missing browser fixture {}: {e}\nregenerate with SIGIL_BLESS=1",
            path.display()
        )
    });
    assert_eq!(
        expected,
        actual,
        "{} changed — if intended, re-bless and re-run the wasm parity script",
        path.display()
    );
}

#[test]
fn the_browser_fixture_matches_the_native_render() {
    let root = repo_root();
    let source = std::fs::read_to_string(root.join("examples/sigil/ceremony.cant"))
        .expect("the ceremony example exists");

    // Canonical, so the fixture is a function of the program rather than of a
    // seeded rotation. Everything else is the browser's defaults.
    let options = RenderOptions {
        canonical: true,
        seed: "canonical".into(),
        ..RenderOptions::default()
    };
    let result = render_cant("ceremony.cant", &source, &options);
    assert!(result.ok, "{:?}", result.diagnostics);

    check(
        &root.join("fixtures/sigil/browser/ceremony.svg"),
        &result.svg.expect("an svg"),
    );
    check(
        &root.join("fixtures/sigil/browser/ceremony.fingerprint"),
        &result.fingerprint.expect("a fingerprint"),
    );
}
