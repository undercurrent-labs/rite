//! Every tutorial ends with a complete, runnable script. This runs it.
//!
//! Tutorial code fences are `native_only`, so `rite docs check` only *parses* them —
//! it proves the syntax is current and nothing more. A tutorial can therefore parse
//! perfectly while describing behaviour the language no longer has, which is the
//! failure mode that matters: a reader trusts a tutorial enough to type it in.
//!
//! So the final script of each tutorial is extracted from the markdown, run against
//! a fixture directory, and its output compared to the output printed beside it. The
//! tutorial is the source of truth; there is no second copy of the script to drift.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rite_bin() -> PathBuf {
    let root = workspace();
    for rel in ["target/debug/rite", "target/release/rite"] {
        let p = root.join(rel);
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("rite")
}

/// The heading that marks the runnable section. Tutorials without one are skipped
/// loudly rather than silently — see `every_tutorial_has_a_runnable_script`.
const SECTION: &str = "## The whole script";

/// Marks a tutorial whose script CI does not run.
///
/// Some scripts cannot go in a CI gate for reasons no flag will fix: a server that
/// blocks until it is stopped, a `rite build` that costs minutes of cold cargo, a
/// Rust host program that is not a `.rite` script at all, a query that needs the
/// network. Those are still written by running them — locally, with
/// `cargo test -p rite-cli --test tutorial_scripts -- --ignored`, which is the same
/// convention the compiler's cold-build tests use.
///
/// The marker is deliberately not a silent skip: `local_only_tutorials_say_so`
/// requires the page to tell the reader, because the whole value of the gate is that
/// a reader can trust printed output, and an unmarked exception spends that trust.
const LOCAL_ONLY: &str = "<!-- ci: local-only -->";

/// The sentence a local-only page must carry, so the exception is visible to a
/// reader rather than only to whoever greps the markdown.
const LOCAL_ONLY_NOTICE: &str = "not run in CI";

struct Runnable {
    /// The complete script, from the section's ```rite fence.
    script: String,
    /// Argv after `rite`, from the section's ```bash fence.
    args: Vec<String>,
    /// What the tutorial says it prints, from the section's ```text fence.
    expected: String,
    /// The `.rite` filename the command names.
    file: String,
}

/// Pull the fences out of the section. Deliberately positional — first ```rite,
/// first ```bash, first ```text — so a tutorial cannot quietly test a different
/// block than the one a reader sees.
fn parse_section(md: &str) -> Option<Runnable> {
    let start = md.find(SECTION)?;
    let body = &md[start..];
    // Stop at the next same-level heading so a later section cannot be picked up.
    let body = match body[SECTION.len()..].find("\n## ") {
        Some(i) => &body[..SECTION.len() + i],
        None => body,
    };

    let mut script = None;
    let mut command = None;
    let mut expected = None;
    let mut lines = body.lines();
    while let Some(line) = lines.next() {
        let Some(info) = line.trim().strip_prefix("```") else {
            continue;
        };
        let lang = info.split_whitespace().next().unwrap_or("");
        let mut buf = String::new();
        for l in lines.by_ref() {
            if l.trim_end() == "```" {
                break;
            }
            buf.push_str(l);
            buf.push('\n');
        }
        match lang {
            "rite" if script.is_none() => script = Some(buf),
            "bash" if command.is_none() => command = Some(buf),
            "text" if expected.is_none() => expected = Some(buf),
            _ => {}
        }
    }

    let command = command?;
    let invocation = command
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("rite "))?;
    let args: Vec<String> = invocation
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect();
    let file = args.iter().find(|a| a.ends_with(".rite"))?.clone();

    Some(Runnable {
        script: script?,
        args,
        expected: expected?,
        file,
    })
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// `mtimes` in a fixture directory sets modification times, because "which files are
/// stale" cannot be demonstrated against files all created a moment ago. One
/// `<name> <unix-seconds>` per line — seconds rather than a date string to keep this
/// harness free of a date-parsing dependency.
fn apply_mtimes(dir: &Path) {
    let spec = dir.join("mtimes");
    let Ok(text) = std::fs::read_to_string(&spec) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, secs) = line
            .split_once(char::is_whitespace)
            .expect("`<name> <unix-seconds>`");
        let secs: u64 = secs
            .trim()
            .parse()
            .unwrap_or_else(|e| panic!("bad timestamp in {}: {e}", spec.display()));
        let f = std::fs::File::options()
            .write(true)
            .open(dir.join(name.trim()))
            .unwrap_or_else(|e| panic!("no fixture file {name}: {e}"));
        f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }
    // Removed so it cannot show up in a glob the tutorial runs.
    std::fs::remove_file(&spec).unwrap();
}

fn tutorials() -> Vec<(String, String)> {
    let dir = workspace().join("docs/tutorials");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tutorials dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let slug = path.file_stem().unwrap().to_string_lossy().to_string();
        if slug == "README" {
            continue;
        }
        out.push((slug, std::fs::read_to_string(&path).unwrap()));
    }
    out.sort();
    assert!(!out.is_empty(), "no tutorials found");
    out
}

/// A tutorial whose final script is not runnable is a tutorial nothing checks, so
/// the absence of the section is itself a failure. Skipping quietly is how a
/// tutorial ends up untested without anyone noticing.
#[test]
fn every_tutorial_has_a_runnable_script() {
    for (slug, md) in tutorials() {
        assert!(
            parse_section(&md).is_some(),
            "docs/tutorials/{slug}.md has no runnable `{SECTION}` section \
             (needs a ```rite script, a ```bash `rite run …` line, and a ```text output)"
        );
    }
}

/// A page that opts out of the CI gate has to say so where a reader will see it.
#[test]
fn local_only_tutorials_say_so() {
    for (slug, md) in tutorials() {
        if !md.contains(LOCAL_ONLY) {
            continue;
        }
        assert!(
            md.contains(LOCAL_ONLY_NOTICE),
            "docs/tutorials/{slug}.md is marked `{LOCAL_ONLY}` but never tells the reader — \
             the page must contain the words \"{LOCAL_ONLY_NOTICE}\""
        );
    }
}

#[test]
fn every_tutorial_script_runs_and_prints_what_it_claims() {
    run_tutorial_scripts(false);
}

/// The ones CI cannot run. Kept out of the default run for the reasons in
/// `LOCAL_ONLY`, and run on demand with `-- --ignored`.
#[test]
#[ignore]
fn local_only_tutorial_scripts_run_and_print_what_they_claim() {
    run_tutorial_scripts(true);
}

fn run_tutorial_scripts(local_only: bool) {
    let mut ran = 0;
    for (slug, md) in tutorials() {
        if md.contains(LOCAL_ONLY) != local_only {
            continue;
        }
        let Some(run) = parse_section(&md) else {
            continue; // reported by the test above
        };
        ran += 1;

        let dir = std::env::temp_dir().join(format!("rite_tutorial_{slug}"));
        let _ = std::fs::remove_dir_all(&dir);
        let fixtures = workspace().join("docs/tutorials/fixtures").join(&slug);
        if fixtures.is_dir() {
            copy_dir(&fixtures, &dir);
        } else {
            std::fs::create_dir_all(&dir).unwrap();
        }
        apply_mtimes(&dir);
        std::fs::write(dir.join(&run.file), &run.script).unwrap();

        let out = Command::new(rite_bin())
            .args(&run.args)
            .envs(fixture_env(&dir))
            .current_dir(&dir)
            .output()
            .expect("spawn rite");

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "{slug}: the tutorial's own command failed ({}):\n{stderr}",
            out.status
        );
        assert_eq!(
            stdout.trim_end(),
            run.expected.trim_end(),
            "{slug}: output does not match the tutorial\n--- stderr ---\n{stderr}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(
        ran > 0,
        "no {} tutorials ran — a filter that matches nothing is a pass that proves nothing",
        if local_only { "local-only" } else { "CI" }
    );
}

/// `env` in a fixture directory sets environment variables for the run, one
/// `KEY=VALUE` per line. `@http.listen` blocks until it is stopped, and honours
/// `RITE_HTTP_TEST=1` with `RITE_HTTP_TEST_SECS`, which is what lets a server
/// tutorial terminate at all.
fn fixture_env(dir: &Path) -> Vec<(String, String)> {
    let spec = dir.join("env");
    let Ok(text) = std::fs::read_to_string(&spec) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').expect("`KEY=VALUE`");
        out.push((k.trim().to_string(), v.trim().to_string()));
    }
    // Removed so it cannot show up in a glob the tutorial runs.
    let _ = std::fs::remove_file(&spec);
    out
}

/// The data a tutorial shows and the data its test runs on must be the same bytes,
/// or the walkthrough describes numbers the reader cannot reproduce.
#[test]
fn shown_input_files_match_the_fixtures() {
    for (slug, md) in tutorials() {
        let fixtures = workspace().join("docs/tutorials/fixtures").join(&slug);
        if !fixtures.is_dir() {
            continue;
        }
        // `Save this as orders.json:` followed by a fenced block.
        for (idx, _) in md.match_indices("Save this as `") {
            let rest = &md[idx + "Save this as `".len()..];
            let name = rest.split('`').next().unwrap();
            let file = fixtures.join(name);
            if !file.is_file() {
                continue;
            }
            let fence = rest.find("```").expect("a block after the filename");
            let after = &rest[fence..];
            let body = after
                .split_once('\n')
                .expect("fence body")
                .1
                .split("\n```")
                .next()
                .unwrap();
            let on_disk = std::fs::read_to_string(&file).unwrap();
            assert_eq!(
                body.trim_end(),
                on_disk.trim_end(),
                "{slug}: `{name}` in the tutorial differs from the fixture it is tested against"
            );
        }
    }
}

/// Every fixture file must actually be committed.
///
/// `.gitignore` carries `*.log`, and the directory-audit tutorial globs `logs/*.log`,
/// so its sample files were silently excluded: `git add -A` reported nothing, the
/// suite passed locally where the files existed, and CI failed on a machine that had
/// only what was checked in. A fixture that is not tracked is not a fixture.
#[test]
fn every_fixture_file_is_tracked_by_git() {
    let root = workspace().join("docs/tutorials/fixtures");
    if !root.is_dir() {
        return;
    }
    let mut files = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    walk(&root, &mut files);
    assert!(!files.is_empty(), "no tutorial fixtures found");

    for file in &files {
        // Ask whether the file is *tracked*, not whether it matches an ignore rule.
        // `git check-ignore` skips anything already in the index, so it answers "not
        // ignored" for a committed file and would have passed here while the real
        // question — will a fresh checkout have this? — went unasked.
        let out = Command::new("git")
            .args(["ls-files", "--error-unmatch"])
            .arg(file)
            .current_dir(workspace())
            .output();
        let Ok(out) = out else {
            return; // no git available; nothing to assert
        };
        assert!(
            out.status.success(),
            "{} is not tracked by git, so it will not exist in a fresh checkout \
             (a .gitignore rule may be swallowing it — `git add -A` says nothing when it does)",
            file.strip_prefix(workspace()).unwrap_or(file).display()
        );
    }
}
