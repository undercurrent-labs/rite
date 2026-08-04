//! Canonical serialization and the graph fingerprint.
//!
//! Determinism is not a nice property here, it is the thing that makes golden
//! tests, native/WASM parity, and a stable default seed possible at all. All
//! three reduce to one question: **what exactly is being hashed?**
//!
//! # The rules
//!
//! Canonical form is JSON with:
//!
//! * object keys sorted lexicographically, at every depth;
//! * no insignificant whitespace;
//! * absent and default-valued optional fields omitted, so a graph that spells
//!   out a default and one that omits it hash alike;
//! * semantic ordering preserved — `nodes`, `edges` and `regions` keep their
//!   array order, because that order is part of what the graph says;
//! * integers rendered without a decimal point, floats with the shortest
//!   representation that round-trips.
//!
//! # What is excluded, and why it matters
//!
//! The fingerprint is over the *meaning* of the graph, so anything that can
//! change without the program changing is stripped first:
//!
//! * **the source file's name** — renaming a file must not change the artifact;
//! * **producer name and version** — a `cant` release that changes no graph must
//!   not invalidate every render anyone has cached;
//! * **source snippets** — a comment edit that does not move a span should not
//!   re-seed the ornament. Spans themselves are kept: moving a stage is a real
//!   change to where the program is.
//!
//! This is the list the specification's §7.1 asks for, and each exclusion is a
//! decision about what "the same program" means rather than a convenience.
//!
//! # The hash
//!
//! SHA-256 over the canonical bytes, truncated to 128 bits and rendered as 32
//! lowercase hex characters. SHA-256 because it is already a workspace
//! dependency, is stable across platforms and Rust versions by specification
//! rather than by convention, and compiles to WASM — the parity requirement in
//! ADR 0005 rules out anything whose output depends on the host. Truncated
//! because 128 bits is far past collision concern for identifying a program and
//! 64 hex characters in a filename or a `<desc>` is unpleasant.
//!
//! `DefaultHasher` was rejected: `std`'s `SipHash` output is explicitly not
//! guaranteed stable across Rust releases, so every toolchain bump would have
//! re-seeded every sigil in existence.
//!
//! # Why comparisons go through text, not through structures
//!
//! **`serde_json` does not round-trip every `f64` exactly.** Writing
//! `927.9171087042969` and reading it back yields `927.9171087042968` — one unit
//! in the last place low. The writer is correct; the parser mis-rounds.
//!
//! That is load-bearing for the parity requirement in ADR 0005. "Native scene
//! JSON equals browser scene JSON" cannot be checked by deserializing both and
//! comparing the structures, because deserialization is the step that loses
//! information. Comparisons compare **canonical text**, which is produced from
//! the live `f64` values on each side and never parsed back.
//!
//! `float_round_trip_is_not_exact_which_is_why_text_is_canonical` pins this. If
//! `serde_json` ever fixes it, that test fails and this note can go — but the
//! text-comparison discipline should stay regardless, since it costs nothing and
//! removes a class of question.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt;

use crate::graph::SigilGraph;

/// A stable identity for a graph's meaning: 32 lowercase hex characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphFingerprint(String);

impl GraphFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first 64 bits, as the seed for the deterministic PRNG.
    ///
    /// Reading the seed off the fingerprint rather than hashing again is what
    /// makes "the default seed is the graph" literally true, and it means a
    /// user who copies the fingerprint out of a render's metadata can reproduce
    /// that render exactly.
    pub fn seed(&self) -> u64 {
        let mut bytes = [0u8; 8];
        for (i, byte) in bytes.iter_mut().enumerate() {
            let start = i * 2;
            *byte = u8::from_str_radix(&self.0[start..start + 2], 16).unwrap_or(0);
        }
        u64::from_be_bytes(bytes)
    }
}

impl fmt::Display for GraphFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for GraphFingerprint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for GraphFingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(GraphFingerprint(String::deserialize(d)?))
    }
}

/// Keys stripped before fingerprinting, wherever they appear.
///
/// A flat name list rather than paths, because every one of these means the same
/// thing at every depth and a path list would have to grow an entry each time
/// the shape nests differently.
const NOT_SEMANTIC: &[&str] = &[
    "source_name",
    "source_length",
    "producer",
    "producer_version",
    "snippet",
];

/// The canonical JSON text of a graph, for hashing and for golden tests.
pub fn canonical_json(graph: &SigilGraph) -> String {
    let value = serde_json::to_value(graph).unwrap_or(Value::Null);
    let mut buf = String::new();
    write_canonical(&value, &mut buf);
    buf
}

/// The canonical JSON text with the non-semantic keys removed. What is hashed.
pub fn semantic_json(graph: &SigilGraph) -> String {
    let mut value = serde_json::to_value(graph).unwrap_or(Value::Null);
    strip(&mut value);
    let mut buf = String::new();
    write_canonical(&value, &mut buf);
    buf
}

/// The fingerprint of a graph's meaning.
pub fn fingerprint(graph: &SigilGraph) -> GraphFingerprint {
    fingerprint_of_bytes(semantic_json(graph).as_bytes())
}

/// The same hash over arbitrary bytes, so a caller can fingerprint a scene or a
/// set of render options with the one documented algorithm.
pub fn fingerprint_of_bytes(bytes: &[u8]) -> GraphFingerprint {
    let digest = Sha256::digest(bytes);
    GraphFingerprint(hex::encode(&digest[..16]))
}

fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in NOT_SEMANTIC {
                map.remove(*key);
            }
            for (_, child) in map.iter_mut() {
                strip(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip),
        _ => {}
    }
}

/// Write JSON with sorted keys, no whitespace, and stable numbers.
fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&canonical_number(n)),
        Value::String(s) => write_json_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // `serde_json::Map` preserves insertion order unless the
            // `preserve_order` feature is off, and the workspace does not set
            // it — but sorting here rather than relying on that is the point:
            // canonical form must not depend on a feature flag somewhere else
            // in the dependency graph.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

/// Numbers, without the ways JSON lets the same value be spelled differently.
///
/// An integer-valued float is written as an integer (`3`, not `3.0`), because a
/// producer that writes `3.0` and one that writes `3` mean the same graph.
/// Non-finite values cannot reach here — validation rejects them — but a `null`
/// fallback is safer than a panic in a serializer.
fn canonical_number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    match n.as_f64() {
        Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => {
            format!("{}", f as i64)
        }
        Some(f) if f.is_finite() => {
            // Rust's shortest round-tripping representation. Documented here
            // because "whatever `{}` does" is exactly the kind of thing that
            // silently differs between a native and a wasm32 build if it is not
            // pinned to a specification — `f64`'s `Display` is, and `ryu`-style
            // shortest-round-trip is what it guarantees.
            let mut s = format!("{f}");
            if s.contains('e') || s.contains('E') {
                // Exponent form is legal JSON but has spellings; normalize to
                // plain decimal so two producers cannot differ.
                s = format!("{f:.17}");
                while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
                    s.pop();
                }
            }
            s
        }
        _ => "null".to_string(),
    }
}

/// A JSON string with the escapes JSON requires and no others.
///
/// Not `serde_json`'s, because this must be stable independently of how that
/// crate chooses to escape a character it is allowed to escape either way.
fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A deterministic PRNG for ornament and mark variation.
///
/// SplitMix64: sixty-four bits of state, one multiply-xor-shift round, and a
/// fully specified update — the same sequence on every platform, every compiler
/// and every future version of this crate, which is what ADR 0005's parity
/// requirement needs and what `rand`'s `StdRng` explicitly does not promise
/// across its own releases.
///
/// It is not cryptographic and does not need to be. What it needs to be is
/// *reproducible*, and to have no correlation between adjacent seeds — two
/// nodes whose IDs differ by one character must not get near-identical marks.
/// SplitMix64's avalanche gives that.
#[derive(Debug, Clone)]
pub struct Prng {
    state: u64,
}

impl Prng {
    pub fn new(seed: u64) -> Self {
        Prng { state: seed }
    }

    /// A generator derived from this one and a label, for giving each node its
    /// own independent stream without threading a counter through the layout.
    ///
    /// Two nodes with different IDs get uncorrelated streams; the same node gets
    /// the same stream every time, whatever order the nodes were visited in.
    /// That last property is why this exists: a per-node stream that depended on
    /// visit order would make mark generation depend on iteration order.
    pub fn derive(&self, label: &str) -> Prng {
        let mut hasher = Sha256::new();
        hasher.update(self.state.to_be_bytes());
        hasher.update(label.as_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        Prng::new(u64::from_be_bytes(bytes))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A float in `[0, 1)`.
    ///
    /// Built from 53 bits so every representable `f64` in the range is
    /// reachable and the result is exactly reproducible — a division by
    /// `2^53` is exact in binary floating point, so this cannot differ by a
    /// rounding step between two platforms.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// An integer in `[low, high)`. Returns `low` for an empty range rather
    /// than panicking: this runs over untrusted graph data, and a renderer that
    /// aborts on a degenerate range is a renderer a malformed graph can crash.
    pub fn range(&mut self, low: i64, high: i64) -> i64 {
        if high <= low {
            return low;
        }
        let span = (high - low) as u64;
        low + (self.next_u64() % span) as i64
    }

    /// `true` with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.next_f64() < p
    }

    /// One of `items`, or `None` when empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            return None;
        }
        items.get(self.range(0, items.len() as i64) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{SigilNode, SigilNodeKind, SourceLanguage};

    fn graph() -> SigilGraph {
        let mut g = SigilGraph::new(SourceLanguage::Cant, "n0");
        g.nodes.push(SigilNode::new("n0", SigilNodeKind::Source));
        g.nodes.push(SigilNode::new("n1", SigilNodeKind::Stage));
        g.exits.push("n1".into());
        g
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"b":1,"a":{"z":2,"y":[{"q":3,"p":4}]}}"#).unwrap();
        let mut out = String::new();
        write_canonical(&value, &mut out);
        assert_eq!(out, r#"{"a":{"y":[{"p":4,"q":3}],"z":2},"b":1}"#);
    }

    /// The property everything else rests on.
    #[test]
    fn the_same_graph_fingerprints_the_same_way_every_time() {
        let a = fingerprint(&graph());
        let b = fingerprint(&graph());
        assert_eq!(a, b);
        assert_eq!(a.as_str().len(), 32);
        assert!(a.as_str().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_different_graph_fingerprints_differently() {
        let mut other = graph();
        other.nodes.push(SigilNode::new("n2", SigilNodeKind::Ward));
        assert_ne!(fingerprint(&graph()), fingerprint(&other));
    }

    /// The exclusions are the whole point of a *semantic* fingerprint: rename
    /// the file, ship a new `cant`, and the artifact must not change.
    #[test]
    fn non_semantic_metadata_does_not_change_the_fingerprint() {
        let base = fingerprint(&graph());

        let mut renamed = graph();
        renamed.metadata.source_name = Some("somewhere/else.cant".into());
        renamed.metadata.source_length = Some(4096);
        assert_eq!(fingerprint(&renamed), base, "a file name is not a program");

        let mut republished = graph();
        republished.metadata.producer = Some("cant".into());
        republished.metadata.producer_version = Some("99.0.0".into());
        assert_eq!(
            fingerprint(&republished),
            base,
            "a producer release that changed no graph must not re-seed every render"
        );

        let mut with_snippet = graph();
        with_snippet.nodes[0].source = Some(crate::graph::SourceRef {
            span: rite_core::Span::from_range(0, 4),
            snippet: Some("[1, 2]".into()),
        });
        let mut with_other_snippet = graph();
        with_other_snippet.nodes[0].source = Some(crate::graph::SourceRef {
            span: rite_core::Span::from_range(0, 4),
            snippet: Some("[3, 4]".into()),
        });
        assert_eq!(
            fingerprint(&with_snippet),
            fingerprint(&with_other_snippet),
            "the snippet is display text; the span is the position"
        );
    }

    /// A span *is* semantic — moving a stage moves the program.
    #[test]
    fn a_span_change_does_change_the_fingerprint() {
        let mut a = graph();
        a.nodes[0].source = Some(crate::graph::SourceRef {
            span: rite_core::Span::from_range(0, 4),
            snippet: None,
        });
        let mut b = graph();
        b.nodes[0].source = Some(crate::graph::SourceRef {
            span: rite_core::Span::from_range(10, 14),
            snippet: None,
        });
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    /// Pins the defect the module docs describe. Not a wish — a measurement.
    #[test]
    fn float_round_trip_is_not_exact_which_is_why_text_is_canonical() {
        let value: f64 = 927.9171087042969;
        let text = serde_json::to_string(&value).expect("serializes");
        assert_eq!(text, "927.9171087042969", "the writer is not the problem");
        let back: f64 = serde_json::from_str(&text).expect("parses");
        assert_ne!(
            back.to_bits(),
            value.to_bits(),
            "serde_json now round-trips this exactly — the note in the module \
             documentation about comparing canonical text can be revisited"
        );

        // And the point: canonical text of the *same* value is stable, which is
        // what parity comparisons actually rely on.
        let a = canonical_number(&serde_json::Number::from_f64(value).expect("finite"));
        let b = canonical_number(&serde_json::Number::from_f64(value).expect("finite"));
        assert_eq!(a, b);
    }

    #[test]
    fn integers_and_integral_floats_canonicalize_alike() {
        let a: serde_json::Value = serde_json::from_str(r#"{"n":3}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"n":3.0}"#).unwrap();
        let (mut sa, mut sb) = (String::new(), String::new());
        write_canonical(&a, &mut sa);
        write_canonical(&b, &mut sb);
        assert_eq!(sa, sb, "3 and 3.0 are the same number");
    }

    #[test]
    fn strings_escape_exactly_what_json_requires() {
        let mut out = String::new();
        write_json_string("a\"b\\c\nd\te\u{1}", &mut out);
        assert_eq!(out, "\"a\\\"b\\\\c\\nd\\te\\u0001\"");
        // A raw control character in the output would be invalid JSON, and this
        // runs over untrusted labels, so the escape is a correctness property
        // rather than a formatting preference.
        assert!(!out.chars().any(|c| (c as u32) < 0x20));
    }

    /// The seed is read out of the fingerprint, so a user who copies a
    /// fingerprint from a render's metadata can reproduce that render.
    #[test]
    fn the_seed_is_the_first_64_bits_of_the_fingerprint() {
        let fp = fingerprint(&graph());
        let expected = u64::from_be_bytes(
            hex::decode(&fp.as_str()[..16]).expect("hex")[..8]
                .try_into()
                .expect("8 bytes"),
        );
        assert_eq!(fp.seed(), expected);
        assert_eq!(fp.seed(), fingerprint(&graph()).seed());
    }

    #[test]
    fn the_prng_is_reproducible_and_uncorrelated_across_labels() {
        let a: Vec<u64> = (0..8).map(|_| Prng::new(42).next_u64()).collect();
        assert!(
            a.windows(2).all(|w| w[0] == w[1]),
            "same seed, same first draw"
        );

        let mut p = Prng::new(42);
        let first: Vec<u64> = (0..8).map(|_| p.next_u64()).collect();
        let mut q = Prng::new(42);
        let second: Vec<u64> = (0..8).map(|_| q.next_u64()).collect();
        assert_eq!(first, second);

        // Adjacent labels must not produce near-identical streams: two nodes
        // named `n1` and `n2` would otherwise get marks nobody can tell apart.
        let root = Prng::new(42);
        let x = root.derive("n1").next_u64();
        let y = root.derive("n2").next_u64();
        assert_ne!(x, y);
        assert!(
            (x ^ y).count_ones() > 16,
            "adjacent labels gave correlated streams: {} differing bits",
            (x ^ y).count_ones()
        );
    }

    #[test]
    fn deriving_does_not_depend_on_visit_order() {
        let root = Prng::new(7);
        let in_one_order = [root.derive("a").next_u64(), root.derive("b").next_u64()];
        let mut reversed = [root.derive("b").next_u64(), root.derive("a").next_u64()];
        reversed.reverse();
        assert_eq!(in_one_order, reversed);
    }

    /// Runs over untrusted data, so a degenerate range returns rather than
    /// panics.
    #[test]
    fn a_degenerate_range_does_not_panic() {
        let mut p = Prng::new(1);
        assert_eq!(p.range(5, 5), 5);
        assert_eq!(p.range(5, 1), 5);
        assert_eq!(p.pick::<u8>(&[]), None);
    }

    #[test]
    fn floats_stay_in_the_unit_interval() {
        let mut p = Prng::new(99);
        for _ in 0..1000 {
            let f = p.next_f64();
            assert!((0.0..1.0).contains(&f), "{f} out of range");
        }
    }
}
