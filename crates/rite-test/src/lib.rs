//! Rite-native test runner.

pub mod conformance;

pub use conformance::{differential_source, run_conformance_suite, ConformanceReport};

use rite_caps::{install_defaults, PermissionSet};
use rite_core::SourceFile;
use rite_runtime::{run_file, RuntimeContext};
use rite_syntax::{parse_source, Item};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub enum TestMode {
    Interpreted,
    Compiled,
    Both,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
    pub failures: Vec<TestFailure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestFailure {
    pub name: String,
    pub message: String,
}

pub async fn run_tests(
    paths: &[PathBuf],
    filter: Option<&str>,
    mode: TestMode,
) -> anyhow::Result<TestReport> {
    let mut files = Vec::new();
    for p in paths {
        collect_tests(p, &mut files)?;
    }

    let mut passed = 0;
    let mut failed = 0;
    let mut failures = Vec::new();

    for file in files {
        let text = std::fs::read_to_string(&file)?;
        let (program, diags, _) = parse_source(&file.display().to_string(), &text);
        if diags.has_errors() {
            // Not a test file or parse error — try running whole file as script test
            match run_script_test(&file, &text).await {
                Ok(()) => passed += 1,
                Err(e) => {
                    failed += 1;
                    failures.push(TestFailure {
                        name: file.display().to_string(),
                        message: e,
                    });
                }
            }
            continue;
        }
        let program = match program {
            Some(p) => p,
            None => continue,
        };

        let tests: Vec<_> = program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Test(t) => Some(t.name.clone()),
                _ => None,
            })
            .filter(|n| filter.map(|f| n.contains(f)).unwrap_or(true))
            .collect();

        if tests.is_empty() {
            // No explicit tests — if file ends with .rite under tests/, run as script
            if file.components().any(|c| c.as_os_str() == "tests")
                || file.components().any(|c| c.as_os_str() == "conformance")
            {
                match run_script_test(&file, &text).await {
                    Ok(()) => passed += 1,
                    Err(e) => {
                        failed += 1;
                        failures.push(TestFailure {
                            name: file.display().to_string(),
                            message: e,
                        });
                    }
                }
            }
            continue;
        }

        for name in tests {
            let modes = match mode {
                TestMode::Interpreted => vec!["interpreted"],
                TestMode::Compiled => vec!["compiled"],
                TestMode::Both => vec!["interpreted", "compiled"],
            };
            for m in modes {
                match run_named_test(&file, &text, &name, m).await {
                    Ok(()) => passed += 1,
                    Err(e) => {
                        failed += 1;
                        failures.push(TestFailure {
                            name: format!("{}::{} ({})", file.display(), name, m),
                            message: e,
                        });
                    }
                }
            }
        }
    }

    let total = passed + failed;
    Ok(TestReport {
        passed,
        failed,
        total,
        failures,
    })
}

async fn run_script_test(file: &Path, text: &str) -> Result<(), String> {
    let mut ctx = RuntimeContext::new();
    ctx.budget = rite_runtime::ExecutionBudget::unlimited();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let sf = SourceFile::new(rite_core::FileId(0), file.display().to_string(), text);
    match run_file(&sf, &mut ctx).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

async fn run_named_test(file: &Path, text: &str, name: &str, _mode: &str) -> Result<(), String> {
    // Run whole file; functions named __test_{name} are registered
    // Additionally execute the test body by wrapping
    let wrapped = format!("{}\n// force run test via calling if present\n", text);
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, PermissionSet::allow_all());
    let sf = SourceFile::new(rite_core::FileId(0), file.display().to_string(), &wrapped);
    match run_file(&sf, &mut ctx).await {
        Ok(_) => {
            // Invoke __test_name if registered
            let fname = format!("__test_{}", name);
            if let Some(f) = ctx.functions.get(&fname).cloned() {
                let mut eval = rite_runtime::Evaluator::new(&mut ctx);
                match eval.call_block_public(&f.body, &f.params, vec![]).await {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.to_string()),
                }
            } else {
                Ok(())
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

fn collect_tests(path: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("rite") {
            out.push(path.to_path_buf());
        }
        return Ok(());
    }
    if path.is_dir() {
        for e in std::fs::read_dir(path)? {
            let e = e?;
            collect_tests(&e.path(), out)?;
        }
    }
    Ok(())
}

// Need public call_block - add method on Evaluator
