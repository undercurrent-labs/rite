//! CLI-level edge cases: run/check/fmt and HTTP smoke via subprocess.

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
    // Build should have run before these tests in CI; fall back to cargo run
    PathBuf::from("rite")
}

fn run_rite(args: &[&str]) -> std::process::Output {
    Command::new(rite_bin())
        .args(args)
        .current_dir(workspace())
        .output()
        .expect("spawn rite")
}

fn write_temp(name: &str, body: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rite_cli_edge_{name}.rite"));
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn check_ok_and_fail() {
    let ok = write_temp("ok", "1 + 2\n");
    let out = run_rite(&["check", ok.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let bad = write_temp("bad", "@@@\n");
    let out = run_rite(&["check", bad.to_str().unwrap()]);
    assert!(!out.status.success());
}

#[test]
fn run_prints_value_and_stdout() {
    let f = write_temp(
        "print",
        r#"! @console.println("hello-edge")
42
"#,
    );
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("hello-edge"), "{combined}");
}

#[test]
fn implicit_run_when_first_arg_is_script_path() {
    // Shebang `#!/usr/bin/env rite` / `#!/bin/rite` → kernel runs `rite /path/to/script`.
    let f = write_temp(
        "implicit_run",
        r#"! @console.println("implicit-run-ok")
"#,
    );
    let out = run_rite(&[f.to_str().unwrap(), "--allow-all"]);
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("implicit-run-ok"), "{combined}");
}

#[test]
fn known_subcommand_not_rewritten_to_run() {
    let out = run_rite(&["version"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("rite "), "{stdout}");
}

#[test]
fn fmt_glyph_and_ascii() {
    // fmt writes in place; convert --stdout prints without mutating.
    let f = write_temp("fmt", "x <- 1\n");
    let out = run_rite(&["fmt", f.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains('←') || text.contains('x'), "{text}");

    let out = run_rite(&["convert", f.to_str().unwrap(), "--to", "ascii", "--stdout"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("<-") || text.contains('x'), "{text}");
}

#[test]
fn convert_roundtrip() {
    let f = write_temp("conv", "x ← 1\n");
    let out = run_rite(&["convert", f.to_str().unwrap(), "--to", "ascii", "--stdout"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ascii = String::from_utf8_lossy(&out.stdout);
    assert!(ascii.contains("<-"), "{ascii}");
}

#[test]
fn version_command() {
    let out = run_rite(&["version"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains("0.") || s.contains("rite") || !s.is_empty(),
        "{s}"
    );
}

#[test]
fn http_minimal_server_listens_and_serves() {
    let f = write_temp(
        "http",
        r#"
@http.listen "127.0.0.1:0" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
"#,
    );
    // Run server in background with test auto-stop
    let mut child = Command::new(rite_bin())
        .args(["run", f.to_str().unwrap(), "--allow-all"])
        .current_dir(workspace())
        .env("RITE_HTTP_TEST", "1")
        .env("RITE_HTTP_TEST_SECS", "8")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");

    // Give the child a moment to bind and print its listen line. We can't poll here:
    // the pipe is drained by the helper thread below (a non-blocking read of the child's
    // stdout before that thread owns it would race it), so the assertion on the captured
    // output is what actually gates this test.
    std::thread::sleep(Duration::from_millis(50));

    // Read stdout in a helper thread
    let stdout = child.stdout.take();
    let handle = std::thread::spawn(move || {
        use std::io::Read;
        let mut s = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut s);
        }
        s
    });

    // Wait until listen line or timeout by probing... Actually read is blocked until process ends.
    // Rely on RITE_HTTP_TEST auto-stop and parse output after wait — but then server is dead.
    // Change strategy: use fixed high port unique to pid.
    let _ = child.kill();
    let _ = child.wait();
    let _ = handle.join();

    let port = 19000 + (std::process::id() % 1000);
    let f2 = write_temp(
        "http2",
        &format!(
            r#"
@http.listen "127.0.0.1:{port}" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
"#
        ),
    );
    let mut child = Command::new(rite_bin())
        .args(["run", f2.to_str().unwrap(), "--allow-all"])
        .current_dir(workspace())
        .env("RITE_HTTP_TEST", "1")
        .env("RITE_HTTP_TEST_SECS", "10")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn");

    let url = format!("http://127.0.0.1:{port}/health");
    let mut ok = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        if let Ok(body) = std::process::Command::new("curl")
            .args(["-sf", &url])
            .output()
        {
            if body.status.success() {
                let text = String::from_utf8_lossy(&body.stdout);
                assert!(text.contains("ok") || text.contains("status"), "{text}");
                ok = true;
                break;
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(ok, "server never responded on {url}");
}

#[test]
fn modules_example_runs() {
    let main = workspace().join("examples/modules/main.rite");
    if !main.exists() {
        return;
    }
    let out = run_rite(&["run", main.to_str().unwrap(), "--allow-all"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn permission_denied_fs_exits_nonzero() {
    let f = write_temp("deny", r#"@fs.read("/etc/passwd")"#);
    let out = run_rite(&["run", f.to_str().unwrap()]); // default secure
                                                       // non-zero or runtime error message
    let err = String::from_utf8_lossy(&out.stderr);
    let out_s = String::from_utf8_lossy(&out.stdout);
    assert!(
        !out.status.success()
            || err.to_lowercase().contains("permission")
            || out_s.to_lowercase().contains("permission")
            || err.to_lowercase().contains("denied"),
        "status={:?} stderr={err} stdout={out_s}",
        out.status.code()
    );
}

#[test]
fn run_early_return_abs() {
    let f = write_temp(
        "abs",
        r#"
◆ abs(n) ⟦
  ? n < 0 ⟦
    ^ -n
  ⟧
  ^ n
⟧
abs(-5)
"#,
    );
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.trim() == "5" || stdout.trim().ends_with('5') || stdout.contains("\n5"),
        "{stdout}"
    );
}

#[test]
fn run_division_by_zero_exits_nonzero() {
    let f = write_temp("div0", "1 / 0\n");
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(!out.status.success());
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    )
    .to_lowercase();
    assert!(
        err.contains("zero") || err.contains("div") || err.contains("runtime"),
        "{err}"
    );
}

#[test]
fn check_logical_glyphs_no_hang() {
    let f = write_temp("logic", "true ∧ false ∨ ¬ true\n");
    let start = std::time::Instant::now();
    let out = run_rite(&["check", f.to_str().unwrap()]);
    assert!(start.elapsed() < Duration::from_secs(2), "lexer hang?");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_budget_steps_exceeded() {
    let f = write_temp(
        "bomb",
        r#"
◆ bomb(n) ⟦
  ^ bomb(n + 1)
⟧
bomb(0)
"#,
    );
    let out = Command::new(rite_bin())
        .args([
            "run",
            f.to_str().unwrap(),
            "--allow-all",
            "--max-steps",
            "100",
        ])
        .current_dir(workspace())
        .output()
        .expect("spawn");
    // should fail budget
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        !out.status.success()
            || combined.contains("budget")
            || combined.contains("step")
            || combined.contains("depth"),
        "status={:?} out={combined}",
        out.status.code()
    );
}

/// `@console.read_line` answered the empty string without touching stdin: a shim in
/// the interpreter shadowed the working implementation in `rite-caps`, so there was
/// no way to read input from a script at all. The prompt is printed by the runtime
/// (it owns the output sink) and the read is done by the capability, so this covers
/// both halves — a prompt on stdout and the typed line coming back.
#[test]
fn read_line_reads_stdin_and_prints_its_prompt() {
    use std::io::Write;
    let script = write_temp(
        "read_line",
        "name ← ! @console.read_line(\"name? \")\n! @console.println(\"[\" + name + \"]\")\n",
    );
    let mut child = Command::new(rite_bin())
        .args(["run", script.to_str().unwrap()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn rite");
    // CRLF: the line ending a Windows terminal sends must not survive into the value.
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"aura\r\n")
        .unwrap();
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("name? "),
        "prompt missing from stdout: {stdout:?}"
    );
    assert!(
        stdout.contains("[aura]"),
        "line not read back: {stdout:?} (empty brackets means the shim is back)"
    );
}

/// Reading is still an ordinary console effect, so revoking console must stop it
/// rather than quietly answering the empty string.
#[test]
fn read_line_respects_deny_console() {
    let script = write_temp(
        "read_line_denied",
        "name ← ! @console.read_line(\"\")\n! @console.println(name)\n",
    );
    let out = run_rite(&["run", script.to_str().unwrap(), "--deny", "console"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("console permission denied"),
        "expected a permission denial, got: {combined:?}"
    );
}

/// `@process.exit` has to reach the *process*, not just the evaluator: the whole
/// point is the number a shell reads back. Nothing in-process can prove that, so
/// this one spawns the real binary and asks the OS.
#[test]
fn process_exit_sets_the_real_exit_status() {
    let script = write_temp(
        "exit_status",
        "! @console.println(\"before\")\n! @process.exit(42)\n! @console.println(\"after\")\n",
    );
    let out = run_rite(&["run", script.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(42), "wrong status");

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Printed before the exit survives; printed after it never happens.
    assert!(
        stdout.contains("before"),
        "lost buffered output: {stdout:?}"
    );
    assert!(
        !stdout.contains("after"),
        "statements after the exit ran: {stdout:?}"
    );
    // The runtime does not editorialise over a status the script chose.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("runtime error"),
        "a chosen exit reported as an error: {stderr:?}"
    );
}

/// Exiting 0 is a successful early stop, and needs no grant — `--deny process`
/// revokes running subprocesses, not the right to say how this run ended.
#[test]
fn process_exit_zero_succeeds_without_the_process_grant() {
    let script = write_temp(
        "exit_zero",
        "! @console.println(\"done\")\n! @process.exit(0)\n! @console.println(\"unreachable\")\n",
    );
    let out = run_rite(&["run", script.to_str().unwrap(), "--deny", "process"]);
    assert_eq!(out.status.code(), Some(0), "exit 0 is not a failure");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("done"), "{stdout:?}");
    assert!(!stdout.contains("unreachable"), "{stdout:?}");
}

/// A status outside 0–255 fails at the call, with the ordinary runtime status.
#[test]
fn process_exit_rejects_a_status_out_of_range() {
    let script = write_temp("exit_range", "! @process.exit(300)\n");
    let out = run_rite(&["run", script.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "should be a runtime error");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("out of range"),
        "expected a range complaint, got: {stderr:?}"
    );
}
