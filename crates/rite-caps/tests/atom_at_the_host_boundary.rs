//! An atom crossing a capability boundary carries its name.
//!
//! `Display for Value` cannot resolve an atom and renders it as its interner index. That
//! was fixed in the builtins (`str`, `join`, `panic`) but the capabilities had their own
//! copies of the same mistake — and `@fs.write` is the worst place for it, because the
//! wrong bytes land on the user's disk rather than on a screen where someone might notice.

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
