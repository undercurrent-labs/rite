//! Turning command-line strings into a [`PermissionSet`] and an
//! [`ExecutionBudget`].
//!
//! Every tool that runs Rite has to do this, and for a long time `rite-cli` was
//! the only one — so it did it inline in `main.rs`, with `parse_duration` as a
//! private function beside it. Any second executable needs exactly the same
//! behaviour, and two copies of "what does `--allow fs:read=./data` mean" is the
//! kind of duplication that stays in sync right up until it does not.
//!
//! Deliberately **no `clap`**. The reusable part is the meaning of the strings,
//! not the flag declarations: a tool's own `--help` text, argument names and
//! defaults are its business, and adding `clap` to the embedding crate would put
//! an argument parser in the dependency tree of every host that just wants to
//! run a script.
//!
//! ```
//! use rite::options::RuntimeOptions;
//! let opts = RuntimeOptions {
//!     allow: vec!["fs:read=./data".into()],
//!     timeout: Some("30s".into()),
//!     ..Default::default()
//! };
//! let perms = opts.permissions().unwrap();
//! let budget = opts.budget().unwrap();
//! assert_eq!(budget.timeout, Some(std::time::Duration::from_secs(30)));
//! let _ = perms;
//! ```

use rite_caps::{Permission, PermissionSet};
use rite_runtime::ExecutionBudget;
use std::time::Duration;

/// The permission and resource options a Rite run accepts.
///
/// Field names match the flags they come from, so a `clap` struct maps onto this
/// one field for field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOptions {
    /// `--allow fs:read=./data`, repeatable.
    pub allow: Vec<String>,
    /// `--deny console`, repeatable. Applied after grants, so denying something
    /// granted in the same invocation denies it.
    pub deny: Vec<String>,
    /// `--allow-all`. Starts from every permission instead of the secure default.
    pub allow_all: bool,
    /// `--timeout 30s`. `500ms`, `30s`, `5m`, or a bare number of seconds.
    pub timeout: Option<String>,
    pub max_steps: Option<u64>,
    pub max_call_depth: Option<usize>,
    pub max_collection_size: Option<usize>,
    pub max_string_size: Option<usize>,
    /// `--env-file .env`, repeatable. Later files win.
    ///
    /// The values land in the run's environment overlay, and reading exactly
    /// those names is granted implicitly — see [`RuntimeOptions::env_file`].
    pub env_files: Vec<std::path::PathBuf>,
}

impl RuntimeOptions {
    /// Build the permission set.
    ///
    /// Grants first, then denials — so `--allow-all --deny net` is "everything
    /// except the network", which is the only reading that makes the pair useful.
    pub fn permissions(&self) -> Result<PermissionSet, String> {
        let mut perms = if self.allow_all {
            PermissionSet::allow_all()
        } else {
            PermissionSet::default_secure()
        };
        for spec in &self.allow {
            perms.grant(Permission::parse(spec)?);
        }
        // A `--env-file` grants read access to exactly the names it defines,
        // and to nothing else. The argument is the one already written into
        // `@process.args`: a file the invoker named on this command line is
        // their own input to the program, not ambient state they are being
        // asked to expose. Granted *before* the denials, so `--deny env` still
        // takes it away.
        for (name, _) in self.env_file()? {
            perms.grant(Permission::Env(name));
        }
        for spec in &self.deny {
            // A bad `--deny` used to be discarded silently by `rite run`, which
            // meant a typo in a *revocation* left the permission in place. It is
            // an error here for the same reason a bad `--allow` is.
            perms.deny(Permission::parse(spec)?);
        }
        Ok(perms)
    }

    /// The variables every `--env-file` defines, in order, later files winning.
    pub fn env_file(&self) -> Result<Vec<(String, String)>, String> {
        let mut out: Vec<(String, String)> = Vec::new();
        for path in &self.env_files {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read --env-file {}: {e}", path.display()))?;
            for (name, value) in
                parse_env_file(&text).map_err(|e| format!("{}: {e}", path.display()))?
            {
                out.retain(|(existing, _)| existing != &name);
                out.push((name, value));
            }
        }
        Ok(out)
    }

    /// Build the execution budget, leaving anything unset at its default.
    pub fn budget(&self) -> Result<ExecutionBudget, String> {
        let mut budget = ExecutionBudget::new();
        if let Some(text) = &self.timeout {
            budget = budget.with_timeout(
                parse_duration(text).map_err(|e| format!("invalid --timeout {text:?}: {e}"))?,
            );
        }
        if let Some(n) = self.max_steps {
            budget = budget.with_max_steps(n);
        }
        if let Some(n) = self.max_call_depth {
            budget.max_call_depth = n;
        }
        if let Some(n) = self.max_collection_size {
            budget.max_collection_size = n;
        }
        if let Some(n) = self.max_string_size {
            budget.max_string_size = n;
        }
        Ok(budget)
    }
}

/// Parse `500ms`, `30s`, `5m`, or a bare number of seconds.
///
/// Was private in `rite-cli`. Public because every tool that takes a timeout
/// needs the same spellings, and because an embedder configuring a budget from
/// its own config file wants them too.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
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

/// Parse a `.env` file: `KEY=VALUE`, one per line.
///
/// `#` starts a comment, blank lines are skipped, a leading `export ` is
/// accepted because that is what people paste, and a value may be wrapped in
/// single or double quotes. Escapes inside double quotes are honoured for
/// `\n`, `\t`, `\"` and `\\`; single quotes are literal, as in a shell.
///
/// \*\*There is no interpolation.\*\* `$FOO` is literal. Expanding it would mean
/// choosing between the file's `FOO`, the process's, and the overlay's, and
/// callers disagree about which is right.
///
/// Values that override the inherited environment, deliberately: the file was
/// named on this command line, and a stale exported variable quietly winning
/// over it is the worse surprise.
pub fn parse_env_file(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        let Some((name, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `NAME=VALUE`", n + 1));
        };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("line {}: `{name}` is not a variable name", n + 1));
        }
        out.push((name.to_string(), unquote_env_value(value.trim())));
    }
    Ok(out)
}

fn unquote_env_value(value: &str) -> String {
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        return inner.to_string();
    }
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        // Unquoted: an inline `#` starts a comment, which is the one place a
        // bare value differs from a quoted one.
        return value
            .split_once(" #")
            .map(|(v, _)| v.trim_end())
            .unwrap_or(value)
            .to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            // Anything else keeps both characters: a Windows path in an
            // unescaped double-quoted value should survive intact.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    /// The `.env` shapes people actually paste.
    #[test]
    fn an_env_file_takes_the_shapes_people_write() {
        let parsed = parse_env_file(
            "# a comment\n\nAPI_KEY=secret\nexport PORT=\"8080\"\n\
             QUOTED='literal $NOPE'\nESCAPED=\"a\\nb\"\nTRAILING=value # note\n",
        )
        .expect("parses");
        assert_eq!(
            parsed,
            vec![
                ("API_KEY".to_string(), "secret".to_string()),
                ("PORT".to_string(), "8080".to_string()),
                ("QUOTED".to_string(), "literal $NOPE".to_string()),
                ("ESCAPED".to_string(), "a\nb".to_string()),
                ("TRAILING".to_string(), "value".to_string()),
            ]
        );
    }

    /// No interpolation, deliberately. A file that expanded `$FOO` would have to
    /// decide whose `FOO`, and every answer surprises someone.
    #[test]
    fn an_env_file_does_not_interpolate() {
        let parsed = parse_env_file("A=$HOME\nB=\"$HOME\"\n").expect("parses");
        assert_eq!(parsed[0].1, "$HOME");
        assert_eq!(parsed[1].1, "$HOME");
    }

    #[test]
    fn a_malformed_env_line_is_an_error_with_its_number() {
        let e = parse_env_file("GOOD=1\nnot a pair\n").expect_err("refused");
        assert!(e.contains("line 2"), "{e}");
        let e = parse_env_file("HAS SPACE=1\n").expect_err("refused");
        assert!(e.contains("not a variable name"), "{e}");
    }

    /// The point of the feature: `--env-file` grants reading exactly the names
    /// it defines, so a one-liner needs no `--allow`. And nothing else.
    #[test]
    fn an_env_file_grants_exactly_its_own_names() {
        let dir = std::env::temp_dir().join("rite-options-env-file");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("test.env");
        std::fs::write(&path, "API_KEY=secret\nPORT=8080\n").expect("write");

        let options = RuntimeOptions {
            env_files: vec![path.clone()],
            ..Default::default()
        };
        let perms = options.permissions().expect("permissions");
        assert!(perms.check_env("API_KEY").is_ok());
        assert!(perms.check_env("PORT").is_ok());
        assert!(perms.check_env("HOME").is_err(), "and nothing else");
        // Reading is not writing, even for a name the file defined.
        assert!(perms.check_env_write("API_KEY").is_err());

        // An explicit `--deny env` still takes it away: the implicit grant is a
        // convenience, not an override.
        let denied = RuntimeOptions {
            env_files: vec![path],
            deny: vec!["env".into()],
            ..Default::default()
        }
        .permissions()
        .expect("permissions");
        assert!(denied.check_env("API_KEY").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Later files win, and the order of what is left is stable.
    #[test]
    fn a_later_env_file_overrides_an_earlier_one() {
        let dir = std::env::temp_dir().join("rite-options-env-file-order");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("a.env"), "A=1\nB=1\n").expect("write");
        std::fs::write(dir.join("b.env"), "B=2\nC=2\n").expect("write");
        let values = RuntimeOptions {
            env_files: vec![dir.join("a.env"), dir.join("b.env")],
            ..Default::default()
        }
        .env_file()
        .expect("reads");
        assert_eq!(
            values,
            vec![
                ("A".to_string(), "1".to_string()),
                ("B".to_string(), "2".to_string()),
                ("C".to_string(), "2".to_string()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_env_file_is_an_error_naming_the_path() {
        let e = RuntimeOptions {
            env_files: vec![std::path::PathBuf::from("/nonexistent/none.env")],
            ..Default::default()
        }
        .permissions()
        .expect_err("refused");
        assert!(e.contains("none.env"), "{e}");
    }

    use super::*;

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

    #[test]
    fn defaults_are_the_secure_ones() {
        let perms = RuntimeOptions::default().permissions().expect("defaults");
        // console/clock/random allowed, fs/net/env/process/db denied.
        assert_eq!(
            format!("{perms:?}"),
            format!("{:?}", PermissionSet::default_secure())
        );
    }

    #[test]
    fn allow_all_then_deny_leaves_a_hole() {
        let opts = RuntimeOptions {
            allow_all: true,
            deny: vec!["console".into()],
            ..Default::default()
        };
        let perms = opts.permissions().expect("permissions");
        assert!(!perms.allow_all, "the blanket flag must be cleared");
        assert!(!perms.console, "console was denied");
    }

    /// `net` and `env` are host- and name-scoped, so the bare word is not a
    /// permission. Pinned because the error is what tells someone `--deny net`
    /// did not do what they assumed, and it used to be swallowed.
    #[test]
    fn a_scoped_permission_needs_its_scope() {
        for spec in ["net", "fs"] {
            let opts = RuntimeOptions {
                deny: vec![spec.to_string()],
                ..Default::default()
            };
            let err = opts
                .permissions()
                .expect_err("{spec} should not parse bare");
            assert!(err.contains(spec), "{err}");
        }
        // With a scope, both work.
        let opts = RuntimeOptions {
            allow: vec!["net=example.com".into(), "fs:read=./data".into()],
            ..Default::default()
        };
        assert!(opts.permissions().is_ok());
    }

    #[test]
    fn a_bad_permission_is_an_error_on_both_sides() {
        let bad_allow = RuntimeOptions {
            allow: vec!["not a permission".into()],
            ..Default::default()
        };
        assert!(bad_allow.permissions().is_err());
        // A typo in a *revocation* used to be discarded silently.
        let bad_deny = RuntimeOptions {
            deny: vec!["not a permission".into()],
            ..Default::default()
        };
        assert!(bad_deny.permissions().is_err());
    }

    #[test]
    fn every_budget_knob_is_reachable() {
        let opts = RuntimeOptions {
            timeout: Some("2s".into()),
            max_steps: Some(11),
            max_call_depth: Some(22),
            max_collection_size: Some(33),
            max_string_size: Some(44),
            ..Default::default()
        };
        let budget = opts.budget().expect("budget");
        assert_eq!(budget.timeout, Some(Duration::from_secs(2)));
        assert_eq!(budget.max_steps, 11);
        assert_eq!(budget.max_call_depth, 22);
        assert_eq!(budget.max_collection_size, 33);
        assert_eq!(budget.max_string_size, 44);
    }

    #[test]
    fn a_bad_timeout_is_reported_rather_than_ignored() {
        let opts = RuntimeOptions {
            timeout: Some("later".into()),
            ..Default::default()
        };
        let err = opts.budget().expect_err("bad timeout");
        assert!(err.contains("later"), "{err}");
    }
}
