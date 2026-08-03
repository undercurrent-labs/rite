//! Every documented example is executed.
//!
//! Not parsed — **executed**. Parsing proved the examples were well-formed while
//! `examples/cant/04-scatter-collect` contained `[[1, 2], [3]]`, which is not a
//! valid Rite list and so was not a valid program. A documentation example that
//! cannot run is worse than no example, because a reader trusts it enough to
//! type it in.
//!
//! Two corpora:
//!
//! * `examples/cant/*/main.cant` — run, and required to succeed.
//! * ` ```cant ` fences in `docs/cant/*.md` and `examples/cant/README.md` —
//!   checked, and required to pass. Checked rather than run because a doc fence
//!   is often an illustration with a name nothing defines; a fence that *is*
//!   runnable can say so, and then it is run.
//!
//! A fence may opt out with ` ```cant ignore ` when it is deliberately a
//! fragment. Each one has to say why in a comment on the line above, which is
//! the point: an opt-out that needs no justification is one that gets used to
//! silence a real problem.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cant-cli has two ancestors")
        .to_path_buf()
}

fn cant(args: &[&str], dir: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cant"));
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command.output().expect("cant binary")
}

// ---- examples

#[test]
fn every_example_runs() {
    let root = repo_root().join("examples/cant");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("examples/cant")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.is_dir())
        .collect();
    cases.sort();
    assert!(cases.len() >= 6, "expected one example per construct");

    let mut failures = Vec::new();
    for case in cases {
        let path = case.join("main.cant");
        if !path.is_file() {
            failures.push(format!("{}: no main.cant", case.display()));
            continue;
        }
        // From inside the example's own directory, so a relative path in the
        // program means what a reader following the README would expect.
        let out = cant(&["run", "main.cant", "--allow-all"], Some(&case));
        if !out.status.success() {
            failures.push(format!(
                "{}: exited {:?}\n{}",
                case.display(),
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} example(s) that do not run:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// An example's README must show the program it sits beside.
///
/// The two drifted once already — the README kept `[[1, 2], …]` after the
/// program was fixed — and a README showing something the file does not contain
/// is the most confusing possible outcome.
#[test]
fn every_example_readme_quotes_its_own_program() {
    let root = repo_root().join("examples/cant");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&root).expect("examples/cant") {
        let case = entry.expect("entry").path();
        if !case.is_dir() {
            continue;
        }
        let program = std::fs::read_to_string(case.join("main.cant")).expect("main.cant");
        let readme = std::fs::read_to_string(case.join("README.md")).expect("README.md");
        let program = program.trim();
        if !readme.contains(program) {
            failures.push(format!(
                "{}: the README does not contain the program:\n  {program}",
                case.display()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- documentation fences

struct Fence {
    file: String,
    line: usize,
    source: String,
    /// ` ```cant run ` — execute it, not just check it.
    run: bool,
}

/// Extract ` ```cant ` fences, honouring `ignore` and `run` modifiers.
fn fences(markdown: &str, file: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut lines = markdown.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        let Some(info) = line.trim_start().strip_prefix("```cant") else {
            continue;
        };
        let info = info.trim();
        let mut body = String::new();
        for (_, line) in lines.by_ref() {
            if line.trim_start().starts_with("```") {
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        if info.contains("ignore") {
            continue;
        }
        out.push(Fence {
            file: file.to_string(),
            line: n + 1,
            source: body,
            run: info.contains("run"),
        });
    }
    out
}

fn doc_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut out: Vec<PathBuf> = std::fs::read_dir(root.join("docs/cant"))
        .expect("docs/cant")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    out.push(root.join("examples/cant/README.md"));
    out.push(root.join("docs/adr/0001-cant-sibling-frontend.md"));
    out.push(root.join("docs/adr/0002-cant-lowers-through-rite.md"));
    out.sort();
    out
}

#[test]
fn every_documented_cant_fence_is_a_real_program() {
    let root = repo_root();
    // Counted *including* the ignored ones: this guards the extractor, and an
    // extractor that stopped matching would otherwise be masked by documentation
    // that happens to opt out.
    let mut seen = 0;
    let mut failures = Vec::new();

    for path in doc_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        seen += text.matches("```cant").count();
        for fence in fences(&text, &relative) {
            let command = if fence.run { "run" } else { "check" };
            let mut args = vec![command, "-e", fence.source.trim()];
            if fence.run {
                args.push("--allow-all");
            }
            let out = cant(&args, None);
            if !out.status.success() {
                failures.push(format!(
                    "{}:{} — `cant {command}` exited {:?}\n{}\n{}",
                    fence.file,
                    fence.line,
                    out.status.code(),
                    fence.source.trim(),
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }

    // The floor guards the *extractor*, not the documentation: if a change to
    // the fence parsing silently matched nothing, every assertion below would
    // pass vacuously. `docs/cant/language.md` alone carries ten.
    assert!(
        seen >= 10,
        "only {seen} fences found — the extractor is probably broken"
    );
    assert!(
        failures.is_empty(),
        "{} documented fence(s) that are not real programs:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The tracked graph pictures are what `cant graph` currently produces.
///
/// A diagram that has drifted from the tool is worse than none: the whole claim
/// of `cant graph` is that the picture *is* the program. Compares the DOT rather
/// than the SVG — Graphviz's layout depends on its own version, so comparing
/// rendered output would fail on a different machine for a reason nobody caused.
#[test]
fn the_documented_graph_pictures_are_current() {
    let root = repo_root();
    let dir = root.join("docs/cant/graphs");

    // Must match `scripts/build-cant-graphs.sh`. Duplicated deliberately: the
    // script is what regenerates them and this is what notices, and a shared
    // data file would let both drift from the documentation together.
    let cases: &[(&str, &str)] = &[
        ("flow", "[1, 2, 3] -> * -> ?{ $ % 2 = 0 } -> []"),
        ("fork", "5 -> |{ $ + 1 ; $ * 2 ; $ * $ } -> []"),
        (
            "orbit",
            "[1, 2] -> * -> ~{ ?{ $ < 8 } -> $ * 2 } :by str :max 64 -> []",
        ),
        (
            "nested",
            "4 -> |{ ?{ $ > 2 } -> $ * 10 ; ~{ ?{ $ < 8 } -> $ + 2 } :max 8 } -> []",
        ),
        (
            "effects",
            r#""data.json" -> !@fs.read? -> @json.decode -> .name"#,
        ),
    ];

    let mut stale = Vec::new();
    for (name, program) in cases {
        let svg = dir.join(format!("{name}.svg"));
        assert!(
            svg.is_file(),
            "{} is missing — run `bash scripts/build-cant-graphs.sh`",
            svg.display()
        );
        let rendered = std::fs::read_to_string(&svg).expect("svg");
        // Graphviz rewrites label text on its way into the SVG: runs of spaces
        // become `&#160;`, quotes and angle brackets become entities. Comparing
        // raw substrings would be comparing its escaping rules, not the content,
        // so both sides are normalized to plain words.
        let haystack = normalize(&text_content(&rendered));

        let out = cant(&["graph", "--format", "dot", "-e", program], None);
        assert!(out.status.success(), "cant graph failed for {name}");
        let dot = String::from_utf8_lossy(&out.stdout);

        // Every node label the tool emits must appear in the picture. That is
        // the part a stale SVG gets wrong, and unlike coordinates it does not
        // depend on which Graphviz drew it.
        for label in dot.lines().filter_map(|l| {
            l.split_once("[label=\"")
                .and_then(|(_, rest)| rest.split_once("\" "))
                .map(|(label, _)| label.to_string())
        }) {
            for piece in label.split("\\n") {
                let needle = normalize(&piece.replace("\\\"", "\""));
                if needle.is_empty() {
                    continue;
                }
                if !haystack.contains(&needle) {
                    stale.push(format!("{name}.svg does not contain {needle:?}"));
                }
            }
        }
    }
    assert!(
        stale.is_empty(),
        "{} stale graph picture(s) — run `bash scripts/build-cant-graphs.sh`:\n  {}",
        stale.len(),
        stale.join("\n  ")
    );
}

/// The text of every `<text>` element, concatenated.
fn text_content(svg: &str) -> String {
    let mut out = String::new();
    for chunk in svg.split("<text").skip(1) {
        let Some((_, rest)) = chunk.split_once('>') else {
            continue;
        };
        let Some((body, _)) = rest.split_once("</text>") else {
            continue;
        };
        out.push_str(body);
        out.push(' ');
    }
    out
}

/// Entities decoded, whitespace collapsed. Enough to compare words.
fn normalize(text: &str) -> String {
    let decoded = text
        .replace("&#160;", " ")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every picture a document embeds exists.
#[test]
fn every_embedded_graph_exists() {
    let root = repo_root();
    let mut missing = Vec::new();
    for path in doc_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for name in text
            .split("](graphs/")
            .skip(1)
            .filter_map(|rest| rest.split(')').next().map(str::to_string))
        {
            if !root.join("docs/cant/graphs").join(&name).is_file() {
                missing.push(format!(
                    "{}: graphs/{name}",
                    path.strip_prefix(&root).unwrap_or(&path).display()
                ));
            }
        }
    }
    assert!(missing.is_empty(), "embedded but missing: {missing:?}");
}

/// Every link in a published document goes somewhere.
///
/// A published page linking an unpublished one is the failure this catches:
/// `language.md` pointed at `internals.md` after `internals.md` was taken off the
/// site, and the site rendered "no document named internals" to anyone who
/// followed it. On GitHub the same link is fine, which is why nothing else
/// notices.
#[test]
fn published_documents_only_link_to_things_that_exist() {
    let root = repo_root();
    let published = published_docs(&root);
    assert!(
        published.len() >= 4,
        "only {} published document(s) — the reader of docs.ts is probably broken",
        published.len()
    );

    let mut broken = Vec::new();
    for file in &published {
        let text = std::fs::read_to_string(root.join("docs/cant").join(file)).expect("doc");
        for target in markdown_link_targets(&text) {
            // Anchors and absolute URLs are somebody else's problem.
            if target.starts_with('#') || target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let path = target.split('#').next().unwrap_or(&target);
            // A link to a Markdown file must be to one the site publishes; the
            // reader is on the site, and the site is where the link lands.
            if path.ends_with(".md") {
                let name = path.rsplit('/').next().unwrap_or(path);
                if !published.iter().any(|p| p == name) {
                    broken.push(format!("{file} → {target} (not published)"));
                }
                continue;
            }
            // Anything else is a repository path, relative to docs/cant.
            if !root.join("docs/cant").join(path).exists() {
                broken.push(format!("{file} → {target} (no such path)"));
            }
        }
    }
    assert!(
        broken.is_empty(),
        "{} broken link(s) in published documentation:\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

/// The site bundles exactly the documents it publishes.
///
/// Vite bundles every file a glob matches, whether or not a route serves it, so
/// `docs/cant/*.md` shipped the internal notes as a fetchable chunk on a site
/// whose navigation deliberately does not link them. The glob names its files,
/// and this is what keeps that list honest.
#[test]
fn published_documents_are_the_documents_bundled() {
    let root = repo_root();
    let source =
        std::fs::read_to_string(root.join("apps/cant-web/src/lib/docs.ts")).expect("docs.ts");

    let pattern = source
        .split_once("docs/cant/{")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(inner, _)| inner.to_string())
        .expect("the glob should name its files as `docs/cant/{a,b}.md`");
    let mut bundled: Vec<String> = pattern
        .split(',')
        .map(|n| format!("{}.md", n.trim()))
        .collect();
    bundled.sort();

    let mut published = published_docs(&root);
    published.sort();

    assert_eq!(
        bundled, published,
        "the glob in docs.ts and CANT_DOCS disagree; the site would bundle documents it does not serve"
    );
}

/// The file names in `apps/cant-web/src/lib/docs.ts`.
///
/// Read from the site's own list rather than duplicated here, so taking a page
/// off the site is what makes this test start guarding it.
fn published_docs(root: &Path) -> Vec<String> {
    let source = std::fs::read_to_string(root.join("apps/cant-web/src/lib/docs.ts"))
        .expect("apps/cant-web/src/lib/docs.ts");
    source
        .split("file: \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next().map(str::to_string))
        .collect()
}

/// The targets of `[text](target)` links, images included.
fn markdown_link_targets(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        if let Some(end) = rest.find(')') {
            out.push(rest[..end].trim().to_string());
            rest = &rest[end..];
        }
    }
    out
}

/// A fence that opts out has to say why, on the line above.
///
/// Without this the modifier becomes the way to silence a failing example, which
/// is exactly the drift this file exists to prevent.
#[test]
fn every_ignored_fence_explains_itself() {
    let root = repo_root();
    let mut failures = Vec::new();
    for path in doc_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let Some(info) = line.trim_start().strip_prefix("```cant") else {
                continue;
            };
            if !info.contains("ignore") {
                continue;
            }
            // The line above may be the *end* of a multi-line HTML comment, so
            // the test is "is it comment or quote", not "does it start one".
            let above = n.checked_sub(1).map(|i| lines[i].trim()).unwrap_or("");
            let is_prose = above.starts_with("<!--")
                || above.ends_with("-->")
                || above.starts_with('>')
                || above.contains("ignore:");
            if !is_prose {
                failures.push(format!(
                    "{}:{} — an ignored fence with no reason above it",
                    path.strip_prefix(&root).unwrap_or(&path).display(),
                    n + 1
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
