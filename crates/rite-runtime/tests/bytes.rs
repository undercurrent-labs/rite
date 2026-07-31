//! Authoring bytes, not just relaying them.
//!
//! `Value::Bytes` existed but could only be counted and compared, so a program
//! could echo a datagram and not build one — the DNS query that motivated `@udp`
//! was unwritable. `@crypto.hex_decode` looks like the way in and is not: it
//! answers a *string* and rejects anything that is not valid UTF-8, which most
//! binary is.

use rite_runtime::{run_source, RuntimeContext, Value};

async fn eval(src: &str) -> Value {
    let mut ctx = RuntimeContext::new();
    run_source("b.rite", src, &mut ctx)
        .await
        .unwrap_or_else(|e| panic!("eval failed for `{src}`: {e}"))
}

async fn text(src: &str) -> String {
    match eval(src).await {
        Value::String(s) => s.to_string(),
        other => panic!("expected a string from `{src}`, got {other:?}"),
    }
}

/// The case that was impossible: a DNS header is not valid UTF-8.
#[tokio::test]
async fn arbitrary_bytes_can_be_authored() {
    assert_eq!(
        text(r#"to_hex(from_hex("abcd01000001000000000000")?)"#).await,
        "abcd01000001000000000000"
    );
    // The single byte that proves it is not string-shaped.
    assert_eq!(text(r#"to_hex(from_hex("ff")?)"#).await, "ff");
}

#[tokio::test]
async fn bytes_from_numbers_and_text() {
    assert_eq!(text("to_hex(bytes([0, 1, 255]))").await, "0001ff");
    assert_eq!(text(r#"to_hex(bytes("hi"))"#).await, "6869");
    assert_eq!(text("to_hex(bytes([]))").await, "");
}

/// Refusing beats truncating: a wrapped 0x1ff is a packet that goes out wrong
/// and gets debugged at the far end.
#[tokio::test]
async fn out_of_range_is_refused_not_wrapped() {
    for src in ["bytes([256])", "bytes([0 - 1])", r#"bytes([1, "x"])"#] {
        let mut ctx = RuntimeContext::new();
        let err = run_source("b.rite", src, &mut ctx)
            .await
            .expect_err("should refuse");
        assert!(
            format!("{err}").contains("bytes expects"),
            "`{src}` should say what it wanted"
        );
    }
}

/// Assembling a header and a body is most of what authoring is for.
#[tokio::test]
async fn concat_and_slice_stay_bytes() {
    assert_eq!(
        text(r#"to_hex(concat(from_hex("abcd")?, bytes([1, 2])))"#).await,
        "abcd0102"
    );
    assert_eq!(
        text(r#"to_hex(slice(from_hex("abcd0102")?, 0, 2))"#).await,
        "abcd"
    );
    assert_eq!(
        text(r#"to_hex(slice(from_hex("abcd0102")?, -2))"#).await,
        "0102"
    );
    // A string joins bytes, since a string is bytes with an encoding.
    assert_eq!(text(r#"to_hex(concat(bytes([0]), "A"))"#).await, "0041");
}

#[tokio::test]
async fn byte_at_indexes_and_answers_none_past_the_end() {
    assert_eq!(
        eval(r#"byte_at(from_hex("abcd")?, 0)"#).await.as_int(),
        Some(0xab)
    );
    assert_eq!(
        eval(r#"byte_at(from_hex("abcd")?, -1)"#).await.as_int(),
        Some(0xcd)
    );
    assert!(matches!(
        eval(r#"byte_at(from_hex("abcd")?, 99)"#).await,
        Value::None
    ));
}

/// Both directions admit when they cannot: untrusted hex in, non-text bytes out.
#[tokio::test]
async fn conversions_answer_results() {
    assert_eq!(text(r#"to_text(bytes("hi"))?"#).await, "hi");
    assert!(matches!(
        eval(r#"is_err(to_text(from_hex("ff")?))"#).await,
        Value::Bool(true)
    ));
    assert!(matches!(
        eval(r#"is_err(from_hex("abc"))"#).await,
        Value::Bool(true)
    ));
    assert!(matches!(
        eval(r#"is_err(from_hex("zz"))"#).await,
        Value::Bool(true)
    ));
    // Whitespace in hex is convenience, not an error — packets get pasted in
    // groups of two.
    assert_eq!(text(r#"to_hex(from_hex("ab cd  01")?)"#).await, "abcd01");
}

#[tokio::test]
async fn count_measures_bytes_not_characters() {
    // "é" is two bytes and one character; the two must not agree here.
    assert_eq!(eval(r#"count(bytes("é"))"#).await.as_int(), Some(2));
    assert_eq!(eval(r#"count("é")"#).await.as_int(), Some(1));
}
