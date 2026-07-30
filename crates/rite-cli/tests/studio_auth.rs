//! `rite studio` is an arbitrary-code-execution endpoint: these tests pin the
//! authentication, the DNS-rebinding guard, and the restricted-by-default
//! permission set.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
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

/// Kill the server even when an assertion unwinds.
struct Studio {
    child: Child,
    port: u16,
    token: String,
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start_studio(port: u16, extra: &[&str]) -> Studio {
    let mut child = Command::new(rite_bin())
        .args(["studio", "--port", &port.to_string(), "--no-open"])
        .args(extra)
        .current_dir(workspace())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rite studio");

    // The session token is printed at startup; read it without blocking on EOF.
    let stdout = child.stdout.take().expect("piped stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                return;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut token = None;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(line) => {
                if let Some(rest) = line.trim().strip_prefix("session token:") {
                    token = Some(rest.trim().to_string());
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let token = token.expect("studio should print a session token");
    assert!(!token.is_empty());

    // Wait for the listener to accept connections.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Studio { child, port, token }
}

/// Returns (http status, body).
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

fn port_for(offset: u16) -> u16 {
    24_000 + (std::process::id() as u16 % 500) * 2 + offset
}

const HELLO: &str = r#"{"source":"! @console.println(\"studio-auth-ok\")"}"#;

#[test]
fn run_requires_a_token_and_accepts_the_right_one() {
    let studio = start_studio(port_for(0), &[]);
    let url = format!("http://127.0.0.1:{}/api/v1/run", studio.port);

    // 1. No token at all
    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 401, "unauthenticated /run must be rejected: {body}");
    assert!(body.contains("session token"), "{body}");

    // 2. Wrong token
    let (code, _) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        "authorization: Bearer not-the-token",
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 401);

    // 3. Correct token in the Authorization header
    let auth = format!("authorization: Bearer {}", studio.token);
    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        &auth,
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("studio-auth-ok"), "{body}");

    // 4. Correct token in the JSON body
    let with_body_token = format!(
        r#"{{"source":"! @console.println(\"body-token-ok\")","token":"{}"}}"#,
        studio.token
    );
    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-d",
        &with_body_token,
        &url,
    ]);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("body-token-ok"), "{body}");
}

#[test]
fn foreign_host_header_is_rejected() {
    let studio = start_studio(port_for(1), &[]);
    let url = format!("http://127.0.0.1:{}/api/v1/run", studio.port);
    let auth = format!("authorization: Bearer {}", studio.token);

    // DNS rebinding: attacker page resolves evil.example to 127.0.0.1, so the
    // request is same-origin for the browser and carries a foreign Host.
    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        "host: evil.example",
        "-H",
        &auth,
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 403, "foreign Host must be rejected: {body}");

    // Right host name, wrong port is also not us.
    let wrong_port = format!("host: 127.0.0.1:{}", studio.port + 1);
    let (code, _) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        &wrong_port,
        "-H",
        &auth,
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 403);

    // The genuine loopback Host still works.
    let good = format!("host: localhost:{}", studio.port);
    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        &good,
        "-H",
        &auth,
        "-d",
        HELLO,
        &url,
    ]);
    assert_eq!(code, 200, "{body}");
}

#[test]
fn every_executing_route_is_gated() {
    let studio = start_studio(port_for(2), &[]);
    let auth = format!("authorization: Bearer {}", studio.token);
    for route in ["parse", "analyze", "format", "check", "emit-rust", "run"] {
        let url = format!("http://127.0.0.1:{}/api/v1/{route}", studio.port);
        let (code, body) = curl(&[
            "-X",
            "POST",
            "-H",
            "content-type: application/json",
            "-d",
            HELLO,
            &url,
        ]);
        assert_eq!(code, 401, "/{route} must require a token: {body}");

        let (code, body) = curl(&[
            "-X",
            "POST",
            "-H",
            "content-type: application/json",
            "-H",
            &auth,
            "-d",
            HELLO,
            &url,
        ]);
        assert_eq!(code, 200, "/{route} with a token: {body}");
    }
}

#[test]
fn version_is_a_minimal_unauthenticated_probe() {
    let studio = start_studio(port_for(3), &["--project", "crates/rite-cli"]);
    let url = format!("http://127.0.0.1:{}/api/v1/version", studio.port);

    let (code, body) = curl(&[&url]);
    assert_eq!(code, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["token_required"], serde_json::json!(true));
    // No project path leak before authentication.
    assert!(v.get("project").is_none(), "{body}");
    assert!(v.get("authenticated").is_none(), "{body}");

    let auth = format!("authorization: Bearer {}", studio.token);
    let (code, body) = curl(&["-H", &auth, &url]);
    assert_eq!(code, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["authenticated"], serde_json::json!(true));
    assert_eq!(v["permissions"], serde_json::json!("restricted"));
    assert!(
        v["project"]
            .as_str()
            .unwrap_or_default()
            .contains("rite-cli"),
        "{body}"
    );

    // Foreign Host is rejected even for the probe.
    let (code, _) = curl(&["-H", "host: evil.example", &url]);
    assert_eq!(code, 403);
}

#[test]
fn ui_shell_requires_the_token_too() {
    let studio = start_studio(port_for(4), &[]);
    let base = format!("http://127.0.0.1:{}/", studio.port);

    let (code, body) = curl(&[&base]);
    assert_eq!(code, 401, "{body}");
    assert!(body.contains("session token"), "{body}");

    let with_token = format!("{base}?token={}", studio.token);
    let (code, body) = curl(&[&with_token]);
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("Rite Studio"), "{body}");
    // The page must fetch with the token it was opened with.
    assert!(body.contains("Bearer"), "{body}");
}

#[test]
fn scripts_are_restricted_unless_allow_all() {
    let studio = start_studio(port_for(5), &[]);
    let url = format!("http://127.0.0.1:{}/api/v1/run", studio.port);
    let auth = format!("authorization: Bearer {}", studio.token);
    let read_secret = r#"{"source":"! @fs.read(\"/etc/hosts\")"}"#;

    let (code, body) = curl(&[
        "-X",
        "POST",
        "-H",
        "content-type: application/json",
        "-H",
        &auth,
        "-d",
        read_secret,
        &url,
    ]);
    assert_eq!(code, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        v["ok"],
        serde_json::json!(false),
        "filesystem access must be denied by default: {body}"
    );
}
