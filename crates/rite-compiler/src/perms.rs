//! Bake a `PermissionSet` into the generated wrapper crate.
//!
//! A compiled binary must enforce the same capability model as `rite run`; the
//! grants handed to `build_script` are therefore emitted as Rust source that
//! reconstructs the exact same `PermissionSet` at startup.
//!
//! ## Path semantics
//!
//! `PermissionSet::grant` canonicalizes `fs:read` / `fs:write` / `db` grants at
//! grant time, so by the time we see them they are absolute *build machine*
//! paths, and the original spec string (`./out`) is gone. Baking absolutes into a
//! distributable binary is wrong for the common case: `--allow fs:write=./out`
//! means "./out wherever the binary runs".
//!
//! So a grant that lands under the build directory is re-emitted **relative** to
//! it and re-resolved against the CWD of the compiled binary; a grant outside the
//! build directory is baked **absolute**, because that is what the user named.
//!
//! Known limitation: because the pre-canonicalization spec is not retained by
//! `PermissionSet`, an *explicitly absolute* grant that happens to point inside
//! the build directory (`--allow fs:read=/home/me/proj/data` while building in
//! `/home/me/proj`) is indistinguishable from `--allow fs:read=./data` and is
//! treated as relative.

use rite_caps::PermissionSet;
use std::path::Path;

/// Rust source for `rite_perms()`, the baked capability grants, plus the
/// `grant_path` helper it needs.
pub(crate) fn emit_permission_set(perms: &PermissionSet, build_cwd: &Path) -> String {
    let mut out = String::new();
    out.push_str(
        r#"/// Capability grants baked in at `rite build` time.
///
/// Compiled binaries enforce the same capability model as `rite run`: this is the
/// exact `PermissionSet` the `--allow` / `--allow-all` flags produced at build time.
/// Path grants given relative to the build directory are re-resolved against the
/// CWD of *this* binary (`--allow fs:write=./out` means "./out wherever this binary
/// runs"); grants named absolutely stay absolute.
fn rite_perms() -> PermissionSet {
    let mut p = PermissionSet::default();
"#,
    );
    out.push_str(&format!("    p.allow_all = {};\n", perms.allow_all));
    out.push_str(&format!("    p.console = {};\n", perms.console));
    out.push_str(&format!("    p.clock = {};\n", perms.clock));
    out.push_str(&format!("    p.random = {};\n", perms.random));
    out.push_str(&format!("    p.process = {};\n", perms.process));
    out.push_str(&format!("    p.env_all = {};\n", perms.env_all));
    out.push_str(&format!("    p.db_memory = {};\n", perms.db_memory));
    if !perms.env_vars.is_empty() {
        out.push_str(&format!(
            "    p.env_vars = [{}].into_iter().map(String::from).collect();\n",
            string_list(sorted(&perms.env_vars))
        ));
    }
    if !perms.net.is_empty() {
        out.push_str(&format!(
            "    p.net = [{}].into_iter().map(String::from).collect();\n",
            string_list(sorted(&perms.net))
        ));
    }
    for (field, paths) in [
        ("fs_read", &perms.fs_read),
        ("fs_write", &perms.fs_write),
        ("db_paths", &perms.db_paths),
    ] {
        if paths.is_empty() {
            continue;
        }
        let items: Vec<String> = paths
            .iter()
            .map(|p| format!("grant_path({:?})", portable_path(p, build_cwd)))
            .collect();
        out.push_str(&format!("    p.{} = vec![{}];\n", field, items.join(", ")));
    }
    out.push_str(
        r#"    p
}

/// Re-resolve a baked grant against the CWD of this process.
#[allow(dead_code)]
fn grant_path(spec: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(spec);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p)
    }
}
"#,
    );
    out
}

/// The permission set as recorded in `rite-manifest.json`, in the same
/// (portable) form that is baked into the binary.
pub(crate) fn permissions_json(perms: &PermissionSet, build_cwd: &Path) -> serde_json::Value {
    let paths = |v: &[std::path::PathBuf]| -> Vec<String> {
        v.iter().map(|p| portable_path(p, build_cwd)).collect()
    };
    serde_json::json!({
        "allow_all": perms.allow_all,
        "console": perms.console,
        "clock": perms.clock,
        "random": perms.random,
        "process": perms.process,
        "env_all": perms.env_all,
        "env_vars": sorted(&perms.env_vars),
        "net": sorted(&perms.net),
        "fs_read": paths(&perms.fs_read),
        "fs_write": paths(&perms.fs_write),
        "db_memory": perms.db_memory,
        "db_paths": paths(&perms.db_paths),
    })
}

/// Grants under the build directory become relative (re-resolved at the binary's
/// runtime CWD); everything else stays absolute. See the module docs.
fn portable_path(path: &Path, build_cwd: &Path) -> String {
    match path.strip_prefix(build_cwd) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

fn sorted(set: &std::collections::HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = set.iter().cloned().collect();
    v.sort();
    v
}

/// Rust source for a list of string literals, properly escaped.
fn string_list(items: Vec<String>) -> String {
    items
        .iter()
        .map(|s| format!("{:?}", s))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rite_caps::Permission;
    use std::path::PathBuf;

    #[test]
    fn default_secure_bakes_no_blanket_grant() {
        let src = emit_permission_set(&PermissionSet::default_secure(), Path::new("/build"));
        assert!(src.contains("p.allow_all = false;"), "{}", src);
        assert!(src.contains("p.console = true;"));
        assert!(src.contains("p.process = false;"));
        assert!(!src.contains("PermissionSet::allow_all()"));
        // No grants: no path vectors emitted at all.
        assert!(!src.contains("p.fs_read"));
        assert!(!src.contains("p.fs_write"));
    }

    #[test]
    fn allow_all_is_preserved() {
        let src = emit_permission_set(&PermissionSet::allow_all(), Path::new("/build"));
        assert!(src.contains("p.allow_all = true;"), "{}", src);
        assert!(src.contains("p.process = true;"));
    }

    #[test]
    fn grants_under_build_dir_are_relative_others_absolute() {
        let mut perms = PermissionSet::default_secure();
        perms.fs_read.push(PathBuf::from("/build/data"));
        perms.fs_write.push(PathBuf::from("/build/out/logs"));
        perms.db_paths.push(PathBuf::from("/srv/db"));
        let src = emit_permission_set(&perms, Path::new("/build"));
        assert!(
            src.contains(r#"p.fs_read = vec![grant_path("data")];"#),
            "{}",
            src
        );
        assert!(
            src.contains(r#"p.fs_write = vec![grant_path("out/logs")];"#),
            "{}",
            src
        );
        assert!(
            src.contains(r#"p.db_paths = vec![grant_path("/srv/db")];"#),
            "{}",
            src
        );
    }

    #[test]
    fn grant_of_build_dir_itself_becomes_dot() {
        let mut perms = PermissionSet::default_secure();
        perms.fs_read.push(PathBuf::from("/build"));
        let src = emit_permission_set(&perms, Path::new("/build"));
        assert!(src.contains(r#"grant_path(".")"#), "{}", src);
    }

    #[test]
    fn quotes_and_spaces_in_paths_are_escaped() {
        let mut perms = PermissionSet::default_secure();
        // A path with a space, a double quote and a backslash.
        perms.fs_read.push(PathBuf::from("/build/we\"ird dir\\sub"));
        perms.env_vars.insert("A\"B".to_string());
        let src = emit_permission_set(&perms, Path::new("/build"));
        assert!(src.contains(r#"grant_path("we\"ird dir\\sub")"#), "{}", src);
        assert!(src.contains(r#""A\"B""#), "{}", src);
    }

    #[test]
    fn env_and_net_sets_are_sorted_for_reproducible_output() {
        let mut perms = PermissionSet::default_secure();
        for p in ["env=B", "env=A", "net=z.example", "net=a.example"] {
            perms.grant(Permission::parse(p).unwrap());
        }
        let src = emit_permission_set(&perms, Path::new("/build"));
        assert!(
            src.contains(r#"p.env_vars = ["A", "B"].into_iter().map(String::from).collect();"#),
            "{}",
            src
        );
        assert!(
            src.contains(
                r#"p.net = ["a.example", "z.example"].into_iter().map(String::from).collect();"#
            ),
            "{}",
            src
        );
    }

    #[test]
    fn manifest_records_the_full_set() {
        let mut perms = PermissionSet::default_secure();
        perms.grant(Permission::parse("net=api.example").unwrap());
        perms.fs_write.push(PathBuf::from("/build/out"));
        let json = permissions_json(&perms, Path::new("/build"));
        assert_eq!(json["allow_all"], false);
        assert_eq!(json["net"][0], "api.example");
        assert_eq!(json["fs_write"][0], "out");
        assert_eq!(json["console"], true);
    }
}
