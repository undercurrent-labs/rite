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
        let expected_exit =
            read_trim(&case_dir.join("expected.exit")).unwrap_or_else(|| "0".into());
        let expected_value = read_trim(&case_dir.join("expected.value.json"));
        let expected_stdout = read_trim(&case_dir.join("expected.stdout"));
        let perms = match load_perms(&case_dir.join("permissions.toml")) {
            Ok(p) => p,
            Err(e) => {
                // A fixture that cannot be read is a broken fixture, not a pass.
                failed += 1;
                results.push(CaseResult {
                    path: case_dir.display().to_string(),
                    passed: false,
                    message: e,
                });
                continue;
            }
        };

        // Interpreted
        let interp = run_interpreted(&case_file, perms.clone()).await;
        // IR mode (same as compiled artifact evaluation path)
        let ir = run_ir_mode(&case_file, perms).await;

        let mut ok = true;
        let mut msg = String::new();

        match (&interp, &ir) {
            (Ok((v_i, out_i, _, atoms_i)), Ok(v_c)) => {
                if !v_i.structural_eq(v_c) {
                    ok = false;
                    msg.push_str(&format!("value mismatch: interp={:?} ir={:?}; ", v_i, v_c));
                }
                // Compare as JSON when both sides parse as JSON, so formatting and key
                // order do not decide a case; fall back to the rendered forms otherwise.
                //
                // The chain this replaces ended in `exp.parse::<i64>().ok() != v.as_int()`,
                // which is `None != None` — false — whenever neither side was an integer.
                // A mismatched *string* expectation therefore reported nothing at all, so
                // `expected.value.json` was unchecked for every non-numeric fixture.
                if let Some(ref exp) = expected_value {
                    let exp_clean = exp.trim();
                    let got_display = format!("{}", v_i);
                    let got_json = v_i.to_json(atoms_i);
                    let matches = match serde_json::from_str::<serde_json::Value>(exp_clean) {
                        Ok(want) => want == got_json,
                        // Not JSON: the fixture is written as the rendered form.
                        Err(_) => got_display == exp_clean,
                    };
                    if !matches {
                        ok = false;
                        msg.push_str(&format!(
                            "expected value {} got {} (json {}); ",
                            exp_clean, got_display, got_json
                        ));
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
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Parse a fixture's `permissions.toml`.
///
/// Both branches used to return `allow_all()`, so a case declaring `allow = []` ran
/// with every capability granted and no permission fixture proved anything.
///
/// Deliberately strict, and deliberately not a general TOML parser: the format is two
/// keys, and the failure being fixed here is a *silent* over-grant. An unrecognised
/// declaration now fails its case rather than quietly widening it.
fn load_perms(path: &Path) -> Result<PermissionSet, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        // No declaration means no grants beyond the ones the CLI gives every script.
        return Ok(PermissionSet::default_secure());
    };
    let where_ = |line: usize| format!("{}:{}", path.display(), line);
    let mut perms = PermissionSet::default_secure();

    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "{}: expected `key = value`, got {:?}",
                where_(i + 1),
                raw
            ));
        };
        match (key.trim(), value.trim()) {
            ("allow_all", "true") => perms = PermissionSet::allow_all(),
            ("allow_all", "false") => perms.allow_all = false,
            ("allow_all", other) => {
                return Err(format!(
                    "{}: allow_all must be true or false, got {:?}",
                    where_(i + 1),
                    other
                ))
            }
            ("allow", list) => {
                let inner = list
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .ok_or_else(|| {
                        format!(
                            "{}: allow must be a single-line array, got {:?}",
                            where_(i + 1),
                            list
                        )
                    })?;
                for entry in inner.split(',') {
                    let grant = entry.trim().trim_matches('"');
                    if grant.is_empty() {
                        continue;
                    }
                    let parsed = rite_caps::Permission::parse(grant).map_err(|e| {
                        format!("{}: unknown grant {:?}: {}", where_(i + 1), grant, e)
                    })?;
                    perms.grant(parsed);
                }
            }
            (key, _) => return Err(format!("{}: unknown key {:?}", where_(i + 1), key)),
        }
    }
    Ok(perms)
}

/// In-process differential for a single source string.
///
/// The interpreter is normative and `rite build` claims behavioural parity with it, so
/// this compares what a program *does* as well as what it evaluates to: a script that
/// prints different things down the two paths is not at parity even if the final value
/// happens to agree.
///
/// Errors are compared too — the two paths must agree on failing, not just on
/// succeeding, or a capability that silently no-ops under IR would look identical to
/// one that works.
pub async fn differential_source(name: &str, source: &str) -> Result<(), String> {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, source);
    let file = sources.get(id).unwrap().clone();

    let mut ctx_a = rite_runtime::RuntimeContext::new();
    install_defaults(&mut ctx_a, PermissionSet::allow_all());
    let interpreted = rite_runtime::run_file(&file, &mut ctx_a)
        .await
        .map_err(|e| e.to_string());

    let (ir, diags) = rite_sem::compile_to_ir(&file);
    if diags.has_errors() {
        return Err(format!("ir compile failed: {}", diags.len()));
    }
    let ir = ir.ok_or_else(|| "no ir".to_string())?;
    let mut ctx_b = rite_runtime::RuntimeContext::new();
    install_defaults(&mut ctx_b, PermissionSet::allow_all());
    let via_ir = rite_runtime::run_ir(&ir, &mut ctx_b)
        .await
        .map_err(|e| e.to_string());

    match (&interpreted, &via_ir) {
        (Ok(a), Ok(b)) if !a.structural_eq(b) => {
            return Err(format!("value mismatch: interpreted {a:?}, ir {b:?}"))
        }
        (Ok(a), Err(e)) => return Err(format!("interpreted gave {a:?}, ir failed: {e}")),
        (Err(e), Ok(b)) => return Err(format!("interpreted failed: {e}, ir gave {b:?}")),
        (Err(a), Err(b)) if a != b => {
            return Err(format!("both failed but differently: {a:?} vs {b:?}"))
        }
        _ => {}
    }

    if ctx_a.stdout != ctx_b.stdout {
        return Err(format!(
            "stdout mismatch: interpreted {:?}, ir {:?}",
            ctx_a.stdout.join(""),
            ctx_b.stdout.join("")
        ));
    }
    if ctx_a.stderr != ctx_b.stderr {
        return Err(format!(
            "stderr mismatch: interpreted {:?}, ir {:?}",
            ctx_a.stderr.join(""),
            ctx_b.stderr.join("")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod permission_fixtures {
    use super::*;

    fn parse(body: &str) -> Result<PermissionSet, String> {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("permissions.toml");
        std::fs::write(&path, body).expect("write fixture");
        load_perms(&path)
    }

    #[test]
    fn allow_all_grants_everything() {
        let p = parse("allow_all = true\n").expect("parse");
        assert!(p.allow_all);
        assert!(p.check_process().is_ok());
    }

    #[test]
    fn an_empty_allow_list_does_not_grant_everything() {
        // The regression: this returned `allow_all()`, so every fixture that declared
        // no grants ran with all of them and proved nothing.
        let p = parse("allow = []\n").expect("parse");
        assert!(!p.allow_all, "an empty grant list is not a blanket grant");
        assert!(
            p.check_process().is_err(),
            "process must stay denied without a grant"
        );
        assert!(p.check_fs_read(Path::new("secret.txt")).is_err());
        // Still the CLI's own defaults, so a case may print.
        assert!(p.check_console().is_ok());
    }

    #[test]
    fn listed_grants_are_applied() {
        let p = parse("allow = [\"fs:read=./data\", \"net=api.example.com\"]\n").expect("parse");
        assert!(!p.allow_all);
        assert_eq!(p.fs_read.len(), 1, "the fs grant was applied");
        assert!(p.net.contains("api.example.com"));
        assert!(
            p.check_fs_read(Path::new("elsewhere.txt")).is_err(),
            "a grant for ./data does not open the rest of the disk"
        );
    }

    #[test]
    fn a_missing_file_is_secure_by_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = load_perms(&dir.path().join("nothing.toml")).expect("missing file is fine");
        assert!(!p.allow_all);
        assert!(p.check_process().is_err());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let p = parse("# a comment\n\nallow_all = true\n").expect("parse");
        assert!(p.allow_all);
    }

    #[test]
    fn an_unreadable_fixture_is_an_error_not_a_blanket_grant() {
        // Every one of these used to fall through to `allow_all()`. Failing loudly is
        // the whole point: a fixture nobody can parse must not silently pass.
        for body in [
            "allow_all = yes\n",
            "allow = fs:read=./data\n",
            "permit_everything = true\n",
            "just some words\n",
            "allow = [\"fs:read\", \"not-a-real-capability\"]\n",
        ] {
            assert!(
                parse(body).is_err(),
                "expected {:?} to be rejected, not silently accepted",
                body
            );
        }
    }
}
