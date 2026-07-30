//! Commands must do what they print, and script output must never be lost.
//!
//! Each test here corresponds to a verified defect: buffered output discarded on
//! failure, `fmt` rewriting the whole tree by default, `docs serve` serving
//! nothing, `describe diagnostic` returning a canned string, `--timeout`
//! ignoring bad values, script arguments dropped on the floor.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

fn run_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(rite_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn rite")
}

fn run_rite(args: &[&str]) -> std::process::Output {
    run_in(&workspace(), args)
}

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rite_honesty_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn script(tag: &str, body: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("rite_honesty_{tag}_{}.rite", std::process::id()));
    std::fs::write(&p, body).unwrap();
    p
}

fn stdout_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

// ------------------------------------------------------- output on every path

#[test]
fn output_survives_a_runtime_error() {
    let f = script(
        "runtime_err",
        "! @console.println(\"important progress output\")\ny <- 1 / 0\n",
    );
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(!out.status.success(), "division by zero should fail");
    assert!(
        stdout_of(&out).contains("important progress output"),
        "output printed before the failure must survive it:\nstdout={}\nstderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    assert!(stderr_of(&out).to_lowercase().contains("zero"));
}

#[test]
fn output_survives_a_permission_error() {
    let f = script(
        "perm_err",
        "! @console.println(\"before-permission-denied\")\n! @fs.read(\"/etc/passwd\")\n",
    );
    // No --allow-all: default_secure denies the filesystem.
    let out = run_rite(&["run", f.to_str().unwrap()]);
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));
    assert!(
        stdout_of(&out).contains("before-permission-denied"),
        "{combined}"
    );
    assert!(
        combined.to_lowercase().contains("permission")
            || combined.to_lowercase().contains("denied"),
        "{combined}"
    );
}

#[test]
fn output_survives_a_budget_error() {
    // Straight-line arithmetic, not deep recursion: named-function recursion
    // overflows the native stack and aborts the process before any budget check
    // (a rite-runtime gap), which would test the abort rather than the budget.
    let mut body = String::from("! @console.println(\"before-budget-exceeded\")\nv0 <- 1\n");
    for i in 1..40 {
        body.push_str(&format!("v{i} <- v{} + 1\n", i - 1));
    }
    let f = script("budget_err", &body);
    let out = run_rite(&[
        "run",
        f.to_str().unwrap(),
        "--allow-all",
        "--max-steps",
        "10",
    ]);
    assert_eq!(out.status.code(), Some(8), "stderr={}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("before-budget-exceeded"),
        "stdout={} stderr={}",
        stdout_of(&out),
        stderr_of(&out)
    );
    // The specific budget error is reported, not a generic sentence.
    let err = stderr_of(&out).to_lowercase();
    assert!(
        err.contains("step") || err.contains("depth") || err.contains("timeout"),
        "{err}"
    );
}

#[test]
fn console_error_and_warn_reach_stderr() {
    let f = script(
        "stderr",
        "! @console.error(\"to-stderr-error\")\n! @console.warn(\"to-stderr-warn\")\n! @console.println(\"to-stdout\")\n",
    );
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("to-stderr-error"),
        "{}",
        stderr_of(&out)
    );
    assert!(
        stderr_of(&out).contains("to-stderr-warn"),
        "{}",
        stderr_of(&out)
    );
    assert!(stdout_of(&out).contains("to-stdout"));
}

#[test]
fn final_value_prints_even_when_the_script_also_printed() {
    let f = script(
        "value_and_output",
        "! @console.println(\"logged\")\n40 + 2\n",
    );
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    assert!(stdout.contains("logged"), "{stdout}");
    assert!(
        stdout.contains("42"),
        "final value must not disappear: {stdout}"
    );
}

#[test]
fn trace_reports_steps_and_outcome() {
    let f = script("trace", "! @console.println(\"traced\")\n1 + 1\n");
    let out = run_rite(&["run", f.to_str().unwrap(), "--allow-all", "--trace"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let err = stderr_of(&out);
    assert!(err.contains("trace: steps"), "{err}");
    assert!(err.contains("trace: outcome ok"), "{err}");
    assert!(stdout_of(&out).contains("traced"));
}

// ----------------------------------------------------------------- script args

/// Print the script's own arguments via the capability that exposes them.
fn run_argv_script(tag: &str, extra: &[&str]) -> String {
    let f = script(tag, "argv ← ! @process.args\n! @console.println(argv)\n");
    let mut args = vec!["run", f.to_str().unwrap()];
    args.extend_from_slice(extra);
    let out = run_rite(&args);
    assert!(
        out.status.success(),
        "argv script failed:\n{}{}",
        stdout_of(&out),
        stderr_of(&out)
    );
    stdout_of(&out)
}

#[test]
fn script_arguments_reach_the_script() {
    let stdout = run_argv_script("argv", &["--", "alpha", "beta"]);
    assert!(
        stdout.contains("alpha") && stdout.contains("beta"),
        "{stdout}"
    );
}

#[test]
fn no_arguments_still_yields_an_empty_list() {
    let stdout = run_argv_script("argv_empty", &[]);
    assert!(stdout.contains("[]"), "{stdout}");
}

/// Arguments are the invoker's own input to this program, so reading them needs no
/// grant — unlike `@process.run`, which spawns something new. This pins that
/// distinction: no `--allow` flags at all, and `--deny process` must not block it.
#[test]
fn reading_arguments_needs_no_permission() {
    let f = script(
        "argv_no_perm",
        "argv ← ! @process.args\n! @console.println(argv)\n",
    );
    let out = run_rite(&[
        "run",
        f.to_str().unwrap(),
        "--deny",
        "process",
        "--",
        "kept",
    ]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("kept"), "{}", stdout_of(&out));
}

/// ...but spawning still is gated.
#[test]
fn spawning_a_process_still_needs_permission() {
    let f = script(
        "argv_spawn",
        "r ← ! @process.run(\"echo\", [\"hi\"])\n! @console.println(str(r))\n",
    );
    let out = run_rite(&["run", f.to_str().unwrap()]);
    assert!(!out.status.success(), "{}", stdout_of(&out));
    assert!(
        stderr_of(&out).to_lowercase().contains("permission"),
        "{}",
        stderr_of(&out)
    );
}

// ------------------------------------------------------------------- timeouts

#[test]
fn invalid_timeout_is_an_error_not_a_default() {
    let f = script("timeout", "1 + 1\n");
    let out = run_rite(&["run", f.to_str().unwrap(), "--timeout", "soon"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr_of(&out));
    assert!(
        stderr_of(&out).contains("invalid --timeout"),
        "{}",
        stderr_of(&out)
    );
}

#[test]
fn valid_timeout_is_accepted() {
    let f = script("timeout_ok", "1 + 1\n");
    for value in ["500ms", "30s", "2m", "10"] {
        let out = run_rite(&["run", f.to_str().unwrap(), "--timeout", value]);
        assert!(out.status.success(), "{value}: {}", stderr_of(&out));
    }
}

// ------------------------------------------------------------------------ fmt

#[test]
fn fmt_without_paths_refuses_instead_of_rewriting_the_tree() {
    let dir = scratch("fmt_guard");
    let file = dir.join("a.rite");
    std::fs::write(&file, "x <- 1\n").unwrap();

    let out = run_in(&dir, &["fmt"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr_of(&out));
    assert!(stderr_of(&out).contains("--all"), "{}", stderr_of(&out));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "x <- 1\n",
        "bare `rite fmt` must not rewrite anything"
    );
}

#[test]
fn fmt_all_formats_the_tree_and_skips_build_dirs() {
    let dir = scratch("fmt_all");
    std::fs::write(dir.join("a.rite"), "x <- 1\n").unwrap();
    std::fs::create_dir_all(dir.join("target/debug")).unwrap();
    std::fs::write(dir.join("target/debug/skipme.rite"), "x <- 1\n").unwrap();
    std::fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::fs::write(dir.join("node_modules/skipme.rite"), "x <- 1\n").unwrap();

    let out = run_in(&dir, &["fmt", "--all"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert_eq!(
        std::fs::read_to_string(dir.join("target/debug/skipme.rite")).unwrap(),
        "x <- 1\n",
        "target/ must not be formatted"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("node_modules/skipme.rite")).unwrap(),
        "x <- 1\n",
        "node_modules/ must not be formatted"
    );
}

#[cfg(unix)]
#[test]
fn fmt_all_terminates_on_a_symlink_loop() {
    let dir = scratch("fmt_loop");
    std::fs::write(dir.join("a.rite"), "x <- 1\n").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::os::unix::fs::symlink(&dir, dir.join("sub/loop")).unwrap();

    let start = Instant::now();
    let out = run_in(&dir, &["fmt", "--all", "--check"]);
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "symlink loop should not be followed"
    );
    // Exit code is 0 or 1 depending on whether the file needs formatting; the
    // point is that it terminated rather than recursing forever.
    assert!(
        out.status.code() == Some(0) || out.status.code() == Some(1),
        "status={:?} stderr={}",
        out.status.code(),
        stderr_of(&out)
    );
}

#[test]
fn fmt_dry_run_reports_without_writing() {
    let dir = scratch("fmt_dry");
    let file = dir.join("a.rite");
    std::fs::write(&file, "x <- 1\n").unwrap();
    let before = std::fs::read_to_string(&file).unwrap();

    let out = run_in(&dir, &["fmt", "--dry-run", "a.rite"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    assert!(
        stdout_of(&out).contains("would change"),
        "{}",
        stdout_of(&out)
    );
}

// -------------------------------------------------------- describe diagnostic

#[test]
fn describe_diagnostic_returns_the_real_page() {
    let out = run_rite(&["describe", "diagnostic", "E020"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let stdout = stdout_of(&out);
    // Content from docs/diagnostics/E020.md, not a canned sentence.
    assert!(stdout.contains("Failing example"), "{stdout}");
    assert!(!stdout.contains("See IMPLEMENTATION.md"), "{stdout}");
    assert!(stdout.contains("E020.md"), "{stdout}");
}

#[test]
fn describe_diagnostic_json_carries_the_markdown() {
    let out = run_rite(&["describe", "diagnostic", "e21", "--json"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout_of(&out)).expect("json");
    assert_eq!(v["code"], serde_json::json!("E021"));
    assert_eq!(v["found"], serde_json::json!(true));
    assert!(
        v["markdown"].as_str().unwrap_or_default().contains("E021"),
        "{v}"
    );
}

#[test]
fn describe_diagnostic_404s_honestly() {
    let out = run_rite(&["describe", "diagnostic", "E999"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout_of(&out));
    assert!(
        stderr_of(&out).contains("no documentation page"),
        "{}",
        stderr_of(&out)
    );

    let out = run_rite(&["describe", "diagnostic", "not-a-code"]);
    assert_eq!(out.status.code(), Some(2));
}

// ------------------------------------------------------------------ docs serve

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn curl(args: &[&str]) -> (u32, String) {
    let out = Command::new("curl")
        .args(["-s", "-w", "\n%{http_code}"])
        .args(args)
        .output()
        .expect("curl");
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", "0"));
    (code.trim().parse().unwrap_or(0), body.to_string())
}

#[test]
fn docs_serve_actually_serves_files() {
    let root = scratch("docs_root");
    std::fs::write(root.join("index.html"), "<h1>rite docs index</h1>").unwrap();
    std::fs::create_dir_all(root.join("html")).unwrap();
    std::fs::write(root.join("html/page.html"), "<p>page body</p>").unwrap();
    // A file the server must never reach via a request path.
    std::fs::write(root.parent().unwrap().join("secret.txt"), "top secret").unwrap();

    let port = 25_000 + (std::process::id() as u16 % 500);
    let child = Command::new(rite_bin())
        .args([
            "docs",
            "serve",
            "--port",
            &port.to_string(),
            "--no-open",
            "--root",
            root.to_str().unwrap(),
        ])
        .current_dir(workspace())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn docs serve");
    let _guard = Server(child);

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut ready = false;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(ready, "docs serve never bound port {port}");

    let base = format!("http://127.0.0.1:{port}");
    let (code, body) = curl(&[&format!("{base}/")]);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("rite docs index"), "{body}");

    let (code, body) = curl(&[&format!("{base}/html/page.html")]);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("page body"), "{body}");

    // Path traversal (encoded so curl does not normalize it away).
    let (code, body) = curl(&[&format!("{base}/%2e%2e/secret.txt")]);
    assert_ne!(code, 200, "traversal must not be served: {body}");
    assert!(!body.contains("top secret"), "{body}");

    let (code, _) = curl(&[&format!("{base}/nope.html")]);
    assert_eq!(code, 404);
}

#[test]
fn docs_serve_without_generated_docs_fails_loudly() {
    let missing = scratch("docs_missing").join("not-generated");
    let out = run_rite(&[
        "docs",
        "serve",
        "--root",
        missing.to_str().unwrap(),
        "--no-open",
    ]);
    assert!(!out.status.success());
    assert!(
        stderr_of(&out).contains("rite docs build")
            || stderr_of(&out).contains("no generated docs"),
        "{}",
        stderr_of(&out)
    );
}

// ------------------------------------------------------------------ docs open

#[test]
fn docs_open_resolves_a_real_page() {
    let root = scratch("open_root");
    std::fs::create_dir_all(root.join("html")).unwrap();
    std::fs::write(root.join("html/index.html"), "<h1>index</h1>").unwrap();
    std::fs::write(root.join("http.md"), "# http").unwrap();

    let out = run_rite(&["docs", "open", "--root", root.to_str().unwrap(), "--print"]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(
        stdout_of(&out).contains("index.html"),
        "{}",
        stdout_of(&out)
    );

    let out = run_rite(&[
        "docs",
        "open",
        "http",
        "--root",
        root.to_str().unwrap(),
        "--print",
    ]);
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(stdout_of(&out).contains("http.md"), "{}", stdout_of(&out));
}

#[test]
fn docs_open_missing_page_reports_where_it_looked() {
    let root = scratch("open_empty");
    let out = run_rite(&[
        "docs",
        "open",
        "no-such-symbol",
        "--root",
        root.to_str().unwrap(),
        "--print",
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout_of(&out));
    assert!(
        stderr_of(&out).contains("looked for"),
        "{}",
        stderr_of(&out)
    );
}

// ------------------------------------------------------------- docs out paths

#[test]
fn docs_build_writes_only_where_told() {
    let dir = scratch("docs_build");
    let out_dir = dir.join("gen");
    let out = run_in(
        &dir,
        &[
            "docs",
            "build",
            "--out",
            out_dir.to_str().unwrap(),
            "--no-skill",
        ],
    );
    assert!(out.status.success(), "{}", stderr_of(&out));
    assert!(out_dir.is_dir(), "docs should be written to --out");
    // Nothing scribbled into the working directory.
    assert!(!dir.join("docs").exists(), "no ./docs in the cwd");
    assert!(!dir.join("skills").exists(), "no ./skills in the cwd");
}
