//! End-to-end: a compiled binary must enforce the permissions it was built with.
//!
//! The heavy tests are `#[ignore]`d because a cold `cargo build` of a generated
//! crate compiles the whole Rite runtime (several minutes). Run them explicitly:
//!
//! ```text
//! cargo test -p rite-compiler -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `--test-threads=1` matters: these tests set env vars and the process CWD, both
//! of which are global. The fast assertions on the generated *source text* live in
//! unit tests in `src/lib.rs` and `src/perms.rs` and run on every `cargo test`.

use rite_caps::{Permission, PermissionSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rite-compiler-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn run_in(bin: &Path, cwd: &Path) -> Output {
    Command::new(bin)
        .current_dir(cwd)
        .output()
        .expect("run compiled binary")
}

#[test]
fn unusable_rite_source_dir_fails_up_front() {
    let dir = scratch("badsrc");
    let script = dir.join("hello.rite");
    std::fs::write(&script, "! @console.println(\"hi\")\n").unwrap();
    std::env::set_var("RITE_SOURCE_DIR", &dir); // exists, but is not a checkout
    let err = rite_compiler::build_script(&script, false, false, None, &PermissionSet::default())
        .expect_err("must not attempt a cargo build");
    std::env::remove_var("RITE_SOURCE_DIR");
    assert!(
        err.contains("not a Rite source checkout"),
        "unhelpful error: {}",
        err
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The security property: no `--allow` means denied, `--allow fs:read=./data`
/// means allowed, and the grant follows the binary's CWD rather than the build
/// machine's.
#[test]
#[ignore = "cold cargo build of the generated crate takes minutes; run with --ignored"]
fn compiled_binary_enforces_build_time_permissions() {
    let dir = scratch("perms");
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/secret.txt"), "here-1").unwrap();
    let script = dir.join("read.rite");
    std::fs::write(
        &script,
        "r <- ! @fs.read(\"data/secret.txt\")?\n! @console.println(\"read -> \" + r)\n",
    )
    .unwrap();

    // Build against this checkout, and keep every generated artifact out of it.
    std::env::set_var("RITE_SOURCE_DIR", repo_root());
    std::env::set_var("RITE_BUILD_DIR", dir.join("build"));
    // Use the shared cache target dir so the second build is a quick relink.
    std::env::remove_var("CARGO_TARGET_DIR");
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let denied_bin = dir.join("denied");
    rite_compiler::build_script(
        &script,
        false,
        false,
        Some(&denied_bin),
        &PermissionSet::default_secure(),
    )
    .expect("build with no grants");

    let mut granted = PermissionSet::default_secure();
    granted.grant(Permission::parse("fs:read=./data").unwrap());
    let granted_bin = dir.join("granted");
    rite_compiler::build_script(&script, false, false, Some(&granted_bin), &granted)
        .expect("build with fs:read grant");

    std::env::set_current_dir(&prev_cwd).unwrap();
    std::env::remove_var("RITE_SOURCE_DIR");
    std::env::remove_var("RITE_BUILD_DIR");

    let denied = run_in(&denied_bin, &dir);
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert_eq!(
        denied.status.code(),
        Some(5),
        "expected exit 5 (permission), got {:?}\nstderr: {}",
        denied.status.code(),
        stderr
    );
    assert!(
        stderr.contains("permission denied") && stderr.contains("fs:read"),
        "stderr: {}",
        stderr
    );
    assert!(
        !String::from_utf8_lossy(&denied.stdout).contains("here-1"),
        "unpermitted binary leaked file contents"
    );

    let allowed = run_in(&granted_bin, &dir);
    assert!(
        allowed.status.success(),
        "granted binary failed: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&allowed.stdout).contains("read -> here-1"),
        "stdout: {}",
        String::from_utf8_lossy(&allowed.stdout)
    );

    // The `./data` grant is relative to wherever the binary runs, not to the
    // build machine's directory.
    let elsewhere = scratch("perms-elsewhere");
    std::fs::create_dir_all(elsewhere.join("data")).unwrap();
    std::fs::write(elsewhere.join("data/secret.txt"), "here-2").unwrap();
    let moved = run_in(&granted_bin, &elsewhere);
    assert!(
        String::from_utf8_lossy(&moved.stdout).contains("read -> here-2"),
        "relative grant did not follow the runtime CWD.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&moved.stdout),
        String::from_utf8_lossy(&moved.stderr)
    );

    let _ = std::fs::remove_dir_all(&elsewhere);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Outside a Rite checkout the generated crate must resolve `rite-*` from git
/// instead of emitting path deps that point at nothing. Needs network.
#[test]
#[ignore = "needs network + a cold cargo build; run with --ignored"]
fn builds_outside_a_checkout_via_git_deps() {
    let dir = scratch("gitdeps");
    let script = dir.join("hello.rite");
    std::fs::write(&script, "! @console.println(\"hello from git deps\")\n").unwrap();

    std::env::remove_var("RITE_SOURCE_DIR");
    std::env::set_var("RITE_BUILD_DIR", dir.join("build"));
    std::env::remove_var("CARGO_TARGET_DIR");
    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let out_bin = dir.join("hellobin");
    let built = rite_compiler::build_script(
        &script,
        false,
        false,
        Some(&out_bin),
        &PermissionSet::default_secure(),
    );

    std::env::set_current_dir(&prev_cwd).unwrap();
    std::env::remove_var("RITE_BUILD_DIR");
    built.expect("git-dep build outside a checkout");

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::fs::read_dir(dir.join("build"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
                .join("rite-manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["deps"]["kind"], "git");

    let out = run_in(&out_bin, &dir);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello from git deps"),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
