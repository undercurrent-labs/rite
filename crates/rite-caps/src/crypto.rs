//! `@crypto` — hashing, HMAC, constant-time comparison, and the two encodings
//! that always travel with them.
//!
//! Everything here except `random_bytes` is a pure function of its arguments, so
//! `@crypto.sha256("abc")` takes no `!` marker and needs no grant: it observes
//! nothing outside the program and a second call returns the same digest. That
//! also makes the capability usable anywhere the pure evaluator runs, including
//! the browser build, which has no filesystem and no sockets.
//!
//! `random_bytes` is the one exception. It reads the operating system's entropy
//! pool — outside state, different every call — so it is effectful and rides the
//! existing `random` permission rather than inventing a `crypto` one.
//!
//! ## What is deliberately absent
//!
//! No block ciphers, no AES, no RSA, no `encrypt(key, iv, mode, data)`. A
//! capability that asks the caller to choose an IV and a mode is a capability
//! that ships ECB and a reused nonce. Those belong behind a `cipher` package
//! with one opinionated construction, not in the host surface. Password hashing
//! (argon2, bcrypt) is deferred for the opposite reason: it needs tuning
//! parameters and a stored-format contract, which is a design, not a function.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rand::rngs::OsRng;
use rand::RngCore;
use rite_runtime::{EvalError, Key, Value};
use sha2::{Digest, Sha256, Sha512};

pub struct CryptoCap;

/// Refuse absurd requests rather than letting a typo allocate the machine.
const MAX_RANDOM_BYTES: i64 = 1 << 20;

impl CryptoCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "sha256",
            docs: "SHA-256 digest of a string, as lowercase hex.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "sha512",
            docs: "SHA-512 digest of a string, as lowercase hex.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "hmac_sha256",
            docs: "HMAC-SHA-256 of a message under a key, as lowercase hex.",
            arity: 2,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "random_bytes",
            docs: "n cryptographically secure random bytes, as lowercase hex.",
            arity: 1,
            effectful: true,
            permission: "random",
        },
        NativeFunctionDescriptor {
            name: "constant_time_eq",
            docs: "Compare two strings in time independent of their contents.",
            arity: 2,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "base64_encode",
            docs: "Encode a string as standard base64 (RFC 4648, padded).",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "base64_decode",
            docs: "Decode standard base64 to a string. Answers a Result.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "hex_encode",
            docs: "Encode a string as lowercase hex.",
            arity: 1,
            effectful: false,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "hex_decode",
            docs: "Decode hex to a string. Answers a Result.",
            arity: 1,
            effectful: false,
            permission: "",
        },
    ];

    pub fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        match method {
            "sha256" => {
                let s = arg_str(&args, 0, "crypto.sha256")?;
                Ok(Value::string(hex::encode(Sha256::digest(s.as_bytes()))))
            }
            "sha512" => {
                let s = arg_str(&args, 0, "crypto.sha512")?;
                Ok(Value::string(hex::encode(Sha512::digest(s.as_bytes()))))
            }
            "hmac_sha256" => {
                let key = arg_str(&args, 0, "crypto.hmac_sha256")?;
                let msg = arg_str(&args, 1, "crypto.hmac_sha256")?;
                Ok(Value::string(hex::encode(hmac_sha256(
                    key.as_bytes(),
                    msg.as_bytes(),
                ))))
            }
            // The only effectful entry: it reads the OS entropy pool.
            "random_bytes" => {
                perms.check_random().map_err(EvalError::Permission)?;
                let n = args.first().and_then(|v| v.as_int()).ok_or_else(|| {
                    EvalError::Message("crypto.random_bytes expects an integer".into())
                })?;
                if !(0..=MAX_RANDOM_BYTES).contains(&n) {
                    return Err(EvalError::Message(format!(
                        "crypto.random_bytes: n must be between 0 and {MAX_RANDOM_BYTES}, got {n}"
                    )));
                }
                let mut buf = vec![0u8; n as usize];
                OsRng.fill_bytes(&mut buf);
                Ok(Value::string(hex::encode(buf)))
            }
            "constant_time_eq" => {
                let a = arg_str(&args, 0, "crypto.constant_time_eq")?;
                let b = arg_str(&args, 1, "crypto.constant_time_eq")?;
                Ok(Value::Bool(constant_time_eq(a.as_bytes(), b.as_bytes())))
            }
            "base64_encode" => {
                let s = arg_str(&args, 0, "crypto.base64_encode")?;
                Ok(Value::string(encode_base64(s.as_bytes())))
            }
            "base64_decode" => {
                let s = arg_str(&args, 0, "crypto.base64_decode")?;
                Ok(decode_to_value(
                    "crypto.base64_decode",
                    decode_base64(&s).and_then(bytes_to_string),
                ))
            }
            "hex_encode" => {
                let s = arg_str(&args, 0, "crypto.hex_encode")?;
                Ok(Value::string(hex::encode(s.as_bytes())))
            }
            "hex_decode" => {
                let s = arg_str(&args, 0, "crypto.hex_decode")?;
                Ok(decode_to_value(
                    "crypto.hex_decode",
                    hex::decode(s.as_bytes())
                        .map_err(|e| e.to_string())
                        .and_then(bytes_to_string),
                ))
            }
            other => Err(EvalError::Capability(format!("unknown @crypto.{}", other))),
        }
    }
}

fn arg_str(args: &[Value], index: usize, who: &str) -> Result<String, EvalError> {
    args.get(index)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            EvalError::Message(format!("{who} expects a string at argument {}", index + 1))
        })
}

/// Decoders answer a `Result` rather than raising, because their input is
/// normally untrusted — a header, a query parameter, a file someone else wrote.
/// The error record matches `@json.decode`'s shape so a caller can match on
/// `kind` the same way.
fn decode_to_value(kind: &str, decoded: Result<String, String>) -> Value {
    match decoded {
        Ok(s) => Value::ok(Value::string(s)),
        Err(message) => Value::err(Value::record(vec![
            (Key::String("kind".into()), Value::string(kind)),
            (Key::String("message".into()), Value::string(message)),
        ])),
    }
}

/// Rite strings are text, so a decode that produces arbitrary bytes has to say
/// so rather than lossily substituting replacement characters.
fn bytes_to_string(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| "decoded bytes are not valid UTF-8".to_string())
}

/// HMAC as RFC 2104 defines it, over SHA-256's 64-byte block.
///
/// Written out rather than pulled from a crate: it is nine lines against a
/// hash the workspace already depends on, and the known-answer vectors in
/// `tests/crypto.rs` are what actually establish it is right.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36u8; BLOCK];
    let mut outer_pad = [0x5cu8; BLOCK];
    for ((inner, outer), &byte) in inner_pad
        .iter_mut()
        .zip(outer_pad.iter_mut())
        .zip(padded.iter())
    {
        *inner ^= byte;
        *outer ^= byte;
    }
    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(outer_pad)
        .chain_update(inner)
        .finalize()
        .into()
}

/// Compare without an early exit on the first differing byte.
///
/// The length is not treated as secret — it never is for the digests and tokens
/// this is meant for, and hiding it would mean hashing both sides first.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        difference |= x ^ y;
    }
    difference == 0
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_base64(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let packed = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn sextet(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a') as u32 + 26),
        b'0'..=b'9' => Some((c - b'0') as u32 + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict RFC 4648: padded, canonical, standard alphabet.
///
/// Strict on purpose. A decoder that accepts `"QR=="` and `"QQ=="` as the same
/// byte is a decoder two systems can disagree about, which is how signature
/// checks get bypassed. Whitespace, URL-safe `-_`, and unpadded input are all
/// rejected rather than guessed at.
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "length {} is not a multiple of 4 (input must be padded)",
            bytes.len()
        ));
    }
    let padding = bytes.iter().rev().take_while(|&&c| c == b'=').count();
    if padding > 2 {
        return Err("more than two padding characters".into());
    }
    let body = &bytes[..bytes.len() - padding];
    if body.contains(&b'=') {
        return Err("'=' appears before the end of the input".into());
    }

    let mut out = Vec::with_capacity(body.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for &c in body {
        let value = sextet(c).ok_or_else(|| format!("invalid base64 character {:?}", c as char))?;
        accumulator = (accumulator << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((accumulator >> bits) as u8);
            accumulator &= (1u32 << bits) - 1;
        }
    }
    if accumulator != 0 {
        return Err("trailing bits are not zero (non-canonical encoding)".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10, in full. Hand-written codecs are exactly the thing that
    /// looks right and is wrong on the last chunk.
    #[test]
    fn rfc4648_vectors_round_trip() {
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in vectors {
            assert_eq!(encode_base64(plain.as_bytes()), encoded, "encode {plain:?}");
            assert_eq!(
                decode_base64(encoded).unwrap(),
                plain.as_bytes(),
                "decode {encoded:?}"
            );
        }
    }

    #[test]
    fn base64_rejects_malformed_input() {
        for bad in [
            "Zg=",      // unpadded length
            "Zg===",    // over-padded
            "Z=g=",     // padding in the middle
            "Zm9v!!!!", // invalid character
            "Zh==",     // non-canonical trailing bits
            "Zm-v",     // URL-safe alphabet is a different encoding
        ] {
            assert!(decode_base64(bad).is_err(), "{bad:?} should not decode");
        }
    }

    /// RFC 4231, including the two cases the `@crypto` surface cannot reach:
    /// keys and messages that are not valid UTF-8, and a key longer than the
    /// 64-byte block (the branch that hashes the key first).
    #[test]
    fn rfc4231_hmac_vectors() {
        let long_key = [0xaau8; 131];
        let cases: [(&[u8], &[u8], &str); 6] = [
            (
                &[0x0b; 20],
                b"Hi There",
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe",
                b"what do ya want for nothing?",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                &[0xaa; 20],
                &[0xdd; 50],
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
            (
                &[
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                    0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
                ],
                &[0xcd; 50],
                "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
            ),
            (
                &long_key,
                b"Test Using Larger Than Block-Size Key - Hash Key First",
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
            (
                &long_key,
                b"This is a test using a larger than block-size key and a larger than \
                  block-size data. The key needs to be hashed before being used by the \
                  HMAC algorithm.",
                "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
            ),
        ];
        for (key, message, expected) in cases {
            assert_eq!(
                hex::encode(hmac_sha256(key, message)),
                expected,
                "HMAC-SHA-256 over a {}-byte key and {}-byte message",
                key.len(),
                message.len()
            );
        }
    }

    #[test]
    fn constant_time_eq_still_answers_correctly() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }
}
