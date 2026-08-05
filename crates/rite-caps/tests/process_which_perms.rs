//! `@process.which` reads the `PATH` environment variable, so it must go through
//! the `env` capability instead of around it (it used to call `std::env::var`
//! directly under the `process` grant alone).

use rite_caps::process::ProcessCap;
use rite_caps::{Permission, PermissionSet};
use rite_runtime::{ResultValue, RuntimeContext, Value};

async fn which(perms: &PermissionSet, name: &str) -> Result<Value, String> {
    // `call` takes the context because `@process.args` reads `ctx.script_args`;
    // `which` ignores it.
    let ctx = RuntimeContext::new();
    ProcessCap
        .call(
            "which",
            vec![Value::string(name)],
            perms,
            &ctx,
            &Default::default(),
        )
        .await
        .map_err(|e| e.to_string())
}

fn grant(specs: &[&str]) -> PermissionSet {
    let mut p = PermissionSet::default_secure();
    for s in specs {
        p.grant(Permission::parse(s).expect("permission spec"));
    }
    p
}

#[tokio::test]
async fn denied_without_any_permission() {
    let err = which(&PermissionSet::default_secure(), "sh")
        .await
        .expect_err("process denied by default");
    assert!(err.contains("process permission denied"), "{err}");
}

#[tokio::test]
async fn process_alone_does_not_expose_path() {
    let err = which(&grant(&["process"]), "sh")
        .await
        .expect_err("reading PATH needs the env capability too");
    assert!(
        err.contains("PATH") && err.contains("--allow env=PATH"),
        "error should name the missing grant, got: {err}"
    );
}

#[tokio::test]
async fn env_alone_is_not_enough() {
    let err = which(&grant(&["env"]), "sh")
        .await
        .expect_err("still needs process");
    assert!(err.contains("process permission denied"), "{err}");
}

#[tokio::test]
async fn process_plus_env_path_resolves() {
    for specs in [
        vec!["process", "env=PATH"],
        vec!["process", "env"],
        vec!["all"],
    ] {
        let v = which(&grant(&specs), "sh")
            .await
            .unwrap_or_else(|e| panic!("which(sh) with {specs:?} failed: {e}"));
        match v {
            Value::Result(ResultValue::Ok(inner)) => {
                let path = format!("{inner}");
                assert!(path.ends_with("/sh"), "unexpected path {path}");
            }
            // A machine without /bin/sh would legitimately report err — the point
            // of this test is that permission checking no longer blocks the call.
            Value::Result(ResultValue::Err(inner)) => {
                let msg = format!("{inner}");
                assert!(msg.contains("not found"), "unexpected err: {msg}");
            }
            other => panic!("expected a result, got {other}"),
        }
    }
}

#[tokio::test]
async fn env_grant_for_another_variable_does_not_help() {
    let err = which(&grant(&["process", "env=HOME"]), "sh")
        .await
        .expect_err("env=HOME is not env=PATH");
    assert!(err.contains("--allow env=PATH"), "{err}");
}

#[test]
fn which_descriptor_documents_the_env_requirement() {
    let d = ProcessCap::DESCRIPTORS
        .iter()
        .find(|d| d.name == "which")
        .expect("which descriptor");
    assert!(
        d.docs.contains("env=PATH"),
        "descriptor docs must state the env requirement: {}",
        d.docs
    );
    // Reading PATH and stat-ing candidates observes outside state, and
    // rite_sem::resolve::HOST_EFFECTS classifies `process.which` as effectful —
    // the descriptor (what `rite capabilities` and the docs report) must agree,
    // or E021 and the documentation contradict each other.
    assert!(d.effectful, "which observes the environment and filesystem");
    assert_eq!(d.permission, "process");
}

/// `@process.args` reports what the host put in `script_args`, and needs no grant —
/// the arguments are the invoker's own input to this program, unlike `run`/`which`,
/// which reach outside it.
#[tokio::test]
async fn args_reads_script_args_without_any_permission() {
    let mut ctx = RuntimeContext::new();
    ctx.script_args = vec!["alpha".into(), "beta".into()];
    let out = ProcessCap
        .call(
            "args",
            vec![],
            &PermissionSet::default_secure(),
            &ctx,
            &Default::default(),
        )
        .await
        .expect("args must not require a grant");
    assert_eq!(
        out,
        Value::list(vec![Value::string("alpha"), Value::string("beta")])
    );
}

#[tokio::test]
async fn args_is_empty_when_the_host_set_none() {
    let ctx = RuntimeContext::new();
    let out = ProcessCap
        .call(
            "args",
            vec![],
            &PermissionSet::default_secure(),
            &ctx,
            &Default::default(),
        )
        .await
        .expect("args");
    assert_eq!(out, Value::list(Vec::<Value>::new()));
}
