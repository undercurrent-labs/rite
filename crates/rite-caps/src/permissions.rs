use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    Console,
    FsRead(PathBuf),
    FsWrite(PathBuf),
    Net(String),
    Env(String),
    EnvAll,
    Process,
    Clock,
    Random,
    /// In-memory DuckDB only (`--allow db`).
    DbMemory,
    /// File-backed DuckDB under a path prefix (`--allow db=./data`).
    Db(PathBuf),
    All,
}

impl Permission {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec == "all" || spec == "*" {
            return Ok(Permission::All);
        }
        if spec == "console" {
            return Ok(Permission::Console);
        }
        if spec == "process" {
            return Ok(Permission::Process);
        }
        if spec == "clock" {
            return Ok(Permission::Clock);
        }
        if spec == "random" {
            return Ok(Permission::Random);
        }
        if let Some(rest) = spec.strip_prefix("fs:read=") {
            return Ok(Permission::FsRead(PathBuf::from(rest)));
        }
        if let Some(rest) = spec.strip_prefix("fs:write=") {
            return Ok(Permission::FsWrite(PathBuf::from(rest)));
        }
        if let Some(rest) = spec.strip_prefix("fs:read") {
            let p = if rest.is_empty() || rest == "=" {
                PathBuf::from(".")
            } else {
                PathBuf::from(rest.trim_start_matches('='))
            };
            return Ok(Permission::FsRead(p));
        }
        if let Some(rest) = spec.strip_prefix("fs:write") {
            let p = if rest.is_empty() || rest == "=" {
                PathBuf::from(".")
            } else {
                PathBuf::from(rest.trim_start_matches('='))
            };
            return Ok(Permission::FsWrite(p));
        }
        if let Some(rest) = spec.strip_prefix("net=") {
            return Ok(Permission::Net(rest.to_string()));
        }
        if let Some(rest) = spec.strip_prefix("env=") {
            return Ok(Permission::Env(rest.to_string()));
        }
        if spec == "env" {
            return Ok(Permission::EnvAll);
        }
        if spec == "db" {
            return Ok(Permission::DbMemory);
        }
        if let Some(rest) = spec.strip_prefix("db=") {
            return Ok(Permission::Db(PathBuf::from(rest)));
        }
        Err(format!("unknown permission spec: {}", spec))
    }
}

#[derive(Debug, Clone, Default)]
pub struct PermissionSet {
    pub allow_all: bool,
    pub console: bool,
    pub clock: bool,
    pub random: bool,
    pub process: bool,
    pub env_all: bool,
    pub env_vars: HashSet<String>,
    pub fs_read: Vec<PathBuf>,
    pub fs_write: Vec<PathBuf>,
    pub net: HashSet<String>,
    /// Allow in-memory DuckDB (`:memory:`).
    pub db_memory: bool,
    /// Allowed roots for file-backed DuckDB databases.
    pub db_paths: Vec<PathBuf>,
}

impl PermissionSet {
    /// Default security posture (§18.5).
    pub fn default_secure() -> Self {
        Self {
            allow_all: false,
            console: true,
            clock: true,
            random: true,
            process: false,
            env_all: false,
            env_vars: HashSet::new(),
            fs_read: Vec::new(),
            fs_write: Vec::new(),
            net: HashSet::new(),
            db_memory: false,
            db_paths: Vec::new(),
        }
    }

    /// Allow opening in-memory DBs (or any open when `allow_all`).
    pub fn check_db_open(&self) -> Result<(), String> {
        if self.allow_all || self.db_memory || !self.db_paths.is_empty() {
            Ok(())
        } else {
            Err("db permission denied (use --allow db or --allow db=./path)".into())
        }
    }

    /// Allow opening a file-backed database under an allowed root.
    pub fn check_db_path(&self, path: &Path) -> Result<PathBuf, String> {
        if self.allow_all {
            return Ok(canonicalize_loose(path));
        }
        if self.db_paths.is_empty() {
            return Err(format!(
                "db file permission denied for `{}` (use --allow db=./path)",
                path.display()
            ));
        }
        let canon = canonicalize_loose(path);
        for root in &self.db_paths {
            if path_under(&canon, root) {
                return Ok(canon);
            }
        }
        Err(format!(
            "db file permission denied for `{}`",
            path.display()
        ))
    }

    pub fn allow_all() -> Self {
        Self {
            allow_all: true,
            console: true,
            clock: true,
            random: true,
            process: true,
            env_all: true,
            db_memory: true,
            ..Self::default()
        }
    }

    pub fn grant(&mut self, p: Permission) {
        match p {
            Permission::All => *self = Self::allow_all(),
            Permission::Console => self.console = true,
            Permission::Clock => self.clock = true,
            Permission::Random => self.random = true,
            Permission::Process => self.process = true,
            Permission::EnvAll => self.env_all = true,
            // `--allow env=PATH,HOME` reads as two names. It used to be stored as the
            // single variable `"PATH,HOME"`, which can never exist, so the grant looked
            // accepted and silently granted nothing. An environment variable name
            // cannot contain a comma, so splitting here is unambiguous.
            Permission::Env(v) => {
                for name in v.split(',') {
                    let name = name.trim();
                    if !name.is_empty() {
                        self.env_vars.insert(name.to_string());
                    }
                }
            }
            Permission::FsRead(p) => self.fs_read.push(canonicalize_loose(&p)),
            Permission::FsWrite(p) => self.fs_write.push(canonicalize_loose(&p)),
            Permission::Net(h) => {
                self.net.insert(h);
            }
            Permission::DbMemory => {
                self.db_memory = true;
            }
            Permission::Db(p) => {
                self.db_memory = true; // file grant implies memory too
                self.db_paths.push(canonicalize_loose(&p));
            }
        }
    }

    pub fn deny(&mut self, p: Permission) {
        // Narrowing only: once allow_all is denied for a class, clear the blanket flag.
        match p {
            Permission::All => *self = Self::default_secure(),
            Permission::Console => {
                self.allow_all = false;
                self.console = false;
            }
            Permission::Clock => {
                self.allow_all = false;
                self.clock = false;
            }
            Permission::Random => {
                self.allow_all = false;
                self.random = false;
            }
            Permission::Process => {
                self.allow_all = false;
                self.process = false;
            }
            Permission::EnvAll => {
                self.allow_all = false;
                self.env_all = false;
                self.env_vars.clear();
            }
            Permission::Env(v) => {
                self.allow_all = false;
                // Same list form as the grant, so `--deny env=A,B` revokes both.
                for name in v.split(',') {
                    self.env_vars.remove(name.trim());
                }
            }
            Permission::FsRead(_) => {
                self.allow_all = false;
                self.fs_read.clear();
            }
            Permission::FsWrite(_) => {
                self.allow_all = false;
                self.fs_write.clear();
            }
            Permission::Net(_) => {
                self.allow_all = false;
                self.net.clear();
            }
            Permission::DbMemory | Permission::Db(_) => {
                self.allow_all = false;
                self.db_memory = false;
                self.db_paths.clear();
            }
        }
    }

    pub fn check_console(&self) -> Result<(), String> {
        if self.allow_all || self.console {
            Ok(())
        } else {
            Err("console permission denied".into())
        }
    }

    pub fn check_clock(&self) -> Result<(), String> {
        if self.allow_all || self.clock {
            Ok(())
        } else {
            Err("clock permission denied".into())
        }
    }

    pub fn check_random(&self) -> Result<(), String> {
        if self.allow_all || self.random {
            Ok(())
        } else {
            Err("random permission denied".into())
        }
    }

    pub fn check_process(&self) -> Result<(), String> {
        if self.allow_all || self.process {
            Ok(())
        } else {
            Err("process permission denied".into())
        }
    }

    pub fn check_env(&self, name: &str) -> Result<(), String> {
        if self.allow_all || self.env_all || self.env_vars.contains(name) {
            Ok(())
        } else {
            Err(format!("env permission denied for `{}`", name))
        }
    }

    pub fn check_fs_read(&self, path: &Path) -> Result<PathBuf, String> {
        if self.allow_all {
            return Ok(canonicalize_loose(path));
        }
        let canon = canonicalize_loose(path);
        for root in &self.fs_read {
            if path_under(&canon, root) {
                return Ok(canon);
            }
        }
        // also allow if write root covers it
        for root in &self.fs_write {
            if path_under(&canon, root) {
                return Ok(canon);
            }
        }
        Err(format!(
            "fs:read permission denied for `{}`",
            path.display()
        ))
    }

    pub fn check_fs_write(&self, path: &Path) -> Result<PathBuf, String> {
        if self.allow_all {
            return Ok(canonicalize_loose(path));
        }
        let canon = canonicalize_loose(path);
        for root in &self.fs_write {
            if path_under(&canon, root) {
                return Ok(canon);
            }
        }
        Err(format!(
            "fs:write permission denied for `{}`",
            path.display()
        ))
    }

    pub fn check_net(&self, host: &str) -> Result<(), String> {
        if self.allow_all || self.net.contains("*") || self.net.contains(host) {
            Ok(())
        } else {
            Err(format!("net permission denied for `{}`", host))
        }
    }
}

/// Resolve `path` to an absolute, symlink-free form, even when it does not exist yet.
///
/// Only **one** missing level used to be handled: the parent was canonicalized and
/// the final component re-attached. When the parent was missing too — `a/b` with
/// neither present, which is exactly `@fs.mkdir("a/b")` — canonicalizing it failed,
/// the relative path was kept, and it then sat under no granted root. A grant of the
/// working directory refused to create a directory inside the working directory.
///
/// So walk up to the deepest ancestor that *does* exist, canonicalize that, and
/// re-apply the missing tail. The tail cannot contain symlinks, because it does not
/// exist, which is what makes it safe to resolve `..` in it lexically here — leaving
/// it for `path_under`'s prefix test would let `granted/missing/../../etc` look like
/// it starts with `granted`.
pub(crate) fn canonicalize_loose(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    // Ancestors run longest-first, so the first that canonicalizes is the deepest
    // real directory. Everything past it is folded on by hand.
    for ancestor in abs.ancestors() {
        let Ok(mut out) = ancestor.canonicalize() else {
            continue;
        };
        let rest = abs.strip_prefix(ancestor).unwrap_or_else(|_| Path::new(""));
        for part in rest.components() {
            match part {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        return out;
    }
    abs
}

fn path_under(path: &Path, root: &Path) -> bool {
    let root = canonicalize_loose(root);
    let path = if path.exists() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    path.starts_with(&root)
}
