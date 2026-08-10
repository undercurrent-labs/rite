//! An atom crossing a capability boundary carries its name.
//!
//! `Display for Value` cannot resolve an atom and renders it as its interner index. That
//! was fixed in the builtins (`str`, `join`, `panic`) but the capabilities had their own
//! copies of the same mistake — and `@fs.write` is the worst place for it, because the
//! wrong bytes land on the user's disk rather than on a screen where someone might notice.
//!
//! The encoders were missed in that pass and kept the bug for four releases:
//! `@json.encode(⟨tier: #PRO⟩)` answered `{"tier":"atom:0"}` and `@csv.encode` put
//! `atom:0` in the cell, because the dispatcher called them without the interner.
//! `@store` had it in its keys, where the index also collided with whatever else
//! interned first.

use rite_caps::{install_defaults, PermissionSet};
use rite_runtime::{run_source, RuntimeContext};

async fn run(src: &str) -> (String, RuntimeContext) {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let v = run_source("t.rite", src, &mut ctx).await;
    let out = match v {
        Ok(v) => v.to_display(&ctx.atoms),
        Err(e) => format!("error: {e}"),
    };
    (out, ctx)
}

#[tokio::test]
async fn fs_write_puts_the_atom_name_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.txt");
    // Raw string: a Windows path in an ordinary Rite string is a pile of bad escapes.
    let src = format!(
        "! @fs.write(r\"{}\", #ok)?\n! @fs.read(r\"{}\")?\n",
        path.display(),
        path.display()
    );
    let (out, _) = run(&src).await;
    assert_eq!(out, "#ok", "read back the wrong content");

    let raw = std::fs::read_to_string(&path).expect("read file");
    assert_eq!(raw, "#ok", "the bytes on disk were the interner index");
    assert!(!raw.contains("#0"), "wrote an interner index: {raw:?}");
}

#[tokio::test]
async fn fs_append_puts_the_atom_name_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("log.txt");
    let src = format!(
        "! @fs.append(r\"{}\", #first)?\n! @fs.append(r\"{}\", #second)?\n",
        path.display(),
        path.display()
    );
    let (_, _) = run(&src).await;
    let raw = std::fs::read_to_string(&path).expect("read file");
    assert_eq!(raw, "#first#second", "got {raw:?}");
}

#[tokio::test]
async fn console_prints_the_atom_name() {
    // This path goes through the capability rather than the evaluator's own console
    // special-case, so it needed the same fix separately.
    let (_, ctx) = run("! @console.println(#ready)\n").await;
    let printed = ctx.stdout.join("");
    assert!(
        printed.contains("#ready"),
        "console rendered an interner index: {printed:?}"
    );
}

#[tokio::test]
async fn a_non_atom_value_is_unaffected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("plain.txt");
    let src = format!("! @fs.write(r\"{}\", \"plain text\")?\n", path.display());
    run(&src).await;
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "plain text");
}

/// An atom encodes as its name, bare — in JSON it is a string, and `@json.decode`
/// reads it back as one. `Value::to_json` already chose that spelling.
#[tokio::test]
async fn json_encode_writes_the_atom_name() {
    let (out, _) = run(r#"@json.encode(⟨tier: #PRO, id: 7⟩)"#).await;
    assert_eq!(out, r#"{"id":7,"tier":"PRO"}"#, "got {out:?}");
    assert!(!out.contains("atom:"), "encoded an interner index: {out:?}");
}

/// Recursion, not just the top level: the bug survived one level down because
/// `value_to_serde` recursed into a copy of itself that had no interner either.
#[tokio::test]
async fn json_encode_reaches_nested_atoms() {
    let (out, _) = run(r#"@json.encode(⟨nested: [⟨s: #ok⟩], list: [#a, #b]⟩)"#).await;
    assert_eq!(
        out, r#"{"list":["a","b"],"nested":[{"s":"ok"}]}"#,
        "got {out:?}"
    );
}

#[tokio::test]
async fn json_encode_pretty_writes_the_atom_name() {
    let (out, _) = run(r#"@json.encode_pretty(⟨tier: #PRO⟩)"#).await;
    assert!(out.contains(r#""PRO""#), "got {out:?}");
    assert!(!out.contains("atom:"), "encoded an interner index: {out:?}");
}

/// The CSV cell is the `@fs.write` case with an extra layer: it lands on disk.
#[tokio::test]
async fn csv_encode_writes_the_atom_name() {
    let (out, _) = run(r#"@csv.encode([⟨tier: #PRO⟩, ⟨tier: #FREE⟩])?"#).await;
    assert_eq!(out, "tier\nPRO\nFREE\n", "got {out:?}");
}

/// Two atoms with different names must not collapse, which an index-based
/// spelling would only get right by luck.
#[tokio::test]
async fn distinct_atoms_stay_distinct_through_json() {
    let (out, _) = run(r#"@json.encode([#alpha, #beta, #alpha])"#).await;
    assert_eq!(out, r#"["alpha","beta","alpha"]"#, "got {out:?}");
}

/// `#PRO` and `"PRO"` are different keys. The `#` prefix is what keeps them
/// apart now that the key is the name rather than an index.
#[tokio::test]
async fn store_keys_separate_atoms_from_strings() {
    let src = concat!(
        "! @store.set(\"ns\", #PRO, 1)\n",
        "! @store.set(\"ns\", \"PRO\", 2)\n",
        "^ [@store.get(\"ns\", #PRO), @store.get(\"ns\", \"PRO\")]\n"
    );
    let (out, _) = run(src).await;
    assert_eq!(
        out, "[ok(1), ok(2)]",
        "atom and string keys collided: {out:?}"
    );
}

/// The key survives a round trip rather than merely being written: an index-based
/// key happened to do this too, which is why it went unnoticed for so long.
#[tokio::test]
async fn store_round_trips_an_atom_key() {
    let src = "! @store.set(\"ns\", #tier, #PRO)\n^ @store.get(\"ns\", #tier)\n";
    let (out, _) = run(src).await;
    assert_eq!(out, "ok(#PRO)", "got {out:?}");
}
