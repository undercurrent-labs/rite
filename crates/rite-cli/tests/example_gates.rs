//! Example scripts must keep running (docs/examples contract).
//! Requires `target/debug/rite` (built by CI before tests).

use std::io::BufRead;
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

/// The two child streams, so one reader loop can serve both.
enum Stream {
    Out(std::process::ChildStdout),
    Err(std::process::ChildStderr),
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

    // Wait for the line rather than sleeping a fixed interval and hoping. A single
    // 600ms sleep raced process startup and failed on a loaded macOS runner with empty
    // output — the server had not finished binding yet. Reading until the marker appears
    // is both faster when the machine is idle and reliable when it is not.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    for stream in [
        child.stdout.take().map(Stream::Out),
        child.stderr.take().map(Stream::Err),
    ]
    .into_iter()
    .flatten()
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let reader: Box<dyn std::io::Read + Send> = match stream {
                Stream::Out(o) => Box::new(o),
                Stream::Err(e) => Box::new(e),
            };
            for line in std::io::BufReader::new(reader)
                .lines()
                .map_while(Result::ok)
            {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut seen = String::new();
    let mut found = false;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                let hit = line.contains("listening") || line.contains("127.0.0.1");
                seen.push_str(&line);
                seen.push('\n');
                if hit {
                    found = true;
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            // Both streams closed: the process exited without printing.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        found,
        "http example should print listen URL within 20s; got: {seen}"
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
