use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, RuntimeContext, Value};

pub struct ConsoleCap;

impl ConsoleCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "print",
            docs: "Write a value to stdout without a trailing newline.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
        NativeFunctionDescriptor {
            name: "println",
            docs: "Write a value to stdout with a trailing newline.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
        NativeFunctionDescriptor {
            name: "warn",
            docs: "Write a warning to stderr.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
        NativeFunctionDescriptor {
            name: "error",
            docs: "Write an error to stderr.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
        NativeFunctionDescriptor {
            name: "inspect",
            docs: "Write a debug representation to stdout.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
        NativeFunctionDescriptor {
            name: "read_line",
            docs: "Read a line from stdin with optional prompt.",
            arity: 1,
            effectful: true,
            permission: "console",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        _ctx: &RuntimeContext,
    ) -> Result<Value, EvalError> {
        perms.check_console().map_err(EvalError::Permission)?;
        let msg = args
            .first()
            .map(|v| format!("{}", v))
            .unwrap_or_default();
        match method {
            "print" => {
                print!("{}", msg);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                Ok(Value::None)
            }
            "println" => {
                println!("{}", msg);
                Ok(Value::None)
            }
            "warn" => {
                eprintln!("{}", msg);
                Ok(Value::None)
            }
            "error" => {
                eprintln!("{}", msg);
                Ok(Value::None)
            }
            "inspect" => {
                println!("{:?}", args.first());
                Ok(Value::None)
            }
            "read_line" => {
                if let Some(prompt) = args.first().and_then(|v| v.as_str()) {
                    print!("{}", prompt);
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
                let mut line = String::new();
                std::io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| EvalError::Capability(e.to_string()))?;
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Value::string(line))
            }
            other => Err(EvalError::Capability(format!(
                "unknown @console.{}",
                other
            ))),
        }
    }
}
