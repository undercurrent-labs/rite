use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Key, RuntimeContext, Value};
use std::process::Stdio;

pub struct ProcessCap;

impl ProcessCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "run",
            docs: "Run a command with an argument list (no shell). Answers `ok(⟨status, stdout, stderr⟩)`; a non-zero exit is still `ok`, but a command that cannot be started raises. The third argument is an options record understanding `cwd` (string) and `env` (record, added to the inherited environment); any other key is an error.",
            arity: 3,
            effectful: true,
            permission: "process",
        },
        NativeFunctionDescriptor {
            name: "args",
            docs: "Arguments passed to this script after `--`, as a list of strings. Needs no permission: they are the invoker's own input to this program, not ambient state.",
            arity: 0,
            effectful: true,
            permission: "",
        },
        NativeFunctionDescriptor {
            name: "which",
            docs: "Locate an executable on PATH. Reads the PATH environment variable and probes the filesystem, so it is effectful (`!`) and needs process *and* env access to PATH (--allow process --allow env=PATH).",
            arity: 1,
            effectful: true,
            permission: "process",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        // `args` is exempt from the `process` grant: spawning a subprocess and reading
        // the arguments you were handed are not the same privilege, and requiring
        // `--allow process` to read argv would mean a CLI script had to be able to run
        // arbitrary commands just to see its own flags.
        if method == "args" {
            return Ok(Value::list(
                ctx.script_args
                    .iter()
                    .map(|a| Value::string(a.clone()))
                    .collect::<Vec<_>>(),
            ));
        }
        perms.check_process().map_err(EvalError::Permission)?;
        match method {
            "run" => {
                let cmd = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("process.run expects command".into()))?
                    .to_string();
                let mut argv: Vec<String> = Vec::new();
                if let Some(Value::List(xs)) = args.get(1) {
                    for x in xs {
                        if let Some(s) = x.as_str() {
                            argv.push(s.to_string());
                        } else {
                            argv.push(format!("{}", x));
                        }
                    }
                }
                let mut command = tokio::process::Command::new(&cmd);
                command
                    .args(&argv)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                // The third argument was accepted and thrown away, so `⟨cwd: "…"⟩`
                // looked like it worked and silently did nothing. Unknown keys are an
                // error rather than ignored, for the same reason: a typo in an options
                // record should not be indistinguishable from the default.
                //
                // Neither key needs a permission of its own. `--allow process` already
                // permits running an arbitrary binary, and setting a child's directory
                // or environment reveals nothing back to the script.
                match args.get(2) {
                    None | Some(Value::None) => {}
                    Some(Value::Record(opts)) => {
                        for (key, value) in opts {
                            match key {
                                Key::String(k) if k == "cwd" => {
                                    let dir = value.as_str().ok_or_else(|| {
                                        EvalError::Message(
                                            "process.run: `cwd` must be a string".into(),
                                        )
                                    })?;
                                    command.current_dir(dir);
                                }
                                Key::String(k) if k == "env" => {
                                    let Value::Record(vars) = value else {
                                        return Err(EvalError::Message(
                                            "process.run: `env` must be a record".into(),
                                        ));
                                    };
                                    // Added to the inherited environment rather than
                                    // replacing it: a child that loses PATH usually
                                    // cannot start.
                                    for (name, v) in vars {
                                        let Key::String(name) = name else {
                                            return Err(EvalError::Message(
                                                "process.run: `env` names must be strings".into(),
                                            ));
                                        };
                                        match v.as_str() {
                                            Some(s) => command.env(name, s),
                                            None => command.env(name, format!("{}", v)),
                                        };
                                    }
                                }
                                other => {
                                    return Err(EvalError::Message(format!(
                                        "process.run: unknown option `{other}` — expected `cwd` or `env`"
                                    )));
                                }
                            }
                        }
                    }
                    Some(other) => {
                        return Err(EvalError::Message(format!(
                            "process.run: options must be a record, got `{other}`"
                        )));
                    }
                }

                let output = command
                    .output()
                    .await
                    .map_err(|e| EvalError::Capability(e.to_string()))?;
                Ok(Value::ok(Value::record(vec![
                    (
                        Key::String("status".into()),
                        Value::Int(output.status.code().unwrap_or(-1) as i64),
                    ),
                    (
                        Key::String("stdout".into()),
                        Value::string(String::from_utf8_lossy(&output.stdout).to_string()),
                    ),
                    (
                        Key::String("stderr".into()),
                        Value::string(String::from_utf8_lossy(&output.stderr).to_string()),
                    ),
                ])))
            }
            "which" => {
                let cmd = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("process.which expects name".into()))?;
                // Reading PATH is an env read, so it goes through the env capability
                // instead of around it: `process` alone must not turn into a way to
                // observe the environment. `@process.run` still needs only `process`
                // (it never reports env contents back to the script).
                perms.check_env("PATH").map_err(|_| {
                    EvalError::Permission(
                        "process.which reads the PATH environment variable: also needs `--allow env=PATH` (or --allow env / --allow-all)"
                            .into(),
                    )
                })?;
                // simple PATH search
                if let Ok(path_var) = std::env::var("PATH") {
                    for dir in std::env::split_paths(&path_var) {
                        let candidate = dir.join(cmd);
                        if candidate.is_file() {
                            return Ok(Value::ok(Value::string(candidate.display().to_string())));
                        }
                    }
                }
                Ok(Value::err(Value::string(format!("not found: {}", cmd))))
            }
            other => Err(EvalError::Capability(format!("unknown @process.{}", other))),
        }
    }
}
