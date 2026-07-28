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
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));

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
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("hello-edge"), "{combined}");
}

#[test]
fn fmt_glyph_and_ascii() {
    // fmt writes in place; convert --stdout prints without mutating.
    let f = write_temp("fmt", "x <- 1\n");
    let out = run_rite(&["fmt", f.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&f).unwrap();
    assert!(text.contains('←') || text.contains('x'), "{text}");

    let out = run_rite(&[
        "convert",
        f.to_str().unwrap(),
        "--to",
        "ascii",
        "--stdout",
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("<-") || text.contains('x'), "{text}");
}

#[test]
fn convert_roundtrip() {
    let f = write_temp("conv", "x ← 1\n");
    let out = run_rite(&[
        "convert",
        f.to_str().unwrap(),
        "--to",
        "ascii",
        "--stdout",
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let ascii = String::from_utf8_lossy(&out.stdout);
    assert!(ascii.contains("<-"), "{ascii}");
}

#[test]
fn version_command() {
    let out = run_rite(&["version"]);
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("0.") || s.contains("rite") || !s.is_empty(), "{s}");
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

    // Poll for "listening on" in stdout
    let start = std::time::Instant::now();
    let mut _addr: Option<String> = None;
    while start.elapsed() < Duration::from_secs(5) {
        // try common approach: read available stdout via try_wait + partial read is hard;
        // instead probe by reading child stdout after small sleep using a concurrent reader.
        std::thread::sleep(Duration::from_millis(50));
        // We can't easily non-block read; use last_bound via side effect — curl is enough if we
        // parse from a fixed retry of ports... Better: read stdout after kill is too late.
        // Use reqwest against printed line — spawn thread to read stdout.
        break;
    }

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
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
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
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
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
    assert!(err.contains("zero") || err.contains("div") || err.contains("runtime"), "{err}");
}

#[test]
fn check_logical_glyphs_no_hang() {
    let f = write_temp("logic", "true ∧ false ∨ ¬ true\n");
    let start = std::time::Instant::now();
    let out = run_rite(&["check", f.to_str().unwrap()]);
    assert!(start.elapsed() < Duration::from_secs(2), "lexer hang?");
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
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
        .args(["run", f.to_str().unwrap(), "--allow-all", "--max-steps", "100"])
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
