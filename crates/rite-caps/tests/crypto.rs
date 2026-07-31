//! `@crypto` at the value boundary.
//!
//! The digests are checked against published vectors rather than against
//! themselves: a self-consistent hash is exactly what a wrong implementation
//! produces, and "sha256(x) == sha256(x)" would pass for `x.len()`. Every
//! expected digest below comes from FIPS 180-4 or RFC 4231.
//!
//! The other half of this file is the permission and effect contract: only
//! `random_bytes` reads outside state, so only it consults a grant, and the
//! resolver has to agree about which one that is.

use rite_caps::crypto::CryptoCap;
use rite_caps::{HostCapabilities, PermissionSet};
use rite_runtime::{CapabilityHost, EvalError, ResultValue, RuntimeContext, Value};

fn call(method: &str, args: Vec<Value>) -> Result<Value, EvalError> {
    CryptoCap.call(method, args, &PermissionSet::allow_all())
}

fn text(method: &str, args: &[&str]) -> String {
    call(method, args.iter().map(|s| Value::string(*s)).collect())
        .unwrap_or_else(|e| panic!("@crypto.{method}: {e}"))
        .as_str()
        .unwrap_or_else(|| panic!("@crypto.{method} did not answer a string"))
        .to_string()
}

/// Unwraps the `ok` side, or explains which error record came back instead.
fn ok_value(method: &str, arg: &str) -> String {
    match call(method, vec![Value::string(arg)]).expect("no host error") {
        Value::Result(ResultValue::Ok(v)) => {
            v.as_str().expect("ok payload is a string").to_string()
        }
        Value::Result(ResultValue::Err(e)) => panic!("@crypto.{method}({arg:?}) failed: {e}"),
        other => panic!("@crypto.{method} answered {other:?}, not a Result"),
    }
}

fn is_err(method: &str, arg: &str) -> bool {
    matches!(
        call(method, vec![Value::string(arg)]).expect("no host error"),
        Value::Result(ResultValue::Err(_))
    )
}

// ---------------------------------------------------------------- digests

/// FIPS 180-4 appendix vectors. If these drift, everything downstream is wrong.
#[test]
fn sha256_matches_the_published_vectors() {
    assert_eq!(
        text("sha256", &["abc"]),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        text("sha256", &[""]),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        text(
            "sha256",
            &["abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"]
        ),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn sha512_matches_the_published_vectors() {
    assert_eq!(
        text("sha512", &["abc"]),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
         2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    assert_eq!(
        text("sha512", &[""]),
        "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
         47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
    );
}

/// Digests cover UTF-8 bytes, not chars. Worth pinning independently: a host
/// that transcoded or truncated would still agree with itself.
#[test]
fn digests_cover_the_utf8_bytes_of_the_string() {
    assert_eq!(text("hex_encode", &["é"]), "c3a9");
    // The digest of the two bytes 0xC3 0xA9, computed outside this workspace.
    assert_eq!(
        text("sha256", &["é"]),
        "4a99557e4033c3539de2eb65472017cad5f9557f7a0625a09f1c3f6e2ba69c4c"
    );
}

#[test]
fn hmac_sha256_matches_rfc4231() {
    // RFC 4231 case 2 — the one whose key and message are both plain ASCII, so
    // it survives the trip through Rite's string type intact.
    assert_eq!(
        text("hmac_sha256", &["Jefe", "what do ya want for nothing?"]),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    // The remaining RFC 4231 cases need non-UTF-8 keys; they are exercised
    // against `hmac_sha256` directly in `src/crypto.rs`.
}

/// An HMAC is not a hash of the concatenation, and swapping the arguments must
/// not give the same answer — both are mistakes a plausible-looking
/// implementation makes.
#[test]
fn hmac_is_keyed_and_ordered() {
    let a = text("hmac_sha256", &["key", "message"]);
    let b = text("hmac_sha256", &["message", "key"]);
    assert_ne!(a, b, "hmac_sha256 ignored the key/message distinction");
    assert_ne!(a, text("sha256", &["keymessage"]));
    assert_ne!(a, text("hmac_sha256", &["keyy", "message"]));
}

// --------------------------------------------------------------- encodings

/// RFC 4648 §10, through the capability rather than the private helper.
#[test]
fn base64_matches_rfc4648() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(text("base64_encode", &[plain]), encoded);
        assert_eq!(ok_value("base64_decode", encoded), plain);
    }
}

#[test]
fn hex_matches_rfc4648() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "66"),
        ("fo", "666f"),
        ("foobar", "666f6f626172"),
    ] {
        assert_eq!(text("hex_encode", &[plain]), encoded);
        assert_eq!(ok_value("hex_decode", encoded), plain);
    }
}

/// Decoders take untrusted input, so a bad string is a value the script can
/// match on — never a crash, and never a silently repaired result.
#[test]
fn decoding_garbage_answers_err_rather_than_failing() {
    for bad in ["Zg=", "Zg===", "Z=g=", "Zm9v!!!!", "Zh==", "Zm-v"] {
        assert!(is_err("base64_decode", bad), "base64 accepted {bad:?}");
    }
    for bad in ["6", "zz", "66 6f", "0x66"] {
        assert!(is_err("hex_decode", bad), "hex accepted {bad:?}");
    }
}

/// Decoded bytes that are not text cannot become a Rite string, and saying so
/// beats substituting U+FFFD into a value someone is about to compare.
#[test]
fn decoding_to_non_utf8_answers_err() {
    assert!(is_err("hex_decode", "ff"));
    assert!(is_err("base64_decode", "/w=="));
}

#[test]
fn the_error_record_names_the_function_that_produced_it() {
    let Value::Result(ResultValue::Err(e)) =
        call("base64_decode", vec![Value::string("Zg=")]).unwrap()
    else {
        panic!("expected an err");
    };
    let rendered = format!("{}", e);
    assert!(
        rendered.contains("crypto.base64_decode"),
        "error record does not name its source: {rendered}"
    );
}

// ------------------------------------------------- constant-time comparison

#[test]
fn constant_time_eq_answers_the_same_question_as_equality() {
    let digest = text("sha256", &["abc"]);
    assert_eq!(
        call(
            "constant_time_eq",
            vec![Value::string(digest.clone()), Value::string(digest.clone())]
        )
        .unwrap(),
        Value::Bool(true)
    );
    let other = text("sha256", &["abd"]);
    assert_eq!(
        call(
            "constant_time_eq",
            vec![Value::string(digest.clone()), Value::string(other)]
        )
        .unwrap(),
        Value::Bool(false)
    );
    // Differing lengths, and a difference in the very first byte, both answer
    // false rather than panicking on the zip.
    assert_eq!(
        call(
            "constant_time_eq",
            vec![Value::string(digest), Value::string("")]
        )
        .unwrap(),
        Value::Bool(false)
    );
}

// ------------------------------------------------------------ random_bytes

#[test]
fn random_bytes_returns_the_requested_length_as_hex() {
    for n in [0usize, 1, 16, 32] {
        let got = call("random_bytes", vec![Value::Int(n as i64)])
            .expect("random_bytes")
            .as_str()
            .expect("hex string")
            .to_string();
        assert_eq!(got.len(), n * 2, "{n} bytes should be {} hex chars", n * 2);
        assert!(got
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }
}

#[test]
fn random_bytes_does_not_repeat_itself() {
    let a = call("random_bytes", vec![Value::Int(32)]).unwrap();
    let b = call("random_bytes", vec![Value::Int(32)]).unwrap();
    assert!(
        !a.structural_eq(&b),
        "two draws of 32 bytes were identical — this is not an entropy source"
    );
}

/// The whole point of routing it through `random`: revoking the grant has to
/// stop it, exactly as it stops `@random.int`.
#[test]
fn random_bytes_is_refused_when_random_is_denied() {
    let mut denied = PermissionSet::default_secure();
    denied.random = false;
    let err = CryptoCap
        .call("random_bytes", vec![Value::Int(16)], &denied)
        .expect_err("a denied grant must stop the draw");
    assert!(matches!(err, EvalError::Permission(_)), "{err:?}");
}

/// ...and denying it must *not* disturb the pure half, which is the reason the
/// pure half carries no permission at all.
#[test]
fn the_pure_functions_need_no_grant() {
    let mut nothing = PermissionSet::default_secure();
    nothing.random = false;
    nothing.console = false;
    nothing.clock = false;
    for (method, args) in [
        ("sha256", vec![Value::string("abc")]),
        ("sha512", vec![Value::string("abc")]),
        ("hmac_sha256", vec![Value::string("k"), Value::string("m")]),
        (
            "constant_time_eq",
            vec![Value::string("a"), Value::string("a")],
        ),
        ("base64_encode", vec![Value::string("abc")]),
        ("base64_decode", vec![Value::string("YWJj")]),
        ("hex_encode", vec![Value::string("abc")]),
        ("hex_decode", vec![Value::string("616263")]),
    ] {
        assert!(
            CryptoCap.call(method, args, &nothing).is_ok(),
            "@crypto.{method} asked for a permission it should not need"
        );
    }
}

#[test]
fn an_absurd_length_is_an_error_not_an_allocation() {
    assert!(call("random_bytes", vec![Value::Int(-1)]).is_err());
    assert!(call("random_bytes", vec![Value::Int(i64::MAX)]).is_err());
}

// ------------------------------------------------------------- host wiring

#[test]
fn a_missing_argument_is_a_message_not_a_panic() {
    assert!(matches!(call("sha256", vec![]), Err(EvalError::Message(_))));
    assert!(matches!(
        call("hmac_sha256", vec![Value::string("only-a-key")]),
        Err(EvalError::Message(_))
    ));
    assert!(matches!(
        call("random_bytes", vec![Value::string("sixteen")]),
        Err(EvalError::Message(_))
    ));
}

#[test]
fn an_unknown_method_names_the_capability() {
    let err = call("aes_encrypt", vec![]).expect_err("no such function");
    let rendered = err.to_string();
    assert!(rendered.contains("@crypto.aes_encrypt"), "{rendered}");
}

/// Registered, not merely written: a capability the host does not dispatch is
/// invisible to every script, and `all_descriptors` is what the docs, the
/// editor grammar and the effect-parity test all read.
#[tokio::test]
async fn the_host_dispatches_at_crypto() {
    let host = HostCapabilities::with_defaults(PermissionSet::default_secure());
    assert!(
        host.all_descriptors()
            .iter()
            .any(|(name, _)| *name == "crypto"),
        "@crypto is missing from all_descriptors()"
    );
    let ctx = RuntimeContext::new();
    let got = host
        .call(
            &["crypto".to_string(), "sha256".to_string()],
            vec![Value::string("abc")],
            false,
            &ctx,
        )
        .await
        .expect("dispatch");
    assert_eq!(
        got.as_str().unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// The classification this capability is built around, asserted directly rather
/// than left to `effect_parity.rs` to infer.
#[test]
fn only_random_bytes_is_effectful() {
    for d in CryptoCap::DESCRIPTORS {
        let expected_effectful = d.name == "random_bytes";
        assert_eq!(
            d.effectful, expected_effectful,
            "@crypto.{} is classified effectful: {}",
            d.name, d.effectful
        );
        assert_eq!(
            d.permission,
            if expected_effectful { "random" } else { "" },
            "@crypto.{} names permission {:?}",
            d.name,
            d.permission
        );
        assert_eq!(
            rite_sem::resolve::is_effectful(&format!("crypto.{}", d.name)),
            expected_effectful,
            "the resolver and the descriptor disagree about @crypto.{}",
            d.name
        );
    }
}
