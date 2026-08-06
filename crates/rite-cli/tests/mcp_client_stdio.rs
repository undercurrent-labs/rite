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
    run_client_against(name, SERVER, source, allow)
}

/// `run_client`, with the server script named rather than assumed.
fn run_client_against(name: &str, server_source: &str, source: &str, allow: &[&str]) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mcp-client-stdio");
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let server = write(&dir, &format!("server-{name}"), server_source);
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

/// A server that speaks JSON-RPC by hand, so it can misbehave in the two ways
/// `@mcp.serve` never will. Written in Rite rather than node or python so the test
/// needs nothing installed: `@console.read_line` is stdin and `println` is the wire.
///
/// It writes a banner before the protocol starts and a warning mid-session, both on
/// stdout, and answers `tools/call` with a `roots/list` request numbered with the id
/// of the call it is answering.
const RUDE_SERVER: &str = r#"
! @console.println("Debugger listening on ws://127.0.0.1:9229")

line ↢ ! @console.read_line("")
while line != "" ⟦
  req ← @json.decode(line)?
  id ← req.id
  method ← req.method
  ? method = "server/discover" ⟦
    ! @console.println(@json.encode(⟨jsonrpc: "2.0", id: id, result: ⟨serverInfo: ⟨name: "rude", version: "1"⟩⟩⟩))
  ⟧
  ? method = "tools/call" ⟦
    ! @console.println(@json.encode(⟨jsonrpc: "2.0", id: id, method: "roots/list", params: ⟨⟩⟩))
    ! @console.println("warning: something happened")
    ! @console.println(@json.encode(⟨jsonrpc: "2.0", id: id, result: ⟨content: [⟨type: "text", text: "echoed"⟩], isError: false⟩⟩))
  ⟧
  line := ! @console.read_line("")
⟧
"#;

/// Servers started through `npx` announce themselves on stdout, and a line of that
/// noise used to fail the whole connection with "invalid JSON from server" — at
/// `connect`, before a single tool could be called.
///
/// A server request carrying `method` used to be matched on its id alone. The server's
/// id space starts at 1 exactly as the client's does, so `roots/list` numbered with the
/// in-flight call's id was decoded as that call's reply: `ok("")`, with the real result
/// left in the buffer for whatever asked next.
#[test]
fn banner_lines_and_server_requests_do_not_derail_a_call() {
    let out = run_client_against(
        "rude.rite",
        RUDE_SERVER,
        r#"
c ← ! @mcp.connect(⟨command: "{RITE}", args: ["run", "{SERVER}"]⟩)?
! @console.println("echo: " + str(! @mcp.call_tool(c, "echo", ⟨⟩)?))
! @mcp.close(c)
"#,
        &["process"],
    );
    assert!(out.contains("echo: echoed"), "{out}");
}

/// A tool's own failure record survives the round trip, both halves under test.
///
/// The server used to flatten `^ err(⟨kind, message⟩)` to its rendered text and send
/// that alone, so the client's `e.message` was the string
/// `⟨kind: bad_input, message: cannot divide by zero⟩` — every field gone, and the
/// sentence the tool wrote buried in a rendering of the record holding it.
#[test]
fn a_tools_failure_record_keeps_its_fields_across_the_round_trip() {
    const FAILING_SERVER: &str = r#"
! @mcp.serve "calculator" ⟦
  tool "divide" "Divide, refusing a zero divisor" |a: int, b: int| ⟦
    ? b = 0 ⟦
      ^ err(⟨kind: "bad_input", message: "cannot divide by zero"⟩)
    ⟧
    ^ a / b
  ⟧
⟧
"#;
    let out = run_client_against(
        "tool-error.rite",
        FAILING_SERVER,
        r#"
c ← ! @mcp.connect(⟨command: "{RITE}", args: ["run", "{SERVER}"]⟩)?
d ← ! @mcp.call_tool(c, "divide", ⟨a: 1, b: 0⟩)
~ d ⟦
  ok n → ! @console.println("unexpected ok: " + str(n))
  err e → ⟦
    ! @console.println("kind: " + e.kind)
    ! @console.println("tool: " + e.tool)
    ! @console.println("message: " + e.message)
    ! @console.println("data.kind: " + e.data.kind)
  ⟧
⟧
! @mcp.close(c)
"#,
        &["process"],
    );
    // `kind` names which of the four failures this is; the tool's own name for it is
    // in `data`.
    assert!(out.contains("kind: mcp.tool_error"), "{out}");
    assert!(out.contains("tool: divide"), "{out}");
    assert!(out.contains("message: cannot divide by zero"), "{out}");
    assert!(out.contains("data.kind: bad_input"), "{out}");
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
