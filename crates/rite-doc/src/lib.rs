//! Documentation parser and generator (Markdown, HTML, JSON).

pub mod doctest;

pub use doctest::{run_doctests, DocTestReport, DocTestResult};

use rite_caps::{HostCapabilities, PermissionSet};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct DocIndex {
    pub title: String,
    pub version: String,
    pub sections: Vec<DocSection>,
    pub capabilities: Vec<CapDoc>,
    /// Documentation extracted from user `.rite` sources, when `generate` was given a
    /// path. Empty for the plain language reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptDoc>,
    pub search: Vec<SearchEntry>,
}

/// Every documented function in one `.rite` file.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptDoc {
    pub path: String,
    pub module_doc: Option<String>,
    pub functions: Vec<ScriptFnDoc>,
}

/// One `◆`/`def` declaration and the `///` block above it.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptFnDoc {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<String>,
    pub signature: String,
    pub docs: String,
    pub param_docs: Vec<(String, String)>,
    pub returns: Option<String>,
    pub effects: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocSection {
    pub id: String,
    pub title: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapDoc {
    pub name: String,
    pub functions: Vec<FnDoc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FnDoc {
    pub name: String,
    pub docs: String,
    pub arity: usize,
    pub effectful: bool,
    pub permission: String,
    /// The call answers a `Value::Result`, so postfix `?` applies to it.
    pub returns_result: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchEntry {
    pub title: String,
    pub path: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct DocComment {
    pub text: String,
    pub params: Vec<(String, String)>,
    pub returns: Option<String>,
    pub effects: Vec<String>,
    pub examples: Vec<String>,
}

/// Parse `///` documentation comments with tags.
pub fn parse_doc_comment(lines: &[&str]) -> DocComment {
    let mut text = String::new();
    let mut params = Vec::new();
    let mut returns = None;
    let mut effects = Vec::new();
    let mut examples = Vec::new();
    let mut in_example = false;
    let mut example_buf = String::new();

    for line in lines {
        // Accept a raw source line or one the parser already stripped, and both doc
        // glyphs: `//!` is a documented Rite comment form, and leaving it attached
        // dumped a literal `//!` into the rendered text.
        let line = line
            .trim_start()
            .trim_start_matches("///")
            .trim_start_matches("//!")
            .trim_start();
        if line.starts_with("```") {
            if in_example {
                examples.push(example_buf.clone());
                example_buf.clear();
                in_example = false;
            } else {
                in_example = true;
            }
            continue;
        }
        if in_example {
            example_buf.push_str(line);
            example_buf.push('\n');
            continue;
        }
        if let Some(rest) = line.strip_prefix("@param ") {
            let mut parts = rest.splitn(2, ' ');
            let name = parts.next().unwrap_or("").to_string();
            let desc = parts.next().unwrap_or("").to_string();
            params.push((name, desc));
        } else if let Some(rest) = line.strip_prefix("@returns ") {
            returns = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("@effects ") {
            effects.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("@permission ") {
            effects.push(format!("permission:{}", rest));
        } else {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(line);
        }
    }
    // An example block whose closing fence is missing would otherwise be dropped whole:
    // the buffer just falls off the end of the loop. Keep what was written.
    if in_example && !example_buf.trim().is_empty() {
        examples.push(example_buf);
    }

    DocComment {
        text,
        params,
        returns,
        effects,
        examples,
    }
}

/// Pull the documented declarations out of one `.rite` source.
///
/// Parse errors are not fatal — a file that fails to parse contributes whatever
/// declarations the parser did recover, because a doc build should not go dark on one
/// broken script.
pub fn document_script(path: &str, source: &str) -> ScriptDoc {
    use rite_syntax::ast::Item;

    let (program, _diags, _sources) = rite_syntax::parse_source(path, source);
    let mut functions = Vec::new();
    if let Some(program) = &program {
        for item in &program.items {
            let Item::Function(f) = item else { continue };
            let Some(doc) = &f.doc else { continue };
            let lines: Vec<&str> = doc.lines().collect();
            let parsed = parse_doc_comment(&lines);
            let params: Vec<String> = f.params.iter().map(|p| p.name.name.clone()).collect();
            functions.push(ScriptFnDoc {
                signature: format!("{}({})", f.name.name, params.join(", ")),
                name: f.name.name.clone(),
                is_pub: f.is_pub,
                params,
                docs: parsed.text,
                param_docs: parsed.params,
                returns: parsed.returns,
                effects: parsed.effects,
                examples: parsed.examples,
            });
        }
    }

    ScriptDoc {
        path: path.to_string(),
        module_doc: module_doc(source),
        functions,
    }
}

/// Leading `//!` lines, which describe the file rather than any one declaration.
fn module_doc(source: &str) -> Option<String> {
    let mut out = String::new();
    for line in source.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with("#!") {
            continue;
        }
        let Some(rest) = t.strip_prefix("//!") else {
            break;
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(rest.trim());
    }
    (!out.trim().is_empty()).then_some(out)
}

/// Collect `.rite` files under a file or directory, skipping hidden and build dirs.
fn collect_rite(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    if root.is_file() {
        if root.extension().and_then(|s| s.to_str()) == Some("rite") {
            out.push(root.to_path_buf());
        }
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    // Directory order is filesystem-dependent; sort so generated docs are reproducible.
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        collect_rite(&path, out);
    }
}

/// The builtin-name reference, written from `BUILTIN_NAMES` itself.
///
/// This list doubles as the reserved-name list for module authors: a module
/// export with one of these names shadows the builtin for every importer, so
/// the page exists to be checked before naming an export. Names only — the
/// resolver's list carries no docs or arity.
pub fn builtins_markdown() -> String {
    let mut names: Vec<&str> = rite_sem::resolve::BUILTIN_NAMES.to_vec();
    names.sort_unstable();
    let mut out = String::from("# Builtin functions\n\n");
    out.push_str(&format!(
        "The {} bare names the language resolves without any `@capability` \
         prefix, generated from the resolver's own list.\n\n\
         Treat them as reserved when naming a module export: an exported \
         function with one of these names replaces the builtin for every file \
         that imports the module. A local binding inside one function is fine; \
         an export is not.\n\n",
        names.len()
    ));
    let effectful: &[&str] = rite_sem::resolve::EFFECTFUL_BUILTINS;
    for name in names {
        if effectful.contains(&name) {
            out.push_str(&format!("- `{name}` (effectful)\n"));
        } else {
            out.push_str(&format!("- `{name}`\n"));
        }
    }
    out.push('\n');
    out
}

pub fn generate(path: Option<&Path>, out: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    std::fs::create_dir_all(out.join("html"))?;

    let host = HostCapabilities::with_defaults(PermissionSet::allow_all());
    let mut capabilities = Vec::new();
    let mut search = Vec::new();

    for (name, descs) in host.all_descriptors() {
        let mut functions = Vec::new();
        for d in descs {
            functions.push(FnDoc {
                name: d.name.to_string(),
                docs: d.docs.to_string(),
                arity: d.arity,
                effectful: d.effectful,
                permission: d.permission.to_string(),
                returns_result: d.returns_result,
            });
            search.push(SearchEntry {
                title: format!("@{}.{}", name, d.name),
                path: format!("capabilities.md#{}", name),
                snippet: d.docs.to_string(),
            });
        }
        capabilities.push(CapDoc {
            name: name.to_string(),
            functions,
        });
    }

    let sections = book_sections();
    for s in &sections {
        search.push(SearchEntry {
            title: s.title.clone(),
            path: format!("{}.md", s.id),
            snippet: s.body_markdown.chars().take(120).collect(),
        });
    }

    // A path argument means "document these scripts too". Without it the output is the
    // language reference alone.
    let mut scripts = Vec::new();
    if let Some(root) = path {
        let mut files = Vec::new();
        collect_rite(root, &mut files);
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let doc = document_script(&file.display().to_string(), &text);
            if doc.functions.is_empty() && doc.module_doc.is_none() {
                continue;
            }
            for f in &doc.functions {
                search.push(SearchEntry {
                    title: f.signature.clone(),
                    path: format!("scripts.md#{}", f.name),
                    snippet: f.docs.chars().take(120).collect(),
                });
            }
            scripts.push(doc);
        }
    }

    let index = DocIndex {
        title: "Rite Language Documentation".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        sections: sections.clone(),
        capabilities: capabilities.clone(),
        scripts: scripts.clone(),
        search,
    };

    // JSON index
    std::fs::write(
        out.join("index.json"),
        serde_json::to_string_pretty(&index)?,
    )?;
    std::fs::write(
        out.join("search.json"),
        serde_json::to_string_pretty(&index.search)?,
    )?;

    // Markdown reference
    let mut md = String::from("# Rite Language Reference\n\n");
    md.push_str(&format!("Version {}\n\n", index.version));
    for s in &sections {
        md.push_str(&format!("## {}\n\n{}\n\n", s.title, s.body_markdown));
        std::fs::write(
            out.join(format!("{}.md", s.id)),
            format!("# {}\n\n{}", s.title, s.body_markdown),
        )?;
    }
    md.push_str("## Capability API\n\n");
    for cap in &capabilities {
        md.push_str(&format!("### @{}\n\n", cap.name));
        for f in &cap.functions {
            let eff = if f.effectful { " (effectful)" } else { "" };
            md.push_str(&format!("- `{}{}` — {}\n", f.name, eff, f.docs));
        }
        md.push('\n');
    }
    if !scripts.is_empty() {
        md.push_str("## Script reference\n\n");
        let mut s = String::from("# Script reference\n\n");
        for script in &scripts {
            s.push_str(&format!("## {}\n\n", script.path));
            if let Some(doc) = &script.module_doc {
                s.push_str(&format!("{}\n\n", doc));
            }
            for f in &script.functions {
                let vis = if f.is_pub { "pub " } else { "" };
                s.push_str(&format!("### {}{}\n\n{}\n\n", vis, f.signature, f.docs));
                for (name, desc) in &f.param_docs {
                    s.push_str(&format!("- `{}` — {}\n", name, desc));
                }
                if !f.param_docs.is_empty() {
                    s.push('\n');
                }
                if let Some(r) = &f.returns {
                    s.push_str(&format!("Returns: {}\n\n", r));
                }
                for e in &f.effects {
                    s.push_str(&format!("Effects: {}\n\n", e));
                }
                for ex in &f.examples {
                    s.push_str(&format!("```rite\n{}```\n\n", ex));
                }
                md.push_str(&format!("- `{}{}` — {}\n", vis, f.signature, f.docs));
            }
        }
        md.push('\n');
        std::fs::write(out.join("scripts.md"), s)?;
    }
    std::fs::write(out.join("reference.md"), &md)?;
    std::fs::write(out.join("capabilities.md"), {
        let mut c = String::from("# Capabilities\n\n");
        for cap in &capabilities {
            c.push_str(&format!("## @{}\n\n", cap.name));
            for f in &cap.functions {
                c.push_str(&format!("### {}\n\n{}\n\n", f.name, f.docs));
                c.push_str(&format!(
                    "- arity: {}\n- effectful: {}\n- answers a result (`?` applies): {}\n- permission: {}\n\n",
                    f.arity, f.effectful, f.returns_result, f.permission
                ));
            }
        }
        c
    })?;
    std::fs::write(out.join("builtins.md"), builtins_markdown())?;

    // HTML site
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<title>Rite Documentation</title>
<style>
body { font-family: ui-sans-serif, system-ui, sans-serif; max-width: 900px; margin: 2rem auto; padding: 0 1rem; background: #0b0f14; color: #e6edf3; }
h1,h2,h3 { color: #7ee0ff; }
code, pre { background: #161b22; padding: 0.2em 0.4em; border-radius: 4px; }
pre { padding: 1rem; overflow-x: auto; }
a { color: #ff7edb; }
.glyph { color: #c3e88d; }
nav a { margin-right: 1rem; }
</style>
</head>
<body>
<h1>Rite <span class="glyph">◆</span> Documentation</h1>
<nav>
"#,
    );
    for s in &sections {
        html.push_str(&format!("<a href=\"#{}\">{}</a>\n", s.id, s.title));
    }
    html.push_str("</nav>\n");
    for s in &sections {
        html.push_str(&format!(
            "<section id=\"{}\"><h2>{}</h2><pre>{}</pre></section>\n",
            s.id,
            escape_html(&s.title),
            escape_html(&s.body_markdown)
        ));
    }
    html.push_str("<section id=\"capabilities\"><h2>Capabilities</h2>\n");
    for cap in &capabilities {
        html.push_str(&format!("<h3>@{}</h3><ul>\n", cap.name));
        for f in &cap.functions {
            html.push_str(&format!(
                "<li><code>{}</code> — {}</li>\n",
                f.name,
                escape_html(&f.docs)
            ));
        }
        html.push_str("</ul>\n");
    }
    html.push_str("</section>\n");
    if !scripts.is_empty() {
        html.push_str("<section id=\"scripts\"><h2>Script reference</h2>\n");
        for script in &scripts {
            html.push_str(&format!("<h3>{}</h3>\n", escape_html(&script.path)));
            if let Some(doc) = &script.module_doc {
                html.push_str(&format!("<p>{}</p>\n", escape_html(doc)));
            }
            html.push_str("<ul>\n");
            for f in &script.functions {
                html.push_str(&format!(
                    "<li id=\"fn-{}\"><code>{}{}</code> — {}</li>\n",
                    escape_html(&f.name),
                    if f.is_pub { "pub " } else { "" },
                    escape_html(&f.signature),
                    escape_html(&f.docs)
                ));
            }
            html.push_str("</ul>\n");
        }
        html.push_str("</section>\n");
    }
    html.push_str("</body></html>\n");
    std::fs::write(out.join("html/index.html"), html)?;

    // Copy / note path argument for extra sources
    let _ = path;
    Ok(())
}

/// Escape for both element text and quoted attribute values.
///
/// Quotes matter now that this renders names and prose out of user `.rite` files: a
/// function documented with a `"` used to be able to close an attribute and start one of
/// its own. `&` must be replaced first or it would double-escape the others.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn book_sections() -> Vec<DocSection> {
    vec![
        DocSection {
            id: "tour".into(),
            title: "Language tour".into(),
            body_markdown: r#"Rite is an expression-oriented scripting language with glyphs and ASCII aliases.

```rite
◆ square(value) ⟦
  ^ value * value
⟧

name ← "Aura"
! @console.println("hello, {name}")
```

Install the `rite` CLI, then: `rite run script.rite --allow-all`."#.into(),
        },
        DocSection {
            id: "installation".into(),
            title: "Installation".into(),
            body_markdown: "Build from source with `cargo build -p rite-cli --release`. The binary is `target/release/rite`.".into(),
        },
        DocSection {
            id: "glyphs".into(),
            title: "Glyph / ASCII syntax table".into(),
            body_markdown: r#"| Glyph | ASCII | Meaning |
|-------|-------|---------|
| ◆ | def | Declaration |
| ← | <- | Immutable bind |
| ↢ | <~ | Mutable bind |
| → | -> | Pipeline / match arm |
| ^ | return | Return |
| ? | if | Conditional |
| ~ | match | Pattern match |
| ! | do | Effect |
| @ | host. | Capability |
| #name | :name | Atom |
| ⟦ ⟧ | [[ ]] | Block |
| ⟨ ⟩ | << >> | Record |
| ∈ | in | Membership |
| ∉ | not in | Non-membership |"#.into(),
        },
        DocSection {
            id: "lexical".into(),
            title: "Lexical grammar".into(),
            body_markdown: "UTF-8 source. Line comments `//`, nested block comments, doc comments `///` and `//!`. Shebang supported. Numbers: decimal, hex, binary, floats. Strings: escaped, multiline, raw, interpolation.".into(),
        },
        DocSection {
            id: "expressions".into(),
            title: "Expressions and pipelines".into(),
            body_markdown: "Pipelines pass the previous value as the first argument. Use `$` as a placeholder. Member projection: `users → map .name`.".into(),
        },
        DocSection {
            id: "functions".into(),
            title: "Functions and closures".into(),
            body_markdown: "Functions use `◆ name(params) ⟦ ... ⟧`. Closures use `{ |x| ... }` or blocks with parameters. Final expression is returned; `^` for early return.".into(),
        },
        DocSection {
            id: "matching".into(),
            title: "Pattern matching".into(),
            body_markdown: "Match with `~ value ⟦ pattern → expr ... ⟧`. Supports atoms, lists, records, ok/err/some/none, wildcards.".into(),
        },
        DocSection {
            id: "results".into(),
            title: "Results and errors".into(),
            body_markdown: "Host ops return `ok(value)` / `err(record)`. Postfix `?` unwraps or early-returns errors. `panic` aborts the current script/handler.".into(),
        },
        DocSection {
            id: "effects".into(),
            title: "Effects and permissions".into(),
            body_markdown: "Effectful calls require `!`. CLI: `--allow fs:read=./data --allow net=api.example.com --allow-all`. Default: console/clock/random allowed; fs/net/env/process denied.".into(),
        },
        DocSection {
            id: "modules".into(),
            title: "Modules".into(),
            body_markdown: "`use path.to.module as alias`. Top-level decls private by default; export with `pub ◆`.".into(),
        },
        DocSection {
            id: "contracts".into(),
            title: "Runtime type contracts".into(),
            body_markdown: "Optional annotations like `value: int` are checked at runtime on \
                function entry and exit. Types are `int`, `float`, `number` (either), `string`, \
                `bool`, `atom`, `list`, `record`, `bytes`, `function`, `none`, `any`, plus \
                `[T]`, `result<T>` and `⟨field: T, …⟩`. Checking is structural — an empty list \
                satisfies `[int]`, and a record may carry fields the annotation does not name. \
                A parameter or return with no annotation is unconstrained."
                .into(),
        },
        DocSection {
            id: "http".into(),
            title: "HTTP services".into(),
            body_markdown: r#"```rite
@http.listen "127.0.0.1:4040" ⟦
  GET "/health" ⟦
    ^ 200 ⟨status: #ok⟩
  ⟧
⟧
```"#.into(),
        },
        DocSection {
            id: "game".into(),
            title: "Game scripting".into(),
            body_markdown: "Event DSL: `◆ item #key ⟦ ... ⟧`, `◆ room #vault ⟦ ... ⟧` lowers to `@game` capability.".into(),
        },
        DocSection {
            id: "modes".into(),
            title: "Interpreter versus compiled execution".into(),
            body_markdown: "Interpreter is normative. `rite build` generates Rust that uses the same runtime/capabilities for behavioral parity.".into(),
        },
        DocSection {
            id: "cli".into(),
            title: "CLI reference".into(),
            body_markdown: "`rite run|build|check|fmt|repl|test|doc|explain|ast|ir|capabilities|version`".into(),
        },
        DocSection {
            id: "embedding".into(),
            title: "Embedding guide".into(),
            body_markdown: r#"```rust
let engine = RiteEngine::builder().allow_all().build()?;
let value = engine.run_source("s.rite", source).await?;
```"#.into(),
        },
        DocSection {
            id: "troubleshooting".into(),
            title: "Troubleshooting and diagnostics".into(),
            body_markdown: "Diagnostics include stable codes (E021 effect required, E040 permission denied). Use `rite check --json-errors` for machine-readable output.".into(),
        },
    ]
}

/// Generate the agent skill bundle into `output`, reading its inputs
/// (`skills/rite/SKILL.md`, `grammar/aliases.json`, `grammar/rite.ebnf`) from
/// `repo_root`.
///
/// Inputs used to resolve against the CWD with silent stub fallbacks, so
/// `rite docs build` run from a *subdirectory* of the checkout found none of
/// them and quietly replaced the real SKILL.md with a placeholder and
/// aliases.json with `{}`. Anchoring to `repo_root` and refusing on a missing
/// input closes both halves of that.
pub fn generate_agent_bundle(repo_root: &Path, output: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output.join("references"))?;
    std::fs::create_dir_all(output.join("examples/scripts"))?;
    std::fs::create_dir_all(output.join("machine"))?;

    // `SKILL.md` is hand-written and only copied here. When `output` IS
    // `skills/rite` (what CI and `rite docs agent` do) source and destination
    // are the same file; skip the copy entirely rather than round-tripping it.
    let source_skill = repo_root.join("skills/rite/SKILL.md");
    let dest_skill = output.join("SKILL.md");
    let same_file = match (source_skill.canonicalize(), dest_skill.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same_file {
        let skill = std::fs::read_to_string(&source_skill)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", source_skill.display()))?;
        std::fs::write(&dest_skill, skill)?;
    }

    finish_agent_bundle(repo_root, output)
}

/// Everything in the bundle except `SKILL.md`, which is hand-written and only copied.
fn finish_agent_bundle(repo_root: &Path, output: &Path) -> anyhow::Result<()> {
    let aliases_path = repo_root.join("grammar/aliases.json");
    let aliases = std::fs::read_to_string(&aliases_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", aliases_path.display()))?;
    std::fs::write(output.join("machine/aliases.json"), &aliases)?;
    std::fs::write(
        output.join("machine/version.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "language_version": "1",
            "tool_version": env!("CARGO_PKG_VERSION"),
            "formatter_version": "1",
        }))?,
    )?;
    std::fs::write(
        output.join("machine/language-manifest.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": "rite",
            "version": "1",
            "extensions": [".rite"],
            "dialects": ["ascii", "glyph", "mixed", "preserve"],
        }))?,
    )?;

    // capability manifest
    let host = rite_caps::HostCapabilities::with_defaults(rite_caps::PermissionSet::allow_all());
    let mut caps = serde_json::Map::new();
    for (name, descs) in host.all_descriptors() {
        let funcs: Vec<_> = descs
            .iter()
            .map(|d| {
                serde_json::json!({
                    "name": d.name,
                    "docs": d.docs,
                    "effectful": d.effectful,
                    "permission": d.permission,
                    "arity": d.arity,
                })
            })
            .collect();
        caps.insert(name.to_string(), serde_json::json!(funcs));
    }
    std::fs::write(
        output.join("machine/capabilities.json"),
        serde_json::to_string_pretty(&caps)?,
    )?;
    std::fs::write(
        output.join("machine/diagnostics.json"),
        serde_json::to_string_pretty(&serde_json::json!([
            {"code": "E021", "summary": "effectful capability call requires `!`"},
            {"code": "E020", "summary": "undefined name"},
            {"code": "E040", "summary": "permission denied"},
            {"code": "E024", "summary": "circular import"},
        ]))?,
    )?;
    let ebnf_path = repo_root.join("grammar/rite.ebnf");
    let ebnf = std::fs::read_to_string(&ebnf_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", ebnf_path.display()))?;
    std::fs::write(output.join("machine/grammar.ebnf"), ebnf)?;
    std::fs::write(
        output.join("examples/scripts/hello.rite"),
        "! @console.println(\"hello from agent skill\")\n",
    )?;
    std::fs::write(
        output.join("references/quick-reference.md"),
        "# Rite quick reference\n\nSee grammar/aliases.json and mvp.md.\n",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_doc_tags() {
        let lines = [
            "/// Reads a file.",
            "/// @param path Path to read.",
            "/// @returns File text.",
            "/// @effects fs:read",
        ];
        let d = parse_doc_comment(&lines);
        assert!(d.text.contains("Reads"));
        assert_eq!(d.params.len(), 1);
        assert!(d.returns.is_some());
    }
}
