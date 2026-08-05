//! The `cant` executable.
//!
//! # What is here, and what is not
//!
//! The whole published command surface is here: `version`, `check`, `parse`,
//! `fmt`, `convert`, `graph`, `expand`, `explain`, `run`, `build` and `repl`,
//! plus the top-level `-e` that implies `run`.
//!
//! # Shell quoting
//!
//! Cant's operators include `>`, `|`, `!`, `?` and `*`, which are shell
//! metacharacters. The canonical form quotes the expression, exactly as `awk`,
//! `sed` and `jq` do:
//!
//! ```text
//! cant check -e '["a.txt"] -> * -> !@fs.read -> lines -> * -> ?{ $ != "" } -> []'
//! ```
//!
//! The language is not deformed to make unquoted use safe, and the docs do not
//! claim unquoted one-liners are portable.

use cant_syntax::{CantDiagnostics, Dialect, FormatOptions, ParseResult};
mod highlight;
mod modules;
mod repl;
mod sigil;

use clap::{Parser, Subcommand};
use rite_core::SourceMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

/// Invalid CLI usage, matching Rite's exit contract.
const EXIT_USAGE: u8 = 2;

#[derive(Parser, Debug)]
#[command(
    name = "cant",
    version,
    about = "Cant — a terminal-typeable, graph-oriented sibling to Rite",
    long_about = "Cant is a sibling front end to Rite, not a Rite dialect. It compiles to \
canonical ASCII Rite and runs on Rite's runtime, capabilities and compiler.\n\n\
Sources come from a file, from `-` for standard input, or from `-e`. Quote a `-e` \
expression: Cant's operators are shell metacharacters."
)]
struct Cli {
    /// Run this expression — quote it
    ///
    /// The canonical one-liner form, and the reason it is a top-level flag
    /// rather than a subcommand: `cant -e '…'` should be as short as `awk '…'`.
    // No `conflicts_with`: clap cannot name a subcommand there, and the two
    // spellings are not in conflict anyway — `cant run -e '…'` and `cant -e '…'`
    // are the same command, which is what the shorthand means.
    #[arg(
        long,
        short = 'e',
        value_name = "EXPRESSION",
        allow_hyphen_values = true
    )]
    expr: Option<String>,

    // `global = true` so they are accepted before *or* after the subcommand:
    // `cant run p.cant --allow fs:read=.` is how anyone would write it.
    /// Grant a permission, e.g. `fs:read=./data` or `net=api.example.com`
    #[arg(long = "allow", value_name = "PERM", global = true)]
    allow: Vec<String>,
    /// Grant every permission — trusted programs only
    #[arg(long = "allow-all", global = true)]
    allow_all: bool,
    /// Revoke a permission that is allowed by default (console, clock, random)
    #[arg(long = "deny", value_name = "PERM", global = true)]
    deny: Vec<String>,
    /// Wall-clock limit, e.g. `30s` or `5m`
    #[arg(long, global = true)]
    timeout: Option<String>,
    /// Stop after this many evaluation steps
    #[arg(long = "max-steps", global = true)]
    max_steps: Option<u64>,
    /// Maximum nested call depth
    #[arg(long = "max-call-depth", global = true)]
    max_call_depth: Option<usize>,
    /// Maximum number of elements in one collection
    #[arg(long = "max-collection-size", global = true)]
    max_collection_size: Option<usize>,
    /// Maximum length of one string or byte buffer
    #[arg(long = "max-string-size", global = true)]
    max_string_size: Option<usize>,

    /// Load `KEY=VALUE` pairs from a file into this run's environment
    ///
    /// Repeatable; later files win. Reading exactly the names it defines is
    /// granted implicitly — a file you named on this command line is your own
    /// input to the program, the same argument `@process.args` makes — so
    /// `cant --env-file .env -e '"API_KEY" -> !@env.get'` needs no `--allow`.
    /// The process's own environment is not modified.
    #[arg(long = "env-file", value_name = "PATH", global = true)]
    env_file: Vec<PathBuf>,

    /// Make a module available without the program saying `use`
    ///
    /// Repeatable. For `-e` and the REPL above all, which have no file to put a
    /// `use` line at the top of. Also settable with `CANT_USE=a,b` and with
    /// `use = [...]` in a `cant.toml` found by walking up from here; a flag
    /// adds to those rather than replacing them, and `--no-default-use` turns
    /// them off.
    #[arg(long = "use", value_name = "MODULE", global = true)]
    use_modules: Vec<String>,
    /// Also search this directory for modules
    ///
    /// Repeatable. Also settable with `CANT_MODULE_PATH`, and with
    /// `module-roots = [...]` in a `cant.toml`.
    #[arg(long = "module-root", value_name = "DIR", global = true)]
    module_root: Vec<PathBuf>,
    /// Ignore `CANT_USE`, `CANT_MODULE_PATH` and any `cant.toml`
    #[arg(long = "no-default-use", global = true)]
    no_default_use: bool,

    /// When to colour output: `auto` (a terminal that has not said otherwise),
    /// `always`, or `never`
    ///
    /// `NO_COLOR` and `CLICOLOR_FORCE` are honoured under `auto`. The palette is
    /// built for a dark background — on a light terminal, `never`.
    #[arg(long, value_name = "WHEN", default_value = "auto", global = true)]
    color: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// The permission and budget flags, as `rite::RuntimeOptions`.
///
/// Both executables parse the same strings the same way; the declarations differ
/// because the help text is each tool's own. See `crates/rite/src/options.rs`.
fn runtime_options(cli: &Cli) -> rite::RuntimeOptions {
    rite::RuntimeOptions {
        allow: cli.allow.clone(),
        deny: cli.deny.clone(),
        allow_all: cli.allow_all,
        timeout: cli.timeout.clone(),
        max_steps: cli.max_steps,
        max_call_depth: cli.max_call_depth,
        max_collection_size: cli.max_collection_size,
        max_string_size: cli.max_string_size,
        env_files: cli.env_file.clone(),
    }
}

/// The subcommands.
///
/// `Box`ed variants would shrink the enum — `Sigil` carries fifteen fields and
/// clippy notices — but this value is constructed once per process, moved once,
/// and matched once. Boxing it would trade a readable declaration for an
/// allocation nobody can measure, so the lint is turned off here rather than
/// obeyed.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Debug)]
enum Commands {
    /// Print the Cant, language, graph schema, and Rite versions
    Version {
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
    },
    /// Check a Cant source
    ///
    /// Syntax and graph validation, then the expansion is checked by Rite's
    /// resolver, so name resolution, effect discipline and capability
    /// requirements are all reported here too.
    Check {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Check this expression instead of a file — quote it
        ///
        /// `allow_hyphen_values`: a Cant expression routinely starts with an
        /// operator (`-> f`), which clap would otherwise read as a flag.
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Report diagnostics as JSON on stdout instead of rendered text
        #[arg(long = "json-errors")]
        json_errors: bool,
    },
    /// Run a program and compare its value against an expectation
    ///
    /// The expectation comes from `--expect`, or from the sidecar file
    /// `<source>.expect` beside the program. A mismatch exits 7 — the exit the
    /// contract reserves for test failures — with both values shown; any other
    /// failure keeps its own exit code.
    Test {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Test this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// The expected final value, as `cant run` would print it
        #[arg(long, value_name = "TEXT", allow_hyphen_values = true)]
        expect: Option<String>,
    },
    /// Format Cant source
    ///
    /// Idempotent, and refuses rather than risks losing a comment: the formatter
    /// re-lexes its own output and fails if the comments changed.
    Fmt {
        /// Source file, or `-` for standard input; writes to stdout for both
        /// unless --write is given
        source: Option<PathBuf>,
        /// Format this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Format to ASCII (the canonical spelling, and the default)
        #[arg(long, conflicts_with = "glyph")]
        ascii: bool,
        /// Format to glyphs
        #[arg(long)]
        glyph: bool,
        /// Keep the spelling the source already uses
        #[arg(long, conflicts_with_all = ["ascii", "glyph"])]
        preserve: bool,
        /// Print the whole program on one line
        #[arg(long)]
        compact: bool,
        /// Break lines longer than this
        #[arg(long, value_name = "COLUMNS", default_value_t = cant_syntax::fmt::DEFAULT_MAX_WIDTH)]
        width: usize,
        /// Exit 1 if the source is not already formatted (does not write)
        #[arg(long)]
        check: bool,
        /// Rewrite the file in place
        #[arg(long, conflicts_with = "check")]
        write: bool,
    },
    /// Convert between the ASCII and glyph spellings
    ///
    /// Respells structural operators and nothing else: whitespace, line breaks,
    /// comments, strings and leaf text come through byte for byte.
    Convert {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Convert this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Target spelling
        #[arg(long = "to", value_name = "ascii|glyph")]
        to: String,
        /// Print the result instead of rewriting the file (the default for
        /// stdin and -e)
        #[arg(long)]
        stdout: bool,
        /// Exit 1 if the source is not already in the target spelling
        #[arg(long)]
        check: bool,
    },
    /// Run a Cant program
    ///
    /// Expands to canonical Rite and executes it on Rite's runtime, with Rite's
    /// permissions and budgets. `cant expand` prints exactly what runs.
    Run {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Run this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Report diagnostics as JSON on stdout instead of rendered text
        #[arg(long = "json-errors")]
        json_errors: bool,
        /// Report per-node emission counts after the run (cant.trace JSON on stderr)
        #[arg(long)]
        trace: bool,
        /// Write the trace to a file instead of standard error; implies --trace
        #[arg(long = "trace-out", value_name = "PATH")]
        trace_out: Option<PathBuf>,
        /// Program arguments (after `--`), readable with `! @process.args`
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Compile a Cant program to a native binary
    ///
    /// Through Rite's compiler: the expansion is written beside the source under
    /// `.rite/cant/` and built from there, so the artifact stays auditable.
    Build {
        /// Source file
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
    },
    /// Print the canonical Rite a program expands to
    ///
    /// This is exactly what `cant run` executes. It is a permanent, public
    /// command, not a debugging aid.
    Expand {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Expand this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Also print the Cant ↔ Rite span map
        #[arg(long = "source-map")]
        source_map: bool,
        /// Write to this path instead of standard output
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Explain what a program does, in prose
    ///
    /// A semantic reading, not a syntax-tree dump: the steps in order, the
    /// capabilities it needs, where it touches the world, and what is worth
    /// knowing before running it.
    Explain {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Explain this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Also point at the other two views of the program
        #[arg(long, short)]
        verbose: bool,
    },
    /// Start an interactive session
    ///
    /// Each line is a whole program. Nothing persists between them, because Cant
    /// has no bindings — see `docs/cant/cli.md`.
    Repl,
    /// Refuses, and says to use `rite update`
    ///
    /// `cant` ships inside the Rite release archive and has no updater of its
    /// own. Present as a command rather than absent so the answer is a sentence
    /// instead of clap's "unrecognized subcommand" — someone typing this has a
    /// reasonable question, and the reasonable answer is one line long.
    Update,
    /// Print the flow graph
    ///
    /// JSON is the machine format; DOT is the technical topology view — pipe it
    /// to `dot -Tsvg`. `cant sigil` is the stylized one, and neither replaces
    /// the other (ADR 0008).
    Graph {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Graph this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Output format
        #[arg(long, default_value = "json", value_name = "json|dot")]
        format: String,
    },
    /// Render the program's topology as a sigil
    ///
    /// A deterministic ritual artifact: entry at the centre, flow spiralling
    /// outward, forks in ordered sectors, orbits as closed rings, and host
    /// invocations on the outer boundary. `cant graph` remains the technical
    /// view; neither replaces the other (ADR 0008).
    Sigil {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Render this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Render a `cant.graph` JSON document instead of source; `-` for stdin
        #[arg(long, value_name = "PATH")]
        graph: Option<PathBuf>,
        /// Where to write; `-` for standard output
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// Output format
        #[arg(long, default_value = "svg", value_name = "svg|png|html|scene-json")]
        format: String,
        /// Visual theme
        #[arg(
            long,
            default_value = "neon-ritual",
            value_name = "neon-ritual|void|parchment"
        )]
        theme: String,
        /// How much is visible in the artifact
        #[arg(
            long,
            default_value = "veiled",
            value_name = "veiled|inscribed|revealed"
        )]
        mode: String,
        /// How much is embedded, visible or not
        #[arg(long, default_value = "safe", value_name = "full|safe|minimal|none")]
        metadata: String,
        /// Seed for deterministic variation
        #[arg(
            long,
            default_value = "graph",
            value_name = "graph|canonical|random|INTEGER"
        )]
        seed: String,
        /// The documented fixed orientation and seed, for reproducible output
        #[arg(long)]
        canonical: bool,
        /// Background: `theme`, `transparent`, or a `#rrggbb` colour
        #[arg(long, default_value = "theme", value_name = "theme|transparent|HEX")]
        background: String,
        /// How much non-semantic decoration to draw
        #[arg(
            long,
            default_value = "ritual",
            value_name = "none|sparse|ritual|maximal"
        )]
        ornament: String,
        /// How traces are drawn
        #[arg(
            long,
            default_value = "flowing",
            value_name = "flowing|concentric|circuit"
        )]
        tracery: String,
        /// Weight the render with a traced run (`cant run --trace-out PATH`)
        #[arg(long, value_name = "PATH")]
        weights: Option<PathBuf>,
        /// Ghost an older version of the program beneath the render (needs --canonical)
        #[arg(long, value_name = "OLD.cant")]
        diff: Option<PathBuf>,
        /// Pixel width (the canvas is square, so this sets both dimensions)
        #[arg(long)]
        width: Option<f64>,
        /// PNG rasterisation scale, when `--width` is not given
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// Embed the scene JSON in an HTML export (needs `--metadata full`)
        #[arg(long = "embed-scene")]
        embed_scene: bool,
        /// Draw skeleton marks only — for a graph too dense for full variation
        #[arg(long)]
        simplify: bool,
        /// Refuse a graph larger than this
        #[arg(long, value_name = "N")]
        max_nodes: Option<usize>,
        /// Render and report the fingerprint without writing anything
        #[arg(long)]
        check: bool,
    },
    /// Print the parsed syntax tree
    Parse {
        /// Source file, or `-` for standard input
        source: Option<PathBuf>,
        /// Parse this expression instead of a file — quote it
        #[arg(
            long,
            short = 'e',
            value_name = "EXPRESSION",
            allow_hyphen_values = true
        )]
        expr: Option<String>,
        /// Print the tree as JSON
        #[arg(long)]
        json: bool,
        /// Print the span-free structure used to compare two spellings
        #[arg(long)]
        structure: bool,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = runtime_options(&cli);
    // Taken before the subcommand match moves `cli` apart.
    let module_flags = ModuleFlags::of(&cli);

    // `cant -e '…'` with no subcommand is `cant run -e '…'`. Kept explicit
    // rather than rewritten in argv the way `rite` does it: Cant has no
    // implicit-run-a-file form, so there is nothing ambiguous to guess at.
    let Some(command) = cli.command else {
        return match cli.expr {
            Some(expr) => match load_source(None, Some(&expr)) {
                Ok(input) => match resolve_modules(&module_flags, None) {
                    Ok(env) => run_program(input, options, false, Vec::new(), env).await,
                    Err(e) => usage_error(&e),
                },
                Err(usage) => usage_error(&usage),
            },
            None => {
                eprintln!("cant: no command and no `-e` expression");
                eprintln!("  try `cant --help`, or `cant -e '[1, 2, 3] -> * -> []'`");
                ExitCode::from(EXIT_USAGE)
            }
        };
    };

    match command {
        Commands::Version { json } => {
            let info = cant::version_info();
            if json {
                match serde_json::to_string_pretty(&info.to_json()) {
                    Ok(text) => println!("{text}"),
                    Err(e) => {
                        eprintln!("cant: {e}");
                        return ExitCode::from(1);
                    }
                }
            } else {
                println!("cant {}", info.tool);
                println!("cant_language_version: {}", info.language);
                println!("cant_graph_schema_version: {}", info.graph_schema);
                println!("rite: {}", info.rite);
            }
            ExitCode::SUCCESS
        }
        Commands::Check {
            source,
            expr,
            json_errors,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => {
                match resolve_modules(&module_flags, script_dir_of(source.as_deref()).as_deref()) {
                    Ok(env) => check(input, json_errors, &env),
                    Err(e) => usage_error(&e),
                }
            }
            Err(usage) => usage_error(&usage),
        },
        Commands::Parse {
            source,
            expr,
            json,
            structure,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => print_tree(input, json, structure),
            Err(usage) => usage_error(&usage),
        },
        Commands::Run {
            source,
            expr,
            json_errors,
            trace,
            trace_out,
            args,
        } => {
            // `cant -e` and `cant run -e` are the same thing; taking either is
            // what makes the top-level form a shorthand rather than a special
            // case with its own behaviour.
            let expr = expr.or(cli.expr);
            match load_source(source.as_deref(), expr.as_deref()) {
                Ok(input) => {
                    let dir = script_dir_of(source.as_deref());
                    match resolve_modules(&module_flags, dir.as_deref()) {
                        Ok(env) => {
                            let modules = ModuleContext {
                                script_dir: dir,
                                env,
                            };
                            run_program_in(
                                input,
                                modules,
                                options,
                                json_errors,
                                args,
                                trace,
                                trace_out,
                            )
                            .await
                        }
                        Err(e) => usage_error(&e),
                    }
                }
                Err(usage) => usage_error(&usage),
            }
        }
        Commands::Build {
            file,
            release,
            emit_rust,
            output,
        } => build_program(&file, release, emit_rust, output, options),
        Commands::Expand {
            source,
            expr,
            source_map,
            output,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => {
                match resolve_modules(&module_flags, script_dir_of(source.as_deref()).as_deref()) {
                    Ok(env) => run_expand(input, source_map, output.as_deref(), &env),
                    Err(e) => usage_error(&e),
                }
            }
            Err(usage) => usage_error(&usage),
        },
        Commands::Explain {
            source,
            expr,
            verbose,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => print_explanation(input, verbose),
            Err(usage) => usage_error(&usage),
        },
        Commands::Update => usage_error(concat!(
            "`cant` has no updater — it ships inside the Rite release archive.\n\n",
            "  rite update\n\n",
            "updates both, so they stay in step by construction."
        )),
        Commands::Repl => {
            let permissions = match options.permissions() {
                Ok(perms) => perms,
                Err(e) => return usage_error(&e),
            };
            let mut budget = match options.budget() {
                Ok(budget) => budget,
                Err(e) => return usage_error(&e),
            };
            // An interactive session gets no wall clock unless one was asked
            // for. The default 60s bounds a *program*, and a session is not
            // one: the thing waiting on an interactive line is the person who
            // typed it, and they have Ctrl-C. Asking for `--timeout` still
            // works and then bounds each line, because the REPL restarts the
            // budget before every evaluation.
            if cli.timeout.is_none() {
                budget.timeout = None;
            }
            let color = match rite_render::term::ColorMode::parse(&cli.color) {
                Ok(mode) => rite_render::term::enabled(mode),
                Err(e) => return usage_error(&e),
            };
            let environment = match resolve_modules(&module_flags, None) {
                Ok(env) => env,
                Err(e) => return usage_error(&e),
            };
            let env_values = match options.env_file() {
                Ok(values) => values,
                Err(e) => return usage_error(&e),
            };
            repl::run(permissions, budget, color, environment, env_values).await
        }
        Commands::Graph {
            source,
            expr,
            format,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => print_graph(input, &format),
            Err(usage) => usage_error(&usage),
        },
        Commands::Sigil {
            source,
            expr,
            graph,
            output,
            format,
            theme,
            mode,
            metadata,
            seed,
            canonical,
            background,
            ornament,
            tracery,
            weights,
            diff,
            width,
            scale,
            embed_scene,
            simplify,
            max_nodes,
            check,
        } => sigil::run(sigil::SigilArgs {
            source,
            expr,
            graph,
            output,
            format,
            theme,
            mode,
            metadata,
            seed,
            canonical,
            background,
            ornament,
            tracery,
            weights,
            diff,
            width,
            scale,
            embed_scene,
            simplify,
            max_nodes,
            check,
        }),
        Commands::Test {
            source,
            expr,
            expect,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => {
                let dir = source
                    .as_deref()
                    .filter(|p| p.as_os_str() != "-")
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf());
                let sidecar = source
                    .as_deref()
                    .filter(|p| p.as_os_str() != "-")
                    .map(|p| p.with_extension("expect"));
                match resolve_modules(&module_flags, dir.as_deref()) {
                    Ok(env) => run_test(input, dir, env, options, expect, sidecar).await,
                    Err(e) => usage_error(&e),
                }
            }
            Err(usage) => usage_error(&usage),
        },
        Commands::Fmt {
            source,
            expr,
            ascii,
            glyph,
            preserve,
            compact,
            width,
            check,
            write,
        } => match load_source(source.as_deref(), expr.as_deref()) {
            Ok(input) => {
                let dialect = if glyph {
                    Dialect::Glyph
                } else if preserve {
                    cant_syntax::detect(&input.text)
                } else {
                    let _ = ascii;
                    Dialect::Ascii
                };
                run_fmt(
                    input,
                    source.as_deref(),
                    FormatOptions {
                        dialect,
                        max_width: width,
                        compact,
                        indent_width: 2,
                    },
                    check,
                    write,
                )
            }
            Err(usage) => usage_error(&usage),
        },
        Commands::Convert {
            source,
            expr,
            to,
            stdout,
            check,
        } => {
            let dialect = match to.as_str() {
                "ascii" => Dialect::Ascii,
                "glyph" => Dialect::Glyph,
                other => {
                    return usage_error(&format!(
                        "unknown --to `{other}` — expected `ascii` or `glyph`"
                    ))
                }
            };
            match load_source(source.as_deref(), expr.as_deref()) {
                Ok(input) => run_convert(input, source.as_deref(), dialect, stdout, check),
                Err(usage) => usage_error(&usage),
            }
        }
    }
}

/// Give the formatted program the line ending its destination wants.
fn finish_text(mut text: String, input: &Input) -> String {
    if input.ends_with_newline && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// `--check` found a difference. 1 rather than a diagnostic code: this is a
/// tool disagreeing with a file, not a rejected program.
const EXIT_WOULD_CHANGE: u8 = 1;

fn run_fmt(
    input: Input,
    path: Option<&std::path::Path>,
    options: FormatOptions,
    check: bool,
    write: bool,
) -> ExitCode {
    let formatted = match cant_syntax::format(&input.text, options) {
        Ok(result) => finish_text(result.text, &input),
        Err(e) => {
            // A source that does not parse is reported with its diagnostics, not
            // just with "cannot format": the caller needs to know *what* is
            // wrong, and they asked about this file.
            eprintln!("cant: {e}");
            let (result, sources) = parse_input(&input);
            report(&result.diagnostics, &sources, false);
            return ExitCode::from(result.diagnostics.rejection_exit_code().max(1));
        }
    };

    if check {
        if formatted != input.text {
            eprintln!("would reformat {}", input.name);
            return ExitCode::from(EXIT_WOULD_CHANGE);
        }
        return ExitCode::SUCCESS;
    }

    write_or_print(formatted, input, path, write)
}

fn run_convert(
    input: Input,
    path: Option<&std::path::Path>,
    dialect: Dialect,
    stdout: bool,
    check: bool,
) -> ExitCode {
    // Conversion is byte-preserving, so it neither adds nor removes a trailing
    // newline — unlike formatting, which reprints.
    let converted = cant_syntax::convert(&input.text, dialect);
    if check {
        if converted != input.text {
            eprintln!("would convert {}", input.name);
            return ExitCode::from(EXIT_WOULD_CHANGE);
        }
        return ExitCode::SUCCESS;
    }
    write_or_print(converted, input, path, !stdout)
}

/// Rewrite the file, or print to stdout.
///
/// Writing needs a real file: `-e` and `-` have nowhere to go, so they print
/// whatever was asked for rather than failing on a flag that cannot apply.
fn write_or_print(
    text: String,
    input: Input,
    path: Option<&std::path::Path>,
    write: bool,
) -> ExitCode {
    let target = path.filter(|p| p.as_os_str() != "-");
    match (write, target) {
        (true, Some(path)) => {
            if text == input.text {
                return ExitCode::SUCCESS;
            }
            match std::fs::write(path, &text) {
                Ok(()) => {
                    println!("formatted {}", path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cant: cannot write {}: {e}", path.display());
                    ExitCode::from(1)
                }
            }
        }
        _ => {
            print!("{text}");
            ExitCode::SUCCESS
        }
    }
}

#[derive(Debug)]
struct Input {
    name: String,
    text: String,
    /// Should the formatted form end with a newline?
    ///
    /// A file should; a `-e` argument should not. The formatter itself returns
    /// the program with no trailing newline and leaves this to the destination,
    /// because getting it wrong here made `--check` report every expression on
    /// the command line as needing reformatting.
    ends_with_newline: bool,
}

/// Resolve a source from a path, from standard input, or from `-e`.
///
/// Exactly one of the three. Passing both a file and `-e` is a usage error
/// rather than a silent precedence rule: which one won would be invisible in a
/// script until it mattered.
fn load_source(path: Option<&std::path::Path>, expr: Option<&str>) -> Result<Input, String> {
    match (path, expr) {
        (Some(_), Some(_)) => Err("give a source file or `-e`, not both".to_string()),
        (None, None) => {
            Err("no source: pass a file, `-` for standard input, or `-e 'expression'`".to_string())
        }
        (None, Some(expr)) => Ok(Input {
            name: "<expr>".to_string(),
            text: expr.to_string(),
            ends_with_newline: false,
        }),
        (Some(path), None) if path.as_os_str() == "-" => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("cannot read standard input: {e}"))?;
            Ok(Input {
                name: "<stdin>".to_string(),
                text,
                ends_with_newline: true,
            })
        }
        (Some(path), None) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            Ok(Input {
                name: path.display().to_string(),
                text,
                ends_with_newline: true,
            })
        }
    }
}

fn usage_error(message: &str) -> ExitCode {
    eprintln!("cant: {message}");
    ExitCode::from(EXIT_USAGE)
}

fn parse_input(input: &Input) -> (ParseResult, SourceMap) {
    cant::parse_source(&input.name, &input.text)
}

/// The module flags, taken off `Cli` before the subcommand match consumes it.
struct ModuleFlags {
    uses: Vec<String>,
    roots: Vec<PathBuf>,
    no_defaults: bool,
}

impl ModuleFlags {
    fn of(cli: &Cli) -> Self {
        Self {
            uses: cli.use_modules.clone(),
            roots: cli.module_root.clone(),
            no_defaults: cli.no_default_use,
        }
    }
}

/// The directory a source lives in, for module resolution. `None` for `-e` and
/// for standard input, which have no directory of their own.
fn script_dir_of(source: Option<&std::path::Path>) -> Option<PathBuf> {
    source
        .filter(|p| p.as_os_str() != "-")
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
}

/// Resolve this invocation's modules, and check that every one named exists.
///
/// Checked here rather than left to Rite, because Rite's `E026` names the
/// generated file and a search path — this can name the flag, the environment
/// variable or the config file that asked for it, which is the thing that needs
/// changing.
fn resolve_modules(
    flags: &ModuleFlags,
    script_dir: Option<&std::path::Path>,
) -> Result<cant::Environment, String> {
    let from = script_dir
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let (found, config) = modules::resolve(&flags.uses, &flags.roots, flags.no_defaults, &from)?;

    // The order the run will search in: the program's own directory, then
    // whatever was configured.
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(dir) = script_dir {
        roots.push(dir.to_path_buf());
    }
    roots.extend(found.roots.iter().cloned());

    for name in &found.uses {
        let segments: Vec<String> = name.split('.').map(str::to_string).collect();
        if rite::sem::resolve_module_path(&segments, &from, &roots).is_none() {
            let origin = found
                .origin_of(name)
                .map(|o| o.describe(config.as_deref()))
                .unwrap_or_else(|| "the command line".to_string());
            let searched: Vec<String> = std::iter::once(from.clone())
                .chain(roots.iter().cloned())
                .map(|p| p.display().to_string())
                .collect();
            return Err(format!(
                "no module `{name}`, asked for by {origin}\n  searched: {}",
                searched.join(", ")
            ));
        }
    }

    Ok(cant::Environment {
        preamble: Vec::new(),
        uses: found.uses,
        module_roots: found.roots,
    })
}

fn check(input: Input, json_errors: bool, env: &cant::Environment) -> ExitCode {
    // Three layers: syntax, the flow graph, and what Rite makes of the generated
    // code. The third is how a name that does not resolve or a host call missing
    // its `!` is caught — Cant does not answer those questions, and asking Rite
    // means handing it the program.
    let result = cant::check_with(&input.name, &input.text, env);
    report(&result.diagnostics, &result.analysis.sources, json_errors);
    if result.has_errors() {
        return ExitCode::from(result.exit_code());
    }
    if !json_errors {
        println!("ok");
    }
    ExitCode::SUCCESS
}

/// Where a program's modules come from: its own directory, plus whatever
/// `--use` / `CANT_USE` / `cant.toml` resolved to.
///
/// One parameter rather than two, because they are never useful apart — the
/// directory is the first entry of the search path the environment describes.
struct ModuleContext {
    script_dir: Option<PathBuf>,
    env: cant::Environment,
}

async fn run_program(
    input: Input,
    options: rite::RuntimeOptions,
    json_errors: bool,
    args: Vec<String>,
    env: cant::Environment,
) -> ExitCode {
    let modules = ModuleContext {
        script_dir: None,
        env,
    };
    run_program_in(input, modules, options, json_errors, args, false, None).await
}

async fn run_program_in(
    input: Input,
    modules: ModuleContext,
    options: rite::RuntimeOptions,
    json_errors: bool,
    args: Vec<String>,
    trace: bool,
    trace_out: Option<PathBuf>,
) -> ExitCode {
    let ModuleContext { script_dir, env } = modules;
    let permissions = match options.permissions() {
        Ok(perms) => perms,
        Err(e) => return usage_error(&e),
    };
    let budget = match options.budget() {
        Ok(budget) => budget,
        Err(e) => return usage_error(&e),
    };
    let env_values = match options.env_file() {
        Ok(values) => values,
        Err(e) => return usage_error(&e),
    };

    let tracing = trace || trace_out.is_some();
    let result = cant::run(
        &input.name,
        &input.text,
        cant::RunOptions {
            script_dir,
            permissions,
            budget,
            args,
            output: None,
            trace: tracing,
            preamble: Vec::new(),
            uses: env.uses,
            module_roots: env.module_roots,
            env_values,
        },
    )
    .await;

    // The trace before the value: it goes to stderr (or a file), so the value
    // on stdout stays exactly what an untraced run prints — pipeable either way.
    if tracing {
        if let Some(counts) = &result.trace {
            let nodes: Vec<String> = counts
                .iter()
                .map(|(id, n)| format!("    \"{id}\": {n}"))
                .collect();
            let json = format!(
                "{{\n  \"schema\": \"cant.trace\",\n  \"version\": \"1\",\n  \"source\": {},\n  \"nodes\": {{\n{}\n  }}\n}}\n",
                serde_json::json!(input.name),
                nodes.join(",\n")
            );
            match &trace_out {
                Some(path) => {
                    if let Err(e) = std::fs::write(path, &json) {
                        eprintln!("cant: could not write the trace: {}: {e}", path.display());
                        return ExitCode::from(1);
                    }
                }
                None => eprint!("{json}"),
            }
        }
    }

    report(
        &result.diagnostics,
        &result.check.analysis.sources,
        json_errors,
    );
    // The value is printed whenever it is not `none`, after whatever the program
    // itself wrote — the same rule `rite run` follows.
    if let Some(display) = &result.display {
        println!("{display}");
    }
    ExitCode::from(result.exit_code)
}

/// `cant test`: run, compare, and exit 7 on a mismatch (§ the exit contract).
///
/// The comparison is over the *printed* value — the same text `cant run` shows
/// — trimmed of trailing whitespace on both sides, so a sidecar file ending in
/// a newline compares equal to the value it names. `none` is spelled `none`,
/// so a program expected to answer nothing can say so.
async fn run_test(
    input: Input,
    script_dir: Option<PathBuf>,
    env: cant::Environment,
    options: rite::RuntimeOptions,
    expect: Option<String>,
    sidecar: Option<PathBuf>,
) -> ExitCode {
    let expected = match expect {
        Some(text) => text,
        None => match sidecar {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    return usage_error(&format!(
                        "cant test: no --expect and no sidecar: {}: {e}",
                        path.display()
                    ))
                }
            },
            None => {
                return usage_error(
                    "cant test: an expression needs --expect — there is no file to put a sidecar beside",
                )
            }
        },
    };

    let permissions = match options.permissions() {
        Ok(perms) => perms,
        Err(e) => return usage_error(&e),
    };
    let budget = match options.budget() {
        Ok(budget) => budget,
        Err(e) => return usage_error(&e),
    };

    let result = cant::run(
        &input.name,
        &input.text,
        cant::RunOptions {
            script_dir,
            permissions,
            budget,
            args: Vec::new(),
            output: None,
            trace: false,
            preamble: Vec::new(),
            uses: env.uses,
            module_roots: env.module_roots,
            env_values: match options.env_file() {
                Ok(values) => values,
                Err(e) => return usage_error(&e),
            },
        },
    )
    .await;

    if !result.succeeded() {
        // The program failed before there was a value to compare. Its own
        // failure is the report, and its own exit code stands — a broken
        // program is not the same finding as a wrong answer.
        report(&result.diagnostics, &result.check.analysis.sources, false);
        return ExitCode::from(result.exit_code);
    }

    let actual = result.display.unwrap_or_else(|| "none".to_string());
    if actual.trim_end() == expected.trim_end() {
        println!("ok");
        return ExitCode::SUCCESS;
    }
    eprintln!("test failed: {}", input.name);
    eprintln!("  expected: {}", expected.trim_end());
    eprintln!("  actual:   {}", actual.trim_end());
    ExitCode::from(7)
}

fn build_program(
    file: &std::path::Path,
    release: bool,
    emit_rust: bool,
    output: Option<PathBuf>,
    options: rite::RuntimeOptions,
) -> ExitCode {
    let permissions = match options.permissions() {
        Ok(perms) => perms,
        Err(e) => return usage_error(&e),
    };
    let result = cant::build(
        file,
        cant::BuildOptions {
            release,
            emit_rust,
            output,
            permissions,
        },
    );
    if !result.diagnostics.is_empty() {
        let sources = rite_core::SourceMap::new();
        eprintln!("{}", result.diagnostics.render_all(&sources));
    }
    if let Some(binary) = &result.binary {
        println!("built {}", binary.display());
    }
    if let Some(generated) = &result.generated {
        eprintln!("generated Rite: {}", generated.display());
    }
    ExitCode::from(result.exit_code)
}

fn run_expand(
    input: Input,
    source_map: bool,
    output: Option<&std::path::Path>,
    env: &cant::Environment,
) -> ExitCode {
    let (expansion, analysis) = cant::expand_with(&input.name, &input.text, env);
    if !analysis.diagnostics.is_empty() {
        eprintln!("{}", analysis.render());
    }
    let Some(expansion) = expansion else {
        return ExitCode::from(analysis.diagnostics.rejection_exit_code().max(1));
    };

    if source_map {
        // To stderr, so `cant expand --source-map > out.rite` still writes Rite.
        eprintln!("// source map: {} entries", expansion.map.mappings().len());
        for mapping in expansion.map.mappings() {
            eprintln!(
                "//   cant {}..{} -> rite {}..{}  ({}{})",
                mapping.cant.start,
                mapping.cant.end,
                mapping.rite.start,
                mapping.rite.end,
                mapping.node,
                if mapping.precise { ", leaf" } else { "" }
            );
        }
    }

    match output {
        Some(path) => match std::fs::write(path, &expansion.rite) {
            Ok(()) => {
                println!("expanded to {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("cant: cannot write {}: {e}", path.display());
                ExitCode::from(1)
            }
        },
        None => {
            print!("{}", expansion.rite);
            ExitCode::SUCCESS
        }
    }
}

fn print_explanation(input: Input, verbose: bool) -> ExitCode {
    let analysis = cant::analyze(&input.name, &input.text);
    if !analysis.diagnostics.is_empty() {
        eprintln!("{}", analysis.render());
    }
    let Some(graph) = &analysis.graph else {
        return ExitCode::from(analysis.diagnostics.rejection_exit_code().max(1));
    };
    print!(
        "{}",
        cant_sem::explain::render(
            &cant_sem::explain_with(graph, &analysis.definition_names()),
            verbose,
        )
    );
    if analysis.diagnostics.has_errors() {
        return ExitCode::from(analysis.diagnostics.rejection_exit_code());
    }
    ExitCode::SUCCESS
}

fn print_graph(input: Input, format: &str) -> ExitCode {
    let analysis = cant::analyze(&input.name, &input.text);

    // The graph is printed even when validation failed: seeing the shape is
    // usually how someone works out *why* it failed. Warnings and errors both go
    // to stderr, so a pipe to `dot` or `jq` gets clean output either way.
    if !analysis.diagnostics.is_empty() {
        eprintln!("{}", analysis.render());
    }
    let Some(graph) = analysis.graph else {
        return ExitCode::from(analysis.diagnostics.rejection_exit_code().max(1));
    };

    match format {
        "dot" => print!("{}", cant::to_dot(&graph)),
        "json" => match serde_json::to_string_pretty(&graph.to_json()) {
            Ok(text) => println!("{text}"),
            Err(e) => {
                eprintln!("cant: {e}");
                return ExitCode::from(1);
            }
        },
        other => {
            return usage_error(&format!(
                "unknown --format `{other}` — expected `json` or `dot`"
            ))
        }
    }

    if analysis.diagnostics.has_errors() {
        return ExitCode::from(analysis.diagnostics.rejection_exit_code());
    }
    ExitCode::SUCCESS
}

fn print_tree(input: Input, json: bool, structure: bool) -> ExitCode {
    let (result, sources) = parse_input(&input);
    if result.diagnostics.has_errors() {
        report(&result.diagnostics, &sources, false);
        return ExitCode::from(result.diagnostics.rejection_exit_code());
    }
    let Some(program) = result.program else {
        return ExitCode::from(result.diagnostics.rejection_exit_code().max(1));
    };
    let rendered = if structure {
        serde_json::to_string_pretty(&cant_syntax::structure(&program))
    } else if json {
        serde_json::to_string_pretty(&program)
    } else {
        println!("{program:#?}");
        return ExitCode::SUCCESS;
    };
    match rendered {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cant: {e}");
            ExitCode::from(1)
        }
    }
}

/// Diagnostics go to stderr rendered, or to stdout as JSON.
///
/// The split matches `rite`: rendered output is for a person reading a terminal
/// and belongs on stderr; JSON is the command's product and belongs on stdout
/// where a pipe can take it.
fn report(diagnostics: &CantDiagnostics, sources: &SourceMap, json: bool) {
    if json {
        match serde_json::to_string_pretty(&diagnostics.to_json()) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("cant: {e}"),
        }
    } else if !diagnostics.is_empty() {
        eprintln!("{}", diagnostics.render_all(sources));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_surface_parses() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_source_and_an_expression_together_are_a_usage_error() {
        let err = load_source(Some(std::path::Path::new("a.cant")), Some("x -> f"))
            .expect_err("both should be rejected");
        assert!(err.contains("not both"), "{err}");
    }

    #[test]
    fn no_source_at_all_says_what_the_three_forms_are() {
        let err = load_source(None, None).expect_err("neither should be rejected");
        assert!(err.contains("-e"), "{err}");
        assert!(err.contains("standard input"), "{err}");
    }

    #[test]
    fn an_expression_is_named_so_diagnostics_have_somewhere_to_point() {
        let input = load_source(None, Some("x -> f")).expect("expression");
        assert_eq!(input.name, "<expr>");
        assert_eq!(input.text, "x -> f");
    }

    #[test]
    fn a_missing_file_is_a_usage_error_naming_the_path() {
        let err =
            load_source(Some(std::path::Path::new("nope.cant")), None).expect_err("missing file");
        assert!(err.contains("nope.cant"), "{err}");
    }
}
