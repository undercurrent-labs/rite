//! Ambient facts about the process and the machine.
//!
//! Separate from `@env`, which is about *variables*: putting the working
//! directory in there would make `@env.all()` incoherent, since it answers a
//! record of variables and `cwd` is not one.
//!
//! Everything here is effectful. None of it is constant for the life of a run:
//! `@process.run` takes a `cwd`, a temporary directory can be removed, and a
//! hostname can change.
//!
//! Denied by default, with `--allow sys`. These are small facts, but they are
//! facts about the machine rather than about the program, and the default-secure
//! posture (§18.5) is that ambient authority is asked for.
//!
//! # The browser
//!
//! `cwd`, `home`, `temp_dir`, `pid` and `hostname` have no meaning on `wasm32`
//! and are `#[cfg]`-guarded to say so rather than to panic. `os` and `arch` are
//! answered everywhere, because `std::env::consts` is honest about wasm.

use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Value};

pub struct SysCap;

impl SysCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "cwd",
            docs: "The process's current working directory, as a string. Effectful: it is where relative paths resolve from, and it is not a constant. Native only.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "home",
            docs: "The invoking user's home directory, or `none` when the platform does not say. Read from `HOME` (or `USERPROFILE` on Windows) but gated on `sys` rather than `env`, because it is asking where the user lives rather than reading their environment. Native only.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "temp_dir",
            docs: "The directory the platform offers for temporary files, as a string. Nothing is created; writing there still needs an `fs:write` grant. Native only.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "os",
            docs: "The operating system this is running on: `linux`, `macos`, `windows`, `wasm`, and so on. `std::env::consts::OS`.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "arch",
            docs: "The processor architecture: `x86_64`, `aarch64`, `wasm32`, and so on. `std::env::consts::ARCH`.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "pid",
            docs: "This process's identifier, as an integer. Native only.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "hostname",
            docs: "The machine's hostname, or `none` when it cannot be determined. Native only.",
            arity: 0,
            effectful: true,
            permission: "sys",
            returns_result: false,
        },
    ];

    pub async fn call(
        &self,
        method: &str,
        _args: Vec<Value>,
        perms: &PermissionSet,
    ) -> Result<Value, EvalError> {
        // Every method here is gated the same way, so the check is once rather
        // than seven times — a per-method check is seven chances to forget one.
        perms.check_sys(method).map_err(EvalError::Permission)?;
        match method {
            "os" => Ok(Value::string(std::env::consts::OS)),
            "arch" => Ok(Value::string(std::env::consts::ARCH)),
            "cwd" => native(method, || {
                std::env::current_dir()
                    .map(|p| Value::string(p.display().to_string()))
                    .map_err(|e| EvalError::Message(format!("sys.cwd: {e}")))
            }),
            "temp_dir" => native(method, || {
                Ok(Value::string(std::env::temp_dir().display().to_string()))
            }),
            "home" => native(method, || {
                Ok(match home_dir() {
                    Some(path) => Value::string(path),
                    None => Value::None,
                })
            }),
            "pid" => native(method, || Ok(Value::Int(i64::from(std::process::id())))),
            "hostname" => native(method, || {
                Ok(match hostname() {
                    Some(name) => Value::string(name),
                    None => Value::None,
                })
            }),
            other => Err(EvalError::Capability(format!("unknown @sys.{}", other))),
        }
    }
}

/// Run a native-only body, or say plainly that the browser has no answer.
///
/// A refusal rather than `none`: `none` from `@sys.cwd` would read as "there is
/// no working directory", which is a different claim from "this host cannot
/// tell you", and a script branching on it would take the wrong branch.
#[cfg(not(target_arch = "wasm32"))]
fn native(
    _method: &str,
    body: impl FnOnce() -> Result<Value, EvalError>,
) -> Result<Value, EvalError> {
    body()
}

#[cfg(target_arch = "wasm32")]
fn native(
    method: &str,
    _body: impl FnOnce() -> Result<Value, EvalError>,
) -> Result<Value, EvalError> {
    Err(EvalError::Capability(format!(
        "@sys.{method} is not available in the browser"
    )))
}

#[cfg(not(target_arch = "wasm32"))]
fn home_dir() -> Option<String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string_lossy().into_owned())
}

#[cfg(target_arch = "wasm32")]
fn home_dir() -> Option<String> {
    None
}

/// The hostname, without a dependency.
///
/// `/proc/sys/kernel/hostname` and `/etc/hostname` on Linux, then the
/// conventional environment variables, which is what is available on macOS and
/// Windows without linking `libc` or `winapi`. `None` when none of them answer,
/// which the descriptor documents.
#[cfg(not(target_arch = "wasm32"))]
fn hostname() -> Option<String> {
    for path in ["/proc/sys/kernel/hostname", "/etc/hostname"] {
        if let Ok(text) = std::fs::read_to_string(path) {
            let name = text.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    for var in ["HOSTNAME", "COMPUTERNAME"] {
        if let Some(value) = std::env::var_os(var) {
            let name = value.to_string_lossy().trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn hostname() -> Option<String> {
    None
}
