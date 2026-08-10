//! Environment variables.
//!
//! # `@env.set` writes an overlay, not the process environment
//!
//! `std::env::set_var` mutates process-global state that C libraries read
//! without synchronisation. The runtime is multi-threaded — `@process.run`
//! spawns, `@http.listen` serves — and glibc's `setenv`/`getenv` race is a real
//! crash, not a theoretical one. Rust 2024 made `set_var` `unsafe` for exactly
//! this reason.
//!
//! So a write goes into an overlay this capability owns. `@env.get`,
//! `@env.require` and `@env.all` consult it first, and `@process.run` merges it
//! beneath the caller's explicit `env` record, so a subprocess started *by this
//! program* sees it. A subprocess started any other way does not, and the
//! descriptor for `set` says so — the alternative is a function that appears to
//! change the machine and does not.
//!
//! The overlay is per-`EnvCap`, which is per-run: two `RiteEngine`s in one
//! process do not see each other's writes. That is the same isolation every
//! other capability has.

use crate::args::{required, str_arg};
use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use parking_lot::RwLock;
use rite_runtime::{EvalError, Key, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct EnvCap {
    /// Variables this run has written. See the module documentation for why
    /// this is not the process environment.
    overlay: RwLock<BTreeMap<String, String>>,
}

impl EnvCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "get",
            docs: "Get an environment variable or none.",
            arity: 1,
            effectful: true,
            permission: "env",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "require",
            docs: "Get a required environment variable as result.",
            arity: 1,
            effectful: true,
            permission: "env",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "all",
            docs: "Return the environment variables this script may read, as a record. With `--allow env` that is everything; with `--allow env=NAME,…` it is exactly the names granted. Denied when nothing is granted.",
            arity: 0,
            effectful: true,
            permission: "env",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "set",
            docs: "Set an environment variable for this run. Needs `--allow env:write` (or `env:write=NAME`), which is a *separate* grant from the `env` read permission. The value is visible to `@env.get`, `@env.require` and `@env.all`, and is inherited by commands started with `@process.run`. It does not modify the operating-system environment of this process: writing that is unsafe while other threads are running, and a program started outside `@process.run` will not see it.",
            arity: 2,
            effectful: true,
            permission: "env:write",
            returns_result: false,
        },
    ];

    /// Everything this run has written, for `@process.run` to inherit.
    pub fn overlay(&self) -> BTreeMap<String, String> {
        self.overlay.read().clone()
    }

    /// Seed the overlay from a `--env-file`, before anything runs.
    pub fn seed(&self, values: impl IntoIterator<Item = (String, String)>) {
        let mut overlay = self.overlay.write();
        for (name, value) in values {
            overlay.insert(name, value);
        }
    }

    /// The overlay first, then the process environment.
    fn lookup(&self, name: &str) -> Option<String> {
        if let Some(value) = self.overlay.read().get(name) {
            return Some(value.clone());
        }
        std::env::var(name).ok()
    }

    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        match method {
            "get" => {
                let name = str_arg("env.get", &args, 0)?;
                perms.check_env(&name).map_err(EvalError::Permission)?;
                Ok(match self.lookup(&name) {
                    Some(v) => Value::string(v),
                    None => Value::None,
                })
            }
            "require" => {
                let name = str_arg("env.require", &args, 0)?;
                perms.check_env(&name).map_err(EvalError::Permission)?;
                match self.lookup(&name) {
                    Some(v) => Ok(Value::ok(Value::string(v))),
                    None => Ok(Value::err(Value::record(vec![
                        (Key::String("kind".into()), Value::string("env.missing")),
                        (
                            Key::String("message".into()),
                            Value::string(format!("missing env `{}`", name)),
                        ),
                    ]))),
                }
            }
            "set" => {
                let name = str_arg("env.set", &args, 0)?;
                let value = required("env.set", &args, 1)?;
                let Some(value) = value.as_str() else {
                    return Err(EvalError::Message(format!(
                        "env.set expects a string value, got `{}`",
                        value.type_name()
                    )));
                };
                if name.is_empty() || name.contains('=') || name.contains('\0') {
                    return Err(EvalError::Message(format!(
                        "env.set: `{name}` is not a usable variable name"
                    )));
                }
                perms
                    .check_env_write(&name)
                    .map_err(EvalError::Permission)?;
                self.overlay.write().insert(name, value.to_string());
                Ok(Value::None)
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
                let visible =
                    |k: &String| perms.allow_all || perms.env_all || perms.env_vars.contains(k);
                // The process environment first, then the overlay over the top,
                // so a variable this run wrote reads back as what it wrote —
                // the same precedence `get` uses.
                let mut merged: BTreeMap<String, String> = std::env::vars().collect();
                merged.extend(
                    self.overlay
                        .read()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone())),
                );
                let mut rec = indexmap::IndexMap::new();
                for (k, v) in merged {
                    if visible(&k) {
                        rec.insert(Key::String(k), Value::string(v));
                    }
                }
                Ok(Value::Record(rec))
            }
            other => Err(EvalError::Capability(format!("unknown @env.{}", other))),
        }
    }
}
