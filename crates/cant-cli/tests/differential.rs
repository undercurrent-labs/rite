//! The parity claim, checked: **`cant run` == `rite run <cant expand>`**.
//!
//! This is what ADR 0002 buys and what would silently stop being true first. If
//! expansion and execution ever disagree, `cant expand` becomes a lie — it would
//! print something other than what runs — and the audit story goes with it.
//!
//! Every execution fixture is run three ways and compared on **value, stdout,
//! normalized stderr, and exit code**:
//!
//! 1. `cant run case.cant`
//! 2. `cant expand case.cant > gen.rite` then `rite run gen.rite`
//! 3. `cant build case.cant` and the produced binary — `#[ignore]`d, because each
//!    one is a cold `cargo` build of a generated crate and takes minutes. Run
//!    with `cargo test -p cant-cli --test differential -- --ignored`, the same
//!    convention `rite-compiler`'s heavy tests use.
//!
//! Stderr is normalized rather than compared verbatim: `cant run` reports a
//! failure as a Cant diagnostic pointing at `.cant`, and `rite run` reports the
//! same failure as a Rite one pointing at generated code. Requiring those to
//! match would be requiring the remapping *not* to work. What must match is
//! whether each failed, and with what status.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-cli has two ancestors")
        .to_path_buf()
}

fn cant_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cant"))
}

fn rite_bin() -> Option<PathBuf> {
    let path = cant_bin()
        .parent()
        .expect("bin dir")
        .join(if cfg!(windows) { "rite.exe" } else { "rite" });
    path.is_file().then_some(path)
}

struct Case {
    dir: PathBuf,
    name: String,
    expected_exit: i32,
    /// The value the program should print, or `None` when it prints nothing.
    expected_value: Option<String>,
    /// Permission flags from `permissions.toml`.
    allow: Vec<String>,
}

/// Read `permissions.toml`.
///
/// One key, `allow = ["…", …]`. Hand-parsed for the same reason every other
/// small manifest in this tree is: the workspace has no `toml` dependency, and
/// one array is not worth acquiring one.
fn read_permissions(path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("allow"))
        .flat_map(|l| l.split('"').skip(1).step_by(2).map(str::to_string))
        .collect()
}

fn cases() -> Vec<Case> {
    let root = repo_root().join("conformance/cant/execution");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no execution fixtures");

    dirs.into_iter()
        .map(|dir| {
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?")
                .to_string();
            let expected_exit = std::fs::read_to_string(dir.join("expected.exit"))
                .unwrap_or_else(|e| panic!("{name}: expected.exit: {e}"))
                .trim()
                .parse()
                .unwrap_or_else(|e| panic!("{name}: expected.exit is not a number: {e}"));
            let expected_value = std::fs::read_to_string(dir.join("expected.value"))
                .ok()
                .map(|v| v.trim_end().to_string())
                .filter(|v| !v.is_empty());
            let allow = read_permissions(&dir.join("permissions.toml"));
            Case {
                dir,
                name,
                expected_exit,
                expected_value,
                allow,
            }
        })
        .collect()
}

struct Run {
    stdout: String,
    stderr: String,
    exit: i32,
}

impl Run {
    fn of(output: Output) -> Self {
        Self {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit: output.status.code().unwrap_or(-1),
        }
    }

    /// Did it fail, and how? The comparable part of stderr.
    fn failed(&self) -> bool {
        self.exit != 0
    }
}

/// Run a fixture through `cant run`, from inside its own directory so relative
/// paths in the program resolve the way they would for a user.
fn cant_run(case: &Case) -> Run {
    let mut command = Command::new(cant_bin());
    command.current_dir(&case.dir).arg("run").arg("case.cant");
    for allow in &case.allow {
        command.arg("--allow").arg(allow);
    }
    Run::of(command.output().expect("cant run"))
}

fn cant_expand(case: &Case) -> String {
    let output = Command::new(cant_bin())
        .current_dir(&case.dir)
        .args(["expand", "case.cant"])
        .output()
        .expect("cant expand");
    assert!(
        output.status.success(),
        "{}: cant expand failed: {}",
        case.name,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn rite_run(case: &Case, rite: &Path) -> Option<Run> {
    let generated = case
        .dir
        .join(".rite")
        .join("cant")
        .join("differential.rite");
    std::fs::create_dir_all(generated.parent().expect("parent")).expect("mkdir");
    std::fs::write(&generated, cant_expand(case)).expect("write expansion");

    let mut command = Command::new(rite);
    command.current_dir(&case.dir).arg("run").arg(&generated);
    for allow in &case.allow {
        command.arg("--allow").arg(allow);
    }
    let run = Run::of(command.output().expect("rite run"));
    let _ = std::fs::remove_dir_all(case.dir.join(".rite"));
    Some(run)
}

// ---- the fixtures agree with themselves

#[test]
fn every_fixture_produces_what_it_says_it_will() {
    let mut failures = Vec::new();
    for case in cases() {
        let run = cant_run(&case);
        if run.exit != case.expected_exit {
            failures.push(format!(
                "{}: exit {}, expected {}\n  stderr: {}",
                case.name,
                run.exit,
                case.expected_exit,
                run.stderr.trim()
            ));
            continue;
        }
        let value = run.stdout.trim_end().to_string();
        let expected = case.expected_value.clone().unwrap_or_default();
        if value != expected {
            failures.push(format!(
                "{}: value {value:?}, expected {expected:?}",
                case.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- the parity claim

#[test]
fn cant_run_agrees_with_rite_run_over_the_expansion() {
    let Some(rite) = rite_bin() else {
        println!(
            "note: the `rite` binary is not built, so parity was not checked. \
             `cargo build -p rite-cli` closes this gap."
        );
        return;
    };

    let mut failures = Vec::new();
    for case in cases() {
        let direct = cant_run(&case);
        let Some(expanded) = rite_run(&case, &rite) else {
            continue;
        };

        if direct.stdout != expanded.stdout {
            failures.push(format!(
                "{}: stdout differs\n  cant: {:?}\n  rite: {:?}",
                case.name, direct.stdout, expanded.stdout
            ));
        }
        if direct.exit != expanded.exit {
            failures.push(format!(
                "{}: exit differs — cant {} vs rite {}\n  cant stderr: {}\n  rite stderr: {}",
                case.name,
                direct.exit,
                expanded.exit,
                direct.stderr.trim(),
                expanded.stderr.trim()
            ));
        }
        // Stderr is compared by *outcome*, not text: the two report the same
        // failure in different vocabularies on purpose.
        if direct.failed() != expanded.failed() {
            failures.push(format!(
                "{}: one failed and the other did not\n  cant: {}\n  rite: {}",
                case.name,
                direct.stderr.trim(),
                expanded.stderr.trim()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) where `cant run` and `rite run` disagree:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Capability denial has to behave identically, since it is enforced entirely by
/// Rite in both paths.
#[test]
fn a_denied_capability_fails_the_same_way_both_ways() {
    let Some(rite) = rite_bin() else {
        println!("note: the `rite` binary is not built; denial parity was not checked.");
        return;
    };
    let case = cases()
        .into_iter()
        .find(|c| c.name == "capability-denied")
        .expect("the capability-denied fixture");

    let direct = cant_run(&case);
    let expanded = rite_run(&case, &rite).expect("rite run");
    assert_eq!(direct.exit, 5, "cant: {}", direct.stderr);
    assert_eq!(expanded.exit, 5, "rite: {}", expanded.stderr);
    // And the two vocabularies: Cant names the code, Rite names the permission.
    assert!(direct.stderr.contains("CANT-R002"), "{}", direct.stderr);
    assert!(
        expanded.stderr.contains("permission denied"),
        "{}",
        expanded.stderr
    );
}

/// A Cant failure must never show the user a generated identifier, even when the
/// failure happened inside generated scaffolding.
#[test]
fn no_fixture_leaks_a_generated_name_into_its_diagnostics() {
    for case in cases() {
        let run = cant_run(&case);
        assert!(
            !run.stderr.contains("cant_") && !run.stderr.contains("<generated>"),
            "{} leaked generated code into its output:\n{}",
            case.name,
            run.stderr
        );
    }
}

/// Effects execute in the order the program says, and that order is the same on
/// both paths.
#[test]
fn ordered_effects_match_between_the_two_paths() {
    let Some(rite) = rite_bin() else {
        println!("note: the `rite` binary is not built; effect ordering was not checked.");
        return;
    };
    let dir = std::env::temp_dir().join("cant-differential-effects");
    std::fs::create_dir_all(&dir).expect("temp dir");
    // Fork branches run left to right, and scatter preserves list order, so the
    // printed sequence is fully determined.
    let source = "[1, 2, 3] -> * -> |{ !@console.println($) ; $ * 10 } -> []\n";
    std::fs::write(dir.join("case.cant"), source).expect("write");

    let case = Case {
        dir: dir.clone(),
        name: "ordered-effects".into(),
        expected_exit: 0,
        expected_value: None,
        allow: Vec::new(),
    };
    let direct = cant_run(&case);
    let expanded = rite_run(&case, &rite).expect("rite run");

    assert_eq!(direct.exit, 0, "{}", direct.stderr);
    assert_eq!(direct.stdout, expanded.stdout, "effect order differs");
    // The order itself, not just that the two agree about it.
    let printed: Vec<&str> = direct
        .stdout
        .lines()
        .filter(|l| ["1", "2", "3"].contains(l))
        .collect();
    assert_eq!(printed, vec!["1", "2", "3"], "{}", direct.stdout);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `:par` fork's *value* is deterministic and identical on both paths, and so
/// is its console output.
///
/// The output part is not a promise Cant arranges: `parallel` gives each branch
/// its own output buffer and splices them back in branch order, so two branches
/// printing cannot interleave. What a `:par` fork does not order is effects that
/// reach the world directly — a file written, a host called — and there is
/// nothing here to assert about those beyond that they happen.
#[test]
fn a_parallel_fork_agrees_between_the_two_paths() {
    let Some(rite) = rite_bin() else {
        println!("note: the `rite` binary is not built; parallel parity was not checked.");
        return;
    };
    let dir = std::env::temp_dir().join("cant-differential-parallel");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let source = "[1, 2] -> * -> |{ !@console.println($) ; $ * 10 } :par -> []\n";
    std::fs::write(dir.join("case.cant"), source).expect("write");

    let case = Case {
        dir: dir.clone(),
        name: "parallel-effects".into(),
        expected_exit: 0,
        expected_value: None,
        allow: Vec::new(),
    };
    let direct = cant_run(&case);
    let expanded = rite_run(&case, &rite).expect("rite run");

    assert_eq!(direct.exit, 0, "{}", direct.stderr);
    assert_eq!(direct.stdout, expanded.stdout, "the two paths disagree");
    // Branch order, per scattered value: the fork's first branch prints, its
    // second multiplies, and the collected list interleaves them accordingly.
    let printed: Vec<&str> = direct
        .stdout
        .lines()
        .filter(|l| ["1", "2"].contains(l))
        .collect();
    assert_eq!(printed, vec!["1", "2"], "{}", direct.stdout);
    assert!(
        direct.stdout.contains("[none, 10, none, 20]"),
        "the value is not joined in branch order: {}",
        direct.stdout
    );

    // And running it again produces exactly the same bytes.
    let again = cant_run(&case);
    assert_eq!(direct.stdout, again.stdout, "a second run differed");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- compiled parity (slow)

/// `cant run` == the binary `cant build` produces.
///
/// Ignored by default: each fixture is a cold `cargo` build of a generated
/// crate, minutes apiece. `cargo test -p cant-cli --test differential -- --ignored`
/// — the convention `rite-compiler`'s heavy tests already use.
#[test]
#[ignore = "each fixture is a cold cargo build; run with --ignored"]
fn cant_run_agrees_with_the_compiled_binary() {
    let mut failures = Vec::new();
    for case in cases() {
        // A program that is *supposed* to fail is not build-compatible in any
        // useful sense — the interesting comparison is a successful run.
        if case.expected_exit != 0 {
            continue;
        }
        let output = Command::new(cant_bin())
            .current_dir(&case.dir)
            .args(["build", "case.cant"])
            .args(case.allow.iter().flat_map(|a| ["--allow", a]))
            .output()
            .expect("cant build");
        if !output.status.success() {
            failures.push(format!(
                "{}: cant build failed: {}",
                case.name,
                String::from_utf8_lossy(&output.stderr)
            ));
            continue;
        }
        let binary = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|l| l.strip_prefix("built ").map(str::to_string))
            .expect("cant build names the binary it produced");

        let compiled = Run::of(
            Command::new(&binary)
                .current_dir(&case.dir)
                .output()
                .expect("the compiled binary"),
        );
        let direct = cant_run(&case);
        if compiled.stdout != direct.stdout || compiled.exit != direct.exit {
            failures.push(format!(
                "{}: compiled {:?}/{} vs interpreted {:?}/{}",
                case.name, compiled.stdout, compiled.exit, direct.stdout, direct.exit
            ));
        }
        let _ = std::fs::remove_dir_all(case.dir.join(".rite"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
