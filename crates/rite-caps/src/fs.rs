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
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "read_bytes",
            docs: "Read a file as bytes.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "write",
            docs: "Write text to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "append",
            docs: "Append text to a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "lines",
            docs: "Read file lines as a list of strings.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "open",
            docs: "Open a file and answer a handle: `! @fs.open(path, #read)?`. Modes are `#read`, `#write` (creates, truncates) and `#append` (creates, keeps). The permission is decided here — `#read` needs `fs:read` for the path, the other two need `fs:write` — so the reads and writes that follow need no further grant. A handle is closed with `@fs.close`, or when the run ends; one run may hold 1024 open at once.",
            arity: 2,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "read_chunk",
            docs: "Read up to `n` bytes from an open handle, answering `ok(bytes)`. Fewer than `n` means the end of the file is near; **empty** means it has been reached. Unlike `@fs.read_bytes`, peak memory is the chunk rather than the file.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "read_line",
            docs: "Read one line from an open handle, without its line ending. Answers `ok(none)` at the end of the file — an empty line is `ok(\"\")`, which is a different thing, and why this reports the end with `none` rather than an empty string.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "write_chunk",
            docs: "Write text or bytes to a handle opened `#write` or `#append`, answering `ok(count)` of bytes written. Buffered: call `@fs.flush` or `@fs.close` to be sure it reached the disk.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "seek",
            docs: "Move an open handle to a byte offset from the start of the file, answering `ok(position)`. A negative offset counts back from the end, as `slice` does.",
            arity: 2,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "flush",
            docs: "Push a write handle's buffered bytes to the file. `@fs.close` flushes too; this is for when you want the bytes there and the handle still open.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "close",
            docs: "Close an open handle, flushing it first. Closing one that is already closed is `ok`, not an error — a script that closes on both the success and the failure path is being careful, not wrong.",
            arity: 1,
            effectful: true,
            permission: "",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "exists",
            docs: "Check whether a path exists.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: false,
        },
        NativeFunctionDescriptor {
            name: "metadata",
            docs: "Return a file metadata record: `len`, `is_file`, `is_dir`, `is_symlink`, and `mtime` as an RFC3339 UTC string (comparable against `@clock.now`). Follows symlinks, so every field but `is_symlink` describes the target.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "glob",
            docs: "Expand a glob pattern to matching paths. The pattern must point inside a granted read root; matches outside every granted root are dropped.",
            arity: 1,
            effectful: true,
            permission: "fs:read",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "mkdir",
            docs: "Create a directory.",
            arity: 1,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "remove",
            docs: "Remove a file, or a directory and everything inside it. Recursive and irreversible, like `rm -rf`.",
            arity: 1,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "copy",
            docs: "Copy a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
        },
        NativeFunctionDescriptor {
            name: "move",
            docs: "Move/rename a file.",
            arity: 2,
            effectful: true,
            permission: "fs:write",
            returns_result: true,
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
        ctx: &rite_runtime::RuntimeContext,
    ) -> Result<Value, EvalError> {
        let atoms = &*ctx.atoms;
        match method {
            "open" => open_handle(&args, perms, ctx),
            "read_chunk" => read_chunk(&args, ctx),
            "read_line" => read_line(&args, ctx),
            "write_chunk" => write_chunk(&args, ctx, atoms),
            "seek" => seek(&args, ctx),
            "flush" => flush(&args, ctx),
            "close" => close_handle(&args, ctx),
            "read" => {
                let path = path_arg_for("fs.read", &args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                check_file_size("fs.read", &path, ctx)?;
                match std::fs::read_to_string(&path) {
                    Ok(s) => Ok(Value::ok(Value::string(s))),
                    Err(e) => Ok(io_err("fs.read", &path, e)),
                }
            }
            "read_bytes" => {
                let path = path_arg_for("fs.read_bytes", &args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                check_file_size("fs.read_bytes", &path, ctx)?;
                match std::fs::read(&path) {
                    Ok(b) => Ok(Value::ok(Value::Bytes(b.into()))),
                    Err(e) => Ok(io_err("fs.read_bytes", &path, e)),
                }
            }
            "write" => {
                let path = path_arg_for("fs.write", &args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                // Required. This was `.unwrap_or_default()`, so `@fs.write(path)`
                // with the content argument left off wrote an *empty file* — and
                // `std::fs::write` truncates, so a typo at the call site destroyed
                // whatever was there.
                let content = crate::args::required("fs.write", &args, 1)?.to_display(atoms);
                match std::fs::write(&path, content) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.write", &path, e)),
                }
            }
            "append" => {
                let path = path_arg_for("fs.append", &args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                let content = crate::args::required("fs.append", &args, 1)?.to_display(atoms);
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
                let path = path_arg_for("fs.lines", &args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                check_file_size("fs.lines", &path, ctx)?;
                match std::fs::read_to_string(&path) {
                    Ok(s) => {
                        let lines: Vec<Value> = s.lines().map(Value::string).collect();
                        ctx.budget
                            .limits()
                            .check_collection(lines.len(), "fs.lines")?;
                        Ok(Value::ok(Value::list(lines)))
                    }
                    Err(e) => Ok(io_err("fs.lines", &path, e)),
                }
            }
            "exists" => {
                let path = path_arg_for("fs.exists", &args, 0)?;
                let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
                Ok(Value::Bool(path.exists()))
            }
            "metadata" => {
                let requested = path_arg_for("fs.metadata", &args, 0)?;
                let path = perms
                    .check_fs_read(&requested)
                    .map_err(EvalError::Permission)?;
                match std::fs::metadata(&path) {
                    Ok(m) => Ok(Value::ok(Value::record(vec![
                        (Key::String("len".into()), Value::Int(m.len() as i64)),
                        (Key::String("is_file".into()), Value::Bool(m.is_file())),
                        (Key::String("is_dir".into()), Value::Bool(m.is_dir())),
                        (
                            Key::String("is_symlink".into()),
                            // Deliberately the path as written, not the checked one:
                            // `check_fs_read` canonicalizes, which *resolves* links, so
                            // asking the returned path is always false. Safe because the
                            // canonical target was already permission-checked above — this
                            // only asks whether the requested spelling was a link, and
                            // reads nothing the grant does not already cover.
                            Value::Bool(is_symlink(&requested)),
                        ),
                        (Key::String("mtime".into()), mtime(&m)),
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
                let path = path_arg_for("fs.mkdir", &args, 0)?;
                let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
                match std::fs::create_dir_all(&path) {
                    Ok(()) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.mkdir", &path, e)),
                }
            }
            "remove" => {
                let path = path_arg_for("fs.remove", &args, 0)?;
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
                let from = path_arg_for("fs.copy", &args, 0)?;
                let to = path_arg_for("fs.copy", &args, 1)?;
                let from = perms.check_fs_read(&from).map_err(EvalError::Permission)?;
                let to = perms.check_fs_write(&to).map_err(EvalError::Permission)?;
                match std::fs::copy(&from, &to) {
                    Ok(_) => Ok(Value::ok(Value::None)),
                    Err(e) => Ok(io_err("fs.copy", &from, e)),
                }
            }
            "move" => {
                let from = path_arg_for("fs.move", &args, 0)?;
                let to = path_arg_for("fs.move", &args, 1)?;
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

/// The path argument, as a `PathBuf`.
///
/// `who` names the call: "expected path string" left a caller to work out which of
/// the `@fs` calls on the line had complained.
fn path_arg_for(who: &str, args: &[Value], i: usize) -> Result<PathBuf, EvalError> {
    crate::args::str_arg(who, args, i).map(PathBuf::from)
}

/// Modification time as an RFC3339 UTC string, or `none` where the platform or
/// filesystem does not record one.
///
/// Rite has exactly one spelling of a timestamp and this is it: the same
/// `to_rfc3339()` rendering `@clock.now` produces, so an mtime round-trips
/// through `@clock.parse` and can be compared against `@clock.now` directly.
///
/// That comparison is the point, and it is why the *rendering* matters and not
/// merely the format. chrono writes a UTC offset as `+00:00`, and mixed
/// sub-second precision still orders correctly under plain string comparison
/// (`…:00+00:00` < `…:00.5+00:00`, because `+` sorts below `.`). Switching to
/// the `Z` spelling would silently break it, since `Z` sorts *above* both, so a
/// file modified at the same second as `@clock.now` would compare as later.
/// Rite has no date arithmetic, so ordering is the only tool a script has here.
fn mtime(m: &std::fs::Metadata) -> Value {
    match m.modified() {
        Ok(t) => Value::string(chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
        Err(_) => Value::None,
    }
}

/// Whether the path itself is a symbolic link.
///
/// Every other field in the record describes what the path *resolves to* —
/// `std::fs::metadata` follows links, so a symlink to a file reports
/// `is_file: true` with the target's length. That is the split `ls -l` shows,
/// and telling them apart needs the second, non-following stat.
///
/// Must be given the path as the script wrote it. `check_fs_read` canonicalizes,
/// and canonicalization resolves links, which is how a symlink is stopped from
/// escaping a granted root, so that behaviour stays. It does mean the checked
/// path can never answer `true` here.
///
/// A broken link answers `false` only because `metadata` already failed and this is
/// never reached; see the note in the Files chapter.
fn is_symlink(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|l| l.file_type().is_symlink())
        .unwrap_or(false)
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

// ── Open handles ─────────────────────────────────────────────────────────────
//
// Every `@fs` read before this one was whole-file: `read` and `lines` are
// `read_to_string`, `read_bytes` is `read`. Peak memory was the size of the file
// and nothing could be processed as it arrived — `@fs.lines` was line-by-line as
// an *interface* only, reading everything and then splitting, so at its peak it
// cost more than `read` did.
//
// The handle convention is `@tcp`'s, deliberately: open → opaque handle → close,
// and closing twice is fine. What is different is where the resource lives. A
// `@tcp` connection sits in a process-global map; these sit on the run's
// `RuntimeContext`, so anything a script leaves open is closed when the run ends
// rather than when the process does. Under `rite run` that distinction is
// invisible. Inside an embedder it is the difference between a guest leaking a
// descriptor for the lifetime of the host and not.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};

/// What the handle table holds for one open file.
enum OpenFile {
    /// Buffered, so `read_line` does not read one byte at a time.
    Read(BufReader<std::fs::File>),
    Write(std::io::BufWriter<std::fs::File>),
}

const FILE_HANDLE: &str = "file";

fn handle_id(v: Option<&Value>) -> Result<u64, EvalError> {
    match v {
        Some(Value::Handle(h)) if h.kind == FILE_HANDLE => Ok(h.id),
        Some(Value::Handle(h)) => Err(EvalError::Message(format!(
            "expected an open file, got a `{}` handle",
            h.kind
        ))),
        Some(other) => Err(EvalError::Message(format!(
            "expected an open file handle, got {}",
            other.type_name()
        ))),
        None => Err(EvalError::Message("expected an open file handle".into())),
    }
}

/// A handle that is not in the table is closed, or was never open.
fn closed() -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("io.closed")),
        (
            Key::String("message".into()),
            Value::string("this file handle is closed"),
        ),
    ]))
}

fn open_handle(
    args: &[Value],
    perms: &PermissionSet,
    ctx: &rite_runtime::RuntimeContext,
) -> Result<Value, EvalError> {
    let path = path_arg_for("fs.open", args, 0)?;
    // The mode is an atom (`#read`), so a typo is a resolve-time unknown rather
    // than a string that silently means something else.
    let mode = match args.get(1) {
        Some(Value::Atom(id)) => ctx.atoms.name(*id),
        Some(Value::String(s)) => s.to_string(),
        _ => {
            return Err(EvalError::Message(
                "fs.open expects a mode: #read, #write or #append".into(),
            ))
        }
    };

    // The grant is decided here, once, against the mode being asked for. Reads and
    // writes on the handle afterwards carry no path to check — which is precisely
    // why opening has to be strict.
    let (path, options) = match mode.as_str() {
        "read" => {
            let path = perms.check_fs_read(&path).map_err(EvalError::Permission)?;
            let mut o = std::fs::OpenOptions::new();
            o.read(true);
            (path, o)
        }
        "write" => {
            let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            (path, o)
        }
        "append" => {
            let path = perms.check_fs_write(&path).map_err(EvalError::Permission)?;
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).append(true);
            (path, o)
        }
        other => {
            return Err(EvalError::Message(format!(
                "fs.open: unknown mode `#{other}` — expected #read, #write or #append"
            )))
        }
    };

    let file = match options.open(&path) {
        Ok(f) => f,
        Err(e) => return Ok(io_err("fs.open", &path, e)),
    };
    let open = if mode == "read" {
        OpenFile::Read(BufReader::new(file))
    } else {
        OpenFile::Write(std::io::BufWriter::new(file))
    };

    match ctx.handles.insert(FILE_HANDLE, Box::new(open)) {
        Ok(id) => Ok(Value::ok(Value::Handle(rite_runtime::HostHandle {
            kind: FILE_HANDLE.to_string(),
            id,
        }))),
        // A raise rather than an `err`: a script that has run out of handles is
        // leaking them in a loop, and handing it another result to ignore would
        // let it keep going until the operating system objected in less useful
        // words, from whichever unrelated call happened to be next.
        Err(limit) => Err(EvalError::Message(format!(
            "fs.open: too many open file handles ({limit}). A handle is closed with \
             @fs.close, or when the run ends"
        ))),
    }
}

/// Borrow an open file, or answer the closed error.
fn with_file<R>(
    args: &[Value],
    ctx: &rite_runtime::RuntimeContext,
    f: impl FnOnce(&mut OpenFile) -> R,
) -> Result<Result<R, Value>, EvalError> {
    let id = handle_id(args.first())?;
    Ok(ctx.handles.with::<OpenFile, R>(id, f).ok_or_else(closed))
}

fn wrong_mode(wanted: &str) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("io.wrong_mode")),
        (
            Key::String("message".into()),
            Value::string(format!(
                "this handle was not opened for {wanted} — see the mode given to @fs.open"
            )),
        ),
    ]))
}

fn simple_io_err(op: &str, e: std::io::Error) -> Value {
    Value::err(Value::record(vec![
        (Key::String("kind".into()), Value::string("io.error")),
        (Key::String("message".into()), Value::string(e.to_string())),
        (Key::String("operation".into()), Value::string(op)),
    ]))
}

/// Refuse to buffer a file over the run's `max_string_size`, before paying the
/// allocation the ceiling exists to prevent. Sized from metadata; a file that
/// grows between the check and the read still cannot grow the *check*, and the
/// knob defaults to unlimited so nothing changes for an unconfigured run.
fn check_file_size(
    who: &str,
    path: &std::path::Path,
    ctx: &rite_runtime::RuntimeContext,
) -> Result<(), EvalError> {
    let limits = ctx.budget.limits();
    if limits.max_string_size == usize::MAX {
        return Ok(());
    }
    if let Ok(m) = std::fs::metadata(path) {
        limits.check_string(m.len() as usize, who)?;
    }
    Ok(())
}

fn read_chunk(args: &[Value], ctx: &rite_runtime::RuntimeContext) -> Result<Value, EvalError> {
    let n = args
        .get(1)
        .and_then(|v| v.as_int())
        .ok_or_else(|| EvalError::Message("fs.read_chunk expects a byte count".into()))?;
    if n < 0 {
        return Err(EvalError::Message(
            "fs.read_chunk: byte count cannot be negative".into(),
        ));
    }
    // The buffer is allocated at the caller's size before anything is read, so
    // the caller's size is what the ceiling applies to.
    ctx.budget
        .limits()
        .check_string(n as usize, "fs.read_chunk")?;
    let outcome = with_file(args, ctx, |file| match file {
        OpenFile::Write(_) => wrong_mode("reading"),
        OpenFile::Read(r) => {
            let mut buf = vec![0u8; n as usize];
            let mut filled = 0usize;
            // One `read` can stop short of the buffer without being at the end of
            // the file, so a short read must not be reported as the end.
            while filled < buf.len() {
                match r.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(k) => filled += k,
                    Err(e) => return simple_io_err("fs.read_chunk", e),
                }
            }
            buf.truncate(filled);
            Value::ok(Value::Bytes(buf.into()))
        }
    })?;
    Ok(outcome.unwrap_or_else(|e| e))
}

fn read_line(args: &[Value], ctx: &rite_runtime::RuntimeContext) -> Result<Value, EvalError> {
    let outcome = with_file(args, ctx, |file| match file {
        OpenFile::Write(_) => wrong_mode("reading"),
        OpenFile::Read(r) => {
            let mut line = String::new();
            match r.read_line(&mut line) {
                // Nothing read at all is the end of the file. An empty line is
                // `"\n"` here, so the two never collide.
                Ok(0) => Value::ok(Value::None),
                Ok(_) => {
                    // The terminator is stripped, and `\r\n` counts as one, so a
                    // file written on Windows does not leave a stray `\r` at the
                    // end of every line a script compares.
                    let trimmed = line.strip_suffix('\n').unwrap_or(&line);
                    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
                    Value::ok(Value::string(trimmed))
                }
                Err(e) => simple_io_err("fs.read_line", e),
            }
        }
    })?;
    Ok(outcome.unwrap_or_else(|e| e))
}

fn write_chunk(
    args: &[Value],
    ctx: &rite_runtime::RuntimeContext,
    atoms: &rite_runtime::AtomInterner,
) -> Result<Value, EvalError> {
    let bytes: Vec<u8> = match args.get(1) {
        Some(Value::Bytes(b)) => b.to_vec(),
        Some(Value::String(s)) => s.as_bytes().to_vec(),
        // Same reasoning as `@fs.write`: an atom rendered through `Display` would
        // put its interner index on disk.
        Some(other) => other.to_display(atoms).into_bytes(),
        None => return Err(EvalError::Message("fs.write_chunk expects data".into())),
    };
    let outcome = with_file(args, ctx, |file| match file {
        OpenFile::Read(_) => wrong_mode("writing"),
        OpenFile::Write(w) => match w.write_all(&bytes) {
            Ok(()) => Value::ok(Value::Int(bytes.len() as i64)),
            Err(e) => simple_io_err("fs.write_chunk", e),
        },
    })?;
    Ok(outcome.unwrap_or_else(|e| e))
}

fn seek(args: &[Value], ctx: &rite_runtime::RuntimeContext) -> Result<Value, EvalError> {
    let pos = args
        .get(1)
        .and_then(|v| v.as_int())
        .ok_or_else(|| EvalError::Message("fs.seek expects a byte offset".into()))?;
    // Negative counts back from the end, which is how `slice` reads a negative
    // index — one rule for both rather than a second convention to remember.
    let target = if pos < 0 {
        SeekFrom::End(pos)
    } else {
        SeekFrom::Start(pos as u64)
    };
    let outcome = with_file(args, ctx, |file| {
        let result = match file {
            OpenFile::Read(r) => r.seek(target),
            OpenFile::Write(w) => w.seek(target),
        };
        match result {
            Ok(p) => Value::ok(Value::Int(p as i64)),
            Err(e) => simple_io_err("fs.seek", e),
        }
    })?;
    Ok(outcome.unwrap_or_else(|e| e))
}

fn flush(args: &[Value], ctx: &rite_runtime::RuntimeContext) -> Result<Value, EvalError> {
    let outcome = with_file(args, ctx, |file| match file {
        // Flushing a reader is not an error: it is a no-op, and refusing it would
        // make a cleanup path that flushes everything it holds need to know which
        // kind each handle was.
        OpenFile::Read(_) => Value::ok(Value::None),
        OpenFile::Write(w) => match w.flush() {
            Ok(()) => Value::ok(Value::None),
            Err(e) => simple_io_err("fs.flush", e),
        },
    })?;
    Ok(outcome.unwrap_or_else(|e| e))
}

fn close_handle(args: &[Value], ctx: &rite_runtime::RuntimeContext) -> Result<Value, EvalError> {
    let id = handle_id(args.first())?;
    // Flush before dropping: a `BufWriter` that fails to flush on drop has nowhere
    // to report it, and losing the tail of a file silently is exactly the failure
    // this whole API exists to avoid.
    let flushed = ctx.handles.with::<OpenFile, _>(id, |file| match file {
        OpenFile::Write(w) => w.flush(),
        OpenFile::Read(_) => Ok(()),
    });
    ctx.handles.close(id);
    match flushed {
        Some(Err(e)) => Ok(simple_io_err("fs.close", e)),
        // `None` is a handle that was already closed, which is `ok` on purpose.
        _ => Ok(Value::ok(Value::None)),
    }
}
