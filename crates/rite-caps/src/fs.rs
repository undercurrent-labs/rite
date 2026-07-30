use crate::permissions::PermissionSet;
use crate::registry::NativeFunctionDescriptor;
use rite_runtime::{EvalError, Key, Value};
use std::path::PathBuf;

pub struct FsCap;

impl FsCap {
    pub const DESCRIPTORS: &'static [NativeFunctionDescriptor] = &[
        NativeFunctionDescriptor {
            name: "read",
            docs: "Read a UTF-8 text file.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "read_bytes",
            docs: "Read a file as bytes.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "write",
            docs: "Write text to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
        },
        NativeFunctionDescriptor {
            name: "append",
            docs: "Append text to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
        },
        NativeFunctionDescriptor {
            name: "lines",
            docs: "Read file lines as a list of strings.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "exists",
            docs: "Check whether a path exists.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "metadata",
            docs: "Return file metadata record.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "glob",
            docs: "Expand a glob pattern to matching paths. The pattern must point inside a granted read root; matches outside every granted root are dropped.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
        },
        NativeFunctionDescriptor {
            name: "mkdir",
            docs: "Create a directory.",
            arity: 1,
            effectful: true,
            permission: "fs:write",
        },
        NativeFunctionDescriptor {
            name: "remove",
            docs: "Remove a file or directory.",
            arity: 1,
            effectful: true,
            permission: "fs:write",
        },
        NativeFunctionDescriptor {
            name: "copy",
            docs: "Copy a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
        },
        NativeFunctionDescriptor {
            name: "move",
            docs: "Move/rename a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
        },
    ];

    /// `atoms` is needed to write a value out: `Display` cannot resolve an atom and
    /// renders it as its interner index, so `@fs.write(p, #ok)` wrote the bytes `#0` to
    /// the file. Writing the wrong content to a user's disk is the worst place for that
    /// bug to live, and it is the same one `str` and `join` had.
    pub async fn call(
        &self,
        method: &str,
        args: Vec<Value>,
        perms: &PermissionSet,
        atoms: &rite_runtime::AtomInterner,
    ) -> Result<Value, EvalError> {
        match method {
            "read" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::read_to_string(&path) {
                    Ok(s) => Ok(Value::ok(Value::string(s))),
                    Err(e) => Ok(io_err("fs.read", &path, e)),
                }
            }
            "read_bytes" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::read(&path) {
                    Ok(b) => Ok(Value::ok(Value::Bytes(b.into()))),
                    Err(e) => Ok(io_err("fs.read_bytes", &path, e)),
                }
            }
            "write" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let content = args.get(1).map(|v| v.to_display(atoms)).unwrap_or_default();
                match std::fs::write(&path, content) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.write", &path, e)),
                }
            }
            "append" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let content = args.get(1).map(|v| v.to_display(atoms)).unwrap_or_default();
                use std::io::Write;
                match std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| f.write_all(content.as_bytes()))
                {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.append", &path, e)),
                }
            }
            "lines" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::read_to_string(&path) {
                    Ok(s) => {
                        let lines: Vec<Value> = s.lines().map(Value::string).collect();
                        Ok(Value::ok(Value::list(lines)))
                    }
                    Err(e) => Ok(io_err("fs.lines", &path, e)),
                }
            }
            "exists" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                Ok(Value::Bool(path.exists()))
            }
            "metadata" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                match std::fs::metadata(&path) {
                    Ok(m) => Ok(Value::ok(Value::record(vec![
                        (Key::String("len".into()), Value::Int(m.len() as i64)),
                        (Key::String("is_file".into()), Value::Bool(m.is_file())),
                        (Key::String("is_dir".into()), Value::Bool(m.is_dir())),
                    ]))),
                    Err(e) => Ok(io_err("fs.metadata", &path, e)),
                }
            }
            "glob" => {
                let pattern = args
                    .first()
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| EvalError::Message("glob expects pattern string".into()))?;
                // A glob is a read of everything it expands to, so it is checked twice.
                // 1. The pattern's fixed prefix (everything before the first wildcard)
                //    must itself be readable. A pattern aimed outside the granted roots
                //    (`/etc/ssh/*`, `../*`) is an outright permission error, not an
                //    empty result — the script asked for something it may not have.
                let root = glob_prefix(pattern);
                perms.check_fs_read(&root).map_err(EvalError::Permission)?;
                let mut matches = Vec::new();
                for p in glob::glob(pattern)
                    .map_err(|e| EvalError::Capability(e.to_string()))?
                    .flatten()
                {
                    // 2. Each match is re-checked and non-permitted paths are dropped
                    //    silently: `**` legitimately walks into directories that a
                    //    symlink or a narrower root puts out of bounds, and erroring
                    //    on a stray match would make recursive globs unusable.
                    if perms.check_fs_read(&p).is_ok() {
                        matches.push(Value::string(p.display().to_string()));
                    }
                }
                Ok(Value::ok(Value::list(matches)))
            }
            "mkdir" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                match std::fs::create_dir_all(&path) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.mkdir", &path, e)),
                }
            }
            "remove" => {
                let path = path_arg(&args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match res {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.remove", &path, e)),
                }
            }
            "copy" => {
                let from = path_arg(&args, 0)?;
                let to = path_arg(&args, 1)?;
                let from = perms.check_fs_read(&from).map_err(EvalError::Permission)?;
                let to = perms.check_fs_write(&to).map_err(EvalError::Permission)?;
                match std::fs::copy(&from, &to) {
                    Ok(_) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.copy", &from, e)),
                }
            }
            "move" => {
                let from = path_arg(&args, 0)?;
                let to = path_arg(&args, 1)?;
                let from = perms.check_fs_write(&from).map_err(EvalError::Permission)?;
                let to = perms.check_fs_write(&to).map_err(EvalError::Permission)?;
                match std::fs::rename(&from, &to) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.move", &from, e)),
                }
            }
            other => Err(EvalError::Capability(format!("unknown @fs.{}", other))),
        }
    }
}

/// The fixed directory prefix of a glob pattern: every leading `/`-separated
/// component before the first one containing a wildcard (`*`, `?`, `[`, `{`).
/// `"data/**/*.csv"` → `data`, `"/etc/ssh/*"` → `/etc/ssh`, `"*.rite"` → `.`.
fn glob_prefix(pattern: &str) -> PathBuf {
    let has_meta = |s: &str| s.contains(['*', '?', '[', '{']);
    let mut kept: Vec<&str> = Vec::new();
    for part in pattern.split('/') {
        if has_meta(part) {
            break;
        }
        kept.push(part);
    }
    match kept.as_slice() {
        // No fixed prefix (`*.rite`) — relative to the working directory.
        [] => PathBuf::from("."),
        // Rooted pattern with nothing else fixed (`/*`).
        [""] => PathBuf::from("/"),
        parts => PathBuf::from(parts.join("/")),
    }
}

fn path_arg(args: &[Value], i: usize) -> Result<PathBuf, EvalError> {
    args.get(i)
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| EvalError::Message("expected path string".into()))
}

fn io_err(op: &str, path: &std::path::Path, e: std::io::Error) -> Value {
    let kind = match e.kind() {
        std::io::ErrorKind::NotFound => "io.not_found",
        std::io::ErrorKind::PermissionDenied => "io.permission_denied",
        std::io::ErrorKind::AlreadyExists => "io.already_exists",
        _ => "io.error",
    };
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string(kind)),
        (Key::String("message".into()), Value::string(e.to_string())),
        (Key::String("operation".into()), Value::string(op)),
        (
            Key::String("path".into()),
            Value::string(path.display().to_string()),
        ),
    ]))
}
