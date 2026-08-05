//! `rite docs …` and `rite describe diagnostic` — documentation commands.
//!
//! These used to hardcode repo-relative paths (`docs/book`, `skills/rite`) and
//! print success for work they never did. Every path is now explicit or resolved
//! from a real checkout, and every command either does the thing or fails.

use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::Subcommand;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use crate::util;

/// The CLI reference, written from clap's own command tree.
///
/// The hand-written stub this replaces listed twelve of twenty-five
/// subcommands and had drifted: four places in the book documented a
/// `rite fmt --stdout` flag that has never existed. Deriving the page from the
/// definitions means a new subcommand or flag documents itself, and a removed
/// one cannot linger.
pub fn cli_reference_markdown() -> String {
    use clap::CommandFactory;
    let root = <crate::Cli as CommandFactory>::command();
    let mut out = String::from("# CLI reference\n\n");
    out.push_str(
        "Every subcommand, argument and flag, generated from the command \
         definitions themselves — so this page cannot describe a flag that is \
         not there.\n\n",
    );
    if let Some(about) = root.get_about() {
        out.push_str(&format!("> {about}\n\n"));
    }
    let mut names: Vec<_> = root.get_subcommands().collect();
    names.sort_by_key(|c| c.get_name());
    for sub in names {
        render_command(sub, "rite", &mut out);
    }
    out
}

/// `rite_doc::generate` plus the CLI reference.
///
/// Only this crate can write that page, since it needs clap's command tree, so
/// every path that regenerates documentation has to come through here. When
/// `docs check` regenerated directly it quietly replaced the published CLI
/// reference with rite-doc's one-line placeholder.
fn generate_reference(scripts: Option<&Path>, out: &Path) -> anyhow::Result<()> {
    rite_doc::generate(scripts, out)?;
    std::fs::write(out.join("cli.md"), cli_reference_markdown())?;
    Ok(())
}

fn render_command(cmd: &clap::Command, prefix: &str, out: &mut String) {
    // clap synthesises `help`; it documents itself and adds only noise here.
    if cmd.get_name() == "help" {
        return;
    }
    let path = format!("{prefix} {}", cmd.get_name());
    out.push_str(&format!("## `{path}`\n\n"));
    if let Some(about) = cmd.get_long_about().or_else(|| cmd.get_about()) {
        out.push_str(&format!("{about}\n\n"));
    }

    let mut positionals = Vec::new();
    let mut flags = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        let help = arg
            .get_help()
            .map(|h| h.to_string().replace('\n', " "))
            .unwrap_or_default();
        if arg.is_positional() {
            positionals.push((format!("`<{}>`", arg.get_id()), help));
        } else {
            let mut spelling = String::new();
            if let Some(short) = arg.get_short() {
                spelling.push_str(&format!("`-{short}`, "));
            }
            if let Some(long) = arg.get_long() {
                spelling.push_str(&format!("`--{long}`"));
            }
            if spelling.is_empty() {
                continue;
            }
            flags.push((spelling, help));
        }
    }

    if !positionals.is_empty() {
        out.push_str("| Argument | Meaning |\n|---|---|\n");
        for (name, help) in positionals {
            out.push_str(&format!("| {name} | {help} |\n"));
        }
        out.push('\n');
    }
    if !flags.is_empty() {
        out.push_str("| Flag | Meaning |\n|---|---|\n");
        for (name, help) in flags {
            out.push_str(&format!("| {name} | {help} |\n"));
        }
        out.push('\n');
    }

    let mut subs: Vec<_> = cmd.get_subcommands().collect();
    subs.sort_by_key(|c| c.get_name());
    for sub in subs {
        render_command(sub, &path, out);
    }
}

#[derive(Subcommand, Debug)]
pub enum DocsCmd {
    /// Generate reference docs (+ agent bundle) from a Rite checkout
    Build {
        /// Output directory (default: <checkout>/docs/generated)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Agent skill bundle output (default: <checkout>/skills/rite)
        #[arg(long = "skill-out")]
        skill_out: Option<PathBuf>,
        /// Only generate reference docs, not the agent bundle
        #[arg(long = "no-skill")]
        no_skill: bool,
        /// Also document the `///` comments in this Rite file or directory
        #[arg(long)]
        scripts: Option<PathBuf>,
    },
    /// Serve generated docs over loopback HTTP
    Serve {
        /// Port to serve the generated documentation on
        #[arg(long, default_value = "4042")]
        port: u16,
        /// Directory to serve (default: <checkout>/docs/generated)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Do not open a browser window on start
        #[arg(long = "no-open")]
        no_open: bool,
    },
    /// Run documentation doctests
    Check {
        /// Regenerate reference docs here first (default: <checkout>/docs/generated)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Book directory (default: <checkout>/docs/book)
        #[arg(long)]
        book: Option<PathBuf>,
        /// Tutorials directory (default: <checkout>/docs/tutorials)
        #[arg(long)]
        tutorials: Option<PathBuf>,
        /// Diagnostics directory (default: <checkout>/docs/diagnostics)
        #[arg(long)]
        diagnostics: Option<PathBuf>,
        /// Agent skill directory (default: <checkout>/skills/rite)
        #[arg(long)]
        skill: Option<PathBuf>,
    },
    /// Generate reference docs only (machine-readable JSON included)
    Json {
        /// Output directory (default: <checkout>/docs/generated)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also document the `///` comments in this Rite file or directory
        #[arg(long)]
        scripts: Option<PathBuf>,
    },
    /// Generate the agent skill bundle
    Agent {
        /// Output directory (default: <checkout>/skills/rite)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Open generated documentation for a symbol (or the index)
    Open {
        /// Symbol to open, e.g. `fs.read`; omit for the index
        symbol: Option<String>,
        /// Documentation root (default: <checkout>/docs/generated)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Print the resolved path instead of opening a browser
        #[arg(long)]
        print: bool,
    },
}

pub async fn run(cmd: DocsCmd) -> anyhow::Result<ExitCode> {
    match cmd {
        DocsCmd::Build {
            out,
            skill_out,
            no_skill,
            scripts,
        } => {
            let out = resolve_out(out, "docs/generated", "--out")?;
            generate_reference(scripts.as_deref(), &out)?;
            println!("docs written to {}", out.display());
            if !no_skill {
                let skill = resolve_out(skill_out, "skills/rite", "--skill-out")?;
                rite_doc::generate_agent_bundle(&skill)?;
                println!("agent skill written to {}", skill.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Json { out, scripts } => {
            let out = resolve_out(out, "docs/generated", "--out")?;
            generate_reference(scripts.as_deref(), &out)?;
            println!("docs written to {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Agent { output } => {
            let out = resolve_out(output, "skills/rite", "--output")?;
            rite_doc::generate_agent_bundle(&out)?;
            println!("agent skill written to {}", out.display());
            Ok(ExitCode::SUCCESS)
        }
        DocsCmd::Check {
            out,
            book,
            tutorials,
            diagnostics,
            skill,
        } => check(out, book, tutorials, diagnostics, skill).await,
        DocsCmd::Serve {
            port,
            root,
            no_open,
        } => serve(port, root, no_open).await,
        DocsCmd::Open {
            symbol,
            root,
            print,
        } => open(symbol.as_deref(), root, print),
    }
}

/// Resolve an output directory: explicit flag, else the same path inside a
/// detected checkout, else refuse (instead of scribbling into the cwd).
fn resolve_out(explicit: Option<PathBuf>, rel: &str, flag: &str) -> anyhow::Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    match util::checkout_containing("docs/book") {
        Some(root) => Ok(root.join(rel)),
        None => anyhow::bail!(
            "no Rite checkout found near the current directory, so `{rel}` has no meaning here.\n  \
             pass {flag} <dir>, run from a checkout, or set RITE_REPO_ROOT=<checkout>"
        ),
    }
}

fn resolve_input(explicit: Option<PathBuf>, rel: &str, flag: &str) -> anyhow::Result<PathBuf> {
    let path = match explicit {
        Some(p) => p,
        None => util::require_checkout_path(rel, flag)?,
    };
    if !path.is_dir() {
        anyhow::bail!("{} is not a directory ({flag})", path.display());
    }
    Ok(path)
}

async fn check(
    out: Option<PathBuf>,
    book: Option<PathBuf>,
    tutorials: Option<PathBuf>,
    diagnostics: Option<PathBuf>,
    skill: Option<PathBuf>,
) -> anyhow::Result<ExitCode> {
    let out = resolve_out(out, "docs/generated", "--out")?;
    let book = resolve_input(book, "docs/book", "--book")?;
    let tutorials = resolve_input(tutorials, "docs/tutorials", "--tutorials")?;
    let diagnostics = resolve_input(diagnostics, "docs/diagnostics", "--diagnostics")?;
    let skill = resolve_input(skill, "skills/rite", "--skill")?;

    generate_reference(None, &out)?;
    // Tutorials are executable documentation on the same terms as the book: a
    // tutorial is mostly code, and one that has drifted from the language is worse
    // than none, because a reader trusts it enough to type it in.
    let dirs: Vec<&Path> = vec![&book, &tutorials, &diagnostics, &skill];
    let report = rite_doc::run_doctests(&dirs).await;
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

// ---------------------------------------------------------------- docs serve

#[derive(Clone)]
struct DocsRoot(Arc<PathBuf>);

/// Serve `root` over loopback. Previously this printed a URL and exited.
async fn serve(port: u16, root: Option<PathBuf>, no_open: bool) -> anyhow::Result<ExitCode> {
    use std::net::SocketAddr;

    let root = resolve_out(root, "docs/generated", "--root")?;
    if !root.is_dir() {
        anyhow::bail!(
            "no generated docs at {} — run `rite docs build` first (or pass --root <dir>)",
            root.display()
        );
    }
    let root = root.canonicalize()?;

    let app = Router::new()
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_file))
        .with_state(DocsRoot(Arc::new(root.clone())));

    // Loopback only: local documentation, not a public web server.
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}/");
    println!("serving {} at {url}", root.display());
    println!("  Ctrl-C to stop");
    if !no_open {
        if let Err(e) = util::open_in_browser(&url) {
            eprintln!("  note: {e} — open {url} manually");
        }
    }
    axum::serve(listener, app).await?;
    Ok(ExitCode::SUCCESS)
}

async fn serve_index(State(root): State<DocsRoot>) -> Response {
    respond_with_path(&root.0, &root.0)
}

async fn serve_file(State(root): State<DocsRoot>, AxumPath(path): AxumPath<String>) -> Response {
    match util::safe_join(&root.0, &path) {
        Some(target) => respond_with_path(&root.0, &target),
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn respond_with_path(root: &Path, target: &Path) -> Response {
    if target.is_dir() {
        for candidate in ["index.html", "index.htm"] {
            let idx = target.join(candidate);
            if idx.is_file() {
                return file_response(&idx);
            }
        }
        return directory_listing(root, target);
    }
    if target.is_file() {
        return file_response(target);
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn file_response(path: &Path) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, util::content_type_for(path))],
            bytes,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot read {}: {e}", path.display()),
        )
            .into_response(),
    }
}

fn directory_listing(root: &Path, dir: &Path) -> Response {
    let rel = dir.strip_prefix(root).unwrap_or(Path::new(""));
    let mut entries: Vec<(String, bool)> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (e.file_name().to_string_lossy().to_string(), is_dir)
            })
            .collect(),
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("cannot list {}: {e}", dir.display()),
            )
                .into_response()
        }
    };
    entries.sort();

    let mut body = String::from(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"/>\
         <title>Rite docs</title><style>body{font-family:ui-sans-serif,system-ui,sans-serif;\
         background:#0b0f14;color:#e6edf3;padding:2rem}a{color:#7ee0ff}li{margin:.2rem 0}\
         </style></head><body><h1>Rite documentation</h1>",
    );
    body.push_str(&format!("<p><code>/{}</code></p><ul>", rel.display()));
    for (name, is_dir) in entries {
        let href = if rel.as_os_str().is_empty() {
            format!("/{name}")
        } else {
            format!("/{}/{name}", rel.display())
        };
        let slash = if is_dir { "/" } else { "" };
        body.push_str(&format!("<li><a href=\"{href}\">{name}{slash}</a></li>"));
    }
    body.push_str("</ul></body></html>");
    Html(body).into_response()
}

// ----------------------------------------------------------------- docs open

fn open(symbol: Option<&str>, root: Option<PathBuf>, print_only: bool) -> anyhow::Result<ExitCode> {
    let generated = resolve_out(root, "docs/generated", "--root")?;
    let book = util::checkout_containing("docs/book").map(|r| r.join("docs/book"));

    let mut candidates: Vec<PathBuf> = Vec::new();
    match symbol {
        Some(sym) => {
            let sym = sym.trim().trim_start_matches('@').replace('.', "-");
            candidates.push(generated.join("html").join(format!("{sym}.html")));
            candidates.push(generated.join(format!("{sym}.md")));
            candidates.push(generated.join(&sym));
            if let Some(book) = &book {
                candidates.push(book.join(format!("{sym}.md")));
            }
        }
        None => {
            candidates.push(generated.join("html").join("index.html"));
            candidates.push(generated.join("index.html"));
            candidates.push(generated.join("reference.md"));
            if let Some(book) = &book {
                candidates.push(book.join("README.md"));
            }
        }
    }

    let Some(hit) = candidates.iter().find(|p| p.exists()) else {
        eprintln!(
            "no documentation page for {}",
            symbol.unwrap_or("the index")
        );
        for c in &candidates {
            eprintln!("  looked for {}", c.display());
        }
        eprintln!("run `rite docs build` to generate documentation");
        return Ok(ExitCode::from(2));
    };

    println!("{}", hit.display());
    if print_only {
        return Ok(ExitCode::SUCCESS);
    }
    let url = format!("file://{}", hit.display());
    if let Err(e) = util::open_in_browser(&url) {
        eprintln!("could not open a browser ({e}); the path above is the page");
    }
    Ok(ExitCode::SUCCESS)
}

// ------------------------------------------------------ describe diagnostic

/// Normalize `e21`, `21`, `E021` → `E021`.
pub fn normalize_diagnostic_code(code: &str) -> Option<String> {
    let digits = code.trim().trim_start_matches(['E', 'e']);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    Some(format!("E{n:03}"))
}

/// `rite describe diagnostic <code>` — return the real page, or 404 honestly.
pub fn describe_diagnostic(code: &str, json: bool) -> anyhow::Result<ExitCode> {
    let Some(code) = normalize_diagnostic_code(code) else {
        eprintln!("not a diagnostic code: {code} (expected e.g. E021 or 21)");
        return Ok(ExitCode::from(2));
    };

    let summary = diagnostic_summary(&code);
    let page = diagnostic_page_path(&code);

    let Some(page) = page else {
        let searched = diagnostic_search_roots();
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "code": code,
                    "found": false,
                    "summary": summary,
                    "searched": searched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                }))?
            );
        } else {
            eprintln!("no documentation page for {code}");
            for root in &searched {
                eprintln!("  looked in {}", root.display());
            }
            if let Some(s) = &summary {
                eprintln!("  known summary: {s}");
            }
        }
        return Ok(ExitCode::from(2));
    };

    let markdown = std::fs::read_to_string(&page)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "code": code,
                "found": true,
                "summary": summary,
                "path": page.display().to_string(),
                "markdown": markdown,
            }))?
        );
    } else {
        if let Some(s) = &summary {
            println!("{code}: {s}");
            println!();
        }
        print!("{markdown}");
        if !markdown.ends_with('\n') {
            println!();
        }
        println!();
        println!("source: {}", page.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Places a per-code diagnostics page can live: a checkout, or an installed
/// skill bundle cache.
fn diagnostic_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(dir) = std::env::var("RITE_DIAGNOSTICS_DIR") {
        roots.push(PathBuf::from(dir));
    }
    if let Some(root) = util::checkout_containing("docs/diagnostics") {
        roots.push(root.join("docs/diagnostics"));
    }
    let cache = crate::config::skill_cache_dir();
    if cache.is_dir() {
        roots.push(cache.join("references").join("diagnostics"));
        roots.push(cache.join("diagnostics"));
    }
    roots
}

fn diagnostic_page_path(code: &str) -> Option<PathBuf> {
    for root in diagnostic_search_roots() {
        for name in [format!("{code}.md"), format!("{}.md", code.to_lowercase())] {
            let p = root.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// One-line summary from the machine-readable diagnostics index, if present.
fn diagnostic_summary(code: &str) -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = util::checkout_containing("skills/rite") {
        candidates.push(root.join("skills/rite/machine/diagnostics.json"));
        candidates.push(root.join("docs/generated/machine/diagnostics.json"));
    }
    candidates.push(
        crate::config::skill_cache_dir()
            .join("machine")
            .join("diagnostics.json"),
    );
    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
            continue;
        };
        for e in entries {
            if e.get("code").and_then(|c| c.as_str()) == Some(code) {
                if let Some(s) = e.get("summary").and_then(|s| s.as_str()) {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_codes() {
        assert_eq!(normalize_diagnostic_code("E021").as_deref(), Some("E021"));
        assert_eq!(normalize_diagnostic_code("e21").as_deref(), Some("E021"));
        assert_eq!(normalize_diagnostic_code("21").as_deref(), Some("E021"));
        assert_eq!(normalize_diagnostic_code(" E040 ").as_deref(), Some("E040"));
        assert_eq!(normalize_diagnostic_code("E1234").as_deref(), Some("E1234"));
        assert_eq!(normalize_diagnostic_code("nope"), None);
        assert_eq!(normalize_diagnostic_code(""), None);
        assert_eq!(normalize_diagnostic_code("E"), None);
    }

    #[test]
    fn finds_repo_diagnostic_pages_from_this_checkout() {
        // The test binary lives in target/debug/deps, so checkout discovery has
        // to walk up out of the build tree.
        let page = diagnostic_page_path("E020");
        assert!(page.is_some(), "E020.md should be discoverable");
        assert!(page.unwrap().ends_with("E020.md"));
        assert!(diagnostic_page_path("E999").is_none());
    }

    #[test]
    fn summary_comes_from_machine_index() {
        assert_eq!(
            diagnostic_summary("E021").as_deref(),
            Some("effectful capability call requires `!`")
        );
        assert_eq!(diagnostic_summary("E999"), None);
    }
}
