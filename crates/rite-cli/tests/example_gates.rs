//! Example scripts must keep running (docs/examples contract).
//! Requires `target/debug/rite` (built by CI before tests).

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rite_bin() -> PathBuf {
    let root = workspace();
    for rel in ["target/debug/rite", "target/release/rite"] {
        let p = root.join(rel);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("rite")
}

fn run_example(rel: &str, extra_env: &[(&str, &str)]) -> std::process::Output {
    let path = workspace().join(rel);
    assert!(path.is_file(), "missing example {}", path.display());
    let mut cmd = Command::new(rite_bin());
    cmd.args(["run", path.to_str().unwrap(), "--allow-all"])
        .current_dir(workspace())
        .env("RITE_HTTP_TEST", "1")
        .env("RITE_HTTP_TEST_SECS", "2");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn rite")
}

#[test]
fn sugar_demo_runs() {
    let out = run_example("examples/sugar/demo.rite", &[]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("evens") || stdout.contains("6") || stdout.contains("compose"),
        "{stdout}"
    );
}

#[test]
fn hello_examples_run() {
    for rel in [
        "examples/hello/hello.rite",
        "examples/hello/hello.ascii.rite",
    ] {
        let path = workspace().join(rel);
        if !path.is_file() {
            continue;
        }
        let out = run_example(rel, &[]);
        assert!(
            out.status.success(),
            "{rel} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn http_service_example_listens() {
    let path = workspace().join("examples/http-service/server.rite");
    if !path.is_file() {
        return;
    }
    let mut child = Command::new(rite_bin())
        .args(["run", path.to_str().unwrap(), "--allow-all"])
        .current_dir(workspace())
        .env("RITE_HTTP_TEST", "1")
        .env("RITE_HTTP_TEST_SECS", "6")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn http example");

    // Give server time to bind and print listen URL
    std::thread::sleep(Duration::from_millis(600));
    let _ = child.kill();
    let out = child.wait_with_output().expect("wait");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("listening") || combined.contains("127.0.0.1"),
        "http example should print listen URL; got: {combined}"
    );
}

#[test]
fn values_and_pipelines_examples_if_present() {
    for rel in [
        "examples/01-values/main.rite",
        "examples/02-pipelines/main.rite",
        "examples/04-pattern-matching/main.rite",
    ] {
        let path = workspace().join(rel);
        if !path.is_file() {
            continue;
        }
        let out = run_example(rel, &[]);
        assert!(
            out.status.success(),
            "{rel}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// Curated pure examples that must `rite check` cleanly (contract list).
#[test]
fn pure_examples_check_ok() {
    let must_check = [
        "examples/hello/hello.rite",
        "examples/hello/hello.ascii.rite",
        "examples/sugar/demo.rite",
        "examples/01-values/main.rite",
        "examples/02-pipelines/main.rite",
        "examples/04-pattern-matching/main.rite",
        "examples/http-service/server.rite",
    ];
    let mut checked = 0;
    for rel in must_check {
        let p = workspace().join(rel);
        if !p.is_file() {
            continue;
        }
        let parent = p.parent().unwrap().to_path_buf();
        let file_name = p.file_name().unwrap().to_str().unwrap();
        let out = Command::new(rite_bin())
            .args(["check", file_name])
            .current_dir(&parent)
            .output()
            .expect("check");
        assert!(
            out.status.success(),
            "check failed for {rel}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        checked += 1;
    }
    assert!(checked >= 3, "expected several examples to exist and check");
}
