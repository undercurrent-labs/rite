//! `@mcp.connect` over stdio, with a real subprocess on the other end.
//!
//! The server is `rite run` on a second script, which is exactly how a stdio MCP
//! server is launched in the field. That needs the CLI binary, which is why this test
//! lives here rather than beside the HTTP-transport ones in
//! `crates/rite-caps/tests/mcp_client.rs`: `CARGO_BIN_EXE_rite` guarantees cargo has
//! built it, where a hand-rolled `target/debug/rite` lookup only hopes so.

use std::path::{Path, PathBuf};
use std::process::Command;

const RITE: &str = env!("CARGO_BIN_EXE_rite");

const SERVER: &str = r#"
! @mcp.serve "calculator" ⟦
  tool "add" "Add two numbers" |a: int, b: int| ⟦
    ^ a + b
  ⟧

  tool "stats" "Summarise a list of numbers" |xs: [int]| ⟦
    ^ ⟨count: xs → count, total: xs → sum⟩
  ⟧

  tool "slow" "Reports progress before answering" ⟦
    ! @mcp.progress(0.5, "halfway")
    ^ "finished"
  ⟧

  resource "config://app" "App config" ⟦
    ^ "debug=true"
  ⟧
⟧
"#;

fn write(dir: &Path, name: &str, source: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, source).expect("write script");
    path
}

/// Run a client script and return its stdout, failing loudly with stderr attached.
///
/// `name` is per test: cargo runs these in parallel threads of one process, and two
/// tests writing the same `client.rite` had one of them assert against the other's
/// output. The server script is per test for a nastier version of the same race:
/// `fs::write` truncates before it writes, so a shared `server.rite` could be
/// half-written at the moment another test's freshly-spawned server read it, and
/// that server died at startup — "server closed the connection", sometimes.
fn run_client(name: &str, source: &str, allow: &[&str]) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-client-stdio");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let server = write(&dir, &format!("server-{name}"), SERVER);
    let client = write(
        &dir,
        name,
        &source
            .replace("{RITE}", RITE)
            .replace("{SERVER}", server.to_str().unwrap()),
    );

    let mut cmd = Command::new(RITE);
    cmd.args(["run", client.to_str().unwrap()]);
    for grant in allow {
        cmd.args(["--allow", grant]);
    }
    let out = cmd.output().expect("spawn rite");
    assert!(
        out.status.success(),
        "client failed ({}): {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// The whole surface, against a server started as a subprocess.
#[test]
fn a_script_drives_a_subprocess_server() {
    let out = run_client(
        "drives.rite",
        r#"
c ← ! @mcp.connect(⟨command: "{RITE}", args: ["run", "{SERVER}"]⟩)?
tools ← ! @mcp.tools(c)?
! @console.println("tools: " + str(tools → count))
! @console.println("first: " + (tools → first).name)
! @console.println("sum: " + str(! @mcp.call_tool(c, "add", ⟨a: 20, b: 22⟩)?))
stats ← ! @mcp.call_tool(c, "stats", ⟨xs: [1, 2, 3]⟩)?
! @console.println("total: " + str(stats.total))
! @console.println("config: " + (! @mcp.read_resource(c, "config://app")?))
! @mcp.close(c)
"#,
        &["process"],
    );

    assert!(out.contains("tools: 3"), "{out}");
    assert!(out.contains("first: add"), "{out}");
    assert!(out.contains("sum: 42"), "{out}");
    assert!(out.contains("total: 6"), "{out}");
    assert!(out.contains("config: debug=true"), "{out}");
}

/// A tool that reports progress writes a `notifications/progress` line *before* its
/// result. The client's read loop must skip it and keep reading rather than mistake it
/// for the answer — the failure mode is a decoded value of `none`.
#[test]
fn a_progress_notification_does_not_become_the_result() {
    let out = run_client(
        "progress.rite",
        r#"
c ← ! @mcp.connect(⟨command: "{RITE}", args: ["run", "{SERVER}"]⟩)?
! @console.println("slow: " + (! @mcp.call_tool(c, "slow", ⟨⟩)?))
! @mcp.close(c)
"#,
        &["process"],
    );
    assert!(out.contains("slow: finished"), "{out}");
}

/// Starting a server is running a program of the caller's choosing.
#[test]
fn a_stdio_connect_without_the_process_grant_exits_5() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-client-stdio");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let server = write(&dir, "server.rite", SERVER);
    let client = write(
        &dir,
        "denied.rite",
        &format!(
            r#"! @mcp.connect(⟨command: "{RITE}", args: ["run", "{}"]⟩)?"#,
            server.to_str().unwrap()
        ),
    );

    let out = Command::new(RITE)
        .args(["run", client.to_str().unwrap()])
        .output()
        .expect("spawn rite");
    // 5 is the permission-denied status, and it is part of the CLI's contract.
    assert_eq!(out.status.code(), Some(5), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("process"), "{stderr}");
}
