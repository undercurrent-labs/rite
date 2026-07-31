//! Rite CLI — run, build, check, fmt, repl, test, doc, and more.

mod archive;
mod config;
mod docs_cmd;
mod github;
mod skill_cmd;
mod studio;
mod update_cmd;
mod util;
mod vscode_cmd;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use docs_cmd::DocsCmd;
use rite_caps::{Permission, PermissionSet};
use rite_core::SourceMap;
use rite_runtime::{EvalError, RuntimeContext};
use rite_sem::{compile_to_ir, ir_to_json};
use rite_syntax::parse_source;
use std::path::PathBuf;
use std::process::ExitCode;
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
    ///
    /// Output written by the script is always emitted, including when the script
    /// fails. When the script's final expression evaluates to something other
    /// than `none`, that value is printed after the script's own output.
    ///
    /// Arguments after `--` are readable with `! @process.args`.
    Run {
        /// Script to interpret
        file: PathBuf,
        /// Grant a permission, e.g. `fs:read=./data` or `net=api.example.com`
        #[arg(long = "allow", value_name = "PERM")]
        allow: Vec<String>,
        /// Grant every permission — trusted scripts only
        #[arg(long = "allow-all")]
        allow_all: bool,
        /// Revoke a permission that is allowed by default (console, clock, random)
        #[arg(long = "deny", value_name = "PERM")]
        deny: Vec<String>,
        /// Wall-clock limit for the run, e.g. `30s` or `5m`
        #[arg(long)]
        timeout: Option<String>,
        /// Stop after this many evaluation steps
        #[arg(long = "max-steps")]
        max_steps: Option<u64>,
        /// Print an execution summary (steps, elapsed) and stack traces to stderr
        #[arg(long)]
        trace: bool,
        /// Report diagnostics as JSON on stderr instead of rendered text
        #[arg(long = "json-errors")]
        json_errors: bool,
        /// Script arguments (after `--`), readable with `! @process.args`
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile a Rite script to a native binary
    Build {
        /// Script to compile
        file: PathBuf,
        /// Build with optimisations (slower to build, faster to run)
        #[arg(long)]
        release: bool,
        /// Write the generated Rust instead of linking a binary
        #[arg(long = "emit-rust")]
        emit_rust: bool,
        /// Path for the compiled binary
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Bake in every permission — trusted scripts only
        #[arg(long = "allow-all")]
        allow_all: bool,
        /// Bake a permission into the binary, e.g. `fs:read=./data`
        #[arg(long = "allow", value_name = "PERM")]
        allow: Vec<String>,
    },
    /// Lex, parse, resolve, and effect-check without executing
    Check {
        /// Script to check
        file: PathBuf,
        /// Report diagnostics as JSON on stderr instead of rendered text
        #[arg(long = "json-errors")]
        json_errors: bool,
    },
    /// Format Rite source (rewrites files in place)
    Fmt {
        /// Files or directories to format; required unless --all is given
        paths: Vec<PathBuf>,
        /// Format every .rite file under the current directory
        #[arg(long)]
        all: bool,
        /// Shorthand for --dialect ascii
        #[arg(long)]
        ascii: bool,
        /// Dialect to format to: `glyph` or `ascii`
        #[arg(long, default_value = "glyph")]
        dialect: String,
        /// Exit 1 if any file would change (does not write)
        #[arg(long)]
        check: bool,
        /// List files that would change, then exit 0 (does not write)
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Interactive REPL
    Repl {
        /// Grant every permission for the session — local exploration only
        #[arg(long = "allow-all")]
        allow_all: bool,
    },
    /// Run Rite tests
    Test {
        /// Files or directories to search (default: `tests` and `examples`)
        paths: Vec<PathBuf>,
        /// Only run tests whose name contains this substring
        #[arg(long)]
        filter: Option<String>,
        /// Run through the interpreter (the default)
        #[arg(long)]
        interpreted: bool,
        /// Run through the compiled path instead of the interpreter
        #[arg(long)]
        compiled: bool,
        /// Run both ways and require the results to agree
        #[arg(long)]
        both: bool,
        /// Emit results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate documentation
    Doc {
        /// Rite file or directory whose `///` doc comments to include (optional;
        /// without it only the language reference is generated)
        path: Option<PathBuf>,
        /// Directory to write the generated documentation into
        #[arg(long, default_value = "docs/generated")]
        out: PathBuf,
    },
    /// Show desugared forms
    Explain {
        /// Script whose desugared form to print
        file: PathBuf,
    },
    /// Dump AST
    Ast {
        /// Script to parse
        file: PathBuf,
        /// Print the tree as JSON
        #[arg(long)]
        json: bool,
    },
    /// Dump semantic IR
    Ir {
        /// Script to lower
        file: PathBuf,
        /// Print the IR as JSON
        #[arg(long)]
        json: bool,
    },
    /// List capabilities
    Capabilities,
    /// Convert dialect (ascii/glyph/mixed)
    Convert {
        /// Script to convert
        file: PathBuf,
        /// Target dialect: `glyph`, `ascii` or `mixed`
        #[arg(long = "to", default_value = "glyph")]
        to: String,
        /// Print the result instead of rewriting the file
        #[arg(long)]
        stdout: bool,
        /// Exit 1 if the file is not already in the target dialect
        #[arg(long)]
        check: bool,
    },
    /// Start language server (stdio)
    Lsp,
    /// Launch Rite Studio local service (loopback, token-authenticated)
    Studio {
        /// Port for the loopback API
        #[arg(long, default_value = "4041")]
        port: u16,
        /// Do not open a browser window on start
        #[arg(long = "no-open")]
        no_open: bool,
        /// Project root: relative paths in executed scripts resolve here
        #[arg(long)]
        project: Option<PathBuf>,
        /// Let Studio-executed scripts use the full host (fs, network, processes)
        #[arg(long = "allow-all")]
        allow_all: bool,
    },
    /// Dump syntax tree (alias of ast)
    #[command(name = "syntax-tree")]
    SyntaxTree {
        /// Script to parse
        file: PathBuf,
        /// Print the tree as JSON
        #[arg(long)]
        json: bool,
    },
    /// Dump semantic IR (alias of ir)
    #[command(name = "semantic-ir")]
    SemanticIr {
        /// Script to lower
        file: PathBuf,
        /// Print the IR as JSON
        #[arg(long)]
        json: bool,
    },
    /// Emit generated Rust without full native link
    #[command(name = "emit-rust")]
    EmitRust {
        /// Script to lower into Rust
        file: PathBuf,
    },
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
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
    },
    /// Alias for `update`
    #[command(name = "self-update")]
    SelfUpdate {
        /// Only report; exit 1 if an update is available
        #[arg(long)]
        check: bool,
        /// Reinstall even if versions match
        #[arg(long)]
        force: bool,
        /// Install a specific release tag
        /// Release tag to fetch (default: latest)
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
        /// Include build details and the paths the CLI resolved
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
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
        /// Refresh even if cache looks current
        #[arg(long)]
        force: bool,
    },
    /// Re-fetch skill and reinstall to previously recorded paths
    Update {
        /// Refresh even if the cache looks current
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
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
    },
    /// Download the .vsix without installing
    Download {
        /// Output path for the .vsix
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
    },
    /// Show release asset details and install instructions
    Info {
        /// Release tag to fetch (default: latest)
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DescribeCmd {
    /// Sigils, keywords, capabilities and diagnostics in one payload
    Language {
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Glyph and ASCII spellings for every construct
    Syntax {
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// One host capability and its functions
    Capability {
        /// Capability name, e.g. `fs` or `http`
        name: String,
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// One diagnostic code and what triggers it
    Diagnostic {
        /// Diagnostic code, e.g. `E021`
        code: String,
        /// Emit JSON instead of text
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

/// Known top-level subcommands, read from clap's own command tree.
///
/// This used to be a hand-maintained `matches!` list that had to stay in sync
/// with [`Commands`]; a new subcommand that nobody added here would have been
/// swallowed by the implicit-run rewrite below and reported as a missing script.
fn is_known_subcommand(name: &str) -> bool {
    Cli::command()
        .get_subcommands()
        .any(|sub| sub.get_name() == name || sub.get_all_aliases().any(|alias| alias == name))
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
    use super::{is_known_subcommand, rewrite_argv_for_implicit_run, Cli};
    use clap::CommandFactory;

    fn args(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    /// Every subcommand clap knows about must be recognized by the
    /// implicit-run guard, and vice versa: no drift possible.
    #[test]
    fn subcommand_list_matches_clap() {
        let mut seen = 0;
        for sub in Cli::command().get_subcommands() {
            seen += 1;
            let name = sub.get_name().to_string();
            assert!(
                is_known_subcommand(&name),
                "clap subcommand `{name}` not recognized by is_known_subcommand"
            );
            let rewritten = rewrite_argv_for_implicit_run(args(&["rite", &name]));
            assert_eq!(
                rewritten,
                args(&["rite", &name]),
                "`rite {name}` must not be rewritten into `rite run {name}`"
            );
        }
        assert!(seen > 10, "expected the full command surface, saw {seen}");
        // A script path is not a subcommand.
        assert!(!is_known_subcommand("hello.rite"));
        assert!(!is_known_subcommand("runner"));
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
            // Delegate to the rite-lsp binary. Prefer the one shipped next to
            // this executable (release archives contain both, and an editor
            // pointing at an absolute `rite` should get the matching server)
            // before falling back to PATH.
            let exe_name = if cfg!(windows) {
                "rite-lsp.exe"
            } else {
                "rite-lsp"
            };
            let sibling = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(exe_name)))
                .filter(|p| p.is_file());
            let program = sibling.unwrap_or_else(|| PathBuf::from("rite-lsp"));
            match std::process::Command::new(&program).status() {
                Ok(s) if s.success() => Ok(ExitCode::SUCCESS),
                Ok(s) => Ok(ExitCode::from(s.code().unwrap_or(1) as u8)),
                Err(e) => {
                    eprintln!("cannot start {} ({e})", program.display());
                    eprintln!("install the LSP server or run: cargo run -p rite-lsp");
                    Ok(ExitCode::from(2))
                }
            }
        }
        Commands::Studio {
            port,
            no_open,
            project,
            allow_all,
        } => studio::run(port, no_open, project.as_deref(), allow_all).await,
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
        Commands::Docs { cmd } => docs_cmd::run(cmd).await,
        Commands::Describe { target } => run_describe(target),
        Commands::Check { file, json_errors } => {
            // Path-aware compile: `compile_to_ir` has no notion of where the file lives,
            // so `use math` could not be resolved and `rite check` reported E026 on
            // scripts that `rite run` executes fine. `compile_path` resolves imports
            // relative to the script, matching the runtime.
            let (ir, diags, sources) = rite_sem::compile_path(&file);
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
            all,
            ascii,
            dialect,
            check,
            dry_run,
        } => {
            // `rite fmt` with no argument used to mean "rewrite every .rite file
            // under the current directory" — a destructive default for a
            // no-argument command. Formatting a whole tree is now opt-in.
            if paths.is_empty() && !all {
                eprintln!("rite fmt: no paths given");
                eprintln!(
                    "  pass files or directories, or `--all` to format every .rite file under {}",
                    std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| ".".into())
                );
                eprintln!(
                    "  `--check` reports without writing; `--dry-run` lists what would change"
                );
                return Ok(ExitCode::from(2));
            }
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
            let mut files = Vec::new();
            for path in &paths {
                files.extend(util::collect_rite_files(path)?);
            }
            let writing = !check && !dry_run;
            if writing && files.len() > 1 {
                // Say what is about to be rewritten in place.
                println!("rite fmt: {} file(s) to check", files.len());
            }

            let mut failed = false;
            let mut changed = 0usize;
            for file in files {
                let text = std::fs::read_to_string(&file)?;
                match rite_fmt::format_with_dialect(&text, dialect) {
                    Ok(formatted) => {
                        if formatted.text == text {
                            continue;
                        }
                        changed += 1;
                        if check {
                            eprintln!("would reformat {}", file.display());
                            failed = true;
                        } else if dry_run {
                            println!("would reformat {}", file.display());
                        } else {
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
            if dry_run {
                println!("{changed} file(s) would change");
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
            trace,
            json_errors,
            args,
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
            // Stream output instead of buffering it to the end of the run: a server or a
            // long loop should print as it goes, and a chatty script should not hold
            // every line in memory. `flush_script_output` still runs on every exit path
            // and finds the buffers empty.
            ctx.sink = Some(std::sync::Arc::new(|stream, text: &str| {
                use std::io::Write;
                match stream {
                    rite_runtime::OutputStream::Stdout => {
                        let mut out = std::io::stdout().lock();
                        let _ = out.write_all(text.as_bytes());
                        // Line-buffered by default when piped, so flush to keep the
                        // ordering a reader sees the same as the script's.
                        let _ = out.flush();
                    }
                    rite_runtime::OutputStream::Stderr => {
                        let mut err = std::io::stderr().lock();
                        let _ = err.write_all(text.as_bytes());
                        let _ = err.flush();
                    }
                }
            }));
            // Everything after `--`, readable with `! @process.args`.
            ctx.script_args = args;
            if let Some(parent) = file.parent() {
                ctx.script_dir = Some(parent.to_path_buf());
                ctx.module_roots.push(parent.to_path_buf());
            }
            if let Some(ms) = max_steps {
                ctx.budget = ctx.budget.with_max_steps(ms);
            }
            if let Some(t) = timeout {
                // A bad --timeout used to be discarded silently, leaving the
                // default 60s budget in place.
                match parse_duration(&t) {
                    Ok(dur) => ctx.budget = ctx.budget.with_timeout(dur),
                    Err(e) => {
                        eprintln!("invalid --timeout {t:?}: {e}");
                        return Ok(ExitCode::from(2));
                    }
                }
            }
            rite_caps::install_defaults(&mut ctx, perms);

            let started = std::time::Instant::now();
            let result = rite_runtime::run_file(&sf, &mut ctx).await;
            let elapsed = started.elapsed();

            // Script output is emitted before the result is inspected, so it
            // survives every failure path.
            flush_script_output(&ctx);

            if trace {
                eprintln!("trace: script  {}", file.display());
                eprintln!(
                    "trace: steps   {} in {:.3}ms",
                    ctx.budget.steps(),
                    elapsed.as_secs_f64() * 1000.0
                );
                eprintln!(
                    "trace: outcome {}",
                    match &result {
                        Ok(_) => "ok".to_string(),
                        Err(e) => format!("error: {e}"),
                    }
                );
                let stack = ctx.format_stack_trace();
                if !stack.is_empty() {
                    eprintln!("trace:{stack}");
                }
            }

            match result {
                Ok(value) => {
                    // The final expression's value is printed whenever it is not
                    // `none`. It used to be suppressed as soon as the script had
                    // printed anything, which made the result vanish for any
                    // script that also logged.
                    if !matches!(value, rite_runtime::Value::None) {
                        println!("{}", value.to_display(&ctx.atoms));
                    }
                    Ok(ExitCode::SUCCESS)
                }
                // What each failure *says* is decided here; what it *exits with* is
                // `EvalError::exit_code`, so `rite run` and a compiled binary cannot
                // disagree about the same error.
                Err(e) => {
                    match &e {
                        EvalError::Compile(d) => {
                            if json_errors {
                                println!("{}", serde_json::to_string_pretty(&d.to_json())?);
                            } else {
                                eprint!("{}", d.render_all(&sources));
                            }
                        }
                        // The script chose its own status, so the runtime says
                        // nothing: printing "runtime error: exit 2" would be the CLI
                        // editorialising over a deliberate decision. Output was
                        // already flushed above.
                        EvalError::Exit(_) => {}
                        EvalError::Permission(m) => eprintln!("permission denied: {}", m),
                        EvalError::Budget(b) => eprintln!("budget exceeded: {}", b),
                        other => eprintln!("runtime error: {}", other),
                    }
                    Ok(ExitCode::from(e.exit_code()))
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

/// Write everything the script produced: stdout buffer to stdout, stderr buffer
/// to stderr.
///
/// The runtime buffers `@console` output for the whole program in
/// `ctx.stdout` / `ctx.stderr`. This used to be drained only in the success
/// arm, so `println` followed by a runtime error printed nothing at all, and
/// `@console.warn` / `@console.error` were never shown on any path.
fn flush_script_output(ctx: &RuntimeContext) {
    use std::io::Write;
    if !ctx.stdout.is_empty() {
        let mut out = std::io::stdout().lock();
        for chunk in &ctx.stdout {
            let _ = out.write_all(chunk.as_bytes());
        }
        let _ = out.flush();
    }
    if !ctx.stderr.is_empty() {
        let mut err = std::io::stderr().lock();
        for chunk in &ctx.stderr {
            let _ = err.write_all(chunk.as_bytes());
        }
        let _ = err.flush();
    }
}

/// Parse `500ms`, `30s`, `5m`, or a bare number of seconds.
fn parse_duration(s: &str) -> Result<Duration, String> {
    let t = s.trim();
    let invalid = |unit: &str| format!("expected a number{unit} (e.g. 500ms, 30s, 5m)");
    if let Some(ms) = t.strip_suffix("ms") {
        return ms
            .trim()
            .parse()
            .map(Duration::from_millis)
            .map_err(|_| invalid(" of milliseconds"));
    }
    if let Some(sec) = t.strip_suffix('s') {
        return sec
            .trim()
            .parse()
            .map(Duration::from_secs)
            .map_err(|_| invalid(" of seconds"));
    }
    if let Some(m) = t.strip_suffix('m') {
        return m
            .trim()
            .parse::<u64>()
            .map(|n| Duration::from_secs(n * 60))
            .map_err(|_| invalid(" of minutes"));
    }
    t.parse::<u64>()
        .map(Duration::from_secs)
        .map_err(|_| invalid(" of seconds"))
}

#[cfg(test)]
mod duration_tests {
    use super::parse_duration;
    use std::time::Duration;

    #[test]
    fn parses_units() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(parse_duration("7"), Ok(Duration::from_secs(7)));
        assert_eq!(parse_duration(" 7 "), Ok(Duration::from_secs(7)));
    }

    #[test]
    fn rejects_garbage_instead_of_ignoring_it() {
        for bad in ["", "abc", "1h", "-5s", "1.5s", "ms", "s"] {
            assert!(parse_duration(bad).is_err(), "{bad:?} should not parse");
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
        DescribeCmd::Diagnostic { code, json } => docs_cmd::describe_diagnostic(&code, json),
    }
}
