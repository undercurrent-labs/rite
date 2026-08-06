//! `@proto` beyond what the conformance fixtures reach.
//!
//! The fixtures cover the round trip, the enum-to-atom mapping and the error
//! shapes, running each case interpreted and through IR. What is left here is the
//! part a fixture cannot express: schema handles that span two pools, imports
//! between files, a descriptor set produced by `protoc` rather than compiled in
//! process, and the refusal a build without the `proto` feature answers with.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};

async fn run(src: &str) -> String {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    match run_source("t.rite", src, &mut ctx).await {
        Ok(v) => v.to_display(&ctx.atoms),
        Err(e) => format!("error: {e}"),
    }
}

#[cfg(feature = "proto")]
/// A schema literal for a Rite source string. `.proto` is mostly braces, and a
/// plain Rite string interpolates them, so every brace is doubled — which is the
/// reason `@proto.load_file` is the form the book leads with.
fn schema(body: &str) -> String {
    format!("\"{}\"", body.replace('{', "{{").replace('}', "}}"))
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn a_schema_handle_is_scoped_to_its_own_pool() {
    // Two schemas, each with a message the other does not define. A handle names
    // one pool; it does not become a global registry of every message compiled.
    let a = schema("syntax = \\\"proto3\\\"; package a; message Alpha { int64 n = 1; }");
    let b = schema("syntax = \\\"proto3\\\"; package b; message Beta { string s = 1; }");
    let src = format!(
        "sa ← ! @proto.compile({a})?\nsb ← ! @proto.compile({b})?\n\
         ^ [is_err(@proto.decode(sa, \"b.Beta\", bytes(\"\"))), \
            is_ok(@proto.decode(sb, \"b.Beta\", bytes(\"\")))]\n"
    );
    assert_eq!(run(&src).await, "[true, true]");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn a_value_that_is_not_a_handle_is_refused() {
    let src = "^ is_err(@proto.decode(\"not a handle\", \"demo.User\", bytes(\"\")))\n";
    assert_eq!(run(src).await, "true");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn compile_all_resolves_imports_between_sources() {
    // The in-memory resolver has to serve `common.proto` when `main.proto` imports
    // it. A resolver that only knew the file it was asked for would fail here.
    let common = "syntax = \\\"proto3\\\"; package common; message Id { int64 n = 1; }";
    let main = "syntax = \\\"proto3\\\"; import \\\"common.proto\\\"; package main; \
                message Thing { common.Id id = 1; }";
    let src = format!(
        "files ← ⟨\"common.proto\": {}, \"main.proto\": {}⟩\n\
         s ← ! @proto.compile_all(files)?\n\
         body ← @proto.encode(s, \"main.Thing\", ⟨id: ⟨n: 5⟩⟩)?\n\
         ^ @proto.decode(s, \"main.Thing\", body)?.id.n\n",
        schema(common),
        schema(main)
    );
    assert_eq!(run(&src).await, "5");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn a_missing_import_answers_err_rather_than_panicking() {
    let main = "syntax = \\\"proto3\\\"; import \\\"absent.proto\\\"; message T { int64 n = 1; }";
    let src = format!("r ← ! @proto.compile({})\n^ is_err(r)\n", schema(main));
    assert_eq!(run(&src).await, "true");
}

#[cfg(feature = "proto")]
/// The bytes `protoc --descriptor_set_out` produces, for
/// `message Ping { int64 n = 1; }` in package `demo`. Generated once by
/// compiling that source and encoding the pool, then pasted here: the point is
/// that this path never runs the `.proto` compiler at all.
const PING_FDS_HEX: &str = "0a9b010a0a64656d6f2e70726f746f120464656d6f22140a0450696e67120c0a016e18012001280352016e4a690a05120300003e0a080a010212030013200a090a020400120300213e0a0a0a03040001120300292d0a0b0a0404000200120300303c0a0c0a05040002000112030036370a0c0a0504000200031203003a3b0a0c0a05040002000512030030350a080a010c1203000012620670726f746f33";

#[cfg(feature = "proto")]
#[tokio::test]
async fn load_set_accepts_a_precompiled_descriptor_set() {
    let src = format!(
        "s ← ! @proto.load_set(from_hex(\"{PING_FDS_HEX}\")?)?\n\
         body ← @proto.encode(s, \"demo.Ping\", ⟨n: 42⟩)?\n\
         ^ [@proto.messages(s)?, @proto.decode(s, \"demo.Ping\", body)?.n]\n"
    );
    assert_eq!(run(&src).await, "[[demo.Ping], 42]");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn load_set_rejects_bytes_that_are_not_a_descriptor_set() {
    let src = "^ is_err(! @proto.load_set(from_hex(\"ffffff\")?))\n";
    assert_eq!(run(src).await, "true");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn nested_messages_round_trip_as_nested_records() {
    let body = "syntax = \\\"proto3\\\"; package demo; \
                message Inner { string s = 1; } \
                message Outer { Inner inner = 1; repeated Inner many = 2; }";
    let src = format!(
        "s ← ! @proto.compile({})?\n\
         b ← @proto.encode(s, \"demo.Outer\", ⟨inner: ⟨s: \"a\"⟩, many: [⟨s: \"b\"⟩, ⟨s: \"c\"⟩]⟩)?\n\
         back ← @proto.decode(s, \"demo.Outer\", b)?\n\
         ^ [back.inner.s, back.many]\n",
        schema(body)
    );
    assert_eq!(run(&src).await, "[a, [⟨s: b⟩, ⟨s: c⟩]]");
}

#[cfg(feature = "proto")]
#[tokio::test]
async fn a_wrongly_typed_field_answers_err_rather_than_coercing() {
    let body = "syntax = \\\"proto3\\\"; package demo; message T { int64 n = 1; }";
    let src = format!(
        "s ← ! @proto.compile({})?\n^ is_err(@proto.encode(s, \"demo.T\", ⟨n: \"not a number\"⟩))\n",
        schema(body)
    );
    assert_eq!(run(&src).await, "true");
}

/// An enum number with no name in this schema is a variant a newer `.proto`
/// added. Decoding it to the raw number keeps the message readable instead of
/// failing on a field the program may not even look at.
#[cfg(feature = "proto")]
#[tokio::test]
async fn an_unknown_enum_number_decodes_to_its_number() {
    let body = "syntax = \\\"proto3\\\"; package demo; \
                enum E { ZERO = 0; ONE = 1; } message T { E e = 1; }";
    let src = format!(
        "s ← ! @proto.compile({})?\n\
         ^ [@proto.decode(s, \"demo.T\", from_hex(\"0801\")?)?.e, \
            @proto.decode(s, \"demo.T\", from_hex(\"0863\")?)?.e]\n",
        schema(body)
    );
    assert_eq!(run(&src).await, "[#ONE, 99]");
}

/// Without the `proto` feature the capability is registered and refuses by name,
/// rather than answering "unknown capability `@proto`" — the same shape `@db`
/// uses when it is built without DuckDB.
#[cfg(not(feature = "proto"))]
#[tokio::test]
async fn a_build_without_the_feature_refuses_by_name() {
    let out = run("^ @proto.messages(\"x\")\n").await;
    assert!(
        out.contains("proto") && out.contains("feature"),
        "expected a refusal naming the feature, got {out:?}"
    );
}

/// Bytes for `message N { N next = 1; }` nested `depth` deep.
///
/// Built here rather than encoded from Rite because the point is to exceed a
/// depth Rite cannot conveniently build a record for.
#[cfg(feature = "proto")]
fn nested_bytes(depth: usize) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    for _ in 0..depth {
        let mut next = vec![0x0a]; // field 1, wire type 2
        let mut n = buf.len();
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                next.push(b);
                break;
            }
            next.push(b | 0x80);
        }
        next.extend_from_slice(&buf);
        buf = next;
    }
    buf
}

#[cfg(feature = "proto")]
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Protobuf sets no depth limit of its own, so a self-referencing message can be
/// nested as deeply as the sender likes. Decoding builds a Rite value per level,
/// so the ceiling bounds memory.
#[cfg(feature = "proto")]
#[tokio::test]
async fn a_message_nested_past_the_depth_limit_answers_err() {
    let body = "syntax = \\\"proto3\\\"; package demo; message N { N next = 1; }";
    let shallow = hex(&nested_bytes(8));
    let deep = hex(&nested_bytes(80));
    // The message, not just `is_err`: bytes this deep are still well-formed, so
    // an `err` for any other reason would pass a bare `is_err` check.
    let src = format!(
        "s ← ! @proto.compile({})?\n\
         deep ← @proto.decode(s, \"demo.N\", from_hex(\"{deep}\")?)\n\
         ^ [is_ok(@proto.decode(s, \"demo.N\", from_hex(\"{shallow}\")?)), \
            ~ deep ⟦ err e → contains(e.message, \"nests deeper\") ; _ → false ⟧]\n",
        schema(body)
    );
    assert_eq!(run(&src).await, "[true, true]");
}
