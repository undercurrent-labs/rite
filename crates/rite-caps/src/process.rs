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
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "args",
            docs: "Arguments passed to this script after `--`, as a list of strings. Needs no permission: they are the invoker's own input to this program, not ambient state.",
            arity: 0,
            effectful: true,
            permission: "",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "exit",
            docs: "End the run immediately with a chosen exit status, 0–255. Nothing after the call runs, and the status cannot be caught or overridden. Buffered output is still flushed. Needs no permission: choosing your own exit status is the invoker's own business, like reading `@process.args`. Note that 1–8 are also the statuses the runtime itself uses (1 runtime error, 5 permission denied, 8 budget); a script that picks one takes over that meaning for its own process.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "which",
            docs: "Locate an executable on PATH. Reads the PATH environment variable and probes the filesystem, so it is effectful (`!`) and needs process *and* env access to PATH (--allow process --allow env=PATH).",
            arity: 1,
            effectful: true,
            permission: "process",
            returns_result: true,
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        ctx: &RuntimeContext,
        env_overlay: &std::collections::BTreeMap<String, String>,
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
        // `exit` is exempt for the same reason as `args`, and for one more: an exit
        // status is what a script says to whoever ran it. Gating it behind the grant
        // that also permits running arbitrary binaries would mean a script had to be
        // trusted with subprocesses in order to say "I am done, status 2".
        if method == "exit" {
            let code = match args.first() {
                Some(Value::Int(n)) => *n,
                Some(other) => {
                    return Err(EvalError::Message(format!(
                        "process.exit expects an integer status 0–255, got `{other}`"
                    )));
                }
                None => {
                    return Err(EvalError::Message(
                        "process.exit expects an integer status 0–255".into(),
                    ));
                }
            };
            // Out of range is an error rather than a silent truncation: exiting 300
            // would otherwise become 44 on POSIX, which is a wrong answer dressed as
            // a right one. Unlike the status itself, this check is deterministic —
            // it cannot fire only for the value some subprocess happened to return.
            let code = u8::try_from(code).map_err(|_| {
                EvalError::Message(format!(
                    "process.exit: status {code} is out of range — must be 0–255"
                ))
            })?;
            return Err(EvalError::Exit(code));
        }
        perms.check_process().map_err(EvalError::Permission)?;
        match method {
            "run" => {
                let cmd = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("process.run expects command".into()))?
                    .to_string();
                // Anything that is not a list was silently treated as "no arguments",
                // so `@process.run("sh", ⟦"-c", "…"⟧, ⟨⟩)` — a block where a list
                // belongs, an easy thing to type — ran a bare `sh` that read stdin
                // to EOF and reported success. The command *did* run and *did*
                // answer `ok`, which is the worst way to be wrong. Same reasoning as
                // the options record below: a mistake here must not be
                // indistinguishable from the default.
                let mut argv: Vec<String> = Vec::new();
                match args.get(1) {
                    None | Some(Value::None) => {}
                    Some(Value::List(xs)) => {
                        for x in xs {
                            if let Some(s) = x.as_str() {
                                argv.push(s.to_string());
                            } else {
                                argv.push(format!("{}", x));
                            }
                        }
                    }
                    Some(other) => {
                        return Err(EvalError::Message(format!(
                            "process.run: arguments must be a list, got `{other}`"
                        )));
                    }
                }
                let mut command = tokio::process::Command::new(&cmd);
                command
                    .args(&argv)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());

                // Anything this run set with `@env.set` — see the module
                // documentation in `env.rs` for why that is an overlay rather
                // than the process environment. Applied *before* the caller's
                // `env` record, so an explicit option on this call wins over a
                // variable set earlier.
                for (name, value) in env_overlay {
                    command.env(name, value);
                }

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

                let mut child = command
                    .spawn()
                    .map_err(|e| EvalError::Capability(e.to_string()))?;
                // Both pipes drain concurrently — a child blocked on a full
                // stderr pipe deadlocks a sequential reader — and each drain
                // stops at the configured `max_string_size`. The capture is a
                // buffer the script will hold, so the ceiling applies while it
                // is being filled, not after; `.output()` buffered everything
                // the child cared to write first.
                let cap = ctx.budget.limits().max_string_size;
                async fn drain(
                    stream: Option<impl tokio::io::AsyncRead + Unpin>,
                    cap: usize,
                ) -> Result<Vec<u8>, ()> {
                    use tokio::io::AsyncReadExt;
                    let Some(mut s) = stream else {
                        return Ok(Vec::new());
                    };
                    let mut out = Vec::new();
                    let mut buf = [0u8; 8192];
                    loop {
                        match s.read(&mut buf).await {
                            Ok(0) => return Ok(out),
                            Ok(n) => {
                                if out.len().saturating_add(n) > cap {
                                    return Err(());
                                }
                                out.extend_from_slice(&buf[..n]);
                            }
                            Err(_) => return Ok(out),
                        }
                    }
                }
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let (out, err) = tokio::join!(drain(stdout, cap), drain(stderr, cap));
                let (Ok(stdout), Ok(stderr)) = (out, err) else {
                    let _ = child.kill().await;
                    return Err(EvalError::Budget(
                        rite_runtime::budget::BudgetError::StringTooLarge {
                            who: "process.run output capture".into(),
                            len: cap.saturating_add(1),
                            max: cap,
                        },
                    ));
                };
                let status = child
                    .wait()
                    .await
                    .map_err(|e| EvalError::Capability(e.to_string()))?;
                Ok(Value::ok(Value::record(vec![
                    (
                        Key::String("status".into()),
                        Value::Int(status.code().unwrap_or(-1) as i64),
                    ),
                    (
                        Key::String("stdout".into()),
                        Value::string(String::from_utf8_lossy(&stdout).to_string()),
                    ),
                    (
                        Key::String("stderr".into()),
                        Value::string(String::from_utf8_lossy(&stderr).to_string()),
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
