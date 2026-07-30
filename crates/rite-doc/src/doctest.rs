//! Executable documentation fence runner.

use rite_syntax::parse_source;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct DocTestResult {
    pub file: String,
    pub line: usize,
    pub mode: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocTestReport {
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<DocTestResult>,
}

/// Extract and run ` ```rite ` / ` ```rite exec ` fences under `roots`.
pub async fn run_doctests(roots: &[&Path]) -> DocTestReport {
    let mut files = Vec::new();
    for root in roots {
        collect_md(root, &mut files);
    }
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;

    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for block in extract_rite_blocks(&text) {
            let res = run_block(&file, &block).await;
            if res.ok {
                passed += 1;
            } else {
                failed += 1;
            }
            results.push(res);
        }
    }

    DocTestReport {
        passed,
        failed,
        results,
    }
}

#[derive(Debug)]
struct FenceBlock {
    line: usize,
    mode: String,
    code: String,
}

fn extract_rite_blocks(text: &str) -> Vec<FenceBlock> {
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate().peekable();
    while let Some((i, line)) = lines.next() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```rite") {
            let mode = rest.trim();
            // Default bare ```rite to parse-only; require ```rite exec to run.
            let mode = if mode.is_empty() {
                "no_run".to_string()
            } else {
                mode.to_string()
            };
            // skip no_run unless parse-only
            let mut code = String::new();
            let start_line = i + 1;
            for (_, l) in lines.by_ref() {
                if l.trim().starts_with("```") {
                    break;
                }
                code.push_str(l);
                code.push('\n');
            }
            if !code.trim().is_empty() {
                out.push(FenceBlock {
                    line: start_line,
                    mode,
                    code,
                });
            }
        }
    }
    out
}

async fn run_block(file: &Path, block: &FenceBlock) -> DocTestResult {
    let file_s = file.display().to_string();
    match block.mode.as_str() {
        "no_run" | "native_only" => {
            // Parse only
            let (_p, diags, _) = parse_source(&file_s, &block.code);
            let ok = !diags.has_errors();
            DocTestResult {
                file: file_s,
                line: block.line,
                mode: block.mode.clone(),
                ok,
                message: if ok {
                    "parse ok".into()
                } else {
                    format!("{} parse errors", diags.len())
                },
            }
        }
        "compile" | "compile_fail" => {
            let file_sf =
                rite_core::SourceFile::new(rite_core::FileId(0), &file_s, block.code.clone());
            let (_ir, diags) = rite_sem::compile_to_ir(&file_sf);
            let has_err = diags.has_errors();
            let ok = if block.mode == "compile_fail" {
                has_err
            } else {
                !has_err
            };
            DocTestResult {
                file: file_s,
                line: block.line,
                mode: block.mode.clone(),
                ok,
                message: if ok {
                    "ok".into()
                } else {
                    format!("compile unexpected (errors={})", has_err)
                },
            }
        }
        // "browser" and "exec" land here too — both parse + interpret.
        _ => {
            // Parse + interpret when possible (skip if needs net bind without test mode)
            let (_p, diags, _) = parse_source(&file_s, &block.code);
            if diags.has_errors() {
                return DocTestResult {
                    file: file_s,
                    line: block.line,
                    mode: block.mode.clone(),
                    ok: false,
                    message: format!("parse errors: {}", diags.len()),
                };
            }
            if block.code.contains("@http.listen") || block.code.contains("host.http.listen") {
                return DocTestResult {
                    file: file_s,
                    line: block.line,
                    mode: block.mode.clone(),
                    ok: true,
                    message: "skipped listen (parse ok)".into(),
                };
            }
            if block.code.contains("@process") {
                return DocTestResult {
                    file: file_s,
                    line: block.line,
                    mode: block.mode.clone(),
                    ok: true,
                    message: "skipped process example (parse ok)".into(),
                };
            }
            let result = rite_wasm::run(
                &block.code,
                rite_wasm::RunOptions {
                    allow_all: true,
                    browser_safe: block.mode == "browser",
                    timeout_ms: Some(3000),
                    seed: Some(1),
                    files: Default::default(),
                },
            )
            .await;
            DocTestResult {
                file: file_s,
                line: block.line,
                mode: block.mode.clone(),
                ok: result.ok,
                message: result.error.unwrap_or_else(|| "ok".into()),
            }
        }
    }
}

fn collect_md(root: &Path, out: &mut Vec<PathBuf>) {
    if !root.exists() {
        return;
    }
    if root.is_file() {
        if root.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(root.to_path_buf());
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        collect_md(&e.path(), out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fences() {
        let md = r#"
# Title
```rite exec
1 + 2
```
```rite no_run
def x() [[ return 1 ]]
```
"#;
        let blocks = extract_rite_blocks(md);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].mode, "exec");
        assert_eq!(blocks[1].mode, "no_run");
    }
}
