//! Effect-classification parity between the host descriptors and the checker.
//!
//! `rite-caps` publishes an `effectful:` flag on every
//! `NativeFunctionDescriptor` — that flag is what `rite capabilities`,
//! `rite describe capability --json` and the generated docs report. The
//! resolver decides independently whether a call needs `!` (E021), using
//! `rite_sem::resolve::HOST_EFFECTS`.
//!
//! Those two used to drift silently: whole capabilities (`@db.*`, `@csv.*`)
//! declared `effectful: true` and were never checked, so `rite check` accepted
//! `@db.exec(conn, "CREATE TABLE …")` with no marker at all. This test is the
//! thing that stops the next drift — it fails, in both directions, the moment
//! a descriptor and the table disagree.
//!
//! `HOST_EFFECTS` is the source of truth. When this fails, the fix is normally
//! to correct the descriptor; changing the table is a language-policy change
//! that also needs `docs/` and `examples/` updated.

use rite_caps::{HostCapabilities, PermissionSet};
use rite_sem::resolve::{is_effectful, HOST_EFFECTS};
use std::collections::BTreeMap;

/// Every `("cap.fn", effectful)` pair the host actually registers.
fn descriptor_effects() -> BTreeMap<String, bool> {
    let host = HostCapabilities::with_defaults(PermissionSet::default_secure());
    let mut out = BTreeMap::new();
    for (cap, descriptors) in host.all_descriptors() {
        for d in descriptors {
            let path = format!("{}.{}", cap, d.name);
            assert!(
                out.insert(path.clone(), d.effectful).is_none(),
                "duplicate descriptor for @{}",
                path
            );
        }
    }
    out
}

#[test]
fn every_descriptor_is_classified_by_the_resolver() {
    let descriptors = descriptor_effects();
    let mut missing = Vec::new();
    let mut disagree = Vec::new();

    let table: BTreeMap<&str, bool> = HOST_EFFECTS.iter().copied().collect();
    for (path, effectful) in &descriptors {
        match table.get(path.as_str()) {
            None => missing.push(format!(
                "  @{} (descriptor: effectful: {})",
                path, effectful
            )),
            Some(&classified) if classified != *effectful => disagree.push(format!(
                "  @{}: descriptor says effectful: {}, HOST_EFFECTS says {}",
                path, effectful, classified
            )),
            Some(_) => {}
        }
    }

    assert!(
        missing.is_empty(),
        "host functions absent from rite_sem::resolve::HOST_EFFECTS, so a missing `!` is \
         never diagnosed for them:\n{}\n\nAdd each to HOST_EFFECTS in \
         crates/rite-sem/src/resolve.rs.",
        missing.join("\n")
    );
    assert!(
        disagree.is_empty(),
        "descriptor `effectful:` flags disagree with rite_sem::resolve::HOST_EFFECTS:\n{}\n\n\
         HOST_EFFECTS is the source of truth; correct the descriptor unless the language \
         policy really changed.",
        disagree.join("\n")
    );
}

#[test]
fn table_has_no_entries_for_functions_the_host_does_not_have() {
    let descriptors = descriptor_effects();
    let stale: Vec<&str> = HOST_EFFECTS
        .iter()
        .map(|(path, _)| *path)
        .filter(|path| !descriptors.contains_key(*path))
        .collect();
    assert!(
        stale.is_empty(),
        "rite_sem::resolve::HOST_EFFECTS classifies host functions that no capability \
         registers (renamed or removed?): {:?}",
        stale
    );
}

/// The public predicate the resolver calls must agree with the table it reads,
/// so the two assertions above really do cover what E021 does.
#[test]
fn is_effectful_agrees_with_the_table() {
    for (path, effectful) in HOST_EFFECTS {
        assert_eq!(
            is_effectful(path),
            *effectful,
            "is_effectful(\"{}\") disagrees with its own HOST_EFFECTS entry",
            path
        );
    }
    // Unlisted paths are not diagnosed: an unknown capability is E042 at the
    // call site, and embedders may register capabilities rite-sem cannot see.
    assert!(!is_effectful("fs.no_such_function"));
    assert!(!is_effectful("totally_unknown.thing"));
    // Prefix matching used to make `is_effectful("console.anything")` true and
    // silently classify unregistered names. It must be an exact lookup.
    assert!(!is_effectful("console.printlnx"));
    assert!(!is_effectful("console."));
    assert!(!is_effectful("fs"));
}

/// Guards the specific regression this pair of tests was written for.
#[test]
fn previously_unenforced_capabilities_are_effectful() {
    for path in [
        // `@db.*` was entirely absent from the resolver's list.
        "db.open",
        "db.exec",
        "db.query",
        "db.prepare",
        "db.begin",
        "db.commit",
        "db.rollback",
        // `@csv.write` and the `@game`/`@store` mutators were absent too.
        "csv.write",
        "game.register_room",
        "game.command",
        "store.delete",
    ] {
        assert!(
            is_effectful(path),
            "@{} performs an external effect but is not classified effectful",
            path
        );
    }
}
