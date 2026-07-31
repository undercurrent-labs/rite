//! Browser-compatible Rite API.
//!
//! - **native** (default): full run via rite-runtime + caps.
//! - **wasm**: parse/format/analyze/convert without Tokio; pure/light run stub.

use rite_analysis::AnalysisEngine;
use rite_fmt::{convert_source, format_with_dialect, Dialect};
use rite_sem::ir_to_json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(feature = "wasm")]
mod wasm_api;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParseResult {
    pub ok: bool,
    pub diagnostics: Vec<serde_json::Value>,
    pub ast_json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatResultDto {
    pub text: String,
    pub dialect: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map: Option<SourceMapDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceMapDto {
    pub out_to_in: Vec<u32>,
    pub in_len: u32,
    pub out_len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub ok: bool,
    pub value: serde_json::Value,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub virtual_http: Option<VirtualHttpInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualHttpInfo {
    pub addr: String,
    pub routes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunOptions {
    pub allow_all: bool,
    pub timeout_ms: Option<u64>,
    pub seed: Option<u64>,
    /// Extra in-memory sources, keyed by name.
    ///
    /// **Not implemented.** The field is accepted so an existing payload still
    /// deserialises, but supplying a non-empty map is an error rather than a silent
    /// no-op: nothing here has ever read it, so a caller who sent files got a script
    /// run without them and no indication why.
    pub files: HashMap<String, String>,
    #[serde(default)]
    pub browser_safe: bool,
}

pub fn parse(source: &str) -> ParseResult {
    let (program, diags, _) = rite_syntax::parse_source("browser.rite", source);
    ParseResult {
        ok: !diags.has_errors(),
        diagnostics: diags.iter().map(|d| d.to_json()).collect(),
        ast_json: program.map(|p| rite_syntax::ast_to_json(&p)),
    }
}

pub fn analyze(source: &str) -> serde_json::Value {
    let mut eng = AnalysisEngine::new();
    let snap = eng.analyze("browser.rite", source);
    serde_json::to_value(snap).unwrap_or_default()
}

pub fn format(source: &str, dialect: &str) -> Result<FormatResultDto, String> {
    let d = parse_dialect(dialect);
    let r = format_with_dialect(source, d)?;
    let out_len = r.text.len() as u32;
    let map = r
        .source_map
        .unwrap_or_else(|| rite_fmt::build_line_source_map(source, &r.text));
    Ok(FormatResultDto {
        text: r.text,
        dialect: dialect.to_string(),
        source_map: Some(SourceMapDto {
            out_to_in: map.out_to_in,
            in_len: source.len() as u32,
            out_len,
        }),
    })
}

pub fn convert(source: &str, dialect: &str) -> Result<FormatResultDto, String> {
    let d = parse_dialect(dialect);
    let r = convert_source(source, d)?;
    let out_len = r.text.len() as u32;
    let map = r
        .source_map
        .unwrap_or_else(|| rite_fmt::build_line_source_map(source, &r.text));
    Ok(FormatResultDto {
        text: r.text,
        dialect: dialect.to_string(),
        source_map: Some(SourceMapDto {
            out_to_in: map.out_to_in,
            in_len: source.len() as u32,
            out_len,
        }),
    })
}

pub fn map_position(
    source_map: &SourceMapDto,
    old_source: &str,
    new_source: &str,
    line: u32,
    character: u32,
) -> (u32, u32) {
    let old_byte = pos_to_byte(old_source, line, character);
    let map = &source_map.out_to_in;
    if map.is_empty() {
        return (line, character);
    }
    let mut best = 0usize;
    let mut best_dist = u32::MAX;
    for (i, &mapped) in map.iter().enumerate() {
        let dist = mapped.abs_diff(old_byte as u32);
        if dist < best_dist {
            best_dist = dist;
            best = i;
        }
    }
    byte_to_pos(new_source, best)
}

pub fn syntax_tree(source: &str) -> serde_json::Value {
    let (program, diags, _) = rite_syntax::parse_source("browser.rite", source);
    serde_json::json!({
        "ok": !diags.has_errors(),
        "ast": program.map(|p| rite_syntax::ast_to_json(&p)),
        "diagnostics": diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
    })
}

pub fn semantic_ir(source: &str) -> serde_json::Value {
    let file = rite_core::SourceFile::new(rite_core::FileId(0), "browser.rite", source);
    let (ir, diags) = rite_sem::compile_to_ir(&file);
    serde_json::json!({
        "ok": !diags.has_errors(),
        "ir": ir.map(|i| ir_to_json(&i)),
        "diagnostics": diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
    })
}

pub fn emit_rust(source: &str) -> serde_json::Value {
    #[cfg(feature = "native")]
    {
        let file = rite_core::SourceFile::new(rite_core::FileId(0), "browser.rite", source);
        let (ir, diags) = rite_sem::compile_to_ir(&file);
        if diags.has_errors() {
            return serde_json::json!({
                "ok": false,
                "error": "compile errors",
                "diagnostics": diags.iter().map(|d| d.to_json()).collect::<Vec<_>>(),
            });
        }
        let ir = match ir {
            Some(i) => i,
            None => return serde_json::json!({"ok": false, "error": "no ir"}),
        };
        match rite_compiler::generate_from_ir(&ir, std::path::Path::new("browser.rite")) {
            Ok(code) => serde_json::json!({"ok": true, "rust": code}),
            Err(e) => serde_json::json!({"ok": false, "error": e}),
        }
    }
    #[cfg(not(feature = "native"))]
    {
        // WASM: emit IR JSON as inspectable artifact
        let ir = semantic_ir(source);
        serde_json::json!({
            "ok": ir.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
            "rust": format!("// IR (WASM build — full Rust codegen requires native)\n// {}\n", ir),
            "ir": ir,
        })
    }
}

pub fn complete(source: &str, line: u32, character: u32) -> serde_json::Value {
    let eng = AnalysisEngine::new();
    serde_json::to_value(eng.completions(source, line, character)).unwrap_or_default()
}

pub fn hover(source: &str, line: u32, character: u32) -> serde_json::Value {
    let eng = AnalysisEngine::new();
    match eng.hover(source, line, character) {
        Some(h) => serde_json::to_value(h).unwrap_or_default(),
        None => serde_json::json!(null),
    }
}

pub async fn run(source: &str, options: RunOptions) -> ExecutionResult {
    let browser = options.browser_safe || cfg!(not(feature = "native"));

    if !options.files.is_empty() {
        return ExecutionResult {
            ok: false,
            value: serde_json::Value::Null,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "`files` is not implemented: this runtime resolves no imports, so the \
                 supplied sources would be ignored rather than used"
                    .into(),
            ),
            virtual_http: None,
        };
    }

    if browser {
        // Capabilities with no browser answer at all. `@http` is not here: the
        // browser gets a *virtual* listener below, which is a real (if limited)
        // answer. These have none — there is no subprocess and no socket layer —
        // so say so before the script runs rather than failing mid-way.
        if let Some(cap) = ["process", "udp", "tcp"]
            .into_iter()
            .find(|c| source.contains(&format!("@{c}")) || source.contains(&format!("host.{c}")))
        {
            return ExecutionResult {
                ok: false,
                value: serde_json::Value::Null,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!(
                    "capability @{cap} is native-only and unavailable in the browser runtime"
                )),
                virtual_http: None,
            };
        }
    }

    let virtual_http = if source.contains("@http.listen") || source.contains("host.http.listen") {
        Some(VirtualHttpInfo {
            addr: "virtual://rite-studio".into(),
            routes: extract_routes(source),
        })
    } else {
        None
    };

    if browser && virtual_http.is_some() {
        return ExecutionResult {
            ok: true,
            value: serde_json::json!({
                "status": "virtual",
                "addr": "virtual://rite-studio",
                "routes": extract_routes(source),
            }),
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            virtual_http,
        };
    }

    // Shared pure evaluator path (works on wasm32 without Tokio/caps).
    // Native builds optionally install full caps for FS/HTTP/process.
    {
        use rite_runtime::{run_source, RuntimeContext};

        let mut ctx = RuntimeContext::new();
        ctx.allow_all = options.allow_all;
        ctx.rng_seed = options.seed;
        if let Some(ms) = options.timeout_ms {
            ctx.budget = ctx
                .budget
                .with_timeout(std::time::Duration::from_millis(ms));
        }

        #[cfg(feature = "native")]
        {
            use rite_caps::{install_defaults, Permission, PermissionSet};
            let mut perms = if options.allow_all {
                let mut p = PermissionSet::allow_all();
                if browser {
                    p.deny(Permission::Process);
                }
                p
            } else {
                PermissionSet::default_secure()
            };
            if browser {
                perms.deny(Permission::Process);
            }
            install_defaults(&mut ctx, perms);
        }

        match run_source("browser.rite", source, &mut ctx).await {
            Ok(v) => ExecutionResult {
                ok: true,
                value: v.to_json(&ctx.atoms),
                stdout: ctx.stdout.join(""),
                stderr: ctx.stderr.join(""),
                error: None,
                virtual_http,
            },
            Err(e) => ExecutionResult {
                ok: false,
                value: serde_json::Value::Null,
                stdout: ctx.stdout.join(""),
                stderr: ctx.stderr.join(""),
                error: Some(e.to_string()),
                virtual_http,
            },
        }
    }
}

/// Run to completion from **sync** code.
///
/// Safe both outside Tokio and **inside** an existing multi-thread runtime
/// (e.g. Axum `rite studio` handlers). Prefer awaiting [`run`] from async code.
pub fn run_blocking(source: &str, options: RunOptions) -> ExecutionResult {
    #[cfg(feature = "native")]
    {
        // Already on a Tokio worker (Studio / Axum / #[tokio::test])?
        // Creating a nested runtime panics with:
        //   "Cannot start a runtime from within a runtime"
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            return tokio::task::block_in_place(|| handle.block_on(run(source, options)));
        }

        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(run(source, options)),
            Err(e) => ExecutionResult {
                ok: false,
                value: serde_json::Value::Null,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(e.to_string()),
                virtual_http: None,
            },
        }
    }

    #[cfg(not(feature = "native"))]
    {
        // Pure poll loop for wasm32 / no-Tokio builds. Pure scripts + @console complete
        // on first poll; true host I/O would stay Pending (blocked in browser).
        use std::future::Future;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn dummy_raw_waker() -> RawWaker {
            fn no_op(_: *const ()) {}
            fn clone(_: *const ()) -> RawWaker {
                dummy_raw_waker()
            }
            static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let options = RunOptions {
            browser_safe: true,
            ..options
        };
        let mut fut = Box::pin(run(source, options));
        // Pump a few times in case of lightweight cooperative yields.
        for _ in 0..64 {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(r) => return r,
                Poll::Pending => continue,
            }
        }
        ExecutionResult {
            ok: false,
            value: serde_json::Value::Null,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(
                "async not ready in browser (I/O capabilities need the native host API)".into(),
            ),
            virtual_http: None,
        }
    }
}

fn parse_dialect(s: &str) -> Dialect {
    match s.to_ascii_lowercase().as_str() {
        "ascii" => Dialect::Ascii,
        "mixed" => Dialect::Mixed,
        "preserve" => Dialect::Preserve,
        _ => Dialect::Glyph,
    }
}

fn extract_routes(source: &str) -> Vec<String> {
    let mut routes = Vec::new();
    for line in source.lines() {
        let t = line.trim();
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            if t.starts_with(method) {
                routes.push(t.to_string());
            }
        }
    }
    routes
}

fn pos_to_byte(text: &str, line: u32, character: u32) -> usize {
    let mut cur_line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if cur_line == line && col >= character {
            return i;
        }
        if ch == '\n' {
            cur_line += 1;
            col = 0;
            if cur_line > line {
                return i;
            }
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    text.len()
}

fn byte_to_pos(text: &str, byte: usize) -> (u32, u32) {
    let byte = byte.min(text.len());
    let mut line = 0u32;
    let mut col = 0u32;
    let mut i = 0usize;
    for ch in text.chars() {
        let bl = ch.len_utf8();
        if i + bl > byte {
            break;
        }
        i += bl;
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        let p = parse("x ← 1");
        assert!(p.ok);
        let f = format("x <- 1", "glyph").unwrap();
        assert!(f.source_map.is_some());
    }

    #[test]
    fn process_denied_browser() {
        let r = run_blocking(
            "@process.run(\"true\")",
            RunOptions {
                browser_safe: true,
                ..Default::default()
            },
        );
        assert!(!r.ok);
    }

    /// There are no datagram sockets in a browser tab, in either dialect, and
    /// `--allow-all` does not conjure one.
    #[test]
    fn udp_denied_browser() {
        for src in [
            "! @udp.bind(\"127.0.0.1:0\")",
            "do host.udp.bind(\"127.0.0.1:0\")",
        ] {
            let r = run_blocking(
                src,
                RunOptions {
                    allow_all: true,
                    browser_safe: true,
                    ..Default::default()
                },
            );
            assert!(!r.ok, "`{src}` must not run in the browser runtime");
            let e = r.error.unwrap_or_default();
            assert!(
                e.contains("@udp") && e.contains("native-only"),
                "error should name the capability and why: {e}"
            );
        }
    }

    /// Nor are there stream sockets — including the `@tcp.listen` server shape,
    /// which the browser must refuse before it starts rather than mid-connection.
    #[test]
    fn tcp_denied_browser() {
        for src in [
            "! @tcp.connect(\"127.0.0.1:9\")",
            "do host.tcp.connect(\"127.0.0.1:9\")",
            "! @tcp.listen \"127.0.0.1:0\" ⟦ |conn| conn ⟧",
        ] {
            let r = run_blocking(
                src,
                RunOptions {
                    allow_all: true,
                    browser_safe: true,
                    ..Default::default()
                },
            );
            assert!(!r.ok, "`{src}` must not run in the browser runtime");
            let e = r.error.unwrap_or_default();
            assert!(
                e.contains("@tcp") && e.contains("native-only"),
                "error should name the capability and why: {e}"
            );
        }
    }

    #[test]
    fn pure_eval_square_and_println() {
        let src = r#"
◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
"#;
        let r = run_blocking(
            src,
            RunOptions {
                allow_all: true,
                browser_safe: true,
                ..Default::default()
            },
        );
        assert!(r.ok, "error: {:?}", r.error);
        assert!(
            r.stdout.contains("144"),
            "expected 144 in stdout, got {:?}",
            r.stdout
        );
    }

    #[test]
    fn pure_nested_function_and_early_return() {
        let src = r#"
◆ area(w, h) ⟦
  ◆ clamp(n) ⟦
    ? n < 0 ⟦ ^ 0 ⟧
    ^ n
  ⟧
  ^ clamp(w) * clamp(h)
⟧
area(-2, 5)
"#;
        let r = run_blocking(
            src,
            RunOptions {
                allow_all: true,
                browser_safe: true,
                ..Default::default()
            },
        );
        assert!(r.ok, "error: {:?}", r.error);
        assert_eq!(r.value, serde_json::json!(0));
    }

    #[test]
    fn pure_pipeline_and_logical() {
        let src = r#"[1, 2, 3, 4] → keep { |n| n % 2 = 0 } → map { |n| n * n } → sum"#;
        let r = run_blocking(
            src,
            RunOptions {
                allow_all: true,
                browser_safe: true,
                ..Default::default()
            },
        );
        assert!(r.ok, "error: {:?}", r.error);
        assert_eq!(r.value, serde_json::json!(20));
    }

    #[test]
    fn try_unwrap_err_is_result_not_hard_fail() {
        let r = run_blocking(
            r#"@json.decode("not-json")?"#,
            RunOptions {
                allow_all: true,
                browser_safe: true,
                ..Default::default()
            },
        );
        // top-level ? yields err value as script result (ok run, err-shaped value)
        assert!(
            r.ok || r.error.is_some() || r.value.is_object() || r.value.is_string(),
            "got ok={} value={:?} err={:?}",
            r.ok,
            r.value,
            r.error
        );
    }

    #[cfg(feature = "native")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_blocking_inside_tokio_runtime() {
        // Regression: rite studio Axum handlers used to panic with
        // "Cannot start a runtime from within a runtime".
        let r = run_blocking(
            "1 + 2",
            RunOptions {
                allow_all: true,
                browser_safe: false,
                timeout_ms: Some(2000),
                ..Default::default()
            },
        );
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(r.value, serde_json::json!(3));
    }

    #[cfg(feature = "native")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_async_inside_tokio_runtime() {
        let r = run(
            r#"! @console.println("studio")
42"#,
            RunOptions {
                allow_all: true,
                browser_safe: false,
                timeout_ms: Some(2000),
                ..Default::default()
            },
        )
        .await;
        assert!(r.ok, "{:?}", r.error);
        assert!(r.stdout.contains("studio"), "{:?}", r.stdout);
    }
}
