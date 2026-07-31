use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Key, Value};

pub struct EnvCap;

impl EnvCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "get",
            docs: "Get an environment variable or none.",
            arity: 1,
            effectful: true,
            permission: "env",
        },
        NativeFunctionDescriptor {
            name: "require",
            docs: "Get a required environment variable as result.",
            arity: 1,
            effectful: true,
            permission: "env",
        },
        NativeFunctionDescriptor {
            name: "all",
            docs: "Return the environment variables this script may read, as a record. With `--allow env` that is everything; with `--allow env=NAME,…` it is exactly the names granted. Denied when nothing is granted.",
            arity: 0,
            effectful: true,
            permission: "env",
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        match method {
            "get" => {
                let name = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("env.get expects name".into()))?;
                perms.check_env(name).map_err(EvalError::Permission)?;
                Ok(match std::env::var(name) {
                    Ok(v) => Value::string(v),
                    Err(_) => Value::None,
                })
            }
            "require" => {
                let name = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("env.require expects name".into()))?;
                perms.check_env(name).map_err(EvalError::Permission)?;
                match std::env::var(name) {
                    Ok(v) => Ok(Value::ok(Value::string(v))),
                    Err(_) => Ok(Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("env.missing")),
                        (
                            Key::String("message".into()),
                            Value::string(format!("missing env `{}`", name)),
                        ),
                    ]))),
                }
            }
            "all" => {
                // A scoped grant answers the scoped subset rather than being refused.
                // The filter below already said that was the intent, but the guard made
                // it unreachable, so `--allow env=PATH` failed outright on a call that
                // could only ever have returned PATH. Nothing is revealed that
                // `@env.get` would not already answer one name at a time; a grant of
                // nothing is still a denial.
                if !perms.allow_all && !perms.env_all && perms.env_vars.is_empty() {
                    return Err(EvalError::Permission(
                        "env.all requires env permission: grant `--allow env` for everything, or `--allow env=NAME,…` for a subset".into(),
                    ));
                }
                let mut rec = indexmap::IndexMap::new();
                for (k, v) in std::env::vars() {
                    if perms.allow_all || perms.env_all || perms.env_vars.contains(&k) {
                        rec.insert(Key::String(k), Value::string(v));
                    }
                }
                Ok(Value::Record(rec))
            }
            other => Err(EvalError::Capability(format!("unknown @env.{}", other))),
        }
    }
}
