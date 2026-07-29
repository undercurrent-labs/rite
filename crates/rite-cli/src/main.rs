//! Rite CLI — run, build, check, fmt, repl, test, doc, and more.

mod config;
mod github;
mod skill_cmd;
mod update_cmd;
mod vscode_cmd;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand, ValueEnum};
use rite_caps::{Permission, PermissionSet};
use rite_core::SourceMap;
use rite_runtime::{EvalError, RuntimeContext};
use rite_sem::{compile_to_ir, ir_to_json};
use rite_syntax::parse_source;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "rite",
    version,
    about = "Rite — esoteric Rust-backed scripting language"
)]
struct Cli {
    /// Verbose version info (with --version)
    #[arg(long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a Rite script through the interpreter
    Run {
        file: PathBuf,
        #[arg(long = "allow", value_name = "PERM")]
        allow: Vec<String>,
        #[arg(long = "allow-all")]
        allow_all: bool,
        #[arg(long = "deny", value_name = "PERM")]
        deny: Vec<String>,
        #[arg(long)]
        timeout: Option<String>,
        #[arg(long = "max-steps")]
        max_steps: Option<u64>,
        #[arg(long)]
        trace: bool,
        #[arg(long = "json-errors")]
        json_errors: bool,
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile a Rite script to a native binary
    Build {
        file: PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(long = "emit-rust")]
        emit_rust: bool,
        #[arg(long, short)]
        output: Option<PathBuf>,
        #[arg(long = "allow-all")]
        allow_all: bool,
        #[arg(long = "allow", value_name = "PERM")]
        allow: Vec<String>,
    },
    /// Lex, parse, resolve, and effect-check without executing
    Check {
        file: PathBuf,
        #[arg(long = "json-errors")]
        json_errors: bool,
    },
    /// Format Rite source
    Fmt {
        paths: Vec<PathBuf>,
        #[arg(long)]
        ascii: bool,
        #[arg(long, default_value = "glyph")]
        dialect: String,
        #[arg(long)]
        check: bool,
    },
    /// Interactive REPL
    Repl {
        #[arg(long = "allow-all")]
        allow_all: bool,
    },
    /// Run Rite tests
    Test {
        paths: Vec<PathBuf>,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        interpreted: bool,
        #[arg(long)]
        compiled: bool,
        #[arg(long)]
        both: bool,
        #[arg(long)]
        json: bool,
    },
    /// Generate documentation
    Doc {
        path: Option<PathBuf>,
        #[arg(long, default_value = "docs/generated")]
        out: PathBuf,
    },
    /// Show desugared forms
    Explain { file: PathBuf },
    /// Dump AST
    Ast {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Dump semantic IR
    Ir {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// List capabilities
    Capabilities,
    /// Convert dialect (ascii/glyph/mixed)
    Convert {
        file: PathBuf,
        #[arg(long = "to", default_value = "glyph")]
        to: String,
        #[arg(long)]
        stdout: bool,
        #[arg(long)]
        check: bool,
    },
    /// Start language server (stdio)
    Lsp,
    /// Launch Rite Studio local service
    Studio {
        #[arg(long, default_value = "4041")]
        port: u16,
        #[arg(long = "no-open")]
        no_open: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Dump syntax tree (alias of ast)
    #[command(name = "syntax-tree")]
    SyntaxTree {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Dump semantic IR (alias of ir)
    #[command(name = "semantic-ir")]
    SemanticIr {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Emit generated Rust without full native link
    #[command(name = "emit-rust")]
    EmitRust { file: PathBuf },
    /// Documentation commands
    Docs {
        #[command(subcommand)]
        cmd: DocsCmd,
    },
    /// Machine-readable language description
    Describe {
        #[command(subcommand)]
        target: DescribeCmd,
    },
    /// Install / update the agent skill bundle (Grok, Claude, Cursor, …)
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },
    /// Check for / install CLI and skill updates
    Update {
        /// Only report; exit 1 if an update is available
        #[arg(long)]
        check: bool,
        /// Reinstall even if versions match
        #[arg(long)]
        force: bool,
        /// Install a specific release tag (e.g. v0.1.7)
        #[arg(long)]
        version: Option<String>,
    },
    /// Alias for `update`
    #[command(name = "self-update")]
    SelfUpdate {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        version: Option<String>,
    },
    /// Download / install the VS Code (or Cursor) extension
    Vscode {
        #[command(subcommand)]
        cmd: VscodeCmd,
    },
    /// Print version
    Version {
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SkillCmd {
    /// Download the skill into the local cache and install into agent skill dirs
    Install {
        /// Targets: grok, claude, cursor, project, all, cache (comma-separated)
        #[arg(long, default_value = "grok")]
        target: String,
        /// Install to a specific directory (overrides --target)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Source: local path, archive, or URL
        #[arg(long)]
        from: Option<String>,
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
        /// Refresh even if cache looks current
        #[arg(long)]
        force: bool,
    },
    /// Re-fetch skill and reinstall to previously recorded paths
    Update {
        #[arg(long)]
        force: bool,
    },
    /// Show install state, cache path, and last pull time
    Status,
    /// Print default skill install paths
    Path,
}

#[derive(Subcommand, Debug)]
enum VscodeCmd {
    /// Download the .vsix and install via `code` / `cursor` if available
    Install {
        /// Editor CLI: code, cursor, codium (default: auto-detect)
        #[arg(long)]
        editor: Option<String>,
        /// Only download; print path and metadata
        #[arg(long)]
        download_only: bool,
        /// Output path for the .vsix
        #[arg(long, short)]
        out: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
    },
    /// Download the .vsix without installing
    Download {
        #[arg(long, short)]
        out: Option<PathBuf>,
        #[arg(long)]
        version: Option<String>,
    },
    /// Show release asset details and install instructions
    Info {
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DocsCmd {
    Build {
        #[arg(long, default_value = "docs/generated")]
        out: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "4042")]
        port: u16,
    },
    Check,
    Json {
        #[arg(long, default_value = "docs/generated")]
        out: PathBuf,
    },
    Agent {
        #[arg(long, default_value = "skills/rite")]
        output: PathBuf,
    },
    Open {
        symbol: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DescribeCmd {
    Language {
        #[arg(long)]
        json: bool,
    },
    Syntax {
        #[arg(long)]
        json: bool,
    },
    Capability {
        name: String,
        #[arg(long)]
        json: bool,
    },
    Diagnostic {
        code: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, ValueEnum, Debug)]
enum FormatStyle {
    Glyph,
    Ascii,
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt::try_init();
    // Shebang / direct-exec: `rite script.rite` → `rite run script.rite` when the
    // first positional arg is not a known subcommand (kernel passes the script path).
    let cli = Cli::parse_from(rewrite_argv_for_implicit_run(std::env::args().collect()));
    match run(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {:#}", e);
            ExitCode::from(2)
        }
    }
}

/// Known top-level subcommands (must stay in sync with [`Commands`]).
fn is_known_subcommand(name: &str) -> bool {
    matches!(
        name,
        "run"
            | "build"
            | "check"
            | "fmt"
            | "repl"
            | "test"
            | "doc"
            | "explain"
            | "ast"
            | "ir"
            | "capabilities"
            | "convert"
            | "lsp"
            | "studio"
            | "syntax-tree"
            | "semantic-ir"
            | "emit-rust"
            | "docs"
            | "describe"
            | "skill"
            | "update"
            | "self-update"
            | "vscode"
            | "version"
            | "help"
    )
}

/// If the first positional argument is not a subcommand, treat it as a script path
/// and insert `run` so clap parses `rite script.rite …` like `rite run script.rite …`.
///
/// Skips leading global flags (`--verbose`, `-V`, etc.). Does not rewrite when the
/// first positional is already a known command.
pub(crate) fn rewrite_argv_for_implicit_run(mut args: Vec<String>) -> Vec<String> {
    if args.len() < 2 {
        return args;
    }
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            break;
        }
        if a.starts_with('-') {
            // Global options only (no value-taking globals today).
            i += 1;
            continue;
        }
        // First positional argument
        if is_known_subcommand(a) {
            return args;
        }
        args.insert(i, "run".to_string());
        return args;
    }
    args
}

#[cfg(test)]
mod argv_tests {
    use super::rewrite_argv_for_implicit_run;

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn injects_run_for_script_path() {
        let out = rewrite_argv_for_implicit_run(args(&["rite", "hello.rite", "--allow-all"]));
        assert_eq!(out, args(&["rite", "run", "hello.rite", "--allow-all"]));
    }

    #[test]
    fn leaves_known_subcommand() {
        let out = rewrite_argv_for_implicit_run(args(&["rite", "version"]));
        assert_eq!(out, args(&["rite", "version"]));
        let out = rewrite_argv_for_implicit_run(args(&["rite", "fmt", "a.rite"]));
        assert_eq!(out, args(&["rite", "fmt", "a.rite"]));
    }

    #[test]
    fn skips_global_verbose_flag() {
        let out = rewrite_argv_for_implicit_run(args(&["rite", "--verbose", "script.rite"]));
        assert_eq!(out, args(&["rite", "--verbose", "run", "script.rite"]));
    }

    #[test]
    fn absolute_path_from_shebang() {
        let out = rewrite_argv_for_implicit_run(args(&["rite", "/tmp/tool.rite"]));
        assert_eq!(out, args(&["rite", "run", "/tmp/tool.rite"]));
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let global_verbose = cli.verbose;
    match cli.command {
        Commands::Version { verbose } => {
            println!("rite {}", env!("CARGO_PKG_VERSION"));
            if verbose || global_verbose {
                println!("language_version: 1");
                println!("formatter_version: 1");
                println!("runtime: interpreter+ir");
                println!("compiler: ir-json-embed");
                println!("lsp: tower-lsp stdio");
                println!("wasm: rite-wasm (browser-safe subset)");
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Capabilities => {
            print_capabilities();
            Ok(ExitCode::SUCCESS)
        }
        Commands::Convert {
            file,
            to,
            stdout,
            check,
        } => {
            let text = std::fs::read_to_string(&file)?;
            let dialect = match to.as_str() {
                "ascii" => rite_fmt::Dialect::Ascii,
                "mixed" => rite_fmt::Dialect::Mixed,
                "preserve" => rite_fmt::Dialect::Preserve,
                _ => rite_fmt::Dialect::Glyph,
            };
            let converted =
                rite_fmt::convert_source(&text, dialect).map_err(|e| anyhow::anyhow!(e))?;
            if check {
                if converted.text != text {
                    eprintln!("would convert {}", file.display());
                    return Ok(ExitCode::from(1));
                }
                return Ok(ExitCode::SUCCESS);
            }
            if stdout {
                print!("{}", converted.text);
            } else {
                std::fs::write(&file, &converted.text)?;
                println!("converted {} -> {:?}", file.display(), dialect);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Lsp => {
            // Delegate to rite-lsp binary if present, else note
            let status = std::process::Command::new("rite-lsp").status();
            match status {
                Ok(s) if s.success() => Ok(ExitCode::SUCCESS),
                Ok(s) => Ok(ExitCode::from(s.code().unwrap_or(1) as u8)),
                Err(_) => {
                    eprintln!("rite-lsp not found on PATH; run: cargo run -p rite-lsp");
                    Ok(ExitCode::from(2))
                }
            }
        }
        Commands::Studio {
            port,
            no_open,
            project,
        } => run_studio(port, no_open, project.as_deref()).await,
        Commands::SyntaxTree { file, json } => {
            // reuse Ast
            let text = std::fs::read_to_string(&file)?;
            let (program, diags, sources) = parse_source(&file.display().to_string(), &text);
            if diags.has_errors() {
                eprint!("{}", diags.render_all(&sources));
                return Ok(ExitCode::from(3));
            }
            let program = program.expect("program");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rite_syntax::ast_to_json(&program))?
                );
            } else {
                println!("{:#?}", program);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::SemanticIr { file, json } => {
            let text = std::fs::read_to_string(&file)?;
            let mut sources = SourceMap::new();
            let id = sources.add_file(file.display().to_string(), &text);
            let sf = sources.get(id).unwrap().clone();
            let (ir, diags) = compile_to_ir(&sf);
            if diags.has_errors() {
                eprint!("{}", diags.render_all(&sources));
                return Ok(ExitCode::from(4));
            }
            let ir = ir.expect("ir");
            if json {
                println!("{}", serde_json::to_string_pretty(&ir_to_json(&ir))?);
            } else {
                println!("{:#?}", ir);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::EmitRust { file } => {
            let (ir, diags, _) = rite_sem::compile_path(&file);
            if diags.has_errors() {
                return Ok(ExitCode::from(4));
            }
            let ir = ir.ok_or_else(|| anyhow::anyhow!("no ir"))?;
            let code =
                rite_compiler::generate_from_ir(&ir, &file).map_err(|e| anyhow::anyhow!(e))?;
            println!("{}", code);
            Ok(ExitCode::SUCCESS)
        }
        Commands::Docs { cmd } => run_docs(cmd).await,
        Commands::Describe { target } => run_describe(target),
        Commands::Check { file, json_errors } => {
            let text = std::fs::read_to_string(&file)?;
            let name = file.display().to_string();
            let mut sources = SourceMap::new();
            let id = sources.add_file(&name, &text);
            let sf = sources.get(id).unwrap().clone();
            let (ir, diags) = compile_to_ir(&sf);
            if json_errors {
                println!("{}", serde_json::to_string_pretty(&diags.to_json())?);
            } else if !diags.is_empty() {
                eprint!("{}", diags.render_all(&sources));
            }
            if diags.has_errors() {
                Ok(ExitCode::from(4))
            } else {
                let _ = ir;
                println!("ok");
                Ok(ExitCode::SUCCESS)
            }
        }
        Commands::Ast { file, json } => {
            let text = std::fs::read_to_string(&file)?;
            let (program, diags, sources) = parse_source(&file.display().to_string(), &text);
            if diags.has_errors() {
                eprint!("{}", diags.render_all(&sources));
                return Ok(ExitCode::from(3));
            }
            let program = program.expect("program");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rite_syntax::ast_to_json(&program))?
                );
            } else {
                println!("{:#?}", program);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Ir { file, json } => {
            let text = std::fs::read_to_string(&file)?;
            let mut sources = SourceMap::new();
            let id = sources.add_file(file.display().to_string(), &text);
            let sf = sources.get(id).unwrap().clone();
            let (ir, diags) = compile_to_ir(&sf);
            if diags.has_errors() {
                eprint!("{}", diags.render_all(&sources));
                return Ok(ExitCode::from(4));
            }
            let ir = ir.expect("ir");
            if json {
                println!("{}", serde_json::to_string_pretty(&ir_to_json(&ir))?);
            } else {
                println!("{:#?}", ir);
            }
            Ok(ExitCode::SUCCESS)
        }
        Commands::Explain { file } => {
            let text = std::fs::read_to_string(&file)?;
            let mut sources = SourceMap::new();
            let id = sources.add_file(file.display().to_string(), &text);
            let sf = sources.get(id).unwrap().clone();
            let (ir, diags) = compile_to_ir(&sf);
            if diags.has_errors() {
                eprint!("{}", diags.render_all(&sources));
                return Ok(ExitCode::from(4));
            }
            println!("# Desugared IR for {}", file.display());
            println!("{:#?}", ir);
            Ok(ExitCode::SUCCESS)
        }
        Commands::Fmt {
            paths,
            ascii,
            dialect,
            check,
        } => {
            let paths = if paths.is_empty() {
                vec![PathBuf::from(".")]
            } else {
                paths
            };
            let dialect = if ascii {
                rite_fmt::Dialect::Ascii
            } else {
                match dialect.as_str() {
                    "ascii" => rite_fmt::Dialect::Ascii,
                    "mixed" => rite_fmt::Dialect::Mixed,
                    "preserve" => rite_fmt::Dialect::Preserve,
                    _ => rite_fmt::Dialect::Glyph,
                }
            };
            let mut failed = false;
            for path in paths {
                for file in collect_rite_files(&path)? {
                    let text = std::fs::read_to_string(&file)?;
                    match rite_fmt::format_with_dialect(&text, dialect) {
                        Ok(formatted) => {
                            if check {
                                if formatted.text != text {
                                    eprintln!("would reformat {}", file.display());
                                    failed = true;
                                }
                            } else if formatted.text != text {
                                std::fs::write(&file, formatted.text)?;
                                println!("formatted {}", file.display());
                            }
                        }
                        Err(e) => {
                            eprintln!("{}: {}", file.display(), e);
                            failed = true;
                        }
                    }
                }
            }
            Ok(if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        Commands::Run {
            file,
            allow,
            allow_all,
            deny,
            timeout,
            max_steps,
            trace: _,
            json_errors,
            args: _,
        } => {
            let mut perms = if allow_all {
                PermissionSet::allow_all()
            } else {
                PermissionSet::default_secure()
            };
            for a in allow {
                match Permission::parse(&a) {
                    Ok(p) => perms.grant(p),
                    Err(e) => {
                        eprintln!("{}", e);
                        return Ok(ExitCode::from(2));
                    }
                }
            }
            for d in deny {
                if let Ok(p) = Permission::parse(&d) {
                    perms.deny(p);
                }
            }

            let mut sources = SourceMap::new();
            let id = sources.add_path(&file).map_err(|e| anyhow::anyhow!(e))?;
            let sf = sources.get(id).unwrap().clone();

            let mut ctx = RuntimeContext::new();
            ctx.sources = sources.clone();
            if let Some(parent) = file.parent() {
                ctx.script_dir = Some(parent.to_path_buf());
                ctx.module_roots.push(parent.to_path_buf());
            }
            if let Some(ms) = max_steps {
                ctx.budget = ctx.budget.with_max_steps(ms);
            }
            if let Some(t) = timeout {
                if let Some(dur) = parse_duration(&t) {
                    ctx.budget = ctx.budget.with_timeout(dur);
                }
            }
            rite_caps::install_defaults(&mut ctx, perms);

            match rite_runtime::run_file(&sf, &mut ctx).await {
                Ok(value) => {
                    // Print captured stdout
                    for line in &ctx.stdout {
                        print!("{}", line);
                    }
                    if !matches!(value, rite_runtime::Value::None) {
                        // Only print value if nothing was printed? print result for scripts
                        if ctx.stdout.is_empty() {
                            println!("{}", value.to_display(&ctx.atoms));
                        }
                    }
                    Ok(ExitCode::SUCCESS)
                }
                Err(EvalError::Compile(d)) => {
                    if json_errors {
                        println!("{}", serde_json::to_string_pretty(&d.to_json())?);
                    } else {
                        eprint!("{}", d.render_all(&sources));
                    }
                    Ok(ExitCode::from(if d.has_errors() { 3 } else { 4 }))
                }
                Err(EvalError::Permission(m)) => {
                    eprintln!("permission denied: {}", m);
                    Ok(ExitCode::from(5))
                }
                Err(EvalError::Budget(_)) => {
                    eprintln!("execution budget exceeded");
                    Ok(ExitCode::from(8))
                }
                Err(e) => {
                    eprintln!("runtime error: {}", e);
                    Ok(ExitCode::from(1))
                }
            }
        }
        Commands::Build {
            file,
            release,
            emit_rust,
            output,
            allow_all,
            allow,
        } => {
            let mut perms = if allow_all {
                PermissionSet::allow_all()
            } else {
                PermissionSet::default_secure()
            };
            for a in &allow {
                if let Ok(p) = Permission::parse(a) {
                    perms.grant(p);
                }
            }
            match rite_compiler::build_script(&file, release, emit_rust, output.as_deref(), &perms)
            {
                Ok(path) => {
                    println!("built {}", path.display());
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("compilation failed: {}", e);
                    Ok(ExitCode::from(6))
                }
            }
        }
        Commands::Repl { allow_all } => {
            let perms = if allow_all {
                PermissionSet::allow_all()
            } else {
                PermissionSet::default_secure()
            };
            rite_repl::run_repl(perms).await?;
            Ok(ExitCode::SUCCESS)
        }
        Commands::Test {
            paths,
            filter,
            interpreted,
            compiled,
            both,
            json,
        } => {
            let mode = if both {
                rite_test::TestMode::Both
            } else if compiled {
                rite_test::TestMode::Compiled
            } else {
                let _ = interpreted;
                rite_test::TestMode::Interpreted
            };
            let paths = if paths.is_empty() {
                vec![PathBuf::from("tests"), PathBuf::from("examples")]
            } else {
                paths
            };
            let report = rite_test::run_tests(&paths, filter.as_deref(), mode).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "tests: {} passed, {} failed, {} total",
                    report.passed, report.failed, report.total
                );
                for f in &report.failures {
                    eprintln!("FAIL {}: {}", f.name, f.message);
                }
            }
            Ok(if report.failed > 0 {
                ExitCode::from(7)
            } else {
                ExitCode::SUCCESS
            })
        }
        Commands::Doc { path, out } => {
            rite_doc::generate(path.as_deref(), &out)?;
            println!("documentation written to {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
        Commands::Skill { cmd } => match cmd {
            SkillCmd::Install {
                target,
                dir,
                from,
                version,
                force,
            } => skill_cmd::install(&target, dir, from, version, force).await,
            SkillCmd::Update { force } => skill_cmd::update(force).await,
            SkillCmd::Status => skill_cmd::status(),
            SkillCmd::Path => skill_cmd::print_paths(),
        },
        Commands::Update {
            check,
            force,
            version,
        }
        | Commands::SelfUpdate {
            check,
            force,
            version,
        } => update_cmd::run(check, force, version).await,
        Commands::Vscode { cmd } => match cmd {
            VscodeCmd::Install {
                editor,
                download_only,
                out,
                version,
            } => vscode_cmd::install(editor, download_only, out, version).await,
            VscodeCmd::Download { out, version } => vscode_cmd::download(out, version).await,
            VscodeCmd::Info { version } => vscode_cmd::info(version).await,
        },
    }
}

fn print_capabilities() {
    let host = rite_caps::HostCapabilities::with_defaults(PermissionSet::allow_all());
    for (name, descs) in host.all_descriptors() {
        println!("@{}", name);
        for d in descs {
            let eff = if d.effectful { " !" } else { "" };
            println!("  .{}{}  — {}", d.name, eff, d.docs);
        }
        println!();
    }
}

fn collect_rite_files(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(out);
    }
    for entry in walkdir_simple(path)? {
        if entry.extension().and_then(|s| s.to_str()) == Some("rite") {
            out.push(entry);
        }
    }
    Ok(out)
}

fn walkdir_simple(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if path.is_dir() {
        for e in std::fs::read_dir(path)? {
            let e = e?;
            let p = e.path();
            if p.is_dir() {
                out.extend(walkdir_simple(&p)?);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}

fn parse_duration(s: &str) -> Option<Duration> {
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.parse().ok().map(Duration::from_millis);
    }
    if let Some(sec) = s.strip_suffix('s') {
        return sec.parse().ok().map(Duration::from_secs);
    }
    if let Some(m) = s.strip_suffix('m') {
        return m.parse::<u64>().ok().map(|n| Duration::from_secs(n * 60));
    }
    s.parse::<u64>().ok().map(Duration::from_secs)
}

async fn run_docs(cmd: DocsCmd) -> anyhow::Result<ExitCode> {
    match cmd {
        DocsCmd::Build { out } | DocsCmd::Json { out } => {
            rite_doc::generate(None, &out)?;
            rite_doc::generate_agent_bundle(Path::new("skills/rite"))?;
            println!("docs written to {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Check => {
            rite_doc::generate(None, Path::new("docs/generated"))?;
            let report = rite_doc::run_doctests(&[
                Path::new("docs/book"),
                Path::new("docs/diagnostics"),
                Path::new("skills/rite"),
            ])
            .await;
            println!(
                "doctests: {} passed, {} failed",
                report.passed, report.failed
            );
            for r in &report.results {
                if !r.ok {
                    eprintln!("FAIL {}:{} [{}] {}", r.file, r.line, r.mode, r.message);
                }
            }
            if report.failed > 0 {
                Ok(ExitCode::from(7))
            } else {
                println!("docs check ok");
                Ok(ExitCode::SUCCESS)
            }
        }
        DocsCmd::Serve { port } => {
            println!("serving docs at http://127.0.0.1:{port}/ (static files in docs/generated)");
            println!("open docs/generated/html/index.html or use a static file server");
            let _ = port;
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Agent { output } => {
            rite_doc::generate_agent_bundle(&output)?;
            println!("agent skill written to {}", output.display());
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Open { symbol } => {
            println!(
                "open documentation for {}",
                symbol.as_deref().unwrap_or("index")
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_describe(target: DescribeCmd) -> anyhow::Result<ExitCode> {
    match target {
        DescribeCmd::Language { json } => {
            let v = serde_json::json!({
                "language_version": "1",
                "tool_version": env!("CARGO_PKG_VERSION"),
                "formatter_version": "1",
                "dialects": ["ascii", "glyph", "mixed", "preserve"],
                "execution": ["interpreter", "ir-compiled", "wasm-browser-safe"],
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!(
                    "Rite language version 1 (tool {})",
                    env!("CARGO_PKG_VERSION")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        DescribeCmd::Syntax { json } => {
            let aliases = rite_fmt::aliases_json();
            if json {
                println!("{}", serde_json::to_string_pretty(&aliases)?);
            } else {
                println!("{}", aliases);
            }
            Ok(ExitCode::SUCCESS)
        }
        DescribeCmd::Capability { name, json } => {
            let host = rite_caps::HostCapabilities::with_defaults(PermissionSet::allow_all());
            let name = name.trim_start_matches('@');
            for (cap, descs) in host.all_descriptors() {
                if cap == name {
                    let funcs: Vec<_> = descs
                        .iter()
                        .map(|d| {
                            serde_json::json!({
                                "name": d.name,
                                "docs": d.docs,
                                "arity": d.arity,
                                "effectful": d.effectful,
                                "permission": d.permission,
                            })
                        })
                        .collect();
                    let v = serde_json::json!({"capability": cap, "functions": funcs});
                    if json {
                        println!("{}", serde_json::to_string_pretty(&v)?);
                    } else {
                        println!("@{}", cap);
                        for d in descs {
                            println!("  .{} — {}", d.name, d.docs);
                        }
                    }
                    return Ok(ExitCode::SUCCESS);
                }
            }
            eprintln!("unknown capability @{name}");
            Ok(ExitCode::from(2))
        }
        DescribeCmd::Diagnostic { code, json } => {
            let v = serde_json::json!({
                "code": code,
                "summary": "See IMPLEMENTATION.md and diagnostics encyclopedia",
                "docs": format!("docs/diagnostics/{}.md", code),
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("{code}: stable diagnostic — see documentation");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

async fn run_studio(port: u16, no_open: bool, project: Option<&Path>) -> anyhow::Result<ExitCode> {
    use std::net::SocketAddr;

    let token = uuid::Uuid::new_v4().to_string();
    let state = Arc::new(StudioState {
        token: token.clone(),
        project: project.map(|p| p.to_path_buf()),
    });

    let app = Router::new()
        .route("/api/v1/version", get(studio_version))
        .route("/api/v1/parse", post(studio_parse))
        .route("/api/v1/analyze", post(studio_analyze))
        .route("/api/v1/format", post(studio_format))
        .route("/api/v1/run", post(studio_run))
        .route("/api/v1/check", post(studio_check))
        .route("/api/v1/emit-rust", post(studio_emit_rust))
        .route("/", get(|| async { axum::response::Html(STUDIO_HTML) }))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("Rite Studio local service on http://{addr}");
    println!("session token: {token}");
    if let Some(p) = project {
        println!("project root: {}", p.display());
    }
    if !no_open {
        let _ = std::process::Command::new("xdg-open")
            .arg(format!("http://{addr}"))
            .status();
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(ExitCode::SUCCESS)
}

#[derive(Clone)]
struct StudioState {
    /// Session token reserved for authenticated Studio API routes.
    #[allow(dead_code)]
    token: String,
    project: Option<PathBuf>,
}

#[derive(serde::Deserialize)]
struct SourceBody {
    source: String,
    #[serde(default)]
    dialect: Option<String>,
    /// Optional client token (auth not yet enforced).
    #[serde(default)]
    #[allow(dead_code)]
    token: Option<String>,
}

async fn studio_version(State(st): State<Arc<StudioState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "rite": env!("CARGO_PKG_VERSION"),
        "language_version": "1",
        "token_required": true,
        "project": st.project.as_ref().map(|p| p.display().to_string()),
    }))
}

async fn studio_parse(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    Json(serde_json::to_value(rite_wasm::parse(&body.source)).unwrap_or_default())
}

async fn studio_analyze(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    Json(rite_wasm::analyze(&body.source))
}

async fn studio_format(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    let d = body.dialect.as_deref().unwrap_or("glyph");
    match rite_wasm::format(&body.source, d) {
        Ok(r) => Json(serde_json::json!({
            "ok": true,
            "text": r.text,
            "dialect": r.dialect,
            "source_map": r.source_map,
        })),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

async fn studio_run(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    // Await the async runner — do not call run_blocking() here (nested Tokio panic).
    let result = rite_wasm::run(
        &body.source,
        rite_wasm::RunOptions {
            allow_all: true,
            timeout_ms: Some(5000),
            seed: Some(42),
            files: Default::default(),
            browser_safe: false,
        },
    )
    .await;
    Json(serde_json::to_value(result).unwrap_or_default())
}

async fn studio_check(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    Json(rite_wasm::analyze(&body.source))
}

async fn studio_emit_rust(Json(body): Json<SourceBody>) -> Json<serde_json::Value> {
    Json(rite_wasm::emit_rust(&body.source))
}

// Minimal embedded Studio shell (full Vue app lives in apps/rite-studio)
const STUDIO_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>Rite Studio</title>
<style>
:root { color-scheme: dark; --bg:#0b0f14; --panel:#121821; --fg:#e6edf3; --accent:#7ee0ff; --pink:#ff7edb; }
* { box-sizing: border-box; }
body { margin:0; font-family: ui-sans-serif, system-ui, sans-serif; background:var(--bg); color:var(--fg); }
header { display:flex; gap:.5rem; flex-wrap:wrap; padding:.75rem 1rem; border-bottom:1px solid #222; align-items:center; }
header h1 { font-size:1rem; margin:0; color:var(--accent); margin-right:auto; }
button, select { background:#1a2332; color:var(--fg); border:1px solid #334; border-radius:6px; padding:.4rem .7rem; cursor:pointer; }
button:hover { border-color:var(--accent); }
main { display:grid; grid-template-columns: 1fr 1fr; min-height: calc(100vh - 52px); }
@media (max-width: 800px) { main { grid-template-columns: 1fr; } }
textarea { width:100%; height:100%; min-height:50vh; background:var(--panel); color:var(--fg); border:0; padding:1rem; font-family: ui-monospace, monospace; font-size:14px; resize:none; }
.side { display:flex; flex-direction:column; border-left:1px solid #222; }
.tabs { display:flex; gap:.25rem; padding:.5rem; border-bottom:1px solid #222; flex-wrap:wrap; }
.tabs button.active { border-color:var(--pink); color:var(--pink); }
pre { margin:0; padding:1rem; overflow:auto; flex:1; white-space:pre-wrap; font-size:13px; }
</style>
</head>
<body>
<header>
  <h1>Rite Studio ◆</h1>
  <button id="run">Run</button>
  <button id="check">Check</button>
  <button id="fmt">Format</button>
  <select id="dialect"><option value="glyph">glyph</option><option value="ascii">ascii</option></select>
  <button id="convert">Convert</button>
  <button id="emit">Emit Rust</button>
</header>
<main>
  <textarea id="src">◆ square(n) ⟦
  ^ n * n
⟧
! @console.println(str(square(12)))
</textarea>
  <div class="side">
    <div class="tabs">
      <button class="active" data-tab="out">Output</button>
      <button data-tab="diag">Diagnostics</button>
      <button data-tab="ast">AST</button>
      <button data-tab="ir">IR</button>
      <button data-tab="rust">Rust</button>
    </div>
    <pre id="panel">Ready.</pre>
  </div>
</main>
<script>
const panel = document.getElementById('panel');
const src = document.getElementById('src');
let tab = 'out';
document.querySelectorAll('.tabs button').forEach(b => b.onclick = () => {
  document.querySelectorAll('.tabs button').forEach(x => x.classList.remove('active'));
  b.classList.add('active'); tab = b.dataset.tab; panel.textContent = '…';
});
async function post(path, body) {
  const r = await fetch(path, { method:'POST', headers:{'content-type':'application/json'}, body: JSON.stringify(body) });
  return r.json();
}
document.getElementById('run').onclick = async () => {
  const j = await post('/api/v1/run', { source: src.value });
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('check').onclick = async () => {
  const j = await post('/api/v1/analyze', { source: src.value });
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('fmt').onclick = async () => {
  const dialect = document.getElementById('dialect').value;
  const j = await post('/api/v1/format', { source: src.value, dialect });
  if (j.ok) src.value = j.text;
  panel.textContent = JSON.stringify(j, null, 2);
};
document.getElementById('convert').onclick = async () => {
  const dialect = document.getElementById('dialect').value;
  const j = await post('/api/v1/format', { source: src.value, dialect });
  if (j.ok) src.value = j.text;
};
document.getElementById('emit').onclick = async () => {
  const j = await post('/api/v1/emit-rust', { source: src.value });
  panel.textContent = j.rust || JSON.stringify(j, null, 2);
};
</script>
</body>
</html>"#;
