//! Ahead-of-time compilation: IR → Rust → cargo build.

mod codegen;
pub mod lower;
mod perms;

use rite_caps::PermissionSet;
use rite_core::SourceMap;
use rite_sem::compile_path;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub use codegen::generate_from_ir;

/// Repository the generated crate falls back to when there is no local Rite
/// source checkout. Mirrors `[workspace.package] repository` in the root manifest.
const RITE_REPO_URL: &str = "https://github.com/undercurrent-labs/rite";

/// Crates the generated wrapper needs to find in a local checkout.
const REQUIRED_CRATES: [&str; 4] = ["rite-runtime", "rite-caps", "rite-sem", "rite-core"];

/// Where the generated crate gets `rite-*` from.
///
/// Note: publishing to crates.io is *not* an option today — `rite-caps` does not
/// exist there and the `rite-runtime` name is taken by an unrelated project
/// ("execution engine for Rite ceremonies"), so a version dependency would pull in
/// someone else's code. Do not "fix" this by switching to version deps.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DepSource {
    /// Path dependencies on a local Rite source checkout.
    Workspace(PathBuf),
    /// Git dependencies — the path for anyone who installed a released binary.
    Git { url: String, git_ref: String },
}

pub fn build_script(
    file: &Path,
    release: bool,
    emit_rust: bool,
    output: Option<&Path>,
    perms: &PermissionSet,
) -> Result<PathBuf, String> {
    let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    let (ir, diags, _sources) = compile_path(file);
    if diags.has_errors() {
        return Err(format!("compile errors: {} diagnostics", diags.len()));
    }
    let ir = ir.ok_or_else(|| "no IR".to_string())?;

    // Fail on the two "this cannot possibly work" setups before spending minutes
    // in cargo and emitting a confusing manifest error.
    ensure_cargo()?;
    let dep_source = resolve_dep_source()?;

    let hash = hex::encode(&Sha256::digest(text.as_bytes())[..16]);
    let build_dir = build_root().join(&hash);
    std::fs::create_dir_all(build_dir.join("src")).map_err(|e| e.to_string())?;

    // Grants were canonicalized against this directory, so it is what decides
    // which of them are re-resolved at the binary's runtime CWD (see `perms`).
    let build_cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    let generated = generate_from_ir(&ir, file)?;
    let main_rs = generate_main_rs(file, perms, &build_cwd);

    std::fs::write(build_dir.join("src/main.rs"), main_rs).map_err(|e| e.to_string())?;
    std::fs::write(build_dir.join("src/generated.rs"), &generated).map_err(|e| e.to_string())?;
    std::fs::write(
        build_dir.join("src/source_map.rs"),
        "// Source spans live inside embedded ProgramIr JSON.\n// rustc errors that reference generated.rs:line map to IR summary comments.\n",
    )
    .map_err(|e| e.to_string())?;

    // Also write raw IR for inspection
    std::fs::write(
        build_dir.join("program.ir.json"),
        serde_json::to_string_pretty(&ir).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    std::fs::write(
        build_dir.join("Cargo.toml"),
        generate_cargo_toml(&hash, &dep_source, uses_db(&ir)),
    )
    .map_err(|e| e.to_string())?;

    let manifest = serde_json::json!({
        "rite_version": env!("CARGO_PKG_VERSION"),
        "source": file.display().to_string(),
        "source_hash": hash,
        "allow_all": perms.allow_all,
        "permissions": perms::permissions_json(perms, &build_cwd),
        "profile": if release { "release" } else { "dev" },
        "deps": match &dep_source {
            DepSource::Workspace(root) => serde_json::json!({ "kind": "path", "root": root.display().to_string() }),
            DepSource::Git { url, git_ref } => serde_json::json!({ "kind": "git", "url": url, "ref": git_ref }),
        },
        "backend": "ir-json-embed",
    });
    std::fs::write(
        build_dir.join("rite-manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| e.to_string())?;

    if emit_rust {
        println!("emitted rust + IR in {}", build_dir.display());
    }

    // Cargo artifacts go to a shared cache dir rather than into the user's project:
    // a debug build of the runtime is on the order of a gigabyte, and sharing it
    // means the second `rite build` reuses the compiled dependencies instead of
    // paying the full cold-build cost again. Cargo locks the directory, so
    // concurrent builds serialize rather than corrupt it.
    let target_dir = target_dir(&build_dir)?;
    if let DepSource::Git { url, git_ref } = &dep_source {
        eprintln!(
            "note: no local Rite source checkout found; building against {} @ {} (needs network)",
            url, git_ref
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .current_dir(&build_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .arg("--message-format=short");
    if release {
        cmd.arg("--release");
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(match &dep_source {
            DepSource::Git { url, git_ref } => format!(
                "cargo build failed.\n  \
                 No local Rite source checkout was found, so the generated crate depends on {url} @ {git_ref}, \
                 which needs network access and a matching tag.\n  \
                 Point `rite build` at a checkout with RITE_SOURCE_DIR=/path/to/rite (or override the ref with \
                 RITE_BUILD_GIT_REF), or run the script directly with `rite run` — no toolchain needed."
            ),
            DepSource::Workspace(_) => "cargo build failed".to_string(),
        });
    }

    // The `dev` profile puts artifacts in target/debug.
    let bin_name = format!("rite_script_{}", hash);
    let built = target_dir
        .join(if release { "release" } else { "debug" })
        .join(&bin_name);
    if let Some(out) = output {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(&built, out).map_err(|e| e.to_string())?;
        Ok(out.to_path_buf())
    } else {
        Ok(built)
    }
}

/// `main.rs` of the generated wrapper crate.
fn generate_main_rs(file: &Path, perms: &PermissionSet, build_cwd: &Path) -> String {
    let header = format!(
        "//! Generated by rite build from {}\n\
         // rite-compiler version {}\n\
         // Artifact: embedded ProgramIr (JSON/base64), evaluated via run_ir for parity.\n\n\
         mod generated;\n\
         mod source_map;\n\n\
         use rite_caps::{{install_defaults, PermissionSet}};\n\
         use rite_runtime::{{EvalError, RuntimeContext, Value}};\n\n",
        file.display(),
        env!("CARGO_PKG_VERSION")
    );
    // Relative paths inside the script resolve against the CWD of this binary; a
    // compiled binary deliberately has no `script_dir`, since the build machine's
    // source directory does not exist on the machine that runs it.
    let body = r#"#[tokio::main]
async fn main() {
    let mut ctx = RuntimeContext::new();
    install_defaults(&mut ctx, rite_perms());
    // A compiled binary is invoked directly, so its own argv *is* the script's
    // arguments — no `--` separator to strip. Read with `! @process.args`.
    ctx.script_args = std::env::args().skip(1).collect();
    let result = generated::rite_main(&mut ctx).await;

    // Buffered output is flushed on *both* paths: a script that printed and then
    // failed must not silently lose what it printed.
    for line in &ctx.stdout {
        print!("{}", line);
    }
    for line in &ctx.stderr {
        eprint!("{}", line);
    }
    match result {
        Ok(v) => {
            // `rite run` prints a non-none result whether or not the script also printed;
            // this used to suppress it once anything reached stdout, so a compiled
            // `! @console.println("hi")` followed by `1 + 2` lost the `3`.
            if !matches!(v, Value::None) {
                println!("{}", v.to_display(&ctx.atoms));
            }
        }
        Err(e) => {
            match &e {
                // A chosen exit status is not an error to report — the compiled
                // binary must be as silent about it as `rite run` is.
                EvalError::Exit(_) => {}
                EvalError::Permission(m) => eprintln!("permission denied: {}", m),
                EvalError::Budget(_) => eprintln!("execution budget exceeded"),
                other => eprintln!("runtime error: {}", other),
            }
            // The status comes from `EvalError::exit_code` — the same function
            // `rite run` uses — rather than a copy of the table living here, which
            // is how a compiled binary would quietly start disagreeing with the
            // interpreter about what a failure means.
            std::process::exit(e.exit_code() as i32);
        }
    }
}
"#;
    format!(
        "{}{}\n{}",
        header,
        perms::emit_permission_set(perms, build_cwd),
        body
    )
}

/// Whether the program reaches `@db`, and so needs DuckDB linked into the binary.
///
/// DuckDB is a `bundled` dependency: including it compiles the whole database from source,
/// which is most of the multi-minute cold build of a one-line script. A program that never
/// touches `@db` should not pay for it.
///
/// Decided from the serialized IR rather than the source text, so a `@db` reached through
/// an import still counts. Errs toward *including* it: a false positive costs build time,
/// a false negative would leave a capability missing at runtime.
fn uses_db(ir: &rite_sem::ProgramIr) -> bool {
    serde_json::to_string(ir)
        .map(|json| json.contains(r#""path":["db""#))
        .unwrap_or(true)
}

/// `Cargo.toml` of the generated wrapper crate.
fn generate_cargo_toml(hash: &str, dep_source: &DepSource, needs_db: bool) -> String {
    let dep = |crate_name: &str| -> String {
        // Only `rite-caps` has an optional feature worth turning off, and only when the
        // program never reaches `@db`.
        let extra = if crate_name == "rite-caps" && !needs_db {
            ", default-features = false"
        } else {
            ""
        };
        match dep_source {
            DepSource::Workspace(root) => format!(
                "{} = {{ path = {:?}{} }}",
                crate_name,
                root.join("crates").join(crate_name).display().to_string(),
                extra
            ),
            DepSource::Git { url, git_ref } => format!(
                "{} = {{ git = {:?}, {}{} }}",
                crate_name,
                url,
                git_ref_field(git_ref),
                extra
            ),
        }
    };
    format!(
        r#"[workspace]

[package]
name = "rite_script_{hash}"
version = "0.1.0"
edition = "2021"

# Only what the generated crate names directly; everything else the runtime needs
# comes in transitively.
[dependencies]
{runtime}
{caps}
{sem}
{core}
tokio = {{ version = "1", features = ["rt-multi-thread", "macros"] }}
serde_json = "1"

# Build timing reality: a cold `rite build` of a one-line script compiles the whole
# Rite runtime and its dependency tree — several minutes (~7 on a warm-ish machine).
# Subsequent builds reuse the shared target directory and are far quicker. `dev`
# drops debuginfo (nothing here is hand-written Rust worth stepping through), which
# cuts link time and binary size; use `--release` for a binary you intend to ship.
[profile.dev]
debug = 0
incremental = false

[profile.release]
opt-level = 2
lto = "thin"
codegen-units = 16
strip = true
"#,
        hash = hash,
        runtime = dep("rite-runtime"),
        caps = dep("rite-caps"),
        sem = dep("rite-sem"),
        core = dep("rite-core"),
    )
}

/// `tag = "v0.1.9"` / `rev = "<sha>"` / `branch = "main"`, whichever fits the ref.
fn git_ref_field(git_ref: &str) -> String {
    let is_sha = git_ref.len() >= 7
        && git_ref.len() <= 40
        && git_ref.chars().all(|c| c.is_ascii_hexdigit())
        && git_ref.chars().any(|c| c.is_ascii_digit());
    let is_tag = git_ref
        .strip_prefix('v')
        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    if is_tag {
        format!("tag = {:?}", git_ref)
    } else if is_sha {
        format!("rev = {:?}", git_ref)
    } else {
        format!("branch = {:?}", git_ref)
    }
}

/// A Rust toolchain is required; say so plainly instead of leaking an `io::Error`.
fn ensure_cargo() -> Result<(), String> {
    let ok = Command::new("cargo")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        return Ok(());
    }
    Err(
        "`rite build` needs a Rust toolchain, but `cargo` could not be run.\n  \
         Install one from https://rustup.rs, or run the script directly with `rite run <file>` \
         (the interpreter needs no toolchain)."
            .to_string(),
    )
}

/// Decide where the generated crate gets the `rite-*` crates from.
fn resolve_dep_source() -> Result<DepSource, String> {
    if let Some(dir) = env_path("RITE_SOURCE_DIR") {
        if !is_rite_checkout(&dir) {
            return Err(format!(
                "RITE_SOURCE_DIR={} is not a Rite source checkout \
                 (expected {}/crates/rite-runtime/Cargo.toml and friends).",
                dir.display(),
                dir.display()
            ));
        }
        return Ok(DepSource::Workspace(absolute(dir)));
    }
    if let Some(root) = find_workspace_root() {
        return Ok(DepSource::Workspace(root));
    }
    Ok(DepSource::Git {
        url: RITE_REPO_URL.to_string(),
        git_ref: std::env::var("RITE_BUILD_GIT_REF")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
    })
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if is_rite_checkout(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Any Rust workspace has `Cargo.toml` + `crates/`; only a Rite checkout has the
/// crates we depend on. Checking for them keeps us from emitting path deps that
/// point at nothing.
fn is_rite_checkout(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && REQUIRED_CRATES
            .iter()
            .all(|c| dir.join("crates").join(c).join("Cargo.toml").is_file())
}

/// Directory holding the generated crates, `<cwd>/.rite/build` unless overridden
/// with `RITE_BUILD_DIR`. Kept in the project by default so `--emit-rust` output
/// and `program.ir.json` stay where the docs say they are; the bulky cargo target
/// directory lives in the cache instead (see `target_dir`).
fn build_root() -> PathBuf {
    env_path("RITE_BUILD_DIR").unwrap_or_else(|| PathBuf::from(".rite/build"))
}

/// Shared cargo target directory. Honours a user-set `CARGO_TARGET_DIR`, else the
/// OS cache dir, else falls back inside the build directory.
fn target_dir(build_dir: &Path) -> Result<PathBuf, String> {
    if let Some(dir) = env_path("CARGO_TARGET_DIR") {
        // Cargo would resolve a relative value against the build directory it is
        // invoked in; make it absolute against our CWD so both cargo and we agree.
        return Ok(absolute(dir));
    }
    if let Some(cache) = cache_root() {
        let dir = cache.join("build-target");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        return Ok(dir);
    }
    Ok(absolute(build_dir.join("target")))
}

fn cache_root() -> Option<PathBuf> {
    if let Some(dir) = env_path("RITE_CACHE_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        if let Some(dir) = env_path("LOCALAPPDATA") {
            return Some(dir.join("rite"));
        }
    }
    if cfg!(target_os = "macos") {
        if let Some(home) = env_path("HOME") {
            return Some(home.join("Library/Caches/rite"));
        }
    }
    if let Some(dir) = env_path("XDG_CACHE_HOME") {
        return Some(dir.join("rite"));
    }
    env_path("HOME").map(|home| home.join(".cache/rite"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        Err(_) => path,
    }
}

/// A failed differential run: what went wrong, and the status the CLI would have
/// exited with.
///
/// The status travels with the message because a conformance fixture declaring
/// `expected.exit = 5` is making a claim about the *code*, and comparing rendered
/// error text instead would let a permission denial and a runtime error satisfy
/// the same fixture.
#[derive(Debug, Clone)]
pub struct RunFailure {
    pub code: u8,
    pub message: String,
    /// What the script printed before it stopped.
    ///
    /// Both runners flush buffered output on the failure path, and that is a
    /// documented promise — a script that prints and then fails must not lose what
    /// it printed. Carrying it here is what lets a fixture check the promise; while
    /// the error was a bare `String` there was nothing to compare against.
    pub stdout: String,
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl RunFailure {
    fn from_eval(e: rite_runtime::EvalError, stdout: String) -> Self {
        RunFailure {
            code: e.exit_code(),
            message: e.to_string(),
            stdout,
        }
    }

    /// A failure raised by the harness itself rather than by the script.
    fn harness(code: u8, message: impl Into<String>) -> Self {
        RunFailure {
            code,
            message: message.into(),
            stdout: String::new(),
        }
    }
}

/// Differential helper: run IR in-process (same as compiled path).
pub async fn run_ir_mode(
    file: &Path,
    perms: PermissionSet,
) -> Result<rite_runtime::Value, RunFailure> {
    let (ir, diags, _) = compile_path(file);
    if diags.has_errors() {
        // 3 is what `rite run` exits with for a source it cannot compile.
        return Err(RunFailure::harness(
            3,
            format!("compile errors: {}", diags.len()),
        ));
    }
    let ir = ir.ok_or_else(|| RunFailure::harness(3, "no IR"))?;
    let mut ctx = rite_runtime::RuntimeContext::new();
    rite_caps::install_defaults(&mut ctx, perms);
    if let Some(parent) = file.parent() {
        ctx.script_dir = Some(parent.to_path_buf());
    }
    rite_runtime::run_ir(&ir, &mut ctx)
        .await
        .map_err(|e| RunFailure::from_eval(e, ctx.stdout.join("")))
}

/// Interpreted mode for differential tests: `(value, stdout, stderr, atoms)`.
///
/// The interner comes back with the value because an atom is only a number until
/// something can name it. Rendering a returned `#matched` without it produced
/// `"#?0"`, which is what let a conformance fixture expecting `"matched"` pass
/// against the wrong value for as long as the comparison was loose enough to ignore
/// it.
pub async fn run_interpreted(
    file: &Path,
    perms: PermissionSet,
) -> Result<
    (
        rite_runtime::Value,
        String,
        String,
        std::sync::Arc<rite_runtime::AtomInterner>,
    ),
    RunFailure,
> {
    let mut ctx = rite_runtime::RuntimeContext::new();
    rite_caps::install_defaults(&mut ctx, perms);
    if let Some(parent) = file.parent() {
        ctx.script_dir = Some(parent.to_path_buf());
        ctx.module_roots.push(parent.to_path_buf());
    }
    let mut sources = SourceMap::new();
    // An unreadable case file is the harness's problem, not the script's: 2, the
    // usage code, rather than something a fixture could claim to expect.
    let id = sources
        .add_path(file)
        .map_err(|e| RunFailure::harness(2, e.to_string()))?;
    let sf = sources.get(id).unwrap().clone();
    ctx.sources = sources;
    let v = match rite_runtime::run_file(&sf, &mut ctx).await {
        Ok(v) => v,
        Err(e) => return Err(RunFailure::from_eval(e, ctx.stdout.join(""))),
    };
    Ok((
        v,
        ctx.stdout.join(""),
        ctx.stderr.join(""),
        ctx.atoms.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_main_bakes_the_given_permission_set() {
        let mut perms = PermissionSet::default_secure();
        perms.fs_read.push(PathBuf::from("/build/data"));
        let src = generate_main_rs(Path::new("p.rite"), &perms, Path::new("/build"));
        // The whole point: no blanket grant in the generated binary.
        assert!(!src.contains("PermissionSet::allow_all()"), "{}", src);
        assert!(src.contains("p.allow_all = false;"), "{}", src);
        assert!(
            src.contains(r#"p.fs_read = vec![grant_path("data")];"#),
            "{}",
            src
        );
        assert!(
            src.contains("install_defaults(&mut ctx, rite_perms());"),
            "{}",
            src
        );
    }

    #[test]
    fn generated_main_flushes_output_before_exiting_on_error() {
        let src = generate_main_rs(
            Path::new("p.rite"),
            &PermissionSet::default_secure(),
            Path::new("/build"),
        );
        // stdout is drained once, before the success/error split.
        let flush = src.find("for line in &ctx.stdout").expect("flush loop");
        let split = src.find("match result {").expect("match");
        assert!(
            flush < split,
            "output must be flushed on both paths:\n{}",
            src
        );
        // The status comes from the runtime's own table rather than a copy pasted
        // into the generated program. The copy was the risk: `rite run` and a
        // compiled binary could then answer differently for the same failure, and
        // nothing would notice until someone compared two shells.
        assert!(src.contains("e.exit_code()"), "{}", src);
        assert!(
            !src.contains("=> 5"),
            "generated main re-states the table:\n{}",
            src
        );
        assert!(!src.contains("std::process::exit(1)"), "{}", src);
    }

    #[test]
    fn workspace_dep_source_emits_path_deps() {
        let root = PathBuf::from("/src/rite");
        let toml = generate_cargo_toml("abc", &DepSource::Workspace(root.clone()), true);
        // Build the expectation the way the emitter does. A hardcoded POSIX spelling
        // failed on Windows for correct output: `join` produces `\` there, and the
        // emitter's `{:?}` escapes it to `\\`, which is what TOML needs for a literal
        // backslash. The path separator is the platform's business, not this test's.
        let expected = format!(
            "rite-runtime = {{ path = {:?} }}",
            root.join("crates")
                .join("rite-runtime")
                .display()
                .to_string()
        );
        assert!(toml.contains(&expected), "want {expected}\ngot {toml}");
        assert!(toml.contains("[profile.release]"), "{}", toml);
    }

    #[test]
    fn git_dep_source_emits_git_deps() {
        let toml = generate_cargo_toml(
            "abc",
            &DepSource::Git {
                url: RITE_REPO_URL.to_string(),
                git_ref: "v0.1.9".to_string(),
            },
            true,
        );
        assert!(
            toml.contains(
                r#"rite-caps = { git = "https://github.com/undercurrent-labs/rite", tag = "v0.1.9" }"#
            ),
            "{}",
            toml
        );
    }

    #[test]
    fn git_refs_are_classified() {
        assert_eq!(git_ref_field("v0.1.9"), r#"tag = "v0.1.9""#);
        assert_eq!(git_ref_field("main"), r#"branch = "main""#);
        assert_eq!(
            git_ref_field("d05798ba3a69911b1a1ec32a8730ece2c176e2b2"),
            r#"rev = "d05798ba3a69911b1a1ec32a8730ece2c176e2b2""#
        );
    }

    #[test]
    fn only_a_real_rite_checkout_counts_as_a_workspace() {
        let tmp = std::env::temp_dir().join(format!("rite-checkout-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("crates/other")).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[workspace]\n").unwrap();
        // Cargo.toml + crates/ but no rite crates: not a checkout.
        assert!(!is_rite_checkout(&tmp));
        for c in REQUIRED_CRATES {
            std::fs::create_dir_all(tmp.join("crates").join(c)).unwrap();
            std::fs::write(tmp.join("crates").join(c).join("Cargo.toml"), "").unwrap();
        }
        assert!(is_rite_checkout(&tmp));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
