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
    pub search: Vec<SearchEntry>,
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
        let line = line.trim_start_matches("///").trim_start();
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
    DocComment {
        text,
        params,
        returns,
        effects,
        examples,
    }
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

    let index = DocIndex {
        title: "Rite Language Documentation".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        sections: sections.clone(),
        capabilities: capabilities.clone(),
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
        std::fs::write(out.join(format!("{}.md", s.id)), format!("# {}\n\n{}", s.title, s.body_markdown))?;
    }
    md.push_str("## Capability API\n\n");
    for cap in &capabilities {
        md.push_str(&format!("### @{}\n\n", cap.name));
        for f in &cap.functions {
            let eff = if f.effectful { " (effectful)" } else { "" };
            md.push_str(&format!(
                "- `{}{}` — {}\n",
                f.name, eff, f.docs
            ));
        }
        md.push('\n');
    }
    std::fs::write(out.join("reference.md"), &md)?;
    std::fs::write(out.join("capabilities.md"), {
        let mut c = String::from("# Capabilities\n\n");
        for cap in &capabilities {
            c.push_str(&format!("## @{}\n\n", cap.name));
            for f in &cap.functions {
                c.push_str(&format!("### {}\n\n{}\n\n", f.name, f.docs));
                c.push_str(&format!(
                    "- arity: {}\n- effectful: {}\n- permission: {}\n\n",
                    f.arity, f.effectful, f.permission
                ));
            }
        }
        c
    })?;

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
.sigil { color: #c3e88d; }
nav a { margin-right: 1rem; }
</style>
</head>
<body>
<h1>Rite <span class="sigil">◆</span> Documentation</h1>
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
    html.push_str("</section></body></html>\n");
    std::fs::write(out.join("html/index.html"), html)?;

    // Copy / note path argument for extra sources
    let _ = path;
    Ok(())
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn book_sections() -> Vec<DocSection> {
    vec![
        DocSection {
            id: "tour".into(),
            title: "Language tour".into(),
            body_markdown: r#"Rite is an expression-oriented scripting language with glyphic sigils and ASCII aliases.

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
            id: "sigils".into(),
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
            body_markdown: "Optional annotations like `value: int` are checked at runtime on function entry/exit.".into(),
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

/// Generate agent skill bundle under `skills/rite`.
pub fn generate_agent_bundle(output: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(output.join("references"))?;
    std::fs::create_dir_all(output.join("examples/scripts"))?;
    std::fs::create_dir_all(output.join("machine"))?;

    let skill = std::fs::read_to_string("skills/rite/SKILL.md").unwrap_or_else(|_| {
        r#"# Rite Agent Skill

Rite is an expression-oriented scripting language with glyphic and ASCII syntax.

## Rules
- Prefer pipelines and explicit effects (`!` / `do`).
- Capabilities use `@name` (glyph) or `host.name` (ASCII).
- Only `false` and `none` are falsey.
- Do not invent syntax not in grammar/aliases.json.
- Run with `rite run --allow-all file.rite` during development.
- Format with `rite fmt` / convert with `rite convert --to ascii|glyph`.
"#
        .into()
    });
    std::fs::write(output.join("SKILL.md"), skill)?;

    let aliases = std::fs::read_to_string("grammar/aliases.json")
        .unwrap_or_else(|_| "{}".into());
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
    if let Ok(ebnf) = std::fs::read_to_string("grammar/rite.ebnf") {
        std::fs::write(output.join("machine/grammar.ebnf"), ebnf)?;
    }
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
