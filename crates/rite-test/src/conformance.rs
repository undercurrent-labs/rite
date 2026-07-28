//! Conformance suite runner + interpreter/compiler differential gate.

use rite_caps::{install_defaults, PermissionSet};
use rite_compiler::{run_interpreted, run_ir_mode};
use rite_core::SourceMap;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub path: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConformanceReport {
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<CaseResult>,
}

/// Run all conformance fixtures under `root` in interpreted and IR modes.
pub async fn run_conformance_suite(root: &Path) -> anyhow::Result<ConformanceReport> {
    let mut cases = Vec::new();
    collect_cases(root, &mut cases)?;
    let mut results = Vec::new();
    let mut passed = 0;
    let mut failed = 0;

    for case_dir in cases {
        let case_file = case_dir.join("case.rite");
        if !case_file.is_file() {
            continue;
        }
        let expected_exit = read_trim(&case_dir.join("expected.exit")).unwrap_or_else(|| "0".into());
        let expected_value = read_trim(&case_dir.join("expected.value.json"));
        let expected_stdout = read_trim(&case_dir.join("expected.stdout"));
        let perms = load_perms(&case_dir.join("permissions.toml"));

        // Interpreted
        let interp = run_interpreted(&case_file, perms.clone()).await;
        // IR mode (same as compiled artifact evaluation path)
        let ir = run_ir_mode(&case_file, perms).await;

        let mut ok = true;
        let mut msg = String::new();

        match (&interp, &ir) {
            (Ok((v_i, out_i, _)), Ok(v_c)) => {
                if !v_i.structural_eq(v_c) {
                    ok = false;
                    msg.push_str(&format!(
                        "value mismatch: interp={:?} ir={:?}; ",
                        v_i, v_c
                    ));
                }
                if let Some(ref exp) = expected_value {
                    let got = format!("{}", v_i);
                    // compare loosely via display or JSON
                    let exp_clean = exp.trim();
                    let matches = got == exp_clean
                        || v_i.to_json(&rite_runtime::AtomInterner::new()).to_string() == exp_clean;
                    if !matches && exp_clean.parse::<i64>().ok() != v_i.as_int() {
                        // try int equality
                        if v_i.as_int().map(|n| n.to_string()) != Some(exp_clean.to_string()) {
                            ok = false;
                            msg.push_str(&format!(
                                "expected value {} got {}; ",
                                exp_clean, got
                            ));
                        }
                    }
                }
                if let Some(ref exp_out) = expected_stdout {
                    if out_i.trim() != exp_out.trim() {
                        ok = false;
                        msg.push_str(&format!(
                            "stdout mismatch: expected {:?} got {:?}; ",
                            exp_out, out_i
                        ));
                    }
                }
                if expected_exit != "0" {
                    ok = false;
                    msg.push_str("expected non-zero exit but succeeded; ");
                }
            }
            (Err(e), _) if expected_exit != "0" => {
                // expected failure
                ok = true;
                msg.push_str(&format!("expected failure: {}", e));
            }
            (Err(e), _) => {
                ok = false;
                msg.push_str(&format!("interp error: {}", e));
            }
            (Ok(_), Err(e)) => {
                ok = false;
                msg.push_str(&format!("ir mode error: {}", e));
            }
        }

        if ok {
            passed += 1;
        } else {
            failed += 1;
        }
        results.push(CaseResult {
            path: case_dir.display().to_string(),
            passed: ok,
            message: msg,
        });
    }

    Ok(ConformanceReport {
        passed,
        failed,
        results,
    })
}

fn collect_cases(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    if dir.join("case.rite").is_file() {
        out.push(dir.to_path_buf());
    }
    if dir.is_dir() {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            if e.path().is_dir() {
                collect_cases(&e.path(), out)?;
            }
        }
    }
    Ok(())
}

fn read_trim(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn load_perms(path: &Path) -> PermissionSet {
    if let Ok(text) = std::fs::read_to_string(path) {
        if text.contains("allow_all") && text.contains("true") {
            return PermissionSet::allow_all();
        }
    }
    PermissionSet::allow_all()
}

/// In-process differential for a single source string.
pub async fn differential_source(name: &str, source: &str) -> Result<(), String> {
    let mut ctx_a = rite_runtime::RuntimeContext::new();
    install_defaults(&mut ctx_a, PermissionSet::allow_all());
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, source);
    let file = sources.get(id).unwrap().clone();
    let v_a = rite_runtime::run_file(&file, &mut ctx_a)
        .await
        .map_err(|e| e.to_string())?;

    let (ir, diags) = rite_sem::compile_to_ir(&file);
    if diags.has_errors() {
        return Err(format!("ir compile failed: {}", diags.len()));
    }
    let ir = ir.ok_or_else(|| "no ir".to_string())?;
    let mut ctx_b = rite_runtime::RuntimeContext::new();
    install_defaults(&mut ctx_b, PermissionSet::allow_all());
    let v_b = rite_runtime::run_ir(&ir, &mut ctx_b)
        .await
        .map_err(|e| e.to_string())?;

    if v_a.structural_eq(&v_b) {
        Ok(())
    } else {
        Err(format!("mismatch {:?} vs {:?}", v_a, v_b))
    }
}
